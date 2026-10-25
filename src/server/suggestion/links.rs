//! The other backends an observed backend points to. Many answer with addresses: a service directory giving the address
//! of each service, links between resources, a redirect, the address in a WSDL. The application then calls those
//! addresses directly, past Mimicway. From the observed responses (bodies, and the headers that carry an address), this
//! module lists those addresses by origin, and rewrites the suggested rules so that they send the application to the
//! Mimicway service that stands in front of each backend, when one does.
//!
//! Only suggested rules are rewritten, never the relayed traffic: observation never changes what the proxy relays.
//!
//! The scanned text comes from a real backend, possibly hostile: an address is only ever copied into `Literal`
//! fragments, and the one template a rewritten rule gains is written here (`MIMICWAY_BASE`), never taken from the
//! traffic.
use super::{EndpointSuggestion, Suggestion, observed_routes};
use crate::models::{BodyFragment, HeaderEntry, MockConfig, MockResponse};
use crate::server::observation::{ObservationKey, ObservedExchange};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use url::{Host, Url};

/// Addresses and routes listed per origin, as examples.
const MAX_EXAMPLES: usize = 5;
/// Origins listed for one service: the input is already bounded, this bounds the answer.
const MAX_ORIGINS: usize = 50;
/// Longest attribute name read before a quote (`xsi:noNamespaceSchemaLocation`, with room for a long prefix).
const MAX_ATTRIBUTE_NAME: usize = 64;

/// Response headers that carry an address: `Location` and `Content-Location` (RFC 9110 §10.2.2, §8.7), `Link`
/// (RFC 8288).
const ADDRESS_HEADERS: &[&str] = &["location", "content-location", "link"];

/// Mimicway's own address in a rewritten body, written from the request the rule answers, so that the rule keeps working
/// whatever host name and port the application uses: `X-Forwarded-Proto` when a TLS proxy stands in front, `http`
/// otherwise (Mimicway itself only listens in http).
const MIMICWAY_BASE: &str = r#"{{header.x-forwarded-proto | default("http")}}://{{header.host}}"#;

/// A service of the configuration, by its identity (group name, name), as the API names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct ServiceRef {
    pub name: String,
    pub group_name: Option<String>,
}

/// An absolute http(s) address found in a text, with its place in it.
struct Address<'a> {
    range: Range<usize>,
    text: &'a str,
    url: Url,
}

/// A byte that ends an address in a body or a header: whitespace and controls, the quotes and brackets that delimit an
/// address in JSON, XML, HTML or a `Link` header, and the characters RFC 3986 never allows unescaped. A non-ASCII byte
/// does not end it, so an end is always on a character boundary.
fn ends_address(b: u8) -> bool {
    b <= b' '
        || matches!(
            b,
            b'"' | b'\'' | b'<' | b'>' | b'`' | b'\\' | b'{' | b'}' | b'|' | b'^' | 0x7f
        )
}

/// The length of `http://` or `https://` (any case) at `i`, unless it is the end of a longer scheme (`xhttp://`).
fn scheme_at(bytes: &[u8], i: usize) -> Option<usize> {
    if i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || matches!(bytes[i - 1], b'+' | b'-' | b'.'))
    {
        return None;
    }
    let rest = &bytes[i..];
    [&b"https://"[..], b"http://"]
        .into_iter()
        .find(|scheme| {
            rest.len() >= scheme.len() && rest[..scheme.len()].eq_ignore_ascii_case(scheme)
        })
        .map(<[u8]>::len)
}

