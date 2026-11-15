use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Serialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

mod targets;
pub use targets::AllowedTargets;
pub(crate) use targets::Objection;

const PING_TIMEOUT: Duration = Duration::from_secs(3);

/// Whether a service's real target can be reached over the network, judged on a TCP connection only: reachable
/// when the socket opened before the timeout, unreachable on a timeout, a refused connection or a DNS failure. No
/// HTTP request is sent (see `ProxyClient::ping`).
#[derive(Debug, Clone, Serialize)]
pub struct PingStatus {
    pub reachable: bool,
    pub checked_at: u64,
    pub error: Option<String>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[derive(Clone)]
pub struct ProxyClient {
    client: Client,
    /// `PROXY_ALLOWED_TARGETS` when set (decision 3); `None` allows every target.
    targets: Option<Arc<AllowedTargets>>,
}

/// What a relay answers instead of the backend's response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyError {
    /// The backend could not be reached or read: 502, or 504 after a timeout.
    Upstream(StatusCode),
    /// The target is outside `PROXY_ALLOWED_TARGETS`: nothing was sent to it.
    Refused,
}

/// The body of a relay refused by `PROXY_ALLOWED_TARGETS`. Written in English, like every response of a service.
pub const REFUSED_TARGET: &str = "The target of this service is outside the targets this server may relay to \
(PROXY_ALLOWED_TARGETS): the request was not forwarded.";

impl ProxyError {
    pub fn status(self) -> StatusCode {
        match self {
            ProxyError::Upstream(status) => status,
            ProxyError::Refused => StatusCode::BAD_GATEWAY,
        }
    }
}

impl axum::response::IntoResponse for ProxyError {
    fn into_response(self) -> axum::response::Response {
        match self {
            ProxyError::Upstream(status) => status.into_response(),
            ProxyError::Refused => (StatusCode::BAD_GATEWAY, REFUSED_TARGET).into_response(),
        }
    }
}

impl Default for ProxyClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Longest silence accepted from a proxied backend between two reads (`PROXY_READ_TIMEOUT_SECS`, default 120 s).
/// An idle limit rather than a total one, so that long downloads and event streams that keep sending still work.
fn read_timeout_from_env() -> Duration {
    read_timeout(std::env::var("PROXY_READ_TIMEOUT_SECS").ok().as_deref())
}

/// A whole number of seconds above zero, blanks around allowed; anything else gives the default.
fn read_timeout(setting: Option<&str>) -> Duration {
    let secs = setting
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(120);
    Duration::from_secs(secs)
}

impl ProxyClient {
    pub fn new() -> Self {
        Self::with_timeouts(Duration::from_secs(10), read_timeout_from_env())
    }

    /// Without these bounds, a backend that accepts the connection and never answers held the client's request
    /// (and a task) open forever.
    pub fn with_timeouts(connect: Duration, read: Duration) -> Self {
        Self::build(connect, read, None)
    }

    /// A client that connects only where `targets` allows (`PROXY_ALLOWED_TARGETS`), with the usual timeouts.
    pub fn restricted(targets: AllowedTargets) -> Self {
        Self::build(
            Duration::from_secs(10),
            read_timeout_from_env(),
            Some(Arc::new(targets)),
        )
    }

    /// With a list, names resolve through `targets::Gate`, and the system's proxy variables are ignored: a connection
    /// through an HTTP proxy would reach the proxy's address, not the target's, and the target would be resolved by
    /// the proxy, out of reach of the list. Redirects are never followed: a redirect to an address would reach a
    /// connection that neither `Gate` (never asked for an address) nor `check_address` (the first URL only) judged.
    fn build(connect: Duration, read: Duration, targets: Option<Arc<AllowedTargets>>) -> Self {
        let mut builder = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(connect)
            .read_timeout(read);
        if let Some(targets) = &targets {
            builder = builder
                .no_proxy()
                .dns_resolver(Arc::new(targets::Gate(Arc::clone(targets))));
        }
        Self {
            client: builder.build().expect("reqwest client"),
            targets,
        }
    }

    /// The list this client connects within, if any.
    pub fn allowed_targets(&self) -> Option<&AllowedTargets> {
        self.targets.as_deref()
    }

