// Mock rules suggested from the exchanges `server::observation` captured while a user observed a service. Always
// computed from `ObservationStore` when asked: the input is already bounded, so recomputing costs less than a cache
// to invalidate, and suggestions can never lag behind what was observed.
//
// The trap: two calls to the same method and path can legitimately get different answers, and a rule built from
// the first answer seen would silently break the others. Hence three steps: (1) group the observations by
// equivalent response; (2) one group gives an unconditional rule; (3) several groups need a request field that
// tells them apart perfectly (same value, same group; different groups, different values) before anything is
// suggested. When no field does, nothing is suggested: no rule beats a fragile one.
use crate::models::{Condition, ConditionSource, HeaderEntry, MockResponse, Operator};
use crate::server::observation::ObservedExchange;
use std::collections::{BTreeSet, HashMap};

const DEFAULT_MIN_SAMPLES: usize = 3;

/// Observations needed for one endpoint (service, method, sub-path) before anything is suggested: one response, or
/// two identical ones, prove nothing about how stable the endpoint is.
pub fn min_samples() -> usize {
    std::env::var("TRAFFIC_OBSERVATION_MIN_SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MIN_SAMPLES)
}

/// Request headers never used to tell responses apart: they change from call to call by nature (timestamps,
/// correlation and tracing ids, credentials), not because of a decision of the backend. A condition on them could
/// fit the sample perfectly and still be useless or harmful (a rule conditioned on a token never matches again).
const NOISY_HEADERS: &[&str] = &[
    "date",
    "x-request-id",
    "x-correlation-id",
    "traceparent",
    "tracestate",
    "authorization",
    "cookie",
    "set-cookie",
    "user-agent",
];

/// A suggested rule: the unconditional one (no variance observed), or one of the conditional rules covering each
/// distinct response (`condition` is then `Some`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SuggestedRule {
    pub method: String,
    pub sub_path: String,
    pub condition: Option<Condition>,
    pub sample_count: usize,
    pub response: MockResponse,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "outcome")]
pub enum Suggestion {
    /// No variance in the sample: one rule, without condition.
    Unconditional { rule: Box<SuggestedRule> },
    /// Variance explained by a request field: one rule per value seen, each with an `Eq` condition.
    Conditional { rules: Vec<SuggestedRule> },
    /// Variance that no field explains reliably in the sample: nothing to suggest, only a diagnosis.
    VarianceUnexplained {
        sample_count: usize,
        response_class_count: usize,
    },
}

/// What makes two responses equal: status and body, byte for byte. Bodies are already truncated to
/// `observation::max_body_size()`, so two bodies that differ only beyond that point count as equal. Response
/// headers are left out: the same body with another correlation header is not a different answer to explain.
fn response_signature(exchange: &ObservedExchange) -> (u16, &str) {
    (exchange.response_status, exchange.response_body.as_str())
}

/// The suggestion for one endpoint (service, method, sub-path) from its observations. `None` while the sample is
/// smaller than `min_samples()`: no decision yet, either way.
pub fn suggest(
    method: &str,
    sub_path: &str,
    observations: &[ObservedExchange],
) -> Option<Suggestion> {
    if observations.len() < min_samples() {
        return None;
    }

    let mut classes: Vec<Vec<&ObservedExchange>> = Vec::new();
    for obs in observations {
        let sig = response_signature(obs);
        match classes.iter_mut().find(|c| response_signature(c[0]) == sig) {
            Some(class) => class.push(obs),
            None => classes.push(vec![obs]),
        }
    }

    if classes.len() == 1 {
        let representative = classes[0].last().expect("a class is never empty");
        return Some(Suggestion::Unconditional {
            rule: Box::new(SuggestedRule {
                method: method.to_string(),
                sub_path: sub_path.to_string(),
                condition: None,
                sample_count: observations.len(),
                response: build_response(representative),
            }),
        });
    }

    if let Some((source, values_per_class)) = find_discriminator(observations, &classes) {
        let rules = classes
            .iter()
            .zip(values_per_class)
            .map(|(class, value)| {
                let representative = class.last().expect("a class is never empty");
                SuggestedRule {
                    method: method.to_string(),
                    sub_path: sub_path.to_string(),
                    condition: Some(Condition {
                        source: source.clone_with(value.clone()),
                        operator: Operator::Eq(value),
                    }),
                    sample_count: class.len(),
                    response: build_response(representative),
                }
            })
            .collect();
        return Some(Suggestion::Conditional { rules });
    }

    Some(Suggestion::VarianceUnexplained {
        sample_count: observations.len(),
        response_class_count: classes.len(),
    })
}

