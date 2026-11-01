// The judge of the relay: the backend itself, called directly. Each generated request (method, path and query with
// encoded characters, repeated headers, values with bytes beyond ASCII, bodies empty, binary, large or chunked, any
// status, a response sent whole or in chunks) goes to a local backend that answers with what it received, once
// directly and once through each way Mimicway relays: a service in proxy mode, the same service observed for
// suggestions, and a mocked service whose rule says `proxy`. The relayed response must be the direct one, byte for byte,
// in its status, the values of each header in their order, and the body, which holds what the backend received.
//
// The only differences allowed come from `proxy.rs` and the router, never from a list of this test's own:
// - headers that `is_hop_by_hop` names, on both sides, which HTTP lets a proxy change (RFC 9110 §7.6.1);
// - on the request, what `forwarding_headers` adds after the caller's own;
// - on the response, `SERVICE_RESPONSE_POLICY` and the CORS layer's `vary`, which the router appends to every service
//   response; `content-length` is compared as matching the body or not, the relayed echo being longer by what the
//   relay adds.
//
// What it does not see: `OPTIONS`, which the CORS layer answers itself before any service, preflight or not; a request
// with an `Origin`, whose `access-control-*` headers the CORS layer sets over the backend's; the value of
// `content-length` on a response without content (HEAD, 204, 304); the order between headers of different names (not
// meaningful, RFC 9110 §5.3) and the case of header names (both sides are hyper, which writes them in lower case);
// HTTP/2, TLS and the `Expect: 100-continue` exchange; trailers; paths with `.` or `..` segments, which Mimicway
// refuses on purpose; a backend that fails, is slow or closes early (`tests` in `proxy.rs` cover the timeout); a WSDL
// request, relayed by the same `do_proxy` as a service in proxy mode.
use super::{forwarding_headers, is_hop_by_hop};
use crate::server::browser_guard::SERVICE_RESPONSE_POLICY;
use crate::server::test_support::{auth_disabled, serve, temp_data_dir, test_state};
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Request, Response};
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestCaseError, TestRunner};
use std::collections::BTreeMap;

const SEED: u64 = 0x6d69_6d69_6377_6179;
const CASES: u32 = 96;
/// Set by the backend so that hyper does not stamp each response with the current second, which the direct and the
/// relayed calls may straddle.
const BACKEND_DATE: &str = "Sun, 01 Nov 2026 00:00:00 GMT";
const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE"];
const STATUSES: &[u16] = &[200, 201, 204, 301, 304, 400, 404, 418, 500, 503];
const PATH_CHARS: &str = "abcXYZ019-._~!$&'()*+,;=:@";
const QUERY_CHARS: &str = "abcXYZ019-._~!$'()*+,;:@/?";
/// Request headers; `x-resp-<name>` asks the backend to answer `<name>` with the same value.
const HEADERS: &[&str] = &[
    "x-a",
    "x-b",
    "accept",
    "authorization",
    "cookie",
    "content-type",
    "x-forwarded-for",
    "x-forwarded-host",
    "keep-alive",
    "proxy-authorization",
    "x-resp-set-cookie",
    "x-resp-cache-control",
    "x-resp-etag",
    "x-resp-x-a",
    "x-resp-content-type",
    "x-resp-keep-alive",
    "x-resp-content-security-policy",
    "x-resp-vary",
];

type Fields = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone)]
enum BodyShape {
    Empty,
    Small(Vec<u8>),
    /// Expanded from a seed: a body of hundreds of KiB stored in the case would make shrinking slow.
    Large {
        len: usize,
        seed: u64,
    },
    Chunked(Vec<Vec<u8>>),
}

