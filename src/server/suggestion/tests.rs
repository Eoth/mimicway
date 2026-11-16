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

fn xml(status: u16, body: &str) -> ObservedExchange {
    let mut e = exchange(status, body);
    e.response_content_type = Some("text/xml; charset=utf-8".into());
    e
}

fn key(method: &str, path: &str) -> ObservationKey {
    ObservationKey {
        group_name: None,
        service_name: "svc".into(),
        method: method.into(),
        sub_path: path.into(),
    }
}

/// Every exchange observed on its own literal path (a `GET`).
fn observed(calls: Vec<(&str, ObservedExchange)>) -> Vec<(ObservationKey, Vec<ObservedExchange>)> {
    let mut endpoints: Vec<(ObservationKey, Vec<ObservedExchange>)> = Vec::new();
    for (path, e) in calls {
        match endpoints.iter_mut().find(|(k, _)| k.sub_path == path) {
            Some((_, list)) => list.push(e),
            None => endpoints.push((key("GET", path), vec![e])),
        }
    }
    endpoints
}

/// The outcome for observations of one literal path, as the suggestions of a service with that path only.
fn suggest(method: &str, path: &str, observations: &[ObservedExchange]) -> Option<Suggestion> {
    let suggestions = suggest_for_service(&[(key(method, path), observations.to_vec())]);
    assert!(suggestions.len() <= 1, "{suggestions:?}");
    suggestions.into_iter().next().map(|s| s.outcome)
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
            assert_eq!(rule.sub_path, "/orders");
        }
        other => panic!("expected Unconditional, got {other:?}"),
    }
}