/// The absolute http(s) addresses in `text`, in order, leaving out XML namespace and schema addresses: they name a
/// vocabulary, not a backend the application calls. One pass: the last quote seen, and whether only address characters
/// followed it, are kept along the way, so a body full of addresses costs no more than its length.
fn find_addresses(text: &str) -> Vec<Address<'_>> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    // The last quote, whether nothing but address text followed it, and (once asked) whether it opens the value of a
    // namespace or schema attribute.
    let mut quote: Option<usize> = None;
    let mut clean_since_quote = false;
    let mut opens_namespace: Option<bool> = None;
    let mut i = 0;
    while i < bytes.len() {
        let Some(scheme_len) = scheme_at(bytes, i) else {
            match bytes[i] {
                b'"' | b'\'' => {
                    quote = Some(i);
                    clean_since_quote = true;
                    opens_namespace = None;
                }
                b'<' | b'>' | b'=' => clean_since_quote = false,
                _ => {}
            }
            i += 1;
            continue;
        };
        let mut end = i + scheme_len;
        while end < bytes.len() && !ends_address(bytes[end]) {
            end += 1;
        }
        // Punctuation that ends a sentence around an address is not part of it.
        while end > i + scheme_len
            && matches!(bytes[end - 1], b'.' | b',' | b';' | b':' | b'!' | b'?')
        {
            end -= 1;
        }
        let candidate = &text[i..end];
        // `http:///path` has no authority, though URL parsing would read `path` as its host.
        let has_authority =
            end > i + scheme_len && !matches!(bytes[i + scheme_len], b'/' | b'?' | b'#');
        let in_namespace = clean_since_quote
            && quote.is_some_and(|q| {
                *opens_namespace.get_or_insert_with(|| namespace_value_at(text, q))
            });
        if has_authority
            && !in_namespace
            && let Ok(url) = Url::parse(candidate)
            && url.host_str().is_some_and(|h| !h.is_empty())
        {
            found.push(Address {
                range: i..end,
                text: candidate,
                url,
            });
        }
        if candidate.contains('=') {
            clean_since_quote = false;
        }
        i = end.max(i + 1);
    }
    found
}

/// Whether the quote at `quote` opens the value of an attribute that names a namespace or a schema: `xmlns`,
/// `xmlns:…`, `targetNamespace`, `namespace`, `xsi:schemaLocation` and `xsi:noNamespaceSchemaLocation` (whatever the
/// prefix). A plain `schemaLocation` (`xsd:import`) is an address a client may fetch, and stays.
fn namespace_value_at(text: &str, quote: usize) -> bool {
    let Some(name_end) = text[..quote].trim_end().strip_suffix('=') else {
        return false;
    };
    let name_end = name_end.trim_end();
    // Counted in bytes over ASCII name characters only, so the name starts on a character boundary whatever precedes
    // it, and at most one byte past the longest name is read.
    let name_len = name_end
        .bytes()
        .rev()
        .take(MAX_ATTRIBUTE_NAME + 1)
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-'))
        .count();
    let name = &name_end[name_end.len() - name_len..];
    name.len() <= MAX_ATTRIBUTE_NAME
        && (name == "xmlns"
            || name.starts_with("xmlns:")
            || name == "targetNamespace"
            || name == "namespace"
            || name.ends_with(":schemaLocation")
            || name.ends_with(":noNamespaceSchemaLocation"))
}

/// Whether `url` is Mimicway's own address as the caller reached it (`host`, the request's `Host`).
fn is_own(url: &Url, host: Option<&str>) -> bool {
    host.and_then(|h| Url::parse(&format!("{}://{h}", url.scheme())).ok())
        .is_some_and(|own| {
            own.host() == url.host() && own.port_or_known_default() == url.port_or_known_default()
        })
}

/// The text of an address after its authority: the path, query and fragment exactly as written.
fn after_authority(text: &str) -> &str {
    let authority = text.find("://").map_or(0, |p| p + 3);
    let end = text[authority..]
        .find(['/', '?', '#'])
        .map_or(text.len(), |p| authority + p);
    &text[end..]
}

/// A service whose proxy relays to a target: what reaches its address in Mimicway reaches the target.
struct Cover {
    service: ServiceRef,
    /// The path of the service in Mimicway (`/{group code}/{name}`, or `/{name}`), which the proxy strips before
    /// appending the rest to the target (`intercept.rs::do_proxy`).
    prefix: String,
    target: Url,
    /// The target's path, without its trailing `/`: the start of every address it covers.
    target_path: String,
}

/// The services of a configuration, to tell which of them a backend's address reaches.
pub struct Covers {
    covers: Vec<Cover>,
    /// The effective pattern of every service, in the order the interception tries them, with the index of its cover.
    patterns: Vec<(String, Option<usize>)>,
}

impl Covers {
    pub fn from_config(config: &MockConfig) -> Self {
        let mut covers = Vec::new();
        let mut patterns = Vec::new();
        for service in &config.services {
            let code = service.group_name.as_ref().and_then(|name| {
                config
                    .groups
                    .iter()
                    .find(|g| &g.name == name)
                    .map(|g| g.code.as_str())
                    .filter(|c| !c.trim().is_empty())
            });
            let pattern = crate::server::intercept::build_effective_pattern(
                code,
                &service.name,
                &service.listen_path,
            );
            let cover = Url::parse(service.real_target_url.trim())
                .ok()
                .filter(|t| matches!(t.scheme(), "http" | "https") && t.host_str().is_some())
                .map(|target| Cover {
                    service: ServiceRef {
                        name: service.name.clone(),
                        group_name: service.group_name.clone(),
                    },
                    prefix: match code {
                        Some(code) => format!("/{code}/{}", service.name),
                        None => format!("/{}", service.name),
                    },
                    target_path: target.path().trim_end_matches('/').to_string(),
                    target,
                });
            patterns.push((pattern, cover.as_ref().map(|_| covers.len())));
            covers.extend(cover);
        }
        Self { covers, patterns }
    }