    /// A target written as an address never reaches the resolver (`targets::Gate`): it is judged here, on the URL as
    /// the HTTP client reads it, before anything is sent.
    fn check_address(&self, url: &str) -> Result<(), ProxyError> {
        let Some(targets) = &self.targets else {
            return Ok(());
        };
        let address = match reqwest::Url::parse(url).map(|u| u.host().map(|h| h.to_owned())) {
            Ok(Some(url::Host::Ipv4(address))) => std::net::IpAddr::V4(address),
            Ok(Some(url::Host::Ipv6(address))) => std::net::IpAddr::V6(address),
            _ => return Ok(()),
        };
        if targets.admits(address, false) {
            Ok(())
        } else {
            tracing::warn!(url = %crate::server::redaction::redact_url_credentials(url), "proxy target refused");
            Err(ProxyError::Refused)
        }
    }

    /// The error of a request the HTTP client could not send: `Refused` when the resolver found no address the list
    /// admits.
    fn send_error(e: reqwest::Error, url: &str, what: &str) -> ProxyError {
        let url = crate::server::redaction::redact_url_credentials(url);
        if targets::refused_in(&e) {
            tracing::warn!(url = %url, "proxy target refused");
            return ProxyError::Refused;
        }
        let status = upstream_error_status(&e);
        tracing::error!(error = %e.without_url(), url = %url, "{what}");
        ProxyError::Upstream(status)
    }

    /// Opens a TCP connection to the target's host and port (the URL's port, else 443 for https and 80 otherwise) and
    /// closes it. Nothing else: no HTTP request, no TLS handshake, no call to any route of the backend, so the answer
    /// says whether the socket opened in time, not whether the API works.
    pub async fn ping(&self, url: &str) -> PingStatus {
        let unreachable = |error: String| PingStatus {
            reachable: false,
            checked_at: now_ms(),
            error: Some(error),
        };
        let (host, port) = match parse_host_port(url) {
            Ok(hp) => hp,
            Err(e) => return unreachable(e),
        };

        match tokio::time::timeout(PING_TIMEOUT, self.ping_connect(&host, port)).await {
            Ok(Ok(())) => PingStatus {
                reachable: true,
                checked_at: now_ms(),
                error: None,
            },
            Ok(Err(e)) => unreachable(e),
            Err(_elapsed) => unreachable(crate::i18n::tr(
                "no answer after {0} s",
                &[&PING_TIMEOUT.as_secs()],
            )),
        }
    }

    /// Opens and drops one TCP connection, to the addresses the list admits when there is one.
    async fn ping_connect(&self, host: &url::Host<String>, port: u16) -> Result<(), String> {
        let addresses: Vec<SocketAddr> = match (&self.targets, host) {
            (Some(targets), host) => match targets.connectable(borrowed(host), port).await {
                Ok(addresses) => addresses,
                Err(targets::Blocked::Unresolved(e)) => return Err(e.to_string()),
                Err(targets::Blocked::Refused(addresses)) => {
                    let objection = match addresses.is_empty() {
                        true => Objection::Outside {
                            host: host.to_string(),
                        },
                        false => Objection::ResolvesOutside {
                            host: host.to_string(),
                            addresses,
                        },
                    };
                    return Err(objection.message());
                }
            },
            (None, url::Host::Ipv4(address)) => vec![SocketAddr::new((*address).into(), port)],
            (None, url::Host::Ipv6(address)) => vec![SocketAddr::new((*address).into(), port)],
            (None, url::Host::Domain(name)) => tokio::net::lookup_host((name.as_str(), port))
                .await
                .map_err(|e| e.to_string())?
                .collect(),
        };
        tokio::net::TcpStream::connect(&addresses[..])
            .await
            .map(drop)
            .map_err(|e| e.to_string())
    }