/// Response headers never copied into a suggested rule (they stay in `ObservedExchange` for inspection):
/// `content-length` is computed from the rule's actual body when it is rendered (a fixed value would be wrong as
/// soon as the body is edited), and `date` would freeze a timestamp that means nothing in a static configuration.
const NEVER_SUGGESTED_RESPONSE_HEADERS: &[&str] = &["content-length", "date"];

fn build_response(exchange: &ObservedExchange) -> MockResponse {
    let headers = exchange
        .response_headers
        .iter()
        .filter(|(name, _)| {
            !NEVER_SUGGESTED_RESPONSE_HEADERS.contains(&name.to_lowercase().as_str())
                && !crate::server::redaction::is_sensitive_header(name)
        })
        .map(|(name, value)| HeaderEntry {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();
    MockResponse {
        status: exchange.response_status,
        headers,
        body: vec![crate::models::BodyFragment::Literal {
            value: exchange.response_body.clone(),
        }],
        chaos: None,
    }
}

/// A candidate condition source, without its value (known once the field is chosen).
#[derive(Clone)]
enum SourceTemplate {
    QueryParam(String),
    JsonPointer(String),
    Header(String),
}

impl SourceTemplate {
    fn clone_with(&self, _value: String) -> ConditionSource {
        match self {
            SourceTemplate::QueryParam(k) => ConditionSource::QueryParam(k.clone()),
            SourceTemplate::JsonPointer(k) => ConditionSource::JsonPointer(format!("/{k}")),
            SourceTemplate::Header(k) => ConditionSource::Header(k.clone()),
        }
    }

    fn extract(
        &self,
        obs: &ObservedExchange,
        json_fields: &HashMap<String, String>,
    ) -> Option<String> {
        match self {
            SourceTemplate::QueryParam(k) => obs.request_query_params.get(k).cloned(),
            SourceTemplate::JsonPointer(k) => json_fields.get(k).cloned(),
            SourceTemplate::Header(k) => obs.request_headers.get(k).cloned(),
        }
    }
}

/// Reads the request body as a flat JSON object (no arrays or nested objects for now): each scalar field becomes a
/// `JsonPointer("/key")` candidate.
fn json_top_level_fields(body: &str) -> HashMap<String, String> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(body) else {
        return HashMap::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::String(s) => Some((k, s)),
            serde_json::Value::Number(n) => Some((k, n.to_string())),
            serde_json::Value::Bool(b) => Some((k, b.to_string())),
            _ => None,
        })
        .collect()
}

/// Looks, in order of preference (query parameter, top-level JSON field, header), for a field whose value splits
/// the observations exactly like the response groups: same value, same group; different groups, different values.
/// Returns the chosen source and the value that stands for each group (in the order of `classes`).
fn find_discriminator(
    observations: &[ObservedExchange],
    classes: &[Vec<&ObservedExchange>],
) -> Option<(SourceTemplate, Vec<String>)> {
    let json_fields: Vec<HashMap<String, String>> = observations
        .iter()
        .map(|o| json_top_level_fields(&o.request_body))
        .collect();
    let json_fields_by_ptr = |obs_idx: usize| json_fields[obs_idx].clone();
    let _ = json_fields_by_ptr; // helper inutilise directement, cf boucle ci-dessous

    let mut query_keys = BTreeSet::new();
    let mut json_keys = BTreeSet::new();
    let mut header_keys = BTreeSet::new();
    for (i, obs) in observations.iter().enumerate() {
        query_keys.extend(obs.request_query_params.keys().cloned());
        json_keys.extend(json_fields[i].keys().cloned());
        header_keys.extend(
            obs.request_headers
                .keys()
                .filter(|k| !NOISY_HEADERS.contains(&k.to_lowercase().as_str()))
                .cloned(),
        );
    }

    let candidates: Vec<SourceTemplate> = query_keys
        .into_iter()
        .map(SourceTemplate::QueryParam)
        .chain(json_keys.into_iter().map(SourceTemplate::JsonPointer))
        .chain(header_keys.into_iter().map(SourceTemplate::Header))
        .collect();

    for candidate in candidates {
        if let Some(values) = discriminator_values(&candidate, classes, &json_fields, observations)
        {
            return Some((candidate, values));
        }
    }
    None
}

