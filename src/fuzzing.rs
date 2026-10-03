//! Entry points of the fuzz targets (fuzz/fuzz_targets/). Compiled by cargo-fuzz only (`--cfg fuzzing`), never in the
//! shipped binary: each one hands untrusted bytes to a parser or a matcher the way a request or an import reaches it,
//! and checks what must hold whatever the bytes. A panic, a timeout or a failed assertion is a finding.
use crate::engine::template::render_template;
use crate::engine::{MatchEngine, RequestData, TemplateContext};
use crate::models::{Condition, ConditionGroup, ConditionSource, MockConfig, Operator};
use std::collections::HashMap;

/// Splits the input at its 0xFF bytes, which never occur in UTF-8 text, into the `N` fields of one case. Missing fields
/// are empty; the last one keeps the rest of the input.
fn fields<const N: usize>(data: &[u8]) -> [&[u8]; N] {
    let mut out = [&data[..0]; N];
    let mut rest = data;
    for slot in out.iter_mut().take(N - 1) {
        let end = rest.iter().position(|&b| b == 0xFF).unwrap_or(rest.len());
        *slot = &rest[..end];
        rest = rest.get(end + 1..).unwrap_or_default();
    }
    out[N - 1] = rest;
    out
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Query parameters as the interception reads them from a request's URL.
fn query_params(query: &[u8]) -> HashMap<String, String> {
    url::form_urlencoded::parse(query).into_owned().collect()
}

/// A rule's sub-path pattern (`/orders/:id`, `/files/*`) against the rest of a request's path.
pub fn sub_path(data: &[u8]) {
    let [pattern, path] = fields(data);
    let _ = crate::engine::matcher::match_path(&text(pattern), &text(path));
}

/// A request (query string, a header, a body) against one condition per source, all with the input's operator: what the
/// rule tester reports (`evaluate_group`) must agree with what production decides (`matches_group`).
pub fn request_conditions(data: &[u8]) {
    let [selector, query, header, body, key, operand] = fields(data);
    let operator = match selector.first().map_or(0, |b| b % 4) {
        0 => Operator::Eq(text(operand)),
        1 => Operator::Contains(text(operand)),
        2 => Operator::Regex(text(operand)),
        _ => Operator::Exists,
    };
    let key = text(key);
    let conditions: Vec<Condition> = [
        ConditionSource::QueryParam(key.clone()),
        ConditionSource::Header(key.clone()),
        ConditionSource::JsonPointer(key.clone()),
        ConditionSource::XPath(key.clone()),
        ConditionSource::FormField(key.clone()),
        ConditionSource::PathParam(key.clone()),
        ConditionSource::BodyRaw,
    ]
    .into_iter()
    .map(|source| Condition {
        source,
        operator: operator.clone(),
    })
    .collect();
    let req = RequestData {
        query_params: query_params(query),
        headers: HashMap::from([(key.clone(), text(header))]),
        body: body.to_vec(),
        content_type: None,
        path_params: HashMap::from([(key, text(operand))]),
        method: "POST".into(),
        remaining_path: String::new(),
    };
    for group in [
        ConditionGroup {
            all_of: conditions.clone(),
            any_of: Vec::new(),
        },
        ConditionGroup {
            all_of: Vec::new(),
            any_of: conditions,
        },
    ] {
        assert_eq!(
            MatchEngine::evaluate_group(&group, &req).matched,
            MatchEngine::matches_group(&group, &req),
            "the rule tester and production disagree"
        );
    }
}

/// An XML or SOAP body read through a slash-separated path, as conditions and `{{xpath.…}}` read it.
pub fn xml_path(data: &[u8]) {
    let [path, body] = fields(data);
    let _ = MatchEngine::extract_xpath(body, &text(path));
}

/// A response template rendered with a request's values (`{{query.…}}`, `{{json.…}}`, `{{xpath.…}}`, fake data, dates).
pub fn template(data: &[u8]) {
    let [template, query, body] = fields(data);
    let empty = HashMap::new();
    let query_params = query_params(query);
    let ctx = TemplateContext {
        path_params: &empty,
        query_params: &query_params,
        headers: &empty,
        request_body: body,
        seq_counter: 0,
        script_result: None,
        pre_script_result: None,
        post_script_result: None,
    };
    let _ = render_template(&text(template), &ctx);
}

/// The hexadecimal text that raw TCP mocks keep in YAML: any bytes encode and decode back to themselves, and any text
/// that decodes re-encodes to the same bytes.
#[cfg(feature = "tcp-mock")]
pub fn tcp_hex(data: &[u8]) {
    use crate::tcp::hex::{decode, encode};
    assert_eq!(decode(&encode(data)).as_deref(), Ok(data));
    if let Ok(bytes) = decode(&text(data)) {
        assert_eq!(decode(&encode(&bytes)), Ok(bytes));
    }
}

/// A configuration imported through `PUT /api/config`: parsed from JSON and checked as the API checks it, then written
/// and read back the way the store keeps it (YAML), which must give the same configuration.
pub fn config_import(data: &[u8]) {
    let Ok(config) = serde_json::from_slice::<MockConfig>(data) else {
        return;
    };
    let valid = config
        .services
        .iter()
        .all(|service| crate::server::validation::validate_service(service).is_ok());
    if !valid || !config.unknown_group_references().is_empty() {
        return;
    }
    let yaml = serde_yaml::to_string(&config).expect("an accepted configuration serializes");
    let stored: MockConfig =
        serde_yaml::from_str(&yaml).expect("the store reads back what it wrote");
    assert_eq!(
        stored, config,
        "the configuration changed on its way through the file:\n{yaml}"
    );
}