    pub async fn forward(
        &self,
        target_base: &str,
        remaining_path: &str,
        req: Request<Body>,
    ) -> Result<Response<Body>, ProxyError> {
        let (parts, body) = req.into_parts();

        let url = target_url(target_base, remaining_path, parts.uri.query());
        self.check_address(&url)?;

        let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
            .unwrap_or(reqwest::Method::GET);
        let mut builder = self.client.request(method, &url);

        let original_host = parts
            .headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        for (name, value) in &parts.headers {
            if !is_hop_by_hop(name.as_str()) {
                builder = builder.header(name, value);
            }
        }

        for (name, value) in forwarding_headers(&original_host) {
            builder = builder.header(name, value);
        }

        let req_stream = http_body_util::BodyStream::new(body).filter_map(|result| async move {
            match result {
                Ok(frame) => frame.into_data().ok().map(Ok),
                Err(e) => Some(Err(std::io::Error::other(e))),
            }
        });
        builder = builder.body(reqwest::Body::wrap_stream(req_stream));

        let upstream_resp = builder
            .send()
            .await
            .map_err(|e| Self::send_error(e, &url, "proxy forward failed"))?;

        let status = StatusCode::from_u16(upstream_resp.status().as_u16())
            .unwrap_or(StatusCode::BAD_GATEWAY);

        let mut response = Response::builder().status(status);
        for (name, value) in upstream_resp.headers() {
            if !is_hop_by_hop(name.as_str()) {
                response = response.header(name, value);
            }
        }

        let resp_stream = upstream_resp.bytes_stream();
        response
            .body(Body::from_stream(resp_stream))
            .map_err(|_| ProxyError::Upstream(StatusCode::INTERNAL_SERVER_ERROR))
    }

    /// `forward()`, plus a copy of the request and the response for traffic observation (`server::observation`). Only
    /// called for a service a user chose to observe; the default path never captures.
    ///
    /// Each side is captured only when its size is known and within `max_buffer`:
    /// - a request with a larger Content-Length, or sent in chunks (size unknown), goes through `forward()` unchanged
    ///   and returns no capture; a request with neither header (a GET without body, typically) has an empty body and is
    ///   captured;
    /// - a response with the same problem (checked once the backend answered) is still streamed to the client, and no
    ///   capture is returned.
    ///
    /// Either way the client gets exactly what `forward()` would have sent; only the capture is missing. The one
    /// exception is malformed HTTP that hyper never lets through on a real connection: a request with neither header
    /// carrying a body larger than `max_buffer` would get 413.
    pub async fn forward_with_capture(
        &self,
        target_base: &str,
        remaining_path: &str,
        req: Request<Body>,
        max_buffer: usize,
    ) -> Result<(Response<Body>, Option<ProxyCaptureRaw>), ProxyError> {
        let (parts, body) = req.into_parts();

        let request_content_length = parts
            .headers
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok());
        // With neither Content-Length nor chunked encoding, HTTP defines the body as empty: safe to buffer. Only a body of
        // truly unknown size (chunked) falls back to plain streaming.
        let is_chunked = parts
            .headers
            .get(axum::http::header::TRANSFER_ENCODING)
            .is_some();
        let capturable_request = match request_content_length {
            Some(n) => n <= max_buffer,
            None => !is_chunked,
        };

        if !capturable_request {
            let rebuilt = Request::from_parts(parts, body);
            let resp = self.forward(target_base, remaining_path, rebuilt).await?;
            return Ok((resp, None));
        }

        let request_query_params: HashMap<String, String> = parts
            .uri
            .query()
            .map(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let url = target_url(target_base, remaining_path, parts.uri.query());
        self.check_address(&url)?;

        let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
            .unwrap_or(reqwest::Method::GET);
        let mut builder = self.client.request(method, &url);

        let original_host = parts
            .headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let request_content_type = parts
            .headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let mut request_headers = HashMap::new();
        for (name, value) in &parts.headers {
            let name_str = name.as_str();
            if is_hop_by_hop(name_str) {
                continue;
            }
            builder = builder.header(name, value);
            if let Ok(v) = value.to_str() {
                request_headers.insert(name_str.to_string(), v.to_string());
            }
        }
        for (name, value) in forwarding_headers(&original_host) {
            builder = builder.header(name, value);
        }

        let body_bytes = axum::body::to_bytes(body, max_buffer).await.map_err(|e| {
            tracing::error!(error = %e, "observed request body exceeds capture buffer");
            ProxyError::Upstream(StatusCode::PAYLOAD_TOO_LARGE)
        })?;
        builder = builder.body(body_bytes.to_vec());

        let upstream_resp = builder
            .send()
            .await
            .map_err(|e| Self::send_error(e, &url, "proxy forward (observed) failed"))?;

        let status = StatusCode::from_u16(upstream_resp.status().as_u16())
            .unwrap_or(StatusCode::BAD_GATEWAY);
        let response_content_type = upstream_resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let response_content_length = upstream_resp.content_length();

        let mut response_builder = Response::builder().status(status);
        let mut response_headers = HashMap::new();
        for (name, value) in upstream_resp.headers() {
            let name_str = name.as_str();
            if is_hop_by_hop(name_str) {
                continue;
            }
            response_builder = response_builder.header(name, value);
            if let Ok(v) = value.to_str() {
                response_headers.insert(name_str.to_string(), v.to_string());
            }
        }

        if !matches!(response_content_length, Some(n) if (n as usize) <= max_buffer) {
            let resp_stream = upstream_resp.bytes_stream();
            let resp = response_builder
                .body(Body::from_stream(resp_stream))
                .map_err(|_| ProxyError::Upstream(StatusCode::INTERNAL_SERVER_ERROR))?;
            return Ok((resp, None));
        }

        let response_bytes = upstream_resp.bytes().await.map_err(|e| {
            let status = upstream_error_status(&e);
            tracing::error!(
                error = %e.without_url(),
                url = %crate::server::redaction::redact_url_credentials(&url),
                "reading observed response body failed"
            );
            ProxyError::Upstream(status)
        })?;
        let resp = response_builder
            .body(Body::from(response_bytes.clone()))
            .map_err(|_| ProxyError::Upstream(StatusCode::INTERNAL_SERVER_ERROR))?;

        let capture = ProxyCaptureRaw {
            request_host: (!original_host.is_empty()).then_some(original_host),
            request_query_params,
            request_headers,
            request_body: body_bytes.to_vec(),
            request_content_type,
            response_status: status.as_u16(),
            response_headers,
            response_body: response_bytes.to_vec(),
            response_content_type,
        };
        Ok((resp, Some(capture)))
    }
}