/// Checks one candidate: every observation of a group carries the same value for this field (a missing value
/// disqualifies it), and no other group shares that value. Returns the value of each group when it holds.
fn discriminator_values(
    candidate: &SourceTemplate,
    classes: &[Vec<&ObservedExchange>],
    json_fields: &[HashMap<String, String>],
    all_observations: &[ObservedExchange],
) -> Option<Vec<String>> {
    let index_of = |obs: &ObservedExchange| -> usize {
        all_observations
            .iter()
            .position(|o| std::ptr::eq(o, obs))
            .expect("observation appartient au meme slice")
    };

    let mut values = Vec::with_capacity(classes.len());
    for class in classes {
        let mut class_value: Option<String> = None;
        for obs in class {
            let idx = index_of(obs);
            let v = candidate.extract(obs, &json_fields[idx])?;
            match &class_value {
                None => class_value = Some(v),
                Some(existing) if *existing != v => return None,
                Some(_) => {}
            }
        }
        values.push(class_value?);
    }

    let distinct: BTreeSet<&String> = values.iter().collect();
    if distinct.len() != values.len() {
        return None;
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn exchange(status: u16, body: &str) -> ObservedExchange {
        ObservedExchange::new(
            HashMap::new(),
            HashMap::new(),
            b"",
            Some("application/json".into()),
            status,
            HashMap::new(),
            body.as_bytes(),
            Some("application/json".into()),
        )
    }

    fn exchange_with_query(status: u16, body: &str, query: &[(&str, &str)]) -> ObservedExchange {
        let mut e = exchange(status, body);
        e.request_query_params = query
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        e
    }

    fn exchange_with_header(status: u16, body: &str, header: (&str, &str)) -> ObservedExchange {
        let mut e = exchange(status, body);
        e.request_headers
            .insert(header.0.to_string(), header.1.to_string());
        e
    }

    fn exchange_with_json_body_field(
        status: u16,
        resp_body: &str,
        req_field: (&str, &str),
    ) -> ObservedExchange {
        let mut e = exchange(status, resp_body);
        e.request_body = format!(r#"{{"{}":"{}"}}"#, req_field.0, req_field.1);
        e
    }

    #[test]
    fn below_min_samples_returns_none() {
        let obs = vec![exchange(200, "a"), exchange(200, "a")];
        assert!(suggest("GET", "/orders", &obs).is_none());
    }

    #[test]
    fn identical_responses_suggest_unconditional_rule() {
        let obs = vec![
            exchange(200, "ok"),
            exchange(200, "ok"),
            exchange(200, "ok"),
        ];
        let suggestion = suggest("GET", "/orders", &obs).unwrap();
        match suggestion {
            Suggestion::Unconditional { rule } => {
                assert!(rule.condition.is_none());
                assert_eq!(rule.sample_count, 3);
                assert_eq!(rule.response.status, 200);
            }
            other => panic!("expected Unconditional, got {other:?}"),
        }
    }

    #[test]
    fn suggested_response_never_includes_credential_headers() {
        // A rule saved from a suggestion is served to anyone: a Set-Cookie of the real backend would hand out
        // a real session.
        let mut e = exchange(200, "ok");
        e.response_headers
            .insert("Set-Cookie".into(), "session=abc".into());
        e.response_headers.insert("x-env".into(), "prod".into());
        let obs = vec![e.clone(), e.clone(), e];
        let Suggestion::Unconditional { rule } = suggest("GET", "/orders", &obs).unwrap() else {
            panic!("expected an unconditional suggestion");
        };
        let names: Vec<&str> = rule
            .response
            .headers
            .iter()
            .map(|h| h.name.as_str())
            .collect();
        assert_eq!(names, vec!["x-env"]);
    }

    #[test]
    fn suggested_response_never_includes_content_length_or_date() {
        // Captured from a real backend, content-length and date are in the response, but have no place in a saved rule
        // (content-length is computed when rendering, date would freeze a timestamp). An ordinary header such as x-env
        // stays.
        let mut e = exchange(200, "ok");
        e.response_headers
            .insert("content-length".into(), "2".into());
        e.response_headers
            .insert("Date".into(), "Tue, 25 Aug 2026 00:00:00 GMT".into());
        e.response_headers.insert("x-env".into(), "prod".into());
        let obs = vec![e.clone(), e.clone(), e];

        let suggestion = suggest("GET", "/orders", &obs).unwrap();
        match suggestion {
            Suggestion::Unconditional { rule } => {
                let names: Vec<&str> = rule
                    .response
                    .headers
                    .iter()
                    .map(|h| h.name.as_str())
                    .collect();
                assert!(
                    !names
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case("content-length"))
                );
                assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("date")));
                assert!(names.contains(&"x-env"));
            }
            other => panic!("expected Unconditional, got {other:?}"),
        }
    }

    #[test]
    fn variance_explained_by_query_param_suggests_conditional_rules() {
        let obs = vec![
            exchange_with_query(200, r#"{"stock":true}"#, &[("id", "1")]),
            exchange_with_query(200, r#"{"stock":true}"#, &[("id", "1")]),
            exchange_with_query(404, "not found", &[("id", "2")]),
            exchange_with_query(404, "not found", &[("id", "2")]),
        ];
        let suggestion = suggest("GET", "/orders", &obs).unwrap();
        match suggestion {
            Suggestion::Conditional { rules } => {
                assert_eq!(rules.len(), 2);
                let statuses: Vec<u16> = rules.iter().map(|r| r.response.status).collect();
                assert!(statuses.contains(&200));
                assert!(statuses.contains(&404));
                for rule in &rules {
                    let cond = rule.condition.as_ref().unwrap();
                    assert_eq!(cond.source, ConditionSource::QueryParam("id".into()));
                }
            }
            other => panic!("expected Conditional, got {other:?}"),
        }
    }

    #[test]
    fn variance_explained_by_json_body_field() {
        let obs = vec![
            exchange_with_json_body_field(200, "actif", ("mode", "on")),
            exchange_with_json_body_field(200, "actif", ("mode", "on")),
            exchange_with_json_body_field(200, "inactif", ("mode", "off")),
            exchange_with_json_body_field(200, "inactif", ("mode", "off")),
        ];
        let suggestion = suggest("POST", "/toggle", &obs).unwrap();
        match suggestion {
            Suggestion::Conditional { rules } => {
                assert_eq!(rules.len(), 2);
                for rule in &rules {
                    let cond = rule.condition.as_ref().unwrap();
                    assert_eq!(cond.source, ConditionSource::JsonPointer("/mode".into()));
                }
            }
            other => panic!("expected Conditional, got {other:?}"),
        }
    }

    #[test]
    fn variance_explained_by_header() {
        let obs = vec![
            exchange_with_header(200, "prod-response", ("x-env", "prod")),
            exchange_with_header(200, "prod-response", ("x-env", "prod")),
            exchange_with_header(500, "staging-error", ("x-env", "staging")),
            exchange_with_header(500, "staging-error", ("x-env", "staging")),
        ];
        let suggestion = suggest("GET", "/status", &obs).unwrap();
        match suggestion {
            Suggestion::Conditional { rules } => {
                assert_eq!(rules.len(), 2);
                for rule in &rules {
                    let cond = rule.condition.as_ref().unwrap();
                    assert_eq!(cond.source, ConditionSource::Header("x-env".into()));
                }
            }
            other => panic!("expected Conditional, got {other:?}"),
        }
    }

    #[test]
    fn noisy_headers_are_never_used_as_discriminator() {
        // x-request-id changes on every call and predicts nothing: never chosen, even though it splits the sample
        // "perfectly".
        let obs = vec![
            exchange_with_header(200, "ok", ("x-request-id", "r1")),
            exchange_with_header(200, "ok", ("x-request-id", "r2")),
            exchange_with_header(500, "err", ("x-request-id", "r3")),
        ];
        let suggestion = suggest("GET", "/status", &obs).unwrap();
        assert!(matches!(suggestion, Suggestion::VarianceUnexplained { .. }));
    }

    #[test]
    fn unexplainable_variance_suggests_nothing_actionable() {
        // The same request (no field varies) with different responses: nothing explains the variance, so never freeze an
        // unconditional rule on the first response seen.
        let obs = vec![
            exchange(200, "reponse-1"),
            exchange(200, "reponse-2"),
            exchange(200, "reponse-3"),
        ];
        let suggestion = suggest("GET", "/flaky", &obs).unwrap();
        match suggestion {
            Suggestion::VarianceUnexplained {
                sample_count,
                response_class_count,
            } => {
                assert_eq!(sample_count, 3);
                assert_eq!(response_class_count, 3);
            }
            other => panic!("expected VarianceUnexplained, got {other:?}"),
        }
    }

    #[test]
    fn partial_correlation_is_not_treated_as_discriminator() {
        // "id" sometimes has the same value in two groups: it does not split them perfectly and must be rejected.
        let obs = vec![
            exchange_with_query(200, "a", &[("id", "1")]),
            exchange_with_query(404, "b", &[("id", "1")]),
            exchange_with_query(200, "a", &[("id", "2")]),
        ];
        let suggestion = suggest("GET", "/x", &obs).unwrap();
        assert!(matches!(suggestion, Suggestion::VarianceUnexplained { .. }));
    }
}

// Property-based tests: the input is untrusted (bodies of a real proxied backend, possibly hostile). They generate
// random and adversarial observations (statuses, JSON or not, noisy headers) and check invariants no hand-written
// case covers all at once: never a panic, never a computed header (content-length, date) nor a noise header in a
// suggested rule.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashMap;

    fn arb_body() -> impl Strategy<Value = String> {
        prop_oneof![
            "[\\PC]{0,40}",
            "\\{\"[a-z]{1,5}\":\"[a-zA-Z0-9]{0,10}\"\\}",
            Just(String::new()),
            Just("not json at all {{{".to_string()),
        ]
    }

    fn arb_small_map() -> impl Strategy<Value = HashMap<String, String>> {
        prop::collection::hash_map(
            prop_oneof![
                Just("id".to_string()),
                Just("x-env".to_string()),
                Just("x-request-id".to_string()),
                Just("authorization".to_string()),
                "[a-z-]{1,8}",
            ],
            "[a-zA-Z0-9 ]{0,10}",
            0..3,
        )
    }

    fn arb_exchange() -> impl Strategy<Value = ObservedExchange> {
        (
            arb_small_map(),
            arb_small_map(),
            arb_body(),
            100u16..600,
            arb_small_map(),
            arb_body(),
        )
            .prop_map(
                |(query, headers, req_body, status, resp_headers, resp_body)| {
                    ObservedExchange::new(
                        query,
                        headers,
                        req_body.as_bytes(),
                        Some("application/json".into()),
                        status,
                        resp_headers,
                        resp_body.as_bytes(),
                        Some("application/json".into()),
                    )
                },
            )
    }

    fn suggestion_rules(s: &Suggestion) -> Vec<&SuggestedRule> {
        match s {
            Suggestion::Unconditional { rule } => vec![rule.as_ref()],
            Suggestion::Conditional { rules } => rules.iter().collect(),
            Suggestion::VarianceUnexplained { .. } => vec![],
        }
    }

    proptest! {
        #[test]
        fn suggest_never_panics(obs in prop::collection::vec(arb_exchange(), 0..15)) {
            let _ = suggest("GET", "/fuzz", &obs);
        }

        #[test]
        fn identical_responses_always_suggest_unconditional(
            status in 100u16..600,
            body in arb_body(),
            n in 3usize..10,
        ) {
            let obs: Vec<ObservedExchange> = (0..n)
                .map(|_| {
                    ObservedExchange::new(
                        HashMap::new(), HashMap::new(), body.as_bytes(), None,
                        status, HashMap::new(), body.as_bytes(), None,
                    )
                })
                .collect();
            let suggestion = suggest("GET", "/fuzz", &obs);
            let is_unconditional = matches!(suggestion, Some(Suggestion::Unconditional { .. }));
            prop_assert!(is_unconditional);
        }

        #[test]
        fn suggested_headers_never_contain_content_length_or_date(
            obs in prop::collection::vec(arb_exchange(), 3..15)
        ) {
            if let Some(suggestion) = suggest("GET", "/fuzz", &obs) {
                for rule in suggestion_rules(&suggestion) {
                    for h in &rule.response.headers {
                        let lower = h.name.to_lowercase();
                        prop_assert_ne!(lower.as_str(), "content-length");
                        prop_assert_ne!(lower.as_str(), "date");
                    }
                }
            }
        }

        #[test]
        fn suggested_condition_never_uses_a_noisy_header(
            obs in prop::collection::vec(arb_exchange(), 3..15)
        ) {
            if let Some(suggestion) = suggest("GET", "/fuzz", &obs) {
                for rule in suggestion_rules(&suggestion) {
                    if let Some(cond) = &rule.condition
                        && let ConditionSource::Header(k) = &cond.source
                    {
                        prop_assert!(!NOISY_HEADERS.contains(&k.to_lowercase().as_str()));
                    }
                }
            }
        }
    }
}