    /// The service that reaches `address` and the text of the address after that service's target, when there is one:
    /// same scheme, host and port as its target, a path that starts with the target's on a segment boundary, and an
    /// address in Mimicway that the interception routes to that very service (its listen path accepts the rest, and no
    /// service tried before takes it).
    fn find<'a>(&self, address: &Address<'a>) -> Option<(&Cover, &'a str)> {
        if !address.url.username().is_empty() || address.url.password().is_some() {
            return None;
        }
        let path_and_rest = after_authority(address.text);
        self.covers.iter().enumerate().find_map(|(index, cover)| {
            if cover.target.scheme() != address.url.scheme()
                || cover.target.host() != address.url.host()
                || cover.target.port_or_known_default() != address.url.port_or_known_default()
            {
                return None;
            }
            let rest = path_and_rest.strip_prefix(cover.target_path.as_str())?;
            if !(rest.is_empty() || rest.starts_with(['/', '?', '#'])) {
                return None;
            }
            let path = rest.find(['?', '#']).map_or(rest, |end| &rest[..end]);
            let in_mimicway = format!("{}{path}", cover.prefix);
            let routed = self.patterns.iter().find(|(pattern, _)| {
                crate::engine::matcher::match_path(pattern, &in_mimicway).is_some()
            })?;
            (routed.1 == Some(index)).then_some((cover, rest))
        })
    }
}

/// Mimicway's address as the observed caller reached it, for the headers of a rewritten rule. A header keeps the
/// address seen while observing, written as literal text like the rest of a suggested header value.
pub struct OwnAddress {
    proto: &'static str,
    host: String,
}

impl OwnAddress {
    /// From the latest observed call that carried a `Host` a URL can hold.
    pub fn latest(endpoints: &[(ObservationKey, Vec<ObservedExchange>)]) -> Option<Self> {
        let exchange = endpoints
            .iter()
            .flat_map(|(_, exchanges)| exchanges)
            .filter(|e| e.request_host.is_some())
            .max_by_key(|e| e.timestamp)?;
        let host = exchange.request_host.as_deref()?;
        let parsed = Url::parse(&format!("http://{host}")).ok()?;
        if parsed.path() != "/" || !parsed.username().is_empty() || parsed.query().is_some() {
            return None;
        }
        let proto = match exchange
            .request_headers
            .get("x-forwarded-proto")
            .map(String::as_str)
        {
            Some("https") => "https",
            _ => "http",
        };
        Some(Self {
            proto,
            host: host.to_string(),
        })
    }
}

/// Appends literal text, to the last fragment when it is literal too.
fn push_literal(body: &mut Vec<BodyFragment>, text: &str) {
    if text.is_empty() {
        return;
    }
    match body.last_mut() {
        Some(BodyFragment::Literal { value }) => value.push_str(text),
        _ => body.push(BodyFragment::Literal {
            value: text.to_string(),
        }),
    }
}

/// `text` with the covered addresses rewritten: each starts with `MIMICWAY_BASE` in a template fragment of its own,
/// then the service's path and the rest of the address in literal text. `cut` says the text stops at a template
/// fragment (an echo rule's value): an address that reaches its end may go on in that value, so it is rewritten only
/// when the target's part of it is followed by a separator within the text.
fn redirect_text(
    text: &str,
    covers: &Covers,
    cut: bool,
    out: &mut Vec<BodyFragment>,
    redirected: &mut BTreeSet<ServiceRef>,
) {
    let mut copied = 0;
    for address in find_addresses(text) {
        let Some((cover, rest)) = covers.find(&address) else {
            continue;
        };
        if cut && address.range.end == text.len() && rest.is_empty() {
            continue;
        }
        push_literal(out, &text[copied..address.range.start]);
        out.push(BodyFragment::Template {
            template: MIMICWAY_BASE.to_string(),
        });
        push_literal(out, &cover.prefix);
        push_literal(out, rest);
        copied = address.range.end;
        redirected.insert(cover.service.clone());
    }
    push_literal(out, &text[copied..]);
}

