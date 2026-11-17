use super::*;
use crate::engine::escape::Escaping;
use crate::server::observation::{ObservationKey, ObservedExchange};
use crate::server::suggestion::{
    EndpointSuggestion, SuggestedRule, Suggestion, suggest_for_service,
};
use proptest::prelude::*;

fn posted(request: &str, response: &str) -> ObservedExchange {
    ObservedExchange::new(
        HashMap::new(),
        HashMap::new(),
        request.as_bytes(),
        Some("text/xml".into()),
        200,
        HashMap::new(),
        response.as_bytes(),
        Some("text/xml".into()),
    )
}

fn endpoints(calls: Vec<ObservedExchange>) -> Vec<(ObservationKey, Vec<ObservedExchange>)> {
    vec![(
        ObservationKey {
            group_name: None,
            service_name: "svc".into(),
            method: "POST".into(),
            sub_path: "/ws/orders".into(),
            operation: None,
        },
        calls,
    )]
}

fn envelope(operation: &str, inner: &str) -> String {
    format!(
        "<soap:Envelope xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:o=\"urn:orders\">\
         <soap:Body><o:{operation}>{inner}</o:{operation}></soap:Body></soap:Envelope>"
    )
}

fn answer(inner: &str) -> String {
    format!(
        "<soap:Envelope xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\"><soap:Body>{inner}</soap:Body>\
         </soap:Envelope>"
    )
}

/// The one rule, without condition but its operation, of the one suggestion: `Echo` when 0.5.0 could say it, `Rules`
/// otherwise.
fn echo_rule(suggestions: &[EndpointSuggestion]) -> &SuggestedRule {
    let rule = match suggestions {
        [
            EndpointSuggestion {
                outcome: Suggestion::Echo { rule },
                ..
            },
        ] => rule.as_ref(),
        [
            EndpointSuggestion {
                outcome: Suggestion::Rules { rules },
                ..
            },
        ] if rules.len() == 1 => &rules[0],
        other => panic!("expected one rule that repeats the request, got {other:?}"),
    };
    assert!(
        rule.conditions.any_of.is_empty()
            && rule
                .conditions
                .all_of
                .iter()
                .all(|c| c.operator == crate::models::Operator::Exists),
        "{rule:?}"
    );
    rule
}