/// The raw request and response captured by `ProxyClient::forward_with_capture` when both sides could be buffered,
/// complete; truncating them for storage (`server::observation::max_body_size`) is the caller's job.
#[derive(Debug, Clone)]
pub struct ProxyCaptureRaw {
    /// The request's `Host`: Mimicway's own address as the caller reached it, which the relayed request does not carry
    /// (the backend gets its own host, and this one in `X-Forwarded-Host`).
    pub request_host: Option<String>,
    pub request_query_params: HashMap<String, String>,
    pub request_headers: HashMap<String, String>,
    pub request_body: Vec<u8>,
    pub request_content_type: Option<String>,
    pub response_status: u16,
    pub response_headers: HashMap<String, String>,
    pub response_body: Vec<u8>,
    pub response_content_type: Option<String>,
}

/// The backend URL a request is relayed to. One slash is taken from the start of the path, not all: a path that
/// starts with empty segments (`//a`) reaches the backend as the caller wrote it. An empty query (`?`) is kept.
pub(crate) fn target_url(target_base: &str, remaining_path: &str, query: Option<&str>) -> String {
    let path = remaining_path.strip_prefix('/').unwrap_or(remaining_path);
    let base = target_base.trim_end_matches('/');
    match query {
        Some(q) => format!("{base}/{path}?{q}"),
        None => format!("{base}/{path}"),
    }
}

/// Host and port of a URL for the TCP check: the URL's port, else 443 for https and 80 for anything else.
fn parse_host_port(url: &str) -> Result<(url::Host<String>, u16), String> {
    let parsed = url::Url::parse(url).map_err(|e| crate::i18n::tr("invalid URL: {0}", &[&e]))?;
    let host = parsed
        .host()
        .ok_or_else(|| crate::i18n::tr("URL without a host", &[]))?
        .to_owned();
    let port = parsed
        .port()
        .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
    Ok((host, port))
}

fn borrowed(host: &url::Host<String>) -> url::Host<&str> {
    match host {
        url::Host::Domain(name) => url::Host::Domain(name.as_str()),
        url::Host::Ipv4(address) => url::Host::Ipv4(*address),
        url::Host::Ipv6(address) => url::Host::Ipv6(*address),
    }
}

/// 504 when the backend did not answer in time, 502 for any other failure to reach it.
fn upstream_error_status(e: &reqwest::Error) -> StatusCode {
    if e.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    }
}

/// What the relay adds to the request it sends on, after the caller's own headers of the same name: where the caller
/// reached Mimicway, which the backend cannot see otherwise.
fn forwarding_headers(original_host: &str) -> impl Iterator<Item = (&'static str, &str)> {
    let host = (!original_host.is_empty()).then_some(("x-forwarded-host", original_host));
    host.into_iter().chain([("x-forwarded-proto", "http")])
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "host"
            | "connection"
            | "transfer-encoding"
            | "keep-alive"
            | "te"
            | "trailers"
            | "upgrade"
            | "proxy-authorization"
            | "proxy-connection"
    )
}

