// Property-based tests: the input is untrusted (paths and bodies of a real proxied backend, possibly hostile). They
// generate random and adversarial observations (statuses, JSON, XML or text, noisy headers, paths that share or not a
// pattern) and check invariants no hand-written case covers all at once: never a panic, never a computed header
// (content-length, date) nor a noise header in a suggested rule, and above all that the suggested rules, replayed on
// the calls they come from, answer exactly what the backend answered.
use super::replay::rules_of as suggestion_rules;
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

/// Rule-relative paths that share patterns: identifiers and words side by side, the root, and values the matcher would
/// read as a parameter or a wildcard if written as they are.
const PATHS: &[&str] = &[
    "/users/me",
    "/users/1",
    "/users/2",
    "/users/42",
    "/dir/restA",
    "/dir/restB",
    "/dir/9f86d081884c7d65",
    "",
    "/:id",
    "/a%2Fb",
    "/files/%7Bx%7D/x",
    "/orders/1/lines",
    "/orders/2/lines",
    "/users/me/x",
];
/// Indices into `PATHS`, some repeated so that routes often reach the samples a suggestion waits for, the literal
/// `/users/me` next to `/users/{id}` above all.
const WEIGHTED: &[usize] = &[
    0, 0, 0, 1, 1, 2, 2, 3, 4, 4, 5, 5, 6, 7, 7, 8, 9, 10, 11, 12, 13,
];

/// The body of one kind of answer, sometimes repeating the last segment or the query value, so that echo rules and
/// conditional ones both occur.
fn answer(kind: usize, path: &str, query: &str) -> (String, &'static str) {
    let last = crate::engine::matcher::url_decode(path.rsplit('/').next().unwrap_or_default());
    match kind {
        0 => ("ok".to_string(), "text/plain"),
        1 => (format!("<r><v>{last}</v></r>"), "text/xml"),
        2 => (format!("{{\"v\":\"{last}\"}}"), "application/json"),
        3 => ("{\"a\":1}".to_string(), "application/json"),
        4 => (format!("<r><q>{query}</q></r>"), "text/xml"),
        _ => (format!("{last}-{query}"), "text/plain"),
    }
}

/// Calls on `PATHS`: each path answers with its own kind and status, except a few calls that answer otherwise (the
/// variance a suggestion must explain or refuse).
fn arb_calls() -> impl Strategy<Value = Vec<(String, ObservedExchange)>> {
    (
        prop::collection::vec(
            (0usize..6, prop::sample::select(vec![200u16, 404])),
            PATHS.len(),
        ),
        prop::collection::vec(
            (
                prop::sample::select(WEIGHTED),
                prop::option::weighted(0.15, 0usize..6),
                prop::option::of(prop::sample::select(vec!["1", "2", "x"])),
            ),
            0..24,
        ),
    )
        .prop_map(|(per_path, calls)| {
            calls
                .into_iter()
                .map(|(p, odd_kind, query)| {
                    let (kind, status) = per_path[p];
                    let (body, content_type) =
                        answer(odd_kind.unwrap_or(kind), PATHS[p], query.unwrap_or("none"));
                    let query: HashMap<String, String> = query
                        .map(|v| ("q".to_string(), v.to_string()))
                        .into_iter()
                        .collect();
                    let exchange = ObservedExchange::new(
                        query,
                        HashMap::new(),
                        b"",
                        None,
                        status,
                        HashMap::new(),
                        body.as_bytes(),
                        Some(content_type.into()),
                    );
                    (PATHS[p].to_string(), exchange)
                })
                .collect()
        })
}

fn endpoints(calls: &[(String, ObservedExchange)]) -> Vec<(ObservationKey, Vec<ObservedExchange>)> {
    let mut out: Vec<(ObservationKey, Vec<ObservedExchange>)> = Vec::new();
    for (path, exchange) in calls {
        match out.iter_mut().find(|(k, _)| &k.sub_path == path) {
            Some((_, list)) => list.push(exchange.clone()),
            None => out.push((
                ObservationKey {
                    group_name: None,
                    service_name: "svc".into(),
                    method: "GET".into(),
                    sub_path: path.clone(),
                },
                vec![exchange.clone()],
            )),
        }
    }
    out
}

proptest! {
    #[test]
    fn suggest_never_panics(obs in prop::collection::vec(arb_exchange(), 0..15)) {
        let calls: Vec<(String, ObservedExchange)> = obs.into_iter().map(|e| ("/fuzz".to_string(), e)).collect();
        let _ = suggest_for_service(&endpoints(&calls));
    }

    #[test]
    fn identical_responses_always_suggest_unconditional(
        status in 100u16..600,
        body in arb_body(),
        n in 3usize..10,
    ) {
        let calls: Vec<(String, ObservedExchange)> = (0..n)
            .map(|_| {
                let e = ObservedExchange::new(
                    HashMap::new(), HashMap::new(), body.as_bytes(), None,
                    status, HashMap::new(), body.as_bytes(), None,
                );
                ("/fuzz".to_string(), e)
            })
            .collect();
        let suggestions = suggest_for_service(&endpoints(&calls));
        let is_unconditional = matches!(
            suggestions.as_slice(),
            [EndpointSuggestion { outcome: Suggestion::Unconditional { .. }, .. }]
        );
        prop_assert!(is_unconditional);
    }

    #[test]
    fn suggested_headers_never_contain_content_length_or_date(
        obs in prop::collection::vec(arb_exchange(), 3..15)
    ) {
        let calls: Vec<(String, ObservedExchange)> = obs.into_iter().map(|e| ("/fuzz".to_string(), e)).collect();
        for suggestion in suggest_for_service(&endpoints(&calls)) {
            for rule in suggestion_rules(&suggestion.outcome) {
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
        let calls: Vec<(String, ObservedExchange)> = obs.into_iter().map(|e| ("/fuzz".to_string(), e)).collect();
        for suggestion in suggest_for_service(&endpoints(&calls)) {
            for rule in suggestion_rules(&suggestion.outcome) {
                if let Some(cond) = &rule.condition
                    && let ConditionSource::Header(k) = &cond.source
                {
                    prop_assert!(!NOISY_HEADERS.contains(&k.to_lowercase().as_str()));
                }
            }
        }
    }

    #[test]
    fn suggested_rules_answer_every_call_they_come_from_as_observed(calls in arb_calls()) {
        let result = super::replay::check(&endpoints(&calls));
        prop_assert!(result.is_ok(), "{}", result.unwrap_err());
    }
}
