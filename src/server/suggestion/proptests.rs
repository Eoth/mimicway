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

/// Calls on `PATHS`: each path answers with its own kind and status, except a few calls that answer otherwise, and a few
/// that answer a server error nothing explains (the variance a suggestion must explain, set aside or refuse). Paths of
/// one pattern often share a kind, so that several values lead to one response.
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
                prop::option::weighted(0.08, prop::sample::select(vec![500u16, 503])),
            ),
            0..24,
        ),
    )
        .prop_map(|(per_path, calls)| {
            calls
                .into_iter()
                .map(|(p, odd_kind, query, error)| {
                    let (kind, status) = per_path[p];
                    let (body, content_type) =
                        answer(odd_kind.unwrap_or(kind), PATHS[p], query.unwrap_or("none"));
                    let (status, body) = match error {
                        Some(error) => (error, format!("server error {error}")),
                        None => (status, body),
                    };
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
                    operation: None,
                },
                vec![exchange.clone()],
            )),
        }
    }
    out
}

proptest! {
    #![proptest_config(crate::test_seed::config(256, 0x7375_6767_6573_7421))]

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
        obs in prop::collection::vec(arb_exchange(), 3..15),
        noisy in prop::collection::vec((prop::sample::select(NOISY_HEADERS), 0usize..2), 4..12),
    ) {
        // Random exchanges, then exchanges whose noisy header follows the answer, each value seen again: the header
        // fits, and must still never be a condition.
        let mut calls: Vec<(String, ObservedExchange)> = obs.into_iter().map(|e| ("/fuzz".to_string(), e)).collect();
        let name = noisy[0].0.to_ascii_uppercase();
        calls.extend(noisy.iter().map(|&(_, kind)| {
            let e = ObservedExchange::new(
                HashMap::new(),
                HashMap::from([(name.clone(), format!("value-{kind}"))]),
                b"",
                None,
                200,
                HashMap::new(),
                format!("answer {}", ["a", "b"][kind]).as_bytes(),
                None,
            );
            ("/noisy".to_string(), e)
        }));
        for suggestion in suggest_for_service(&endpoints(&calls)) {
            for rule in suggestion_rules(&suggestion.outcome) {
                for cond in rule.conditions.all_of.iter().chain(&rule.conditions.any_of) {
                    if let ConditionSource::Header(k) = &cond.source {
                        prop_assert!(!NOISY_HEADERS.contains(&k.to_lowercase().as_str()), "{rule:?}");
                    }
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

/// The values a body field takes, each written three ways in XML (plain, CDATA, with a reference) so that the same value
/// reads the same whatever its writing.
const VALUES: &[&str] = &["R1", "R2", "R3", "R4", "a&b", "Béziers"];
const BODY_OPERATIONS: &[&str] = &["GetStatus", "Book", "Ping"];

fn xml_text(value: &str, writing: usize) -> String {
    match writing {
        0 => value.replace('&', "&amp;"),
        1 => format!("<![CDATA[{value}]]>"),
        _ => value.replace('&', "&#38;"),
    }
}

/// One call with a body: the operation (SOAP) or nothing (JSON), the value of the deciding field, how it is written,
/// a counter that changes at every call, and repeated items.
#[derive(Debug, Clone)]
struct BodyCall {
    soap: bool,
    prefix: bool,
    operation: usize,
    value: usize,
    writing: usize,
    items: usize,
    odd: Option<u16>,
    /// Sent in ISO-8859-1 rather than UTF-8: the bytes the mock receives are not the text observation keeps.
    latin1: bool,
    /// A SOAP body broken after the operation's start tag: its end tag lost (`false`), or elements nested past the
    /// limit of the XML readers right after it (`true`); a JSON body with a key written twice.
    broken: Option<bool>,
}

/// The bytes of `call`'s request body, as the client sends them.
fn request_bytes(call: &BodyCall, seq: usize) -> Vec<u8> {
    let mut text = request_body(call, seq);
    if call.soap {
        let operation = BODY_OPERATIONS[call.operation];
        let start = format!("<o:{operation}>");
        let end = format!("</o:{operation}>");
        match call.broken {
            Some(false) => text = text.replace(&end, ""),
            Some(true) => {
                let depth = crate::engine::matcher::XML_DEPTH_LIMIT + 10;
                let deep = format!("{start}{}{}", "<o:x>".repeat(depth), "</o:x>".repeat(depth));
                text = text.replace(&start, &deep);
            }
            None => {}
        }
    }
    if !call.latin1 {
        return text.into_bytes();
    }
    let declared = if call.soap {
        format!("<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>{text}")
    } else {
        text
    };
    declared
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

fn request_body(call: &BodyCall, seq: usize) -> String {
    let value = VALUES[call.value];
    if call.soap {
        let (p, ns) = if call.prefix {
            (
                "soap:",
                " xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\"",
            )
        } else {
            ("", "")
        };
        let items: String = (0..call.items)
            .map(|i| format!("<o:item><o:n>{i}</o:n></o:item>"))
            .collect();
        format!(
            "<{p}Envelope{ns} xmlns:o=\"urn:o\"><{p}Header><o:seq>{seq}</o:seq></{p}Header><{p}Body>\
             <o:{op}><o:ref>{text}</o:ref>{items}</o:{op}></{p}Body></{p}Envelope>",
            op = BODY_OPERATIONS[call.operation],
            text = xml_text(value, call.writing),
        )
    } else {
        let items: Vec<String> = (0..call.items).map(|i| format!("{{\"n\":{i}}}")).collect();
        // A broken JSON body writes its key twice: the reader of JSON pointers keeps the last value, and no field of
        // such a body is offered.
        let twice = if call.broken.is_some() {
            "\"ref\":\"R9\","
        } else {
            ""
        };
        format!(
            "{{\"req\":{{\"meta\":{{\"seq\":{seq}}},{twice}\"ref\":{}}},\"items\":[{}]}}",
            serde_json::Value::from(value),
            items.join(",")
        )
    }
}

/// Calls with XML or JSON bodies on one or two paths. Each (operation, value) answers with the response its table gives,
/// so that several values often lead to one response; a few calls answer a server error, or another status, that
/// nothing in the request explains.
fn arb_body_calls() -> impl Strategy<Value = Vec<(String, ObservedExchange, Vec<u8>)>> {
    let call = (
        prop::bool::weighted(0.7),
        any::<bool>(),
        0..BODY_OPERATIONS.len(),
        // The first values more often, so that values are seen again: repetition is what a condition needs.
        prop::sample::select(vec![0usize, 0, 1, 1, 2, 2, 3, 4, 5]),
        0usize..3,
        0usize..3,
        prop::option::weighted(0.1, prop::sample::select(vec![500u16, 503, 404])),
        prop::bool::weighted(0.15),
        prop::option::weighted(0.15, any::<bool>()),
    )
        .prop_map(
            |(soap, prefix, operation, value, writing, items, odd, latin1, broken)| BodyCall {
                soap,
                prefix,
                operation,
                value,
                writing,
                items,
                odd,
                latin1,
                broken,
            },
        );
    (
        prop::collection::vec(0usize..3, BODY_OPERATIONS.len() * VALUES.len()),
        // Every body SOAP, every body JSON, or both.
        0usize..3,
        prop::collection::vec((call, prop::bool::weighted(0.2)), 0..48),
        // Some cases hold bodies that are not UTF-8 or broken XML, among well-formed ones.
        prop::bool::weighted(0.3),
    )
        .prop_map(|(table, kinds, calls, disturbed)| {
            calls
                .into_iter()
                .enumerate()
                .map(|(seq, (mut call, second_path))| {
                    if kinds < 2 {
                        call.soap = kinds == 0;
                    }
                    if !disturbed {
                        call.latin1 = false;
                        call.broken = None;
                    }
                    // A JSON body names no operation: its answer depends on its value alone.
                    if !call.soap {
                        call.operation = 0;
                    }
                    let answer = table[call.operation * VALUES.len() + call.value];
                    let (status, body) = match call.odd {
                        Some(status) => (status, format!("error {status}")),
                        None => (200, format!("<r>{answer}</r>")),
                    };
                    // Through the capture of observation, as the proxy keeps an exchange.
                    let sent = request_bytes(&call, seq);
                    let exchange = ObservedExchange::new(
                        HashMap::new(),
                        HashMap::new(),
                        &sent,
                        None,
                        status,
                        HashMap::new(),
                        body.as_bytes(),
                        Some("text/xml".into()),
                    );
                    let path = if second_path { "/ws/b" } else { "/ws/a" };
                    (path.to_string(), exchange, sent)
                })
                .collect()
        })
}

fn posted_endpoints(
    calls: &[(String, ObservedExchange)],
) -> Vec<(ObservationKey, Vec<ObservedExchange>)> {
    let mut out = endpoints(calls);
    for (key, _) in &mut out {
        key.method = "POST".into();
    }
    out
}

/// `posted_endpoints`, with the request bodies the mock receives, in the same shape.
#[allow(clippy::type_complexity)]
fn posted_with_bodies(
    calls: &[(String, ObservedExchange, Vec<u8>)],
) -> (
    Vec<(ObservationKey, Vec<ObservedExchange>)>,
    Vec<Vec<Vec<u8>>>,
) {
    let kept: Vec<(String, ObservedExchange)> = calls
        .iter()
        .map(|(path, e, _)| (path.clone(), e.clone()))
        .collect();
    let endpoints = posted_endpoints(&kept);
    let received = endpoints
        .iter()
        .map(|(key, _)| {
            calls
                .iter()
                .filter(|(path, _, _)| *path == key.sub_path)
                .map(|(_, _, sent)| sent.clone())
                .collect()
        })
        .collect();
    (endpoints, received)
}

#[test]
fn rules_suggested_from_bodies_answer_every_call_they_come_from_as_observed() {
    use proptest::test_runner::TestRunner;
    use std::cell::Cell;
    let config = crate::test_seed::config(512, 0x626f_6479_5f72_706c);
    let (operations, several_values, set_aside, xml_fields, json_fields) = (
        Cell::new(0),
        Cell::new(0),
        Cell::new(0),
        Cell::new(0),
        Cell::new(0),
    );
    let result = TestRunner::new(config).run(&arb_body_calls(), |calls| {
        let (endpoints, received) = posted_with_bodies(&calls);
        let replayed = super::replay::check_received(&endpoints, &received);
        prop_assert!(replayed.is_ok(), "{}", replayed.unwrap_err());
        for suggestion in suggest_for_service(&endpoints) {
            operations.set(operations.get() + usize::from(suggestion.operation.is_some()));
            set_aside.set(set_aside.get() + usize::from(!suggestion.left_out.is_empty()));
            for rule in suggestion_rules(&suggestion.outcome) {
                several_values
                    .set(several_values.get() + usize::from(rule.conditions.any_of.len() > 1));
                for condition in rule.conditions.all_of.iter().chain(&rule.conditions.any_of) {
                    match (&condition.source, &condition.operator) {
                        (ConditionSource::XPath(_), Operator::Eq(_)) => {
                            xml_fields.set(xml_fields.get() + 1)
                        }
                        (ConditionSource::JsonPointer(_), _) => {
                            json_fields.set(json_fields.get() + 1)
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
    // The cases the judge must see, for the written seed.
    assert!(
        operations.get() >= 100
            && several_values.get() >= 50
            && set_aside.get() >= 10
            && xml_fields.get() >= 50
            && json_fields.get() >= 10,
        "operations {operations:?}, several values {several_values:?}, set aside {set_aside:?}, \
         XML fields {xml_fields:?}, JSON fields {json_fields:?}"
    );
}

/// Values a request sends and its response repeats: markup, quotes, the end of a CDATA section, template braces.
const REPEATED: &[&str] = &[
    "R1", "R2", "R3", "a&b", "<x>", "\"q\"", "it's", "]]>", "{{uuid}}", "}}", "x y", "é",
];

/// How a response writes the values it repeats.
#[derive(Debug, Clone, Copy)]
enum Writing {
    /// XML text, `&`, `<` and `>` escaped.
    XmlText,
    /// XML, quotes escaped too.
    XmlAll,
    /// XML, each value in a CDATA section (a value holding `]]>` escaped instead).
    Cdata,
    Json,
}

fn xml_escaped(value: &str, quotes: bool) -> String {
    let text = value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    if quotes {
        text.replace('"', "&quot;").replace('\'', "&apos;")
    } else {
        text
    }
}

fn written(value: &str, writing: Writing) -> String {
    match writing {
        Writing::XmlText => xml_escaped(value, false),
        Writing::XmlAll => xml_escaped(value, true),
        Writing::Cdata if !value.contains("]]>") => format!("<![CDATA[{value}]]>"),
        Writing::Cdata => xml_escaped(value, false),
        Writing::Json => {
            let quoted = serde_json::Value::from(value).to_string();
            quoted[1..quoted.len() - 1].to_string()
        }
    }
}

/// One call to an operation whose response repeats its request: a reference, and a list of (date, type) slots, each
/// answered with a block; `odd` answers something the request does not explain.
#[derive(Debug, Clone)]
struct RepeatCall {
    reference: usize,
    slots: Vec<(usize, usize)>,
    in_cdata: bool,
    odd: bool,
}

fn repeat_exchange(call: &RepeatCall, writing: Writing, prefix: bool) -> ObservedExchange {
    let value = |i: usize| REPEATED[i];
    let (request, response) = if let Writing::Json = writing {
        let slots: Vec<serde_json::Value> = call
            .slots
            .iter()
            .map(|&(d, t)| serde_json::json!({"date": value(d), "type": value(t)}))
            .collect();
        let request = serde_json::json!({"order": {"ref": value(call.reference)}, "slots": slots});
        let blocks: Vec<String> = call
            .slots
            .iter()
            .map(|&(d, t)| {
                format!(
                    "{{\"date\":\"{}\",\"type\":\"{}\",\"ok\":true}}",
                    written(value(d), writing),
                    written(value(t), writing)
                )
            })
            .collect();
        let response = format!(
            "{{\"ref\":\"{}\",\"booked\":[{}]}}",
            written(value(call.reference), writing),
            blocks.join(",")
        );
        (request.to_string(), response)
    } else {
        let (p, ns) = if prefix {
            (
                "s:",
                " xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"",
            )
        } else {
            ("", "")
        };
        let field = |name: &str, v: &str| {
            let text = if call.in_cdata && !v.contains("]]>") {
                format!("<![CDATA[{v}]]>")
            } else {
                xml_escaped(v, false)
            };
            format!("<o:{name}>{text}</o:{name}>")
        };
        let slots: String = call
            .slots
            .iter()
            .map(|&(d, t)| {
                format!(
                    "<o:slot>{}{}</o:slot>",
                    field("date", value(d)),
                    field("type", value(t))
                )
            })
            .collect();
        let request = format!(
            "<{p}Envelope{ns} xmlns:o=\"urn:o\"><{p}Body><o:Book>{}{slots}</o:Book></{p}Body></{p}Envelope>",
            field("ref", value(call.reference))
        );
        let blocks: String = call
            .slots
            .iter()
            .map(|&(d, t)| {
                format!(
                    "<booked><date>{}</date><type>{}</type></booked>",
                    written(value(d), writing),
                    written(value(t), writing)
                )
            })
            .collect();
        let response = format!(
            "<Envelope><Body><BookResponse><ref>{}</ref>{blocks}</BookResponse></Body></Envelope>",
            written(value(call.reference), writing)
        );
        (request, response)
    };
    let response = if call.odd {
        format!(
            "{response}<!-- {} -->",
            call.slots.len() * 7 + call.reference
        )
    } else {
        response
    };
    ObservedExchange::new(
        HashMap::new(),
        HashMap::new(),
        request.as_bytes(),
        None,
        200,
        HashMap::new(),
        response.as_bytes(),
        None,
    )
}

fn arb_repeat_calls() -> impl Strategy<Value = Vec<(String, ObservedExchange)>> {
    let call = (
        0..REPEATED.len(),
        prop::collection::vec((0..REPEATED.len(), 0..REPEATED.len()), 0..4),
        prop::bool::weighted(0.2),
        prop::bool::weighted(0.05),
    )
        .prop_map(|(reference, slots, in_cdata, odd)| RepeatCall {
            reference,
            slots,
            in_cdata,
            odd,
        });
    (
        prop::sample::select(vec![
            Writing::XmlText,
            Writing::XmlText,
            Writing::XmlAll,
            Writing::Cdata,
            Writing::Json,
            Writing::Json,
        ]),
        any::<bool>(),
        // Some operations hold no list at all: the values alone.
        prop::bool::weighted(0.3),
        prop::collection::vec(call, 3..8),
    )
        .prop_map(|(writing, prefix, listless, mut calls)| {
            if listless {
                calls.iter_mut().for_each(|call| call.slots.clear());
            }
            calls
                .iter()
                .map(|call| ("/ws/a".to_string(), repeat_exchange(call, writing, prefix)))
                .collect()
        })
}

#[test]
fn rules_that_repeat_their_request_answer_every_call_they_come_from_as_observed() {
    use proptest::test_runner::TestRunner;
    use std::cell::Cell;
    let config = crate::test_seed::config(256, 0x7265_7065_6174_7321);
    let (values, lists, refused) = (Cell::new(0), Cell::new(0), Cell::new(0));
    let result = TestRunner::new(config).run(&arb_repeat_calls(), |calls| {
        let endpoints = posted_endpoints(&calls);
        let replayed = super::replay::check(&endpoints);
        prop_assert!(replayed.is_ok(), "{}", replayed.unwrap_err());
        for suggestion in suggest_for_service(&endpoints) {
            match &suggestion.outcome {
                Suggestion::Echo { rule } if rule.list.is_some() => lists.set(lists.get() + 1),
                Suggestion::Echo { rule } if !rule.repeats.is_empty() => {
                    values.set(values.get() + 1)
                }
                _ => refused.set(refused.get() + 1),
            }
            for rule in suggestion_rules(&suggestion.outcome) {
                for fragment in &rule.response.body {
                    if let BodyFragment::Template { template } = fragment {
                        let written = template == "{{script.items}}"
                            || template.starts_with("{{xpath.Envelope/Body/Book/")
                            || template.starts_with("{{body./");
                        prop_assert!(
                            written,
                            "a template not written by the suggestion: {template}"
                        );
                    }
                }
            }
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
    // The cases the judge must see, for the written seed.
    assert!(
        values.get() >= 35 && lists.get() >= 80 && refused.get() >= 50,
        "values {values:?}, lists {lists:?}, others {refused:?}"
    );
}

/// Where a call carries the identifier unique to it.
#[derive(Debug, Clone, Copy)]
enum IdentifierAt {
    Path,
    Header,
    Xml,
    NestedJson,
    TopJson,
}

#[test]
fn an_identifier_unique_to_each_call_never_becomes_a_condition() {
    // The answer depends on a variable the request does not hold, with fewer values than there are calls; each call
    // carries an identifier of its own. The identifier fits any sample, value for answer, and proves nothing.
    use proptest::test_runner::TestRunner;
    let config = crate::test_seed::config(256, 0x6869_6464_656e_2169);
    let strategy = (
        prop::sample::select(vec![
            IdentifierAt::Path,
            IdentifierAt::Header,
            IdentifierAt::Xml,
            IdentifierAt::NestedJson,
            IdentifierAt::TopJson,
        ]),
        prop::collection::vec(0usize..2, 3..12),
        any::<u32>(),
    );
    let result = TestRunner::new(config).run(&strategy, |(at, hidden, salt)| {
        let calls: Vec<(String, ObservedExchange)> = hidden
            .iter()
            .enumerate()
            .map(|(i, &h)| {
                let id = format!("{}", 100_000 + (salt as usize % 50_000) * 16 + i);
                let (path, body) = match at {
                    IdentifierAt::Path => (format!("/orders/{id}"), String::new()),
                    IdentifierAt::Header => ("/orders".to_string(), String::new()),
                    IdentifierAt::Xml => (
                        "/ws".to_string(),
                        format!(
                            "<Envelope><Header><MessageID>{id}</MessageID></Header><Body><Ping/></Body></Envelope>"
                        ),
                    ),
                    IdentifierAt::NestedJson => {
                        ("/orders".to_string(), format!(r#"{{"meta":{{"id":"{id}"}}}}"#))
                    }
                    IdentifierAt::TopJson => ("/orders".to_string(), format!(r#"{{"id":"{id}"}}"#)),
                };
                let headers: HashMap<String, String> = match at {
                    IdentifierAt::Header => HashMap::from([("x-message-tag".to_string(), id)]),
                    _ => HashMap::new(),
                };
                let answer = ["<r>accepted</r>", "<r>refused</r>"][h];
                let exchange = ObservedExchange::new(
                    HashMap::new(),
                    headers,
                    body.as_bytes(),
                    None,
                    200,
                    HashMap::new(),
                    answer.as_bytes(),
                    Some("text/xml".into()),
                );
                (path, exchange)
            })
            .collect();
        let endpoints = posted_endpoints(&calls);
        let replayed = super::replay::check(&endpoints);
        prop_assert!(replayed.is_ok(), "{}", replayed.unwrap_err());
        for suggestion in suggest_for_service(&endpoints) {
            for rule in suggestion_rules(&suggestion.outcome) {
                for condition in rule.conditions.all_of.iter().chain(&rule.conditions.any_of) {
                    let on_identifier = match (&condition.source, at) {
                        (ConditionSource::PathParam(_), IdentifierAt::Path) => true,
                        (ConditionSource::Header(k), IdentifierAt::Header) => k == "x-message-tag",
                        (ConditionSource::XPath(k), IdentifierAt::Xml) => k == "Envelope/Header/MessageID",
                        (ConditionSource::JsonPointer(k), IdentifierAt::NestedJson) => k == "/meta/id",
                        (ConditionSource::JsonPointer(k), IdentifierAt::TopJson) => k == "/id",
                        _ => false,
                    };
                    prop_assert!(!on_identifier, "a condition on the identifier: {rule:?}");
                }
            }
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
}