#[cfg(test)]
mod judge;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_client_creates_successfully() {
        let _client = ProxyClient::new();
    }

    #[tokio::test]
    async fn forward_to_invalid_host_returns_bad_gateway() {
        let client = ProxyClient::new();
        let req = Request::builder()
            .method("GET")
            .uri("/test")
            .body(Body::empty())
            .unwrap();

        let result = client.forward("http://127.0.0.1:1", "/test", req).await;
        assert_eq!(
            result.unwrap_err(),
            ProxyError::Upstream(StatusCode::BAD_GATEWAY)
        );
    }

    #[tokio::test]
    async fn ping_unreachable_host_returns_unreachable() {
        let client = ProxyClient::new();
        let status = client.ping("http://127.0.0.1:1").await;
        assert!(!status.reachable);
        assert!(status.error.is_some());
    }

    #[tokio::test]
    async fn ping_reachable_host_returns_reachable_via_tcp_only() {
        // A bare TCP listener with no HTTP server behind it: an HTTP request would never get a proper answer, so reachable
        // = true shows that only the TCP handshake counts.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let client = ProxyClient::new();
        let status = client.ping(&format!("http://{addr}")).await;
        assert!(status.reachable);
        assert!(status.error.is_none());
    }

    // The instant of a check, whatever its outcome, read against the system clock before and after it.
    #[tokio::test]
    async fn every_ping_is_dated_between_the_clock_readings_around_it() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let reachable = format!("http://{}", listener.local_addr().unwrap());
        let client = ProxyClient::new();
        let clock = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64
        };
        for url in [reachable.as_str(), "http://127.0.0.1:1", "not a url"] {
            let before = clock();
            let status = client.ping(url).await;
            let after = clock();
            assert!(
                (before..=after).contains(&status.checked_at),
                "{url}: {before} <= {} <= {after}",
                status.checked_at
            );
        }
    }

    #[test]
    fn the_read_timeout_setting_is_a_whole_number_of_seconds_above_zero() {
        let cases = [
            (None, 120),
            (Some(""), 120),
            (Some("0"), 120),
            (Some(" 0 "), 120),
            (Some("1"), 1),
            (Some("2"), 2),
            (Some(" 30\n"), 30),
            (Some("120"), 120),
            (Some("-1"), 120),
            (Some("1.5"), 120),
            (Some("5s"), 120),
            (Some("18446744073709551615"), u64::MAX),
            (Some("18446744073709551616"), 120),
        ];
        for (setting, secs) in cases {
            assert_eq!(
                read_timeout(setting),
                Duration::from_secs(secs),
                "{setting:?}"
            );
        }
        // Read from the environment the tests run in, set or not.
        let setting = std::env::var("PROXY_READ_TIMEOUT_SECS").ok();
        assert_eq!(read_timeout_from_env(), read_timeout(setting.as_deref()));
    }

    #[test]
    fn ping_never_issues_http_request() {
        // Parsing only, no network I/O: resolving the host and port never sends a request.
        let (host, port) =
            parse_host_port("http://example.invalid:1234/some/business/path").unwrap();
        assert_eq!(host, url::Host::Domain("example.invalid".to_string()));
        assert_eq!(port, 1234);
    }

    #[test]
    fn parse_host_port_explicit_port() {
        assert_eq!(
            parse_host_port("http://svc.default.svc:9090").unwrap(),
            (url::Host::Domain("svc.default.svc".to_string()), 9090)
        );
    }

    #[test]
    fn parse_host_port_defaults_http_80() {
        assert_eq!(
            parse_host_port("http://svc.default.svc").unwrap(),
            (url::Host::Domain("svc.default.svc".to_string()), 80)
        );
    }

    #[test]
    fn parse_host_port_defaults_https_443() {
        assert_eq!(
            parse_host_port("https://secure.example.com").unwrap(),
            (url::Host::Domain("secure.example.com".to_string()), 443)
        );
    }

    #[test]
    fn parse_host_port_invalid_url_errors() {
        assert!(parse_host_port("not a url at all").is_err());
        assert!(parse_host_port("").is_err());
    }

    #[test]
    fn target_url_joins_base_and_path_with_one_slash() {
        let url = target_url("http://svc.default.svc:8080/", "/api/users", None);
        assert_eq!(url, "http://svc.default.svc:8080/api/users");
        assert_eq!(
            target_url("http://svc:8080", "api/v1", Some("a=1")),
            "http://svc:8080/api/v1?a=1"
        );
    }

    #[test]
    fn target_url_keeps_empty_leading_segments_and_an_empty_query() {
        assert_eq!(target_url("http://svc/", "//a", None), "http://svc//a");
        assert_eq!(target_url("http://svc", "/x", Some("")), "http://svc/x?");
    }

    use crate::server::test_support::capture_one_raw_request;

    #[tokio::test]
    async fn forward_preserves_query_headers_method_and_body() {
        let (port_tx, port_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(capture_one_raw_request(port_tx));
        let port = port_rx.await.unwrap();

        let client = ProxyClient::new();
        let req = Request::builder()
            .method("POST")
            .uri("/svc/foo?a=1&b=two")
            .header("x-custom-header", "custom-value")
            .body(Body::from("payload-body"))
            .unwrap();

        let target_base = format!("http://127.0.0.1:{port}");
        let _ = client.forward(&target_base, "/foo", req).await;

        let raw = server.await.unwrap();

        let request_line = raw.lines().next().unwrap_or("");
        assert!(
            request_line.starts_with("POST "),
            "method not preserved: {request_line}"
        );
        assert!(
            request_line.contains("?a=1&b=two"),
            "query params missing from request line: {request_line}"
        );
        assert!(
            raw.to_lowercase().contains("x-custom-header: custom-value"),
            "custom header missing:\n{raw}"
        );
        // The body is streamed with chunked encoding, which wraps the payload in hexadecimal sizes and a terminator: look
        // for it as a substring.
        assert!(
            raw.contains("payload-body"),
            "body missing/incomplete:\n{raw}"
        );
    }

    // What both relays add, read in the raw request the backend receives: where the caller reached Mimicway (when the
    // request says it), and the scheme, after the caller's own headers of the same names.
    #[tokio::test]
    async fn both_relays_tell_the_backend_where_the_caller_reached_mimicway() {
        for observed in [false, true] {
            for host in [None, Some("mimic.test:7342")] {
                let (port_tx, port_rx) = tokio::sync::oneshot::channel();
                let server = tokio::spawn(capture_one_raw_request(port_tx));
                let target_base = format!("http://127.0.0.1:{}", port_rx.await.unwrap());
                let mut req = Request::builder()
                    .method("GET")
                    .uri("/svc/a")
                    .header("x-forwarded-proto", "https");
                if let Some(host) = host {
                    req = req.header("host", host);
                }
                let req = req.body(Body::empty()).unwrap();
                let client = ProxyClient::new();
                if observed {
                    let _ = client
                        .forward_with_capture(&target_base, "/a", req, 1024)
                        .await;
                } else {
                    let _ = client.forward(&target_base, "/a", req).await;
                }
                let raw = server.await.unwrap().to_ascii_lowercase();
                let lines: Vec<&str> = raw.lines().collect();
                let forwarded_host: Vec<&str> = lines
                    .iter()
                    .filter_map(|line| line.strip_prefix("x-forwarded-host: "))
                    .collect();
                let forwarded_proto: Vec<&str> = lines
                    .iter()
                    .filter_map(|line| line.strip_prefix("x-forwarded-proto: "))
                    .collect();
                let case = format!("observed {observed}, host {host:?}:\n{raw}");
                assert_eq!(
                    forwarded_host,
                    host.into_iter().collect::<Vec<_>>(),
                    "{case}"
                );
                assert_eq!(forwarded_proto, ["https", "http"], "{case}");
            }
        }
    }

    /// A small local Axum server for the `forward_with_capture` tests: they need real HTTP responses with a
    /// Content-Length present or deliberately absent, to reach both the captured and the streamed branches.
    async fn spawn_test_target(
        body: &'static str,
        chunked: bool,
    ) -> (String, tokio::task::JoinHandle<()>) {
        use axum::response::IntoResponse;
        use axum::routing::any;

        async fn respond_fixed(
            axum::extract::State(body): axum::extract::State<&'static str>,
        ) -> impl IntoResponse {
            ([("content-type", "application/json")], body)
        }

        async fn respond_chunked(
            axum::extract::State(body): axum::extract::State<&'static str>,
        ) -> impl IntoResponse {
            let stream = futures_util::stream::once(async move {
                Ok::<_, std::io::Error>(axum::body::Bytes::from_static(body.as_bytes()))
            });
            axum::body::Body::from_stream(stream)
        }

        let app = if chunked {
            axum::Router::new()
                .route("/{*rest}", any(respond_chunked))
                .with_state(body)
        } else {
            axum::Router::new()
                .route("/{*rest}", any(respond_fixed))
                .with_state(body)
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{addr}"), handle)
    }

    #[tokio::test]
    async fn forward_with_capture_captures_small_request_and_response() {
        let (target_base, _server) = spawn_test_target(r#"{"id":1}"#, false).await;
        let client = ProxyClient::new();
        let req = Request::builder()
            .method("POST")
            .uri("/svc/orders?id=1")
            .header("content-type", "application/json")
            .header("content-length", r#"{"q":true}"#.len().to_string())
            .body(Body::from(r#"{"q":true}"#))
            .unwrap();

        let (resp, capture) = client
            .forward_with_capture(&target_base, "/orders", req, 10 * 1024)
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let capture = capture.expect("small request+response must be captured");
        assert_eq!(capture.request_query_params.get("id").unwrap(), "1");
        assert_eq!(capture.request_body, br#"{"q":true}"#);
        assert_eq!(capture.response_status, 200);
        assert_eq!(capture.response_body, br#"{"id":1}"#);
        assert_eq!(
            capture.response_content_type.as_deref(),
            Some("application/json")
        );

        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], br#"{"id":1}"#);
    }

    // The address the caller reached Mimicway at, kept only when the request names one.
    #[tokio::test]
    async fn the_capture_keeps_the_callers_host_only_when_it_has_one() {
        let (target_base, _server) = spawn_test_target("{}", false).await;
        let client = ProxyClient::new();
        for (host, kept) in [
            (None, None),
            (Some(""), None),
            (Some("mimic.test:7342"), Some("mimic.test:7342")),
        ] {
            let mut req = Request::builder().method("GET").uri("/svc/a");
            if let Some(host) = host {
                req = req.header("host", host);
            }
            let (_, capture) = client
                .forward_with_capture(&target_base, "/a", req.body(Body::empty()).unwrap(), 1024)
                .await
                .unwrap();
            assert_eq!(capture.unwrap().request_host.as_deref(), kept, "{host:?}");
        }
    }

    #[tokio::test]
    async fn forward_with_capture_skips_when_request_too_large_for_buffer() {
        let (target_base, _server) = spawn_test_target(r#"{"id":1}"#, false).await;
        let client = ProxyClient::new();
        let req = Request::builder()
            .method("POST")
            .uri("/svc/orders")
            .header("content-length", "10")
            .body(Body::from("0123456789"))
            .unwrap();

        let (resp, capture) = client
            .forward_with_capture(&target_base, "/orders", req, 4)
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            capture.is_none(),
            "a request larger than the buffer must never be captured"
        );
    }

    #[tokio::test]
    async fn forward_with_capture_skips_when_response_has_no_content_length() {
        let (target_base, _server) = spawn_test_target(r#"{"id":1}"#, true).await;
        let client = ProxyClient::new();
        let req = Request::builder()
            .method("GET")
            .uri("/svc/orders")
            .body(Body::empty())
            .unwrap();

        let (resp, capture) = client
            .forward_with_capture(&target_base, "/orders", req, 10 * 1024)
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            capture.is_none(),
            "a response without Content-Length must never be captured"
        );
        // The traffic still reaches the client.
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], br#"{"id":1}"#);
    }

    #[tokio::test]
    async fn a_backend_that_never_answers_gets_a_gateway_timeout() {
        let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = silent.local_addr().unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = silent.accept().await {
                held.push(socket);
            }
        });
        let client = ProxyClient::with_timeouts(
            std::time::Duration::from_secs(2),
            std::time::Duration::from_millis(300),
        );
        let started = std::time::Instant::now();
        let result = client
            .forward(&format!("http://{addr}"), "/x", Request::new(Body::empty()))
            .await;
        assert_eq!(
            result.err(),
            Some(ProxyError::Upstream(StatusCode::GATEWAY_TIMEOUT))
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
}
