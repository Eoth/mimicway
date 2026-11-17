use super::*;
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

fn echo_rule(suggestions: &[EndpointSuggestion]) -> &SuggestedRule {
    match suggestions {
        [
            EndpointSuggestion {
                outcome: Suggestion::Echo { rule },
                ..
            },
        ] => rule,
        other => panic!("expected one echo suggestion, got {other:?}"),
    }
}

/// The expression that repeats an XML field, escaped for the text of an element.
fn xml_value(path: &str) -> String {
    format!(
        r#"{{{{xpath.{path} | replace("&", "&amp;") | replace("<", "&lt;") | replace(">", "&gt;")}}}}"#
    )
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
    assert!(
        matches!(suggestions[0].outcome, Suggestion::Conditional { .. }),
        "{suggestions:?}"
    );
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
                Escape::XmlText.apply(&quick_unescape(date)),
                Escape::XmlText.apply(&quick_unescape(kind))
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
    assert_eq!(
        rule.list,
        Some(RepeatedList {
            items: ConditionSource::XPath("Envelope/Body/Book/slot".into()),
            fields: vec!["date".into(), "type".into()],
        })
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
    assert_eq!(
        rule.list,
        Some(RepeatedList {
            items: ConditionSource::JsonPointer("/lines".into()),
            fields: vec!["sku".into(), "qty".into()],
        })
    );
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
    // as it is; the request names a field with quotes and a backslash.
    let hostile = "\"); throw \"x\"; let a = (\"\\ ${x} `y` {{uuid}} }}\n\r\t\u{0}é";
    let call = |items: &[(&str, &str)]| {
        let request = serde_json::json!({
            "lines": items.iter().map(|(v, w)| serde_json::json!({ "a\"]);\\": v, "b": w })).collect::<Vec<_>>(),
        });
        let blocks: Vec<String> = items
            .iter()
            .map(|(v, w)| {
                let quoted = serde_json::Value::from(*v).to_string();
                format!("{{{{\"{hostile}\":{quoted}, \"w\": \"{w}\"}}}}")
            })
            .collect();
        let mut e = posted(
            &request.to_string(),
            &format!("[{{{{uuid}}}} {hostile}{}]", blocks.join(hostile)),
        );
        e.response_content_type = Some("application/json".into());
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
fn a_list_of_a_hundred_items_stays_within_the_script_budget() {
    // The script runs on every call within the operations a script may run (script_budget.rs): about 60 per item of
    // two fields. Past that, the observed calls are not rendered back and nothing is suggested, as the docs say.
    let suggestions = suggest_for_service(&endpoints(long_lists(100)));
    assert!(echo_rule(&suggestions).list.is_some());
    let suggestions = suggest_for_service(&endpoints(long_lists(200)));
    assert!(
        !matches!(&suggestions[0].outcome, Suggestion::Echo { .. }),
        "{:?}",
        suggestions[0].outcome
    );
}