#[test]
fn suggested_response_never_includes_credential_headers() {
    // A rule saved from a suggestion is served to anyone: a Set-Cookie of the real backend would hand out a real
    // session.
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
fn a_suggested_header_answers_what_the_backend_sent_even_when_it_looks_like_a_template() {
    // Header values are rendered when a rule answers: a backend's `{{seq}}` must come out as written, never as a
    // counter.
    let mut e = exchange(200, "ok");
    e.response_headers
        .insert("x-template".into(), "{{seq}} and {{{{uuid}}}}".into());
    let obs = vec![e.clone(), e.clone(), e];
    let Suggestion::Unconditional { rule } = suggest("GET", "/orders", &obs).unwrap() else {
        panic!("expected an unconditional suggestion");
    };
    let empty = HashMap::new();
    let ctx = crate::engine::TemplateContext {
        path_params: &empty,
        query_params: &empty,
        headers: &empty,
        request_body: b"",
        seq_counter: 3,
        script_result: None,
    };
    assert_eq!(
        crate::engine::TemplateRenderer::render_headers(&rule.response.headers, &ctx),
        vec![(
            "x-template".to_string(),
            "{{seq}} and {{{{uuid}}}}".to_string()
        )]
    );
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

    let Suggestion::Unconditional { rule } = suggest("GET", "/orders", &obs).unwrap() else {
        panic!("expected an unconditional suggestion");
    };
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

#[test]
fn variance_explained_by_query_param_suggests_conditional_rules() {
    let obs = vec![
        exchange_with_query(200, r#"{"stock":true}"#, &[("id", "1")]),
        exchange_with_query(200, r#"{"stock":true}"#, &[("id", "1")]),
        exchange_with_query(404, "not found", &[("id", "2")]),
        exchange_with_query(404, "not found", &[("id", "2")]),
    ];
    let Suggestion::Conditional { rules } = suggest("GET", "/orders", &obs).unwrap() else {
        panic!("expected a conditional suggestion");
    };
    assert_eq!(rules.len(), 2);
    let statuses: Vec<u16> = rules.iter().map(|r| r.response.status).collect();
    assert!(statuses.contains(&200));
    assert!(statuses.contains(&404));
    for rule in &rules {
        let cond = rule.condition.as_ref().unwrap();
        assert_eq!(cond.source, ConditionSource::QueryParam("id".into()));
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
    let Suggestion::Conditional { rules } = suggest("POST", "/toggle", &obs).unwrap() else {
        panic!("expected a conditional suggestion");
    };
    assert_eq!(rules.len(), 2);
    for rule in &rules {
        let cond = rule.condition.as_ref().unwrap();
        assert_eq!(cond.source, ConditionSource::JsonPointer("/mode".into()));
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
    let Suggestion::Conditional { rules } = suggest("GET", "/status", &obs).unwrap() else {
        panic!("expected a conditional suggestion");
    };
    assert_eq!(rules.len(), 2);
    for rule in &rules {
        let cond = rule.condition.as_ref().unwrap();
        assert_eq!(cond.source, ConditionSource::Header("x-env".into()));
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
        exchange(200, "response-1"),
        exchange(200, "response-2"),
        exchange(200, "response-3"),
    ];
    let suggestions = suggest_for_service(&[(key("GET", "/flaky"), obs)]);
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].sample_count, 3);
    match &suggestions[0].outcome {
        Suggestion::VarianceUnexplained {
            response_class_count,
        } => assert_eq!(*response_class_count, 3),
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

// --- Route patterns

#[test]
fn paths_that_differ_by_an_identifier_pool_their_samples() {
    // One call per order: no literal path reaches the three samples a suggestion waits for, the route does.
    let suggestions = suggest_for_service(&observed(vec![
        ("/orders/1", exchange(200, "first")),
        ("/orders/2", exchange(200, "second")),
        ("/orders/3", exchange(404, "missing")),
    ]));
    assert_eq!(suggestions.len(), 1, "{suggestions:?}");
    let s = &suggestions[0];
    assert_eq!(s.sub_path, "/orders/{id}");
    assert_eq!(s.paths, vec!["/orders/1", "/orders/2", "/orders/3"]);
    assert_eq!((s.path_count, s.sample_count), (3, 3));
    let Suggestion::Conditional { rules } = &s.outcome else {
        panic!("expected a conditional suggestion, got {s:?}");
    };
    let conditions: Vec<(&ConditionSource, &Operator)> = rules
        .iter()
        .map(|r| {
            let c = r.condition.as_ref().unwrap();
            (&c.source, &c.operator)
        })
        .collect();
    let id = ConditionSource::PathParam("id".into());
    assert_eq!(
        conditions,
        vec![
            (&id, &Operator::Eq("1".into())),
            (&id, &Operator::Eq("2".into())),
            (&id, &Operator::Eq("3".into())),
        ]
    );
    assert!(rules.iter().all(|r| r.sub_path == "/orders/{id}"));
}

#[test]
fn the_same_answer_for_every_identifier_is_one_rule() {
    let suggestions = suggest_for_service(&observed(vec![
        ("/orders/1/status", exchange(200, "shipped")),
        ("/orders/2/status", exchange(200, "shipped")),
        ("/orders/3/status", exchange(200, "shipped")),
    ]));
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].sub_path, "/orders/{id}/status");
    assert!(matches!(
        suggestions[0].outcome,
        Suggestion::Unconditional { .. }
    ));
}

/// A service directory: one XML answer per service name, giving the address of that service.
fn directory_answer(name: &str) -> ObservedExchange {
    xml(
        200,
        &format!(
            "<soap:Envelope xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\"><soap:Body><lookupResponse>\
             <url>http://backend.internal:8080/ws/{name}</url><name>{name}</name></lookupResponse></soap:Body>\
             </soap:Envelope>"
        ),
    )
}

#[test]
fn a_response_that_repeats_the_value_that_varies_becomes_one_rule() {
    // Names are words, not identifiers: they become a parameter because their answers have the same structure. The
    // answers differ only by the name, which they repeat: one rule repeats it, where one rule per name would only
    // cover the names observed.
    let suggestions = suggest_for_service(&observed(vec![
        (
            "/service/9f86d081884c7d65/restOrders",
            directory_answer("restOrders"),
        ),
        (
            "/service/9f86d081884c7d65/restStock",
            directory_answer("restStock"),
        ),
        (
            "/service/9f86d081884c7d65/restBilling",
            directory_answer("restBilling"),
        ),
    ]));
    assert_eq!(suggestions.len(), 1, "{suggestions:?}");
    let s = &suggestions[0];
    assert_eq!(s.sub_path, "/service/{hash}/{name}");
    assert_eq!(s.path_count, 3);
    let Suggestion::Echo { rule } = &s.outcome else {
        panic!("expected an echo suggestion, got {s:?}");
    };
    assert_eq!(rule.condition, None);
    assert_eq!(rule.echo, Some(ConditionSource::PathParam("name".into())));
    let template = |t: &str| BodyFragment::Template { template: t.into() };
    let literal = |v: &str| BodyFragment::Literal { value: v.into() };
    assert_eq!(
        rule.response.body,
        vec![
            literal(
                "<soap:Envelope xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\"><soap:Body><lookupResponse>\
                 <url>http://backend.internal:8080/ws/"
            ),
            template("{{path.name}}"),
            literal("</url><name>"),
            template("{{path.name}}"),
            literal("</name></lookupResponse></soap:Body></soap:Envelope>"),
        ]
    );
}

#[test]
fn a_value_repeated_in_a_header_is_not_generalized() {
    // Headers are copied as they are when a rule answers: a Location naming the item would be wrong for every other
    // item, so each value keeps its own rule.
    let calls: Vec<ObservedExchange> = ["a1", "b2", "c3"]
        .iter()
        .map(|name| {
            let mut e =
                exchange_with_query(201, &format!("{{\"name\":\"{name}\"}}"), &[("name", name)]);
            e.response_headers
                .insert("location".into(), format!("/items/{name}"));
            e
        })
        .collect();
    let Suggestion::Conditional { rules } = suggest("POST", "/items", &calls).unwrap() else {
        panic!("expected a conditional suggestion");
    };
    assert_eq!(rules.len(), 3);
}

#[test]
fn a_query_parameter_name_that_could_write_a_template_is_never_echoed() {
    // The name comes from the observed traffic: `{{query.a}}{{uuid}}` would be more template than the one variable.
    let calls: Vec<ObservedExchange> = ["x", "y", "z"]
        .iter()
        .map(|v| exchange_with_query(200, &format!("value {v}"), &[("a}}{{uuid", v)]))
        .collect();
    let Suggestion::Conditional { rules } = suggest("GET", "/q", &calls).unwrap() else {
        panic!("expected a conditional suggestion");
    };
    assert!(rules.iter().all(|r| r.echo.is_none()));
    assert!(rules.iter().all(|r| {
        r.response
            .body
            .iter()
            .all(|f| matches!(f, BodyFragment::Literal { .. }))
    }));
}

#[test]
fn a_literal_route_comes_before_the_pattern_that_would_also_match_it() {
    // First match wins: `/users/me` must be tried before `/users/{id}`, or its rule would never apply.
    let mut calls = vec![
        ("/users/1", exchange(200, "one")),
        ("/users/2", exchange(200, "two")),
        ("/users/3", exchange(200, "three")),
    ];
    calls.extend((0..3).map(|_| ("/users/me", exchange(200, "myself"))));
    let suggestions = suggest_for_service(&observed(calls));
    let patterns: Vec<&str> = suggestions.iter().map(|s| s.sub_path.as_str()).collect();
    assert_eq!(patterns, vec!["/users/me", "/users/{id}"]);
}

#[test]
fn a_route_parameter_never_reuses_a_listen_path_parameter_name() {
    // At run time a rule's own parameter hides the listen path's one of the same name.
    let calls = ["1", "2", "3"].map(|id| {
        let mut e = exchange(200, &format!("line {id}"));
        e.request_path_params.insert("id".into(), "order-7".into());
        (format!("/lines/{id}"), e)
    });
    let suggestions = suggest_for_service(&observed(
        calls.iter().map(|(p, e)| (p.as_str(), e.clone())).collect(),
    ));
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].sub_path, "/lines/{id2}");
}

#[test]
fn a_listen_path_parameter_can_tell_the_responses_apart() {
    let calls: Vec<ObservedExchange> = ["7", "8", "9"]
        .iter()
        .map(|id| {
            let mut e = exchange(200, &format!("order {id}"));
            e.request_path_params.insert("order".into(), id.to_string());
            e
        })
        .collect();
    let Suggestion::Echo { rule } = suggest("GET", "", &calls).unwrap() else {
        panic!("expected an echo suggestion");
    };
    assert_eq!(rule.echo, Some(ConditionSource::PathParam("order".into())));
}

#[test]
fn suggestions_serialize_with_their_route_and_outcome_side_by_side() {
    // The interface reads `outcome`, `rule`/`rules` and the route fields at the same level.
    let suggestions = suggest_for_service(&observed(vec![
        ("/orders/1", exchange(200, "same")),
        ("/orders/2", exchange(200, "same")),
        ("/orders/3", exchange(200, "same")),
    ]));
    let json = serde_json::to_value(&suggestions).unwrap();
    assert_eq!(json[0]["outcome"], "Unconditional");
    assert_eq!(json[0]["method"], "GET");
    assert_eq!(json[0]["sub_path"], "/orders/{id}");
    assert_eq!(json[0]["path_count"], 3);
    assert_eq!(json[0]["sample_count"], 3);
    assert_eq!(json[0]["paths"][2], "/orders/3");
    assert_eq!(json[0]["rule"]["sub_path"], "/orders/{id}");
    assert_eq!(json[0]["rule"]["echo"], serde_json::Value::Null);
}

#[test]
fn a_json_field_name_with_a_slash_or_a_tilde_is_escaped_in_its_pointer() {
    // A JSON pointer reads `/` as a level and `~` as an escape (RFC 6901): `/a/b~c` named another field, so the
    // suggested condition never matched the requests it came from.
    let calls: Vec<ObservedExchange> = ["on", "on", "off", "off"]
        .iter()
        .map(|mode| {
            let mut e = exchange(200, mode);
            e.request_body = format!(r#"{{"a/b~c":"{mode}"}}"#);
            e
        })
        .collect();
    let Suggestion::Conditional { rules } = suggest("POST", "/toggle", &calls).unwrap() else {
        panic!("expected a conditional suggestion");
    };
    let pointer = match &rules[0].condition.as_ref().unwrap().source {
        ConditionSource::JsonPointer(p) => p.clone(),
        other => panic!("expected a JSON pointer, got {other:?}"),
    };
    assert_eq!(pointer, "/a~1b~0c");
    let body: serde_json::Value = serde_json::from_str(&calls[0].request_body).unwrap();
    assert_eq!(body.pointer(&pointer), Some(&serde_json::json!("on")));
}

// --- Several values per response, and server errors set aside

/// The field values and the response of one call, as `(query parameters, status, body)`.
type Call<'a> = (&'a [(&'a str, &'a str)], u16, &'a str);

fn query_calls(calls: &[Call]) -> Vec<ObservedExchange> {
    calls
        .iter()
        .map(|(query, status, body)| exchange_with_query(*status, body, query))
        .collect()
}

/// The source, and the values each rule accepts, of a conditional suggestion.
fn accepted_values(suggestion: &Suggestion) -> Option<(ConditionSource, Vec<Vec<String>>)> {
    let Suggestion::Conditional { rules } = suggestion else {
        return None;
    };
    let mut source = None;
    let values = rules
        .iter()
        .map(|rule| {
            let conditions: Vec<&Condition> = rule
                .conditions
                .all_of
                .iter()
                .chain(&rule.conditions.any_of)
                .collect();
            conditions
                .into_iter()
                .map(|c| {
                    source = Some(c.source.clone());
                    match &c.operator {
                        Operator::Eq(v) => v.clone(),
                        other => panic!("expected Eq, got {other:?}"),
                    }
                })
                .collect()
        })
        .collect();
    Some((source?, values))
}

#[test]
fn the_choice_of_a_field_follows_its_table() {
    // One row per line of the table above `find_discriminator`: the calls, then the field chosen and the values each
    // response accepts, or `None` when nothing is suggested.
    let q = |k| ConditionSource::QueryParam(String::from(k));
    type Row<'a> = (
        &'a str,
        Vec<Call<'a>>,
        Option<(ConditionSource, Vec<Vec<&'a str>>)>,
    );
    let rows: Vec<Row> = vec![
        (
            "missing from one call",
            vec![
                (&[("a", "1")], 200, "x"),
                (&[("a", "1")], 200, "x"),
                (&[], 404, "y"),
            ],
            None,
        ),
        (
            "one value seen with two responses",
            vec![
                (&[("a", "1")], 200, "x"),
                (&[("a", "1")], 404, "y"),
                (&[("a", "2")], 404, "y"),
            ],
            None,
        ),
        (
            "one value per response",
            vec![
                (&[("a", "1")], 200, "x"),
                (&[("a", "1")], 200, "x"),
                (&[("a", "2")], 404, "y"),
            ],
            Some((q("a"), vec![vec!["1"], vec!["2"]])),
        ),
        (
            "two values for one response, one of them seen twice",
            vec![
                (&[("a", "1")], 200, "x"),
                (&[("a", "1")], 200, "x"),
                (&[("a", "3")], 200, "x"),
                (&[("a", "2")], 404, "y"),
            ],
            Some((q("a"), vec![vec!["1", "3"], vec!["2"]])),
        ),
        (
            "two values for one response, every value seen once",
            vec![
                (&[("a", "1")], 200, "x"),
                (&[("a", "3")], 200, "x"),
                (&[("a", "2")], 404, "y"),
            ],
            None,
        ),
        (
            "the field with the fewest values wins",
            vec![
                (&[("a", "1"), ("b", "p")], 200, "x"),
                (&[("a", "2"), ("b", "p")], 200, "x"),
                (&[("a", "1"), ("b", "p")], 200, "x"),
                (&[("a", "3"), ("b", "q")], 404, "y"),
                (&[("a", "3"), ("b", "q")], 404, "y"),
            ],
            Some((q("b"), vec![vec!["p"], vec!["q"]])),
        ),
        (
            "on a tie, the name in byte order",
            vec![
                (&[("b", "1"), ("a", "p")], 200, "x"),
                (&[("b", "1"), ("a", "p")], 200, "x"),
                (&[("b", "2"), ("a", "q")], 404, "y"),
            ],
            Some((q("a"), vec![vec!["p"], vec!["q"]])),
        ),
    ];
    for (row, calls, expected) in rows {
        let suggestion = suggest("GET", "/x", &query_calls(&calls)).unwrap();
        let expected = expected.map(|(source, values)| {
            let values = values
                .into_iter()
                .map(|v| v.into_iter().map(String::from).collect())
                .collect();
            (source, values)
        });
        assert_eq!(
            accepted_values(&suggestion),
            expected,
            "{row}: {suggestion:?}"
        );
        if expected.is_none() {
            assert!(
                matches!(suggestion, Suggestion::VarianceUnexplained { .. }),
                "{row}: {suggestion:?}"
            );
        }
    }
}

#[test]
fn a_header_comes_after_a_query_parameter_with_as_many_values() {
    let calls: Vec<ObservedExchange> = [
        ("1", "p", 200, "x"),
        ("1", "p", 200, "x"),
        ("2", "q", 404, "y"),
    ]
    .iter()
    .map(|(h, q, status, body)| {
        let mut e = exchange_with_query(*status, body, &[("a", q)]);
        e.request_headers.insert("x-env".into(), h.to_string());
        e
    })
    .collect();
    let (source, _) = accepted_values(&suggest("GET", "/x", &calls).unwrap()).unwrap();
    assert_eq!(source, ConditionSource::QueryParam("a".into()));
}

/// A directory of services: the address it gives for a name, two names (an alias) giving the same one.
fn directory_address(name: &str) -> &'static str {
    match name {
        "orders" | "ordersV2" => {
            "<lookup><endpoint>http://backend.internal/ws/OrderService</endpoint></lookup>"
        }
        "stock" => "<lookup><endpoint>http://backend.internal/ws/StockService</endpoint></lookup>",
        _ => "<lookup><endpoint>http://backend.internal/ws/BillingService</endpoint></lookup>",
    }
}

#[test]
fn a_response_reached_by_two_names_is_one_rule_that_accepts_both() {
    // Ten lookups of four names, each with an identifier of its own: the name decides, the identifier changes at every
    // call and must not win.
    let names = [
        "orders", "stock", "ordersV2", "billing", "orders", "stock", "ordersV2", "billing",
        "orders", "stock",
    ];
    let calls: Vec<(String, ObservedExchange)> = names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            (
                format!("/directory/find/{name}/9f86d081/{}", 1000 + i),
                xml(200, directory_address(name)),
            )
        })
        .collect();
    let suggestions = suggest_for_service(&observed(
        calls.iter().map(|(p, e)| (p.as_str(), e.clone())).collect(),
    ));
    assert_eq!(suggestions.len(), 1, "{suggestions:?}");
    assert_eq!(
        suggestions[0].sub_path,
        "/directory/find/{name}/{hash}/{id}"
    );
    let (source, values) =
        accepted_values(&suggestions[0].outcome).expect("a conditional suggestion");
    assert_eq!(source, ConditionSource::PathParam("name".into()));
    assert_eq!(
        values,
        vec![
            vec!["orders".to_string(), "ordersV2".to_string()],
            vec!["stock".to_string()],
            vec!["billing".to_string()],
        ]
    );
    let Suggestion::Conditional { rules } = &suggestions[0].outcome else {
        unreachable!()
    };
    // The rule of two names holds them in `any_of`, and has no single condition to show.
    assert_eq!(rules[0].conditions.all_of, vec![]);
    assert_eq!(rules[0].conditions.any_of.len(), 2);
    assert_eq!(rules[0].condition, None);
    assert_eq!(
        rules[1].condition.as_ref(),
        rules[1].conditions.all_of.first()
    );
    let replayed = super::replay::check(&observed(
        calls.iter().map(|(p, e)| (p.as_str(), e.clone())).collect(),
    ));
    assert_eq!(replayed, Ok(()));
}

