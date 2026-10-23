//! Browser-facing protections of the management API (`/api/...`).
//!
//! Any website open in a browser can send requests to a Mimicway reachable from that machine (often `localhost`,
//! often without authentication during development). These rules keep such a page from reading or changing the
//! configuration, while the mocked and proxied services stay callable from any origin, which is their purpose:
//! - CORS: the management API answers cross-origin calls only for the origins listed in `CORS_ALLOWED_ORIGINS`
//!   (none by default; only needed when the UI is served from another origin, see `API_BASE_URL`).
//! - Cross-site writes: a state-changing API request that the browser itself marks as coming from another site
//!   (`Sec-Fetch-Site`) is refused unless its `Origin` is listed. This also covers the requests that a browser
//!   sends without a CORS preflight (a form `POST`, for instance). A browser that sends no `Sec-Fetch-Site` is judged
//!   by its `Origin`: refused unless it is the address called or a listed origin.
//! - DNS rebinding: a page can reach a Mimicway that only its visitor's machine or network reaches by making its own
//!   domain name resolve to that address; the browser then treats the calls as same-origin, and they carry the
//!   attacker's name in `Host`. The management API therefore answers only requests addressed to a loopback name
//!   (`localhost`, `*.localhost`), to an IP address, which no DNS answer can put in `Host`, or to a name listed in
//!   `ALLOWED_HOSTS`. Any name is answered only when `ALLOWED_HOSTS` is `*`, or when it is unset on a server that
//!   listens beyond loopback with authentication on: there the operator chose the names, and a page cannot obtain
//!   a user's token.
//! - Response headers: Mimicway's own pages and API answers carry a content security policy (no inline script,
//!   no framing), `nosniff` and `no-referrer`. Responses of the mocked and proxied services keep the headers and body
//!   configured or sent by the backend, which the applications under test expect, plus one policy: rendered as a
//!   page, they run in an opaque origin (`SERVICE_RESPONSE_POLICY`).
use crate::i18n::tr;
use crate::server::validation::is_management_api_route;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

#[derive(Debug, Clone, Default)]
pub struct BrowserGuard {
    /// Origins allowed to call the management API from a browser, besides the UI's own origin.
    allowed_origins: Arc<Vec<HeaderValue>>,
    /// Host names the management API answers to, besides loopback names and IP addresses; `None`: any name.
    host_names: Option<Arc<Vec<String>>>,
    /// Origin of `API_BASE_URL`, the only other place the UI may call.
    api_origin: Option<String>,
    /// Host name of `API_BASE_URL`, which the management API answers to whenever it checks names.
    api_host: Option<String>,
}

/// Put by the proxy/mock pipeline on the responses of a service, which the headers below must not alter.
#[derive(Clone, Copy)]
pub struct ServiceResponse;

impl BrowserGuard {
    pub fn from_env() -> Self {
        let mut guard = Self::new(&std::env::var("CORS_ALLOWED_ORIGINS").unwrap_or_default());
        let api_url = std::env::var("API_BASE_URL")
            .ok()
            .and_then(|u| url::Url::parse(u.trim()).ok());
        guard.api_origin = api_url
            .as_ref()
            .map(|u| u.origin().ascii_serialization())
            .filter(|o| o != "null");
        guard.api_host = api_url.as_ref().and_then(|u| u.host_str()).map(host_name);
        guard
    }

    /// `raw` is a comma-separated list of origins (`https://ui.example.com,http://localhost:5173`); a trailing slash
    /// is ignored because browsers never send one in `Origin`.
    pub fn new(raw: &str) -> Self {
        let origins = raw
            .split(',')
            .map(|o| o.trim().trim_end_matches('/'))
            .filter(|o| !o.is_empty())
            .filter_map(|o| match HeaderValue::from_str(o) {
                Ok(v) => Some(v),
                Err(_) => {
                    tracing::warn!(
                        origin = o,
                        "CORS_ALLOWED_ORIGINS: ignoring an invalid origin"
                    );
                    None
                }
            })
            .collect();
        Self {
            allowed_origins: Arc::new(origins),
            host_names: None,
            api_origin: None,
            api_host: None,
        }
    }

    /// Sets the host names the management API answers to, from `allowed_hosts` (`ALLOWED_HOSTS`: names separated by
    /// commas, or `*` for any name), whether the server listens on loopback only, and whether authentication is on.
    /// Unset, no name besides loopback names, IP addresses and the host of `API_BASE_URL`, except beyond loopback
    /// with authentication on (see the module documentation).
    pub fn with_host_check(
        mut self,
        allowed_hosts: Option<&str>,
        loopback_only: bool,
        auth_enabled: bool,
    ) -> Self {
        let raw = allowed_hosts.map(str::trim).unwrap_or_default();
        self.host_names = if raw == "*" || (raw.is_empty() && !loopback_only && auth_enabled) {
            None
        } else {
            let names = raw
                .split(',')
                .map(host_name)
                .filter(|name| !name.is_empty())
                .chain(self.api_host.clone());
            Some(Arc::new(names.collect()))
        };
        self
    }

    /// The names checked by `with_host_check`, besides loopback names and IP addresses; `None` when any name is
    /// answered.
    pub fn host_names(&self) -> Option<&[String]> {
        self.host_names.as_deref().map(Vec::as_slice)
    }

    fn allows(&self, origin: &HeaderValue) -> bool {
        self.allowed_origins.iter().any(|o| o == origin)
    }

    pub fn cors_layer(&self) -> CorsLayer {
        let guard = self.clone();
        CorsLayer::new()
            .allow_origin(AllowOrigin::predicate(move |origin, parts| {
                !is_management_api_route(parts.uri.path()) || guard.allows(origin)
            }))
            .allow_methods(Any)
            .allow_headers(Any)
    }