#[derive(Debug, Clone)]
struct Case {
    method: &'static str,
    target: String,
    headers: Vec<(&'static str, Vec<u8>)>,
    body: BodyShape,
    status: u16,
    chunked_response: bool,
}

fn encoded(chars: &'static str) -> impl Strategy<Value = String> {
    let plain = prop::sample::select(chars.chars().collect::<Vec<_>>()).prop_map(String::from);
    let escape = any::<u8>().prop_map(|b| format!("%{b:02X}"));
    prop::collection::vec(prop_oneof![4 => plain, 1 => escape], 0..6).prop_map(|v| v.concat())
}

fn arb_target() -> impl Strategy<Value = String> {
    let segment = encoded(PATH_CHARS).prop_filter("dot segment", |s| {
        !crate::server::validation::has_dot_segment(s)
    });
    let pair = (encoded(QUERY_CHARS), prop::option::of(encoded(QUERY_CHARS)))
        .prop_map(|(k, v)| v.map_or(k.clone(), |v| format!("{k}={v}")));
    let query = prop::option::of(prop::collection::vec(pair, 0..4).prop_map(|p| p.join("&")));
    (prop::collection::vec(segment, 1..4), query).prop_map(|(segments, query)| {
        let path = format!("/{}", segments.join("/"));
        query.map_or(path.clone(), |q| format!("{path}?{q}"))
    })
}

fn arb_value() -> impl Strategy<Value = Vec<u8>> {
    let byte = prop_oneof![8 => 0x21u8..0x7f, 1 => Just(b' '), 1 => 0x80u8..=0xff];
    prop::collection::vec(byte, 1..12).prop_map(|mut v| {
        while v.last() == Some(&b' ') {
            v.pop();
        }
        while v.first() == Some(&b' ') {
            v.remove(0);
        }
        if v.is_empty() { b"v".to_vec() } else { v }
    })
}

fn arb_body() -> impl Strategy<Value = BodyShape> {
    let bytes = || prop::collection::vec(any::<u8>(), 1..64);
    prop_oneof![
        3 => Just(BodyShape::Empty),
        3 => bytes().prop_map(BodyShape::Small),
        1 => (64 * 1024..512 * 1024usize, any::<u64>()).prop_map(|(len, seed)| BodyShape::Large { len, seed }),
        2 => prop::collection::vec(bytes(), 0..4).prop_map(BodyShape::Chunked),
    ]
}

fn arb_case() -> impl Strategy<Value = Case> {
    (
        prop::sample::select(METHODS),
        arb_target(),
        prop::collection::vec((prop::sample::select(HEADERS), arb_value()), 0..8),
        arb_body(),
        prop::sample::select(STATUSES),
        any::<bool>(),
    )
        .prop_map(
            |(method, target, headers, body, status, chunked_response)| Case {
                method,
                target,
                headers,
                body,
                status,
                chunked_response,
            },
        )
}

/// Header values grouped by name, without the hop-by-hop ones, values in the order received. A value is written with
/// its bytes beyond ASCII escaped, which keeps it exact and readable in a failure.
fn fields(headers: &HeaderMap) -> Fields {
    let mut out = Fields::new();
    for (name, value) in headers {
        if !is_hop_by_hop(name.as_str()) {
            out.entry(name.as_str().to_string())
                .or_default()
                .push(value.as_bytes().escape_ascii().to_string());
        }
    }
    out
}

/// The backend: its body is what it received (method, target, headers, body), its status and headers what the request
/// asked for.
async fn echo(req: Request<Body>) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    let mut out = format!("{}\n{}\n", parts.method, parts.uri).into_bytes();
    for (name, values) in fields(&parts.headers) {
        for value in values {
            out.extend_from_slice(format!("{name}\t{value}\n").as_bytes());
        }
    }
    out.push(b'\n');
    out.extend_from_slice(&body);