#[test]
fn an_unexplained_server_error_is_set_aside_and_said() {
    let mut calls: Vec<ObservedExchange> = (0..5).map(|_| exchange(200, "ok")).collect();
    calls.insert(2, exchange(500, "upstream timeout"));
    let suggestions = suggest_for_service(&[(key("GET", "/orders"), calls.clone())]);
    assert_eq!(suggestions.len(), 1);
    let Suggestion::Unconditional { rule } = &suggestions[0].outcome else {
        panic!("expected an unconditional suggestion, got {suggestions:?}");
    };
    assert_eq!((rule.response.status, rule.sample_count), (200, 5));
    assert_eq!(suggestions[0].sample_count, 6);
    assert_eq!(
        suggestions[0].left_out,
        vec![LeftOut {
            status: 500,
            count: 1
        }]
    );
    assert_eq!(
        super::replay::check(&[(key("GET", "/orders"), calls)]),
        Ok(())
    );
}

#[test]
fn a_server_error_that_a_field_explains_keeps_its_rule() {
    let calls = query_calls(&[
        (&[("id", "1")], 200, "ok"),
        (&[("id", "1")], 200, "ok"),
        (&[("id", "9")], 503, "down"),
    ]);
    let suggestions = suggest_for_service(&[(key("GET", "/orders"), calls)]);
    assert_eq!(suggestions[0].left_out, vec![]);
    let (_, values) = accepted_values(&suggestions[0].outcome).unwrap();
    assert_eq!(values.len(), 2);
}