    /// Why the request must be refused, if it must.
    fn refusal(&self, method: &Method, uri: &Uri, headers: &HeaderMap) -> Option<String> {
        if !is_management_api_route(uri.path()) {
            return None;
        }
        // HTTP/2 carries the host in the `:authority` pseudo-header, which lands in the URI, not in `Host`.
        let host = match uri.authority() {
            Some(authority) => Some(authority.as_str().rsplit('@').next().unwrap_or_default()),
            None => headers.get("host").and_then(|h| h.to_str().ok()),
        };
        if let Some(names) = &self.host_names {
            // A request that names no host at all is refused rather than waved through.
            let answered = host.map(host_name).is_some_and(|name| {
                is_loopback_name(&name)
                    || name.parse::<std::net::IpAddr>().is_ok()
                    || names.contains(&name)
            });
            if !answered {
                return Some(tr(
                    "Request refused: this Mimicway does not answer to the name \"{0}\"; call it through localhost or an IP address, or add the name to ALLOWED_HOSTS.",
                    &[&host.unwrap_or_default()],
                ));
            }
        }
        let origin = headers.get("origin");
        let cross_site = match headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
            Some(site) => matches!(site, "cross-site" | "same-site"),
            // Browsers that predate Fetch Metadata (Safari before 16.4, older Firefox) still send `Origin` on a
            // cross-origin write, form or fetch: judge by it. `null` (a sandboxed page, a service response among them)
            // is never the request's own. A client without `Origin` is not a browser.
            None => origin.is_some_and(|o| !is_own_origin(o, host)),
        };
        if !is_safe_method(method) && cross_site && !origin.is_some_and(|o| self.allows(o)) {
            return Some(tr(
                "Cross-site request refused: add this origin to CORS_ALLOWED_ORIGINS to allow it.",
                &[],
            ));
        }
        None
    }
}

/// Whether `origin` (`scheme://host[:port]`) names the authority the request was sent to, `host`, its default port
/// written or not. The scheme is not compared: behind a proxy that ends TLS, the request arrives in plain HTTP.
fn is_own_origin(origin: &HeaderValue, host: Option<&str>) -> bool {
    let (Ok(origin), Some(host)) = (origin.to_str(), host) else {
        return false;
    };
    let Some((scheme, authority)) = origin.split_once("://") else {
        return false;
    };
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => ":80",
        "https" => ":443",
        _ => return false,
    };
    let host = host.trim();
    authority.eq_ignore_ascii_case(host)
        || host
            .strip_suffix(default_port)
            .is_some_and(|bare| authority.eq_ignore_ascii_case(bare))
}

fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// The name or IP address of `host`, a `Host` header value or a list entry with an optional port (`[::1]:7342`),
/// in lower case, without port, brackets or final dot.
fn host_name(host: &str) -> String {
    let host = host.trim();
    match host.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(""),
        None => host.rsplit_once(':').map_or(host, |(name, _port)| name),
    }
    .trim_end_matches('.')
    .to_ascii_lowercase()
}

/// `name` comes from `host_name`. IP addresses are answered whatever they are, loopback ones included.
fn is_loopback_name(name: &str) -> bool {
    name == "localhost" || name.ends_with(".localhost")
}

impl BrowserGuard {
    fn content_security_policy(&self) -> String {
        let connect = match &self.api_origin {
            Some(origin) => format!("'self' {origin}"),
            None => "'self'".to_string(),
        };
        // Svelte sets inline style attributes, hence 'unsafe-inline' for styles only; scripts are files.
        format!(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
             font-src 'self' data:; connect-src {connect}; object-src 'none'; base-uri 'self'; \
             form-action 'self'; frame-ancestors 'none'"
        )
    }
}

/// Added to every service response. Such a page shares the interface's origin, and an editor, a proxied backend or a
/// request value reflected by a template writes it: without this, its scripts would read the interface's token and
/// call the management API as same-origin. The `sandbox` directive renders it in an opaque origin instead, where its
/// calls to the API leave from origin `null`, marked cross-site. Everything a page may need stays allowed except
/// `allow-same-origin` and `allow-popups-to-escape-sandbox`, which would give that origin back. Clients that do not
/// render the response as a page ignore it.
pub const SERVICE_RESPONSE_POLICY: &str =
    "sandbox allow-downloads allow-forms allow-modals allow-popups allow-scripts";

pub async fn security_headers(
    State(guard): State<BrowserGuard>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(req).await;
    if response.extensions().get::<ServiceResponse>().is_some() {
        // Appended: a policy set by the rule or the backend still applies, browsers enforcing every policy received.
        response.headers_mut().append(
            "content-security-policy",
            HeaderValue::from_static(SERVICE_RESPONSE_POLICY),
        );
        return response;
    }
    let headers = response.headers_mut();
    let defaults = [
        ("content-security-policy", guard.content_security_policy()),
        ("x-content-type-options", "nosniff".to_string()),
        ("x-frame-options", "DENY".to_string()),
        ("referrer-policy", "no-referrer".to_string()),
    ];
    for (name, value) in defaults {
        if !headers.contains_key(name)
            && let Ok(value) = HeaderValue::from_str(&value)
        {
            headers.insert(name, value);
        }
    }
    response
}

pub async fn management_api_guard(
    State(guard): State<BrowserGuard>,
    req: Request<Body>,
    next: Next,
) -> Response {
    if let Some(reason) = guard.refusal(req.method(), req.uri(), req.headers()) {
        tracing::warn!(
            method = %req.method(),
            path = %req.uri().path(),
            origin = ?req.headers().get("origin"),
            host = ?req.headers().get("host"),
            "management API request refused by the browser guard"
        );
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({ "error": reason })),
        )
            .into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests;