/// A header value with its covered addresses written as Mimicway's address seen while observing.
fn redirect_header(
    value: &str,
    covers: &Covers,
    own: &OwnAddress,
    redirected: &mut BTreeSet<ServiceRef>,
) -> String {
    let mut out = String::with_capacity(value.len());
    let mut copied = 0;
    for address in find_addresses(value) {
        let Some((cover, rest)) = covers.find(&address) else {
            continue;
        };
        out.push_str(&value[copied..address.range.start]);
        out.push_str(&crate::engine::template::literal_template(&format!(
            "{}://{}{}{rest}",
            own.proto, own.host, cover.prefix
        )));
        copied = address.range.end;
        redirected.insert(cover.service.clone());
    }
    out.push_str(&value[copied..]);
    out
}

/// A suggested response with the addresses that a service covers sent to that service in Mimicway; returns those
/// services. Only `Literal` fragments are read: the other ones were written by the suggestion, not observed.
fn redirect_response(
    response: &mut MockResponse,
    covers: &Covers,
    own: Option<&OwnAddress>,
) -> BTreeSet<ServiceRef> {
    let mut redirected = BTreeSet::new();
    let fragments = std::mem::take(&mut response.body);
    let count = fragments.len();
    for (i, fragment) in fragments.into_iter().enumerate() {
        match fragment {
            BodyFragment::Literal { value } => {
                redirect_text(
                    &value,
                    covers,
                    i + 1 < count,
                    &mut response.body,
                    &mut redirected,
                );
            }
            other => response.body.push(other),
        }
    }
    if let Some(own) = own {
        for HeaderEntry { name, value } in &mut response.headers {
            if ADDRESS_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
                *value = redirect_header(value, covers, own, &mut redirected);
            }
        }
    }
    redirected
}

/// Rewrites every suggested rule so that the addresses a service of Mimicway covers go to that service, and records
/// which services each rule now sends the application to.
pub fn redirect_suggestions(
    suggestions: &mut [EndpointSuggestion],
    covers: &Covers,
    own: Option<&OwnAddress>,
) {
    for suggestion in suggestions {
        let rules = match &mut suggestion.outcome {
            Suggestion::Unconditional { rule } | Suggestion::Echo { rule } => {
                std::slice::from_mut(rule.as_mut())
            }
            Suggestion::Conditional { rules } => rules.as_mut_slice(),
            Suggestion::VarianceUnexplained { .. } => &mut [],
        };
        for rule in rules {
            rule.redirected_to = redirect_response(&mut rule.response, covers, own)
                .into_iter()
                .collect();
        }
    }
}

/// What a new proxy service in front of a backend would be: the service form opens with it, and the user saves it
/// through the usual checks.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ServiceDraft {
    pub name: String,
    pub real_target_url: String,
    pub group_name: Option<String>,
}

/// A backend the observed responses point to.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LinkedBackend {
    /// `scheme://host[:port]`.
    pub origin: String,
    /// A few of the addresses seen, in the order they were found.
    pub addresses: Vec<String>,
    /// A few of the routes whose responses carried them (`GET /service/{name}`).
    pub routes: Vec<String>,
    /// The services of Mimicway that reach some of these addresses: rules suggested from these responses send the
    /// application there.
    pub served_by: Vec<ServiceRef>,
    /// A proxy service in front of the origin, when some of its addresses reach no service yet.
    pub draft: Option<ServiceDraft>,
}

#[derive(Default)]
struct Linked {
    addresses: Vec<String>,
    routes: Vec<String>,
    served_by: BTreeSet<ServiceRef>,
    uncovered: bool,
}