    let status = parts
        .headers
        .get("x-echo-status")
        .and_then(|v| v.to_str().ok()?.parse::<u16>().ok())
        .unwrap_or(200);
    let mut resp = Response::builder()
        .status(status)
        .header("date", BACKEND_DATE);
    for (name, value) in &parts.headers {
        if let Some(asked) = name.as_str().strip_prefix("x-resp-") {
            resp = resp.header(asked, value.clone());
        }
    }
    let body = if parts.headers.contains_key("x-echo-chunked") {
        let chunks: Vec<Result<Bytes, std::io::Error>> = out
            .chunks(out.len().div_ceil(3).max(1))
            .map(|c| Ok(Bytes::copy_from_slice(c)))
            .collect();
        Body::from_stream(futures_util::stream::iter(chunks))
    } else {
        Body::from(out)
    };
    resp.body(body).unwrap()
}

/// What the backend reported receiving, read back from its body.
#[derive(PartialEq)]
struct Received {
    request_line: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Received {
    fn parse(body: &[u8]) -> Option<Self> {
        let end = body.windows(2).position(|w| w == b"\n\n")?;
        let head = std::str::from_utf8(&body[..end]).ok()?;
        let mut lines = head.lines();
        let request_line = format!("{} {}", lines.next()?, lines.next()?);
        let headers = lines
            .map(|l| {
                l.split_once('\t')
                    .map(|(n, v)| (n.to_string(), v.to_string()))
            })
            .collect::<Option<_>>()?;
        Some(Self {
            request_line,
            headers,
            body: body[end + 2..].to_vec(),
        })
    }
}

impl std::fmt::Debug for Received {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Received")
            .field("request_line", &self.request_line)
            .field("headers", &self.headers)
            .field("body", &Summary(&self.body))
            .finish()
    }
}

/// A body shown by its length and first bytes: a full dump of 500 KiB hides the difference.
struct Summary<'a>(&'a [u8]);

impl std::fmt::Debug for Summary<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head = &self.0[..self.0.len().min(48)];
        write!(
            f,
            "{} bytes {:?}",
            self.0.len(),
            String::from_utf8_lossy(head)
        )
    }
}

#[derive(Debug, PartialEq)]
enum Content {
    Echo(Received),
    /// A status without body (HEAD, 204, 304), or a body that is not an echo.
    Other(Vec<u8>),
}

#[derive(Debug, PartialEq)]
struct Observed {
    status: u16,
    /// `content-length` is replaced by whether it matches the body: the relayed echo is longer by what the relay adds.
    headers: Fields,
    content: Content,
}

struct Setup {
    client: reqwest::Client,
    backend: String,
    mimicway: String,
}

impl Setup {
    async fn start() -> Self {
        let backend = serve(axum::Router::new().fallback(echo)).await;
        let rules: Vec<_> = METHODS
            .iter()
            .map(|m| {
                serde_json::json!({
                    "name": format!("relay-{m}"), "method": m, "sub_path": null, "action": "proxy",
                    "script": null, "conditions": {"all_of": [], "any_of": []},
                    "response": {"status": 200, "headers": [], "body": []}
                })
            })
            .collect();
        let service = |name: &str, mocked: bool, rules: &[serde_json::Value]| {
            serde_json::json!({
                "name": name, "listen_path": "", "real_target_url": backend, "is_mocked": mocked,
                "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "mock", "rules": rules
            })
        };
        let config = serde_json::from_value(serde_json::json!({
            "services": [service("relay", false, &[]), service("observed", false, &[]), service("ruled", true, &rules)],
            "groups": []
        }))
        .unwrap();
        let data_dir = temp_data_dir("proxy-judge");
        let state = test_state(&data_dir, config, auth_disabled()).await;
        state.observation.toggle.enable(None, "observed");
        let mimicway = serve(crate::server::build_router(state, &data_dir)).await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap();
        Self {
            client,
            backend,
            mimicway,
        }
    }