/// The expression that repeats an XML field, escaped for the text of an element.
fn xml_value(path: &str) -> String {
    format!(r#"{{{{xpath.{path} | xml_text}}}}"#)
}

fn status_call(reference: &str, date: &str) -> ObservedExchange {
    posted(
        &envelope(
            "GetStatus",
            &format!("<o:ref>{reference}</o:ref><o:asOf>{date}</o:asOf>"),
        ),
        &answer(&format!(
            "<GetStatusResponse><ref>{reference}</ref><state>OPEN</state><asOf>{date}</asOf>\
             <ref2>{reference}</ref2></GetStatusResponse>"
        )),
    )
}

#[test]
fn a_response_that_repeats_several_fields_of_its_request_reads_each_of_them() {
    let endpoints = endpoints(vec![
        status_call("R-100", "2026-01-05"),
        status_call("R-200", "2026-01-07"),
        status_call("R-300", "2026-02-11"),
    ]);
    let suggestions = suggest_for_service(&endpoints);
    let rule = echo_rule(&suggestions);
    let template = |t: &str| BodyFragment::Template { template: t.into() };
    let literal = |v: &str| BodyFragment::Literal { value: v.into() };
    assert_eq!(
        rule.response.body,
        vec![
            literal(
                "<soap:Envelope xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\"><soap:Body>\
                 <GetStatusResponse><ref>"
            ),
            template(&xml_value("Envelope/Body/GetStatus/ref")),
            literal("</ref><state>OPEN</state><asOf>"),
            template(&xml_value("Envelope/Body/GetStatus/asOf")),
            literal("</asOf><ref2>"),
            template(&xml_value("Envelope/Body/GetStatus/ref")),
            literal("</ref2></GetStatusResponse></soap:Body></soap:Envelope>"),
        ]
    );
    assert_eq!(
        rule.repeats,
        vec![
            ConditionSource::XPath("Envelope/Body/GetStatus/ref".into()),
            ConditionSource::XPath("Envelope/Body/GetStatus/asOf".into()),
        ]
    );
    assert_eq!(rule.echo, None);
    assert_eq!(rule.script, None);
    assert_eq!(crate::server::suggestion::replay::check(&endpoints), Ok(()));
}

#[test]
fn a_value_seen_with_one_content_stays_literal() {
    // The date is the same in every call: nothing shows that the response repeats it rather than writes it.
    let endpoints = endpoints(vec![
        status_call("R-100", "2026-01-05"),
        status_call("R-200", "2026-01-05"),
        status_call("R-300", "2026-01-05"),
    ]);
    let suggestions = suggest_for_service(&endpoints);
    let rule = echo_rule(&suggestions);
    assert_eq!(
        rule.repeats,
        vec![ConditionSource::XPath("Envelope/Body/GetStatus/ref".into())]
    );
    assert_eq!(
        rule.echo,
        Some(ConditionSource::XPath("Envelope/Body/GetStatus/ref".into()))
    );
}

#[test]
fn a_response_that_differs_elsewhere_keeps_one_rule_per_value() {
    // The state changes with the reference too, and the request does not hold it: no rule may repeat the reference.
    let mut calls = vec![
        status_call("R-100", "2026-01-05"),
        status_call("R-200", "2026-01-07"),
        status_call("R-300", "2026-02-11"),
    ];
    calls[1].response_body = calls[1].response_body.replace("OPEN", "CLOSED");
    let suggestions = suggest_for_service(&endpoints(calls));
    let rules = crate::server::suggestion::replay::rules_of(&suggestions[0].outcome);
    assert!(
        rules.iter().all(|r| r.repeats.is_empty()),
        "{suggestions:?}"
    );
    assert!(
        !matches!(suggestions[0].outcome, Suggestion::Echo { .. }),
        "{suggestions:?}"
    );
}

/// The request of `book_call`, whole: an observed exchange keeps 16 KiB of it.
fn book_request(slots: &[(&str, &str)]) -> String {
    let request: String = slots
        .iter()
        .map(|(date, kind)| {
            format!("<o:slot><o:date>{date}</o:date><o:type>{kind}</o:type></o:slot>")
        })
        .collect();
    envelope("Book", &format!("<o:ref>B1</o:ref>{request}"))
}

fn book_call(slots: &[(&str, &str)]) -> ObservedExchange {
    let request: String = slots
        .iter()
        .map(|(date, kind)| {
            format!("<o:slot><o:date>{date}</o:date><o:type>{kind}</o:type></o:slot>")
        })
        .collect();
    let blocks: String = slots
        .iter()
        .map(|(date, kind)| {
            format!(
                "<booked><date>{}</date><type>{}</type><state>OK</state></booked>",
                Escaping::XmlText.apply(&quick_unescape(date)),
                Escaping::XmlText.apply(&quick_unescape(kind))
            )
        })
        .collect();
    posted(
        &envelope("Book", &format!("<o:ref>B1</o:ref>{request}")),
        &answer(&format!("<BookResponse>{blocks}</BookResponse>")),
    )
}

/// The text of an XML value written with `&amp;`, `&lt;` and `&gt;`.
fn quick_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// What the rule answers to `request`, rendered by the engine as the interception does.
fn rendered(rule: &SuggestedRule, request: &str) -> String {
    let empty = HashMap::new();
    let result = rule.script.as_ref().map(|code| {
        engine()
            .execute(
                code,
                &ScriptContext {
                    body: request.to_string(),
                    headers: HashMap::new(),
                    query_params: HashMap::new(),
                    path_params: HashMap::new(),
                },
            )
            .unwrap()
    });
    let ctx = TemplateContext {
        path_params: &empty,
        query_params: &empty,
        headers: &empty,
        request_body: request.as_bytes(),
        seq_counter: 0,
        script_result: result.as_ref(),
    };
    TemplateRenderer::render_body(&rule.response.body, &[], &ctx)
}

#[test]
fn a_response_with_a_block_per_item_of_the_request_gets_a_script_that_writes_them() {
    let endpoints = endpoints(vec![
        book_call(&[("2026-03-01", "A"), ("2026-03-02", "B")]),
        book_call(&[("2026-04-09", "C")]),
        book_call(&[
            ("2026-05-01", "A"),
            ("2026-05-02", "A"),
            ("2026-05-03", "B"),
        ]),
    ]);
    let suggestions = suggest_for_service(&endpoints);
    let rule = echo_rule(&suggestions);
    let list = rule.list.as_ref().expect("a list");
    assert_eq!(
        list.items,
        ConditionSource::XPath("Envelope/Body/Book/slot".into())
    );
    assert_eq!(
        list.fields,
        vec!["date".into(), "type".into()] as Vec<String>
    );
    assert!(rule.script.is_some());
    assert_eq!(crate::server::suggestion::replay::check(&endpoints), Ok(()));

    // Requests never seen: no item, one, four, values to escape.
    for slots in [
        vec![],
        vec![("2027-01-01", "Z")],
        vec![
            ("2027-02-01", "a&amp;b"),
            ("2027-02-02", "&lt;x&gt;"),
            ("2027-02-03", "\"q\""),
            ("2027-02-04", "]]&gt;{{uuid}}"),
        ],
    ] {
        let expected = book_call(&slots);
        assert_eq!(
            rendered(rule, &expected.request_body),
            expected.response_body,
            "{slots:?}"
        );
    }
}

fn json_call(reference: &str, lines: &[(&str, i64)]) -> ObservedExchange {
    let request = serde_json::json!({
        "order": { "ref": reference },
        "lines": lines.iter().map(|(sku, qty)| serde_json::json!({"sku": sku, "qty": qty})).collect::<Vec<_>>(),
    });
    let blocks: Vec<String> = lines
        .iter()
        .map(|(sku, qty)| {
            format!(
                "{{\"sku\":{},\"qty\":{qty},\"reserved\":true}}",
                serde_json::Value::from(*sku)
            )
        })
        .collect();
    let mut e = posted(
        &request.to_string(),
        &format!(
            "{{\"ref\":{},\"lines\":[{}]}}",
            serde_json::Value::from(reference),
            blocks.join(",")
        ),
    );
    e.response_content_type = Some("application/json".into());
    e
}

#[test]
fn a_json_array_answered_with_an_object_per_item_gets_a_script_too() {
    let endpoints = endpoints(vec![
        json_call("O-1", &[("S-1", 2), ("S-2", 5)]),
        json_call("O-2", &[("S-9", 1)]),
        json_call("O-3", &[("S-4", 3), ("S-5", 1), ("S-6", 7)]),
    ]);
    let suggestions = suggest_for_service(&endpoints);
    let rule = echo_rule(&suggestions);
    let list = rule.list.as_ref().expect("a list");
    assert_eq!(list.items, ConditionSource::JsonPointer("/lines".into()));
    assert_eq!(list.fields, vec!["sku".into(), "qty".into()] as Vec<String>);
    assert_eq!(
        rule.repeats,
        vec![ConditionSource::JsonPointer("/order/ref".into())]
    );
    assert_eq!(crate::server::suggestion::replay::check(&endpoints), Ok(()));
    for (reference, lines) in [
        ("O-new", vec![]),
        ("O-\"q\"", vec![("S-\\x\"\n", 4), ("S-é", 0)]),
    ] {
        let expected = json_call(reference, &lines);
        assert_eq!(
            rendered(rule, &expected.request_body),
            expected.response_body
        );
    }
}

#[test]
fn text_from_the_traffic_never_becomes_code_nor_a_template() {
    // The backend writes, in each block and around the list, what would be code in a script or a template if copied
    // as it is; the request names a field with quotes and a backslash. A text response, where values are written as
    // they are: the literal text is all the more hostile.
    let hostile = "\"); throw \"x\"; let a = (\"\\ ${x} `y` {{uuid}} }}\n\r\t\u{0}é";
    let call = |items: &[(&str, &str)]| {
        let request = serde_json::json!({
            "lines": items.iter().map(|(v, w)| serde_json::json!({ "a\"]);\\": v, "b": w })).collect::<Vec<_>>(),
        });
        let blocks: Vec<String> = items
            .iter()
            .map(|(v, w)| format!("{{{{\"{hostile}\":{v}, \"w\": \"{w}\"}}}}"))
            .collect();
        let mut e = posted(
            &request.to_string(),
            &format!("[{{{{uuid}}}} {hostile}{}]", blocks.join(hostile)),
        );
        e.response_content_type = Some("text/plain".into());
        e
    };
    let endpoints = endpoints(vec![
        call(&[("x1", "p"), ("x2", "q")]),
        call(&[(hostile, "r")]),
        call(&[("y1", "s"), ("y\"2", "t"), ("y3", "u")]),
    ]);
    let suggestions = suggest_for_service(&endpoints);
    let rule = echo_rule(&suggestions);
    let script = rule.script.as_ref().unwrap();
    assert!(engine().validate(script).is_ok(), "{script}");
    for fragment in &rule.response.body {
        if let BodyFragment::Template { template } = fragment {
            assert_eq!(template, "{{script.items}}");
        }
    }
    // Every observed text is inside a string literal of the script (`a_rhai_string_literal_reads_back_any_text`): its
    // quotes, backslashes and line breaks are escapes, so the text never shows up as it is.
    assert!(!script.contains(hostile), "{script}");
    assert_eq!(crate::server::suggestion::replay::check(&endpoints), Ok(()));
}

proptest! {
    #![proptest_config(crate::test_seed::config(256, 0x7268_6169_5f73_7472))]

    #[test]
    fn a_rhai_string_literal_reads_back_any_text(text in "\\PC{0,40}|[\"\\\\$`{}\n\r\t\u{0}-\u{1f}é😀]{0,20}") {
        let script = format!("#{{ v: {} }}", rhai_string(&text));
        let ctx = ScriptContext {
            body: String::new(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            path_params: HashMap::new(),
        };
        let result = engine().execute(&script, &ctx);
        prop_assert!(result.is_ok(), "{script}: {result:?}");
        prop_assert_eq!(result.unwrap().fields.get("v").cloned(), Some(text));
    }
}

/// Calls of a list of `items` to `items + 2` slots, six of them.
fn long_lists(items: usize) -> Vec<ObservedExchange> {
    (0..6)
        .map(|c| {
            let slots: Vec<(String, String)> = (0..items + c % 3)
                .map(|j| (format!("2026-{c:02}-{j:04}"), format!("T{}", j % 7)))
                .collect();
            let refs: Vec<(&str, &str)> = slots
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect();
            book_call(&refs)
        })
        .collect()
}

#[test]
fn a_list_rule_says_how_many_items_its_script_writes() {
    // The script runs within the limits of any script (script_budget.rs): two fields per item read by
    // `parse_xml_items`, 500 fields at most, hold 250 items. The rule says so, and says it right.
    let suggestions = suggest_for_service(&endpoints(long_lists(10)));
    let rule = echo_rule(&suggestions);
    let max_items = rule.list.as_ref().unwrap().max_items;
    assert_eq!(max_items, 250);
    let slots = |n: usize| -> Vec<(String, String)> {
        (0..n)
            .map(|j| (format!("2027-01-{j:04}"), format!("T{}", j % 7)))
            .collect()
    };
    for (n, whole) in [(max_items, true), (max_items + 1, false)] {
        let slots = slots(n);
        let refs: Vec<(&str, &str)> = slots
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let ctx = ScriptContext {
            body: book_request(&refs),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            path_params: HashMap::new(),
        };
        assert_eq!(
            engine()
                .execute(rule.script.as_ref().unwrap(), &ctx)
                .is_ok(),
            whole,
            "{n} items"
        );
    }
    // Lists observed past it are not generalized.
    let suggestions = suggest_for_service(&endpoints(long_lists(300)));
    let rules = crate::server::suggestion::replay::rules_of(&suggestions[0].outcome);
    assert!(
        rules
            .iter()
            .all(|r| r.script.is_none() && r.repeats.is_empty()),
        "{:?}",
        suggestions[0].outcome
    );
}

// --- Each value escaped for the place it lands, whatever value comes

/// Calls whose request sends `values` (a JSON object `{"v": …}` per call) and whose response is `answer(value)`, with
/// the content type `content_type`.
fn json_calls(
    values: &[serde_json::Value],
    content_type: &str,
    answer: impl Fn(&serde_json::Value) -> String,
) -> Vec<ObservedExchange> {
    values
        .iter()
        .map(|v| {
            let mut e = posted(&serde_json::json!({ "v": v }).to_string(), &answer(v));
            e.response_content_type = Some(content_type.into());
            e
        })
        .collect()
}

fn xml_value_calls(answer: impl Fn(&str) -> String) -> Vec<ObservedExchange> {
    ["R-1", "R-2", "R-3"]
        .iter()
        .map(|v| {
            posted(
                &envelope("GetStatus", &format!("<o:ref>{v}</o:ref>")),
                &answer(v),
            )
        })
        .collect()
}

/// What the one rule of `calls` answers to `request`.
fn answer_to(calls: Vec<ObservedExchange>, request: &str) -> String {
    let suggestions = suggest_for_service(&endpoints(calls));
    rendered(echo_rule(&suggestions), request)
}

#[test]
fn a_value_in_an_attribute_has_its_quotes_escaped_whatever_was_observed() {
    // No observed value held a quote: the attribute still gets `&quot;` and `&apos;`, and no value adds an attribute.
    let calls = xml_value_calls(|v| answer(&format!("<status id=\"{v}\">OPEN</status>")));
    for value in ["a\"b", "x\" admin=\"true", "it's", "<&>"] {
        let request = envelope(
            "GetStatus",
            &format!("<o:ref>{}</o:ref>", Escaping::XmlText.apply(value)),
        );
        assert_eq!(
            answer_to(calls.clone(), &request),
            answer(&format!(
                "<status id=\"{}\">OPEN</status>",
                Escaping::XmlAttr.apply(value)
            ))
        );
    }
}

#[test]
fn a_value_in_a_cdata_section_keeps_it_whole() {
    let calls = xml_value_calls(|v| answer(&format!("<note><![CDATA[{v}]]></note>")));
    for value in ["a]]>b", "x&y<z>", "]]]>"] {
        let request = envelope(
            "GetStatus",
            &format!("<o:ref>{}</o:ref>", Escaping::XmlText.apply(value)),
        );
        assert_eq!(
            answer_to(calls.clone(), &request),
            answer(&format!(
                "<note><![CDATA[{}]]></note>",
                Escaping::Cdata.apply(value)
            ))
        );
    }
}

#[test]
fn a_value_in_a_json_string_has_every_control_character_escaped() {
    let observed = ["a", "b", "c"].map(serde_json::Value::from);
    let write = |v: &serde_json::Value| serde_json::json!({ "echo": v }).to_string();
    let calls = json_calls(&observed, "application/json", write);
    for value in ["\u{1}", "x\"y\\z", "line\nfeed\u{1f}"] {
        let v = serde_json::Value::from(value);
        let got = answer_to(calls.clone(), &serde_json::json!({ "v": v }).to_string());
        assert_eq!(got, write(&v));
    }
    // A response that is a JSON string itself is JSON, not text.
    let calls = json_calls(&observed, "application/json", |v| v.to_string());
    let got = answer_to(calls, r#"{"v":"q\"uote"}"#);
    assert_eq!(serde_json::from_str::<String>(&got).unwrap(), "q\"uote");
}

#[test]
fn a_value_out_of_a_json_string_is_never_written_by_a_template() {
    // `{"n":2}` from `{"v":2}`: a template would write `{"n":abc}` for `{"v":"abc"}`. The value is not generalized.
    let observed = [2, 3, 5].map(serde_json::Value::from);
    let calls = json_calls(&observed, "application/json", |v| format!("{{\"n\":{v}}}"));
    let suggestions = suggest_for_service(&endpoints(calls));
    for rule in crate::server::suggestion::replay::rules_of(&suggestions[0].outcome) {
        assert!(rule.repeats.is_empty(), "{rule:?}");
    }
}

#[test]
fn a_field_of_an_item_out_of_a_json_string_is_written_as_the_json_value_of_the_request() {
    let endpoints = endpoints(vec![
        json_call("O-1", &[("S-1", 2), ("S-2", 5)]),
        json_call("O-2", &[("S-9", 1)]),
        json_call("O-3", &[("S-4", 3), ("S-5", 1), ("S-6", 7)]),
    ]);
    let suggestions = suggest_for_service(&endpoints);
    let rule = echo_rule(&suggestions);
    for (qty, written) in [
        (r#""abc""#, r#""abc""#),
        ("null", "null"),
        (r#"{"a":1}"#, r#"{"a":1}"#),
        ("1.5", "1.5"),
    ] {
        let request = format!(r#"{{"order":{{"ref":"O-9"}},"lines":[{{"sku":"S","qty":{qty}}}]}}"#);
        let got = rendered(rule, &request);
        assert!(
            serde_json::from_str::<serde_json::Value>(&got).is_ok(),
            "{got}"
        );
        assert!(got.contains(&format!("\"qty\":{written}")), "{got}");
    }
    let missing = rendered(rule, r#"{"order":{"ref":"O-9"},"lines":[{"sku":"S"}]}"#);
    assert!(missing.contains("\"qty\":null"), "{missing}");
}

#[test]
fn the_checks_stop_at_their_budget_of_renderings() {
    // Three calls, the values alone, one rendering each: three renderings generalize, two do not (the conditional
    // rules of before stay).
    let endpoints = endpoints(vec![
        status_call("R-100", "2026-01-05"),
        status_call("R-200", "2026-01-07"),
        status_call("R-300", "2026-02-11"),
    ]);
    let generalized = |budget: usize| {
        let suggestions =
            crate::server::suggestion::suggest_within(&endpoints, &Renders::new(budget));
        crate::server::suggestion::replay::rules_of(&suggestions[0].outcome)
            .iter()
            .any(|r| !r.repeats.is_empty())
    };
    assert!(generalized(3));
    assert!(!generalized(2));
}

#[test]
fn items_with_values_longer_than_a_value_followed_are_not_generalized() {
    let calls = |length: usize| {
        let value = |c: usize, j: usize| format!("{c}{j}{}", "v".repeat(length - 2));
        (0..3)
            .map(|c| {
                let slots: Vec<(String, String)> = (0..c + 1)
                    .map(|j| (value(c, j), format!("T{c}{j}")))
                    .collect();
                let refs: Vec<(&str, &str)> = slots
                    .iter()
                    .map(|(a, b)| (a.as_str(), b.as_str()))
                    .collect();
                book_call(&refs)
            })
            .collect::<Vec<_>>()
    };
    let listed = |length: usize| {
        let suggestions = suggest_for_service(&endpoints(calls(length)));
        crate::server::suggestion::replay::rules_of(&suggestions[0].outcome)
            .iter()
            .any(|r| r.list.is_some())
    };
    assert!(listed(MAX_VALUE));
    assert!(!listed(MAX_VALUE + 1));
}