/// The backends that the observed responses of a service point to, by origin, Mimicway's own address left out. `group`
/// is the observed service's group, where a new service would go; `taken` the names already used in that group.
pub fn linked_backends(
    endpoints: &[(ObservationKey, Vec<ObservedExchange>)],
    covers: &Covers,
    group: Option<&str>,
    taken: &BTreeSet<String>,
) -> Vec<LinkedBackend> {
    let mut by_origin: BTreeMap<String, (Url, Linked)> = BTreeMap::new();
    for route in observed_routes(endpoints) {
        let label = format!(
            "{} {}",
            route.method,
            if route.pattern.is_empty() {
                "/"
            } else {
                &route.pattern
            }
        );
        for exchange in route.exchanges() {
            let headers = exchange
                .response_headers
                .iter()
                .filter(|(name, _)| ADDRESS_HEADERS.contains(&name.to_ascii_lowercase().as_str()))
                .map(|(_, value)| value.as_str());
            for text in std::iter::once(exchange.response_body.as_str()).chain(headers) {
                for address in find_addresses(text) {
                    if is_own(&address.url, exchange.request_host.as_deref()) {
                        continue;
                    }
                    let origin = address.url.origin().ascii_serialization();
                    if !by_origin.contains_key(&origin) && by_origin.len() >= MAX_ORIGINS {
                        continue;
                    }
                    let (_, linked) = by_origin
                        .entry(origin)
                        .or_insert_with(|| (address.url.clone(), Linked::default()));
                    if linked.addresses.len() < MAX_EXAMPLES
                        && !linked.addresses.iter().any(|a| a == address.text)
                    {
                        linked.addresses.push(address.text.to_string());
                    }
                    if linked.routes.len() < MAX_EXAMPLES && !linked.routes.contains(&label) {
                        linked.routes.push(label.clone());
                    }
                    match covers.find(&address) {
                        Some((cover, _)) => {
                            linked.served_by.insert(cover.service.clone());
                        }
                        None => linked.uncovered = true,
                    }
                }
            }
        }
    }

    let mut taken = taken.clone();
    by_origin
        .into_iter()
        .map(|(origin, (url, linked))| LinkedBackend {
            draft: linked.uncovered.then(|| ServiceDraft {
                name: draft_name(&url, &mut taken),
                real_target_url: origin.clone(),
                group_name: group.map(str::to_string),
            }),
            origin,
            addresses: linked.addresses,
            routes: linked.routes,
            served_by: linked.served_by.into_iter().collect(),
        })
        .collect()
}

/// A service name taken from a backend's host, valid for the service form (letters, digits, `-`, `_`, not reserved) and
/// not yet in `taken`, to which it is added: the first label of a host name (`orders` for `orders.internal`), the
/// address of an IP host (`backend-10-0-0-5`).
fn draft_name(url: &Url, taken: &mut BTreeSet<String>) -> String {
    let raw = match url.host() {
        Some(Host::Domain(domain)) => domain.split('.').next().unwrap_or_default().to_string(),
        Some(Host::Ipv4(ip)) => format!("backend-{ip}"),
        Some(Host::Ipv6(ip)) => format!("backend-{ip}"),
        None => String::new(),
    };
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut base = cleaned.trim_matches('-').to_string();
    if base.is_empty() {
        base = "backend".to_string();
    } else if crate::server::validation::is_reserved_name(&base) {
        base.push_str("-backend");
    }
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    taken.insert(name.clone());
    name
}

/// What the fuzz target checks on any observed traffic, with one service in front of `http://backend.test`: listing and
/// rewriting never panic, and a rule suggested from identical responses, once rewritten and rendered for a request to
/// Mimicway, holds no address of that backend any more.
#[cfg(any(test, fuzzing))]
pub(crate) fn check(endpoints: &[(ObservationKey, Vec<ObservedExchange>)]) -> Result<(), String> {
    use crate::engine::{TemplateContext, TemplateRenderer};
    let config: MockConfig = serde_json::from_value(serde_json::json!({
        "services": [{ "name": "backend", "listen_path": "", "real_target_url": "http://backend.test",
            "is_mocked": false, "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto",
            "rules": [] }],
        "groups": [],
    }))
    .map_err(|e| e.to_string())?;
    let covers = Covers::from_config(&config);
    linked_backends(endpoints, &covers, None, &BTreeSet::new());
    let mut suggestions = super::suggest_for_service(endpoints);
    redirect_suggestions(
        &mut suggestions,
        &covers,
        OwnAddress::latest(endpoints).as_ref(),
    );

    let empty = std::collections::HashMap::new();
    let headers = std::collections::HashMap::from([("host".to_string(), "mimic.test".to_string())]);
    let ctx = TemplateContext {
        path_params: &empty,
        query_params: &empty,
        headers: &headers,
        request_body: b"",
        seq_counter: 0,
        script_result: None,
    };
    for suggestion in &suggestions {
        if let Suggestion::Unconditional { rule } = &suggestion.outcome {
            let rendered = TemplateRenderer::render_body(&rule.response.body, &[], &ctx);
            if let Some(left) = find_addresses(&rendered)
                .iter()
                .find(|a| covers.find(a).is_some())
            {
                return Err(format!(
                    "{} still sends to the backend: {rendered:?}",
                    left.text
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proptests;