    async fn send(&self, base: &str, case: &Case) -> Result<Observed, TestCaseError> {
        let method = reqwest::Method::from_bytes(case.method.as_bytes()).unwrap();
        let mut req = self
            .client
            .request(method, format!("{base}{}", case.target))
            .header("x-echo-status", case.status.to_string());
        if case.chunked_response {
            req = req.header("x-echo-chunked", "1");
        }
        for (name, value) in &case.headers {
            req = req.header(
                *name,
                reqwest::header::HeaderValue::from_bytes(value).unwrap(),
            );
        }
        req = match &case.body {
            BodyShape::Empty => req,
            BodyShape::Small(bytes) => req.body(bytes.clone()),
            BodyShape::Large { len, seed } => {
                let mut rng = fastrand::Rng::with_seed(*seed);
                req.body((0..*len).map(|_| rng.u8(..)).collect::<Vec<u8>>())
            }
            BodyShape::Chunked(chunks) => {
                let chunks: Vec<Result<Vec<u8>, std::io::Error>> =
                    chunks.iter().cloned().map(Ok).collect();
                req.body(reqwest::Body::wrap_stream(futures_util::stream::iter(
                    chunks,
                )))
            }
        };
        let resp = req
            .send()
            .await
            .map_err(|e| TestCaseError::fail(format!("{base}: {e}")))?;
        let status = resp.status().as_u16();
        let mut headers = fields(resp.headers());
        let body = resp
            .bytes()
            .await
            .map_err(|e| TestCaseError::fail(format!("{base}: body: {e}")))?;
        // A response without content by definition (RFC 9110 §8.6): the value describes no body sent, and hyper on the
        // backend side even writes one on a 204, which the observed relay, buffering an empty body, leaves out.
        if case.method == "HEAD" || matches!(status, 204 | 304) {
            headers.remove("content-length");
        }
        if let Some(lengths) = headers.get_mut("content-length") {
            for length in lengths {
                let matches = *length == body.len().to_string();
                *length = if matches { "=body" } else { "!=body" }.into();
            }
        }
        let content = match Received::parse(&body) {
            Some(received) => Content::Echo(received),
            None => Content::Other(body.to_vec()),
        };
        Ok(Observed {
            status,
            headers,
            content,
        })
    }

    /// The direct response, changed only as `proxy.rs` and the router say a relay may change it.
    fn expected_relayed(&self, mut direct: Observed) -> Observed {
        let headers = &mut direct.headers;
        let mut append =
            |name: &str, value: String| headers.entry(name.into()).or_default().push(value);
        append("content-security-policy", SERVICE_RESPONSE_POLICY.into());
        // The CORS layer's `vary`, appended to every response: tower-http's default, which `cors_layer` keeps.
        let vary: Vec<_> = tower_http::cors::preflight_request_headers()
            .map(|h| h.to_string())
            .collect();
        append("vary", vary.join(", "));
        if let Content::Echo(received) = &mut direct.content {
            let host = self.mimicway.trim_start_matches("http://");
            let mut added: Vec<_> = forwarding_headers(host)
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect();
            // The backend lists names in order; the added values come after the caller's values of the same name.
            for (name, value) in added.drain(..) {
                let at = received
                    .headers
                    .iter()
                    .rposition(|(n, _)| *n <= name)
                    .map_or(0, |i| i + 1);
                received.headers.insert(at, (name, value));
            }
        }
        direct
    }

    async fn check(&self, case: &Case) -> Result<(), TestCaseError> {
        let direct = self.send(&self.backend, case).await?;
        prop_assert_eq!(
            direct.status,
            case.status,
            "the backend itself did not answer as asked"
        );
        let expected = self.expected_relayed(direct);
        for service in ["relay", "observed", "ruled"] {
            let relayed = self
                .send(&format!("{}/{service}", self.mimicway), case)
                .await?;
            prop_assert_eq!(&relayed, &expected, "through `{}`", service);
        }
        Ok(())
    }
}

#[test]
fn every_relay_answers_what_the_backend_answers_directly() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let setup = rt.block_on(Setup::start());
    let config = Config {
        cases: CASES,
        rng_seed: RngSeed::Fixed(SEED),
        failure_persistence: None,
        ..Config::default()
    };
    let result = TestRunner::new(config).run(&arb_case(), |case| rt.block_on(setup.check(&case)));
    if let Err(e) = result {
        panic!("{e}");
    }
}