#[test]
fn errors_are_set_aside_only_when_enough_calls_remain() {
    // Two answers are not the three a suggestion waits for.
    let calls = vec![exchange(200, "ok"), exchange(200, "ok"), exchange(500, "x")];
    let suggestions = suggest_for_service(&[(key("GET", "/orders"), calls)]);
    assert!(matches!(
        suggestions[0].outcome,
        Suggestion::VarianceUnexplained {
            response_class_count: 2
        }
    ));
    assert_eq!(suggestions[0].left_out, vec![]);
}

#[test]
fn a_client_error_is_never_set_aside() {
    let mut calls: Vec<ObservedExchange> = (0..5).map(|_| exchange(200, "ok")).collect();
    calls.push(exchange(404, "missing"));
    let suggestions = suggest_for_service(&[(key("GET", "/orders"), calls)]);
    assert!(matches!(
        suggestions[0].outcome,
        Suggestion::VarianceUnexplained { .. }
    ));
}

#[test]
fn the_new_fields_of_a_suggestion_are_serialized() {
    let mut calls: Vec<ObservedExchange> = (0..3).map(|_| exchange(200, "ok")).collect();
    calls.push(exchange(502, "bad gateway"));
    let json =
        serde_json::to_value(suggest_for_service(&[(key("GET", "/orders"), calls)])).unwrap();
    assert_eq!(
        json[0]["left_out"],
        serde_json::json!([{ "status": 502, "count": 1 }])
    );
    assert_eq!(
        json[0]["rule"]["conditions"],
        serde_json::json!({ "all_of": [], "any_of": [] })
    );
    assert_eq!(json[0]["rule"]["condition"], serde_json::Value::Null);
}
