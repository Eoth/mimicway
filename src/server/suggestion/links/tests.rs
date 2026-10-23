use super::*;
use crate::engine::{TemplateContext, TemplateRenderer};
use std::collections::HashMap;

fn texts(text: &str) -> Vec<&str> {
    find_addresses(text).into_iter().map(|a| a.text).collect()
}

fn config(services: serde_json::Value, groups: serde_json::Value) -> MockConfig {
    serde_json::from_value(serde_json::json!({ "services": services, "groups": groups })).unwrap()
}

fn service(name: &str, listen_path: &str, target: &str, group: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "name": name, "listen_path": listen_path, "real_target_url": target, "is_mocked": false,
        "rewrite_directory_urls": false, "group_name": group, "wsdl_mode": "auto", "rules": [],
    })
}

fn covers(services: Vec<serde_json::Value>) -> Covers {
    Covers::from_config(&config(
        serde_json::Value::Array(services),
        serde_json::json!([]),
    ))
}

/// The service a single address reaches, and the rest of it after the target.
fn covered(covers: &Covers, text: &str) -> Option<(String, String)> {
    let addresses = find_addresses(text);
    let (cover, rest) = covers.find(addresses.first()?)?;
    Some((cover.service.name.clone(), rest.to_string()))
}

fn exchange(body: &str, host: Option<&str>) -> ObservedExchange {
    let mut e = ObservedExchange::new(
        HashMap::new(),
        HashMap::new(),
        b"",
        None,
        200,
        HashMap::new(),
        body.as_bytes(),
        Some(
            if body.starts_with('<') {
                "text/xml"
            } else {
                "application/json"
            }
            .into(),
        ),
    );
    e.request_host = host.map(str::to_string);
    e
}

fn endpoint(
    path: &str,
    exchanges: Vec<ObservedExchange>,
) -> (ObservationKey, Vec<ObservedExchange>) {
    (
        ObservationKey {
            group_name: None,
            service_name: "directory".into(),
            method: "GET".into(),
            sub_path: path.into(),
        },
        exchanges,
    )
}

/// The body of a response rendered as a rule answers a request that reached Mimicway as `host`.
fn render(response: &MockResponse, host: &str) -> String {
    let empty = HashMap::new();
    let headers = HashMap::from([("host".to_string(), host.to_string())]);
    let ctx = TemplateContext {
        path_params: &empty,
        query_params: &empty,
        headers: &headers,
        request_body: b"",
        seq_counter: 0,
        script_result: None,
    };
    TemplateRenderer::render_body(&response.body, &[], &ctx)
}

fn literal(text: &str) -> MockResponse {
    MockResponse {
        status: 200,
        headers: vec![],
        body: vec![BodyFragment::Literal { value: text.into() }],
        chaos: None,
    }
}

#[test]
fn addresses_are_found_in_json_xml_and_link_headers() {
    assert_eq!(
        texts(r#"{"url":"http://orders.internal:8080/v1/orders?page=2","next":"https://stock/x"}"#),
        [
            "http://orders.internal:8080/v1/orders?page=2",
            "https://stock/x"
        ]
    );
    assert_eq!(
        texts(
            "<soap:address location='http://ws.internal/OrderService'/><u>HTTP://Up.Example/a</u>"
        ),
        ["http://ws.internal/OrderService", "HTTP://Up.Example/a"]
    );
    assert_eq!(
        texts(r#"<http://api/items?page=2>; rel="next", <http://api/items?page=9>; rel="last""#),
        ["http://api/items?page=2", "http://api/items?page=9"]
    );
}

#[test]
fn punctuation_after_an_address_and_longer_schemes_are_left_out() {
    assert_eq!(
        texts("See http://docs.internal/guide. Or http://docs.internal/faq, then stop."),
        ["http://docs.internal/guide", "http://docs.internal/faq"]
    );
    assert!(texts("xhttp://not.an/address git+https://nor/this").is_empty());
    assert!(texts("http:// http:///path https://").is_empty());
}

#[test]
fn namespace_and_schema_addresses_are_left_out() {
    let wsdl = r#"<definitions xmlns="http://schemas.xmlsoap.org/wsdl/" xmlns:tns = 'http://example.com/orders'
        targetNamespace="http://example.com/orders"
        xsi:schemaLocation="http://example.com/orders http://example.com/orders.xsd">
        <xsd:import namespace="http://example.com/types" schemaLocation="http://ws.internal/types.xsd"/>
        <soap:address location="http://ws.internal/OrderService"/></definitions>"#;
    assert_eq!(
        texts(wsdl),
        [
            "http://ws.internal/types.xsd",
            "http://ws.internal/OrderService"
        ]
    );
    // A JSON field named like an attribute is data, not a namespace declaration.
    assert_eq!(
        texts(r#"{"xmlns":"http://ws.internal/a"}"#),
        ["http://ws.internal/a"]
    );
}

#[test]
fn a_service_covers_the_addresses_under_its_target_on_a_segment_boundary() {
    let covers = covers(vec![service(
        "orders",
        "",
        "http://orders.internal:8080/api/",
        None,
    )]);
    assert_eq!(
        covered(&covers, "http://orders.internal:8080/api/v1/1?x=1"),
        Some(("orders".into(), "/v1/1?x=1".into()))
    );
    assert_eq!(
        covered(&covers, "http://ORDERS.internal:8080/api"),
        Some(("orders".into(), String::new()))
    );
    assert_eq!(
        covered(&covers, "http://orders.internal:8080/apiv2/x"),
        None
    );
    assert_eq!(covered(&covers, "http://orders.internal:8081/api/x"), None);
    assert_eq!(covered(&covers, "https://orders.internal:8080/api/x"), None);
    assert_eq!(
        covered(&covers, "http://user@orders.internal:8080/api/x"),
        None
    );
}

#[test]
fn the_default_port_and_an_explicit_one_are_the_same_address() {
    let covers = covers(vec![service("stock", "", "https://stock.internal", None)]);
    assert_eq!(
        covered(&covers, "https://stock.internal:443/items"),
        Some(("stock".into(), "/items".into()))
    );
}

#[test]
fn a_service_covers_only_what_its_listen_path_accepts() {
    let covers = covers(vec![service(
        "orders",
        "/v1/*",
        "http://orders.internal",
        None,
    )]);
    assert_eq!(
        covered(&covers, "http://orders.internal/v1/orders"),
        Some(("orders".into(), "/v1/orders".into()))
    );
    assert_eq!(covered(&covers, "http://orders.internal/v2/orders"), None);
}

#[test]
fn an_address_goes_to_the_service_the_interception_would_choose() {
    // `/team/orders/...` is caught by the service without a group named like the group code, tried first: rewriting
    // to the grouped service would send the application to the wrong one.
    let config = config(
        serde_json::json!([
            service("team", "", "", None),
            service("orders", "", "http://orders.internal", Some("Team A")),
        ]),
        serde_json::json!([{ "name": "Team A", "code": "team" }]),
    );
    let covers = Covers::from_config(&config);
    assert_eq!(covered(&covers, "http://orders.internal/x"), None);

    let config = MockConfig {
        services: config.services.into_iter().rev().collect(),
        ..config
    };
    let covers = Covers::from_config(&config);
    let addresses = find_addresses("http://orders.internal/x");
    let (cover, _) = covers.find(&addresses[0]).unwrap();
    assert_eq!(cover.prefix, "/team/orders");
}

#[test]
fn a_rewritten_body_starts_each_covered_address_with_the_address_of_the_request() {
    let covers = covers(vec![service(
        "orders",
        "",
        "http://orders.internal:8080",
        None,
    )]);
    let mut response =
        literal(r#"{"orders":"http://orders.internal:8080/v1","other":"http://elsewhere/v1"}"#);
    let redirected = redirect_response(&mut response, &covers, None);
    assert_eq!(
        redirected.into_iter().map(|s| s.name).collect::<Vec<_>>(),
        ["orders"]
    );
    assert_eq!(
        response.body,
        vec![
            BodyFragment::Literal {
                value: r#"{"orders":""#.into()
            },
            BodyFragment::Template {
                template: MIMICWAY_BASE.into()
            },
            BodyFragment::Literal {
                value: r#"/orders/v1","other":"http://elsewhere/v1"}"#.into()
            },
        ]
    );
    assert_eq!(
        render(&response, "localhost:7342"),
        r#"{"orders":"http://localhost:7342/orders/v1","other":"http://elsewhere/v1"}"#
    );
}

#[test]
fn an_address_cut_by_an_echoed_value_is_rewritten_only_past_the_target() {
    let covers = covers(vec![service("ws", "", "http://ws.internal", None)]);
    let echoed = |before: &str| MockResponse {
        body: vec![
            BodyFragment::Literal {
                value: before.into(),
            },
            BodyFragment::Template {
                template: "{{path.name}}".into(),
            },
        ],
        ..literal("")
    };
    let mut response = echoed("<url>http://ws.internal/ws/");
    redirect_response(&mut response, &covers, None);
    assert_eq!(response.body.len(), 4, "{:?}", response.body);

    // The value may complete the host itself: nothing is known of where this address goes.
    let mut response = echoed("<url>http://ws.internal");
    redirect_response(&mut response, &covers, None);
    assert_eq!(response.body, echoed("<url>http://ws.internal").body);
}

#[test]
fn address_headers_get_the_address_seen_while_observing() {
    let covers = covers(vec![service("orders", "", "http://orders.internal", None)]);
    let endpoints = [endpoint("/", vec![exchange("", Some("mock.local:7342"))])];
    let own = OwnAddress::latest(&endpoints).unwrap();
    let mut response = MockResponse {
        headers: vec![
            HeaderEntry {
                name: "Location".into(),
                value: "http://orders.internal/orders/7".into(),
            },
            HeaderEntry {
                name: "X-Trace".into(),
                value: "http://orders.internal/orders/7".into(),
            },
        ],
        ..literal("")
    };
    redirect_response(&mut response, &covers, Some(&own));
    assert_eq!(
        response.headers[0].value,
        "http://mock.local:7342/orders/orders/7"
    );
    assert_eq!(response.headers[1].value, "http://orders.internal/orders/7");
}

#[test]
fn a_host_that_is_not_an_authority_gives_no_own_address() {
    for host in ["mock.local/evil", "user@mock.local", "mock.local?x", "a b"] {
        let endpoints = [endpoint("/", vec![exchange("", Some(host))])];
        assert!(OwnAddress::latest(&endpoints).is_none(), "{host}");
    }
}

#[test]
fn backends_are_listed_by_origin_with_their_routes_and_who_serves_them() {
    let body = |name: &str| {
        format!(
            r#"<lookup><url>http://ws.internal:8080/ws/{name}</url><self>http://localhost:7342/directory/x</self><doc>http://api.example/{name}</doc></lookup>"#
        )
    };
    let endpoints = [
        endpoint(
            "/service/restOrders",
            vec![exchange(&body("restOrders"), Some("localhost:7342"))],
        ),
        endpoint(
            "/service/restStock",
            vec![exchange(&body("restStock"), Some("localhost:7342"))],
        ),
    ];
    let covers = covers(vec![service("ws", "", "http://ws.internal:8080", None)]);
    let taken = BTreeSet::from(["directory".to_string(), "api-backend".to_string()]);
    let linked = linked_backends(&endpoints, &covers, None, &taken);

    assert_eq!(
        linked.len(),
        2,
        "Mimicway's own address is not a backend: {linked:?}"
    );
    assert_eq!(linked[0].origin, "http://api.example");
    assert_eq!(linked[0].routes, ["GET /service/{name}"]);
    assert!(linked[0].served_by.is_empty());
    let draft = linked[0].draft.as_ref().unwrap();
    assert_eq!(draft.name, "api-backend-2", "reserved, then taken");
    assert_eq!(draft.real_target_url, "http://api.example");

    assert_eq!(linked[1].origin, "http://ws.internal:8080");
    assert_eq!(linked[1].addresses.len(), 2);
    assert_eq!(linked[1].served_by[0].name, "ws");
    assert!(linked[1].draft.is_none());
}

#[test]
fn draft_names_come_from_the_host() {
    let name = |url: &str, taken: &[&str]| {
        let mut taken: BTreeSet<String> = taken.iter().map(|t| t.to_string()).collect();
        draft_name(&Url::parse(url).unwrap(), &mut taken)
    };
    assert_eq!(name("http://orders.internal:8080/x", &[]), "orders");
    assert_eq!(
        name("http://orders.internal", &["orders", "orders-2"]),
        "orders-3"
    );
    assert_eq!(name("http://10.0.0.5:81", &[]), "backend-10-0-0-5");
    assert_eq!(name("http://[::1]:81", &[]), "backend---1");
    assert_eq!(name("http://Auth.example", &[]), "auth-backend");
    assert_eq!(name("http://xn--bcher-kva.example", &[]), "xn--bcher-kva");
}

#[test]
fn suggested_rules_record_the_services_they_send_the_application_to() {
    let body = r#"{"next":"http://orders.internal/orders?page=2"}"#;
    let endpoints = [endpoint(
        "/orders",
        (0..3)
            .map(|_| exchange(body, Some("localhost:7342")))
            .collect(),
    )];
    let mut suggestions = super::super::suggest_for_service(&endpoints);
    let covers = covers(vec![service("orders", "", "http://orders.internal", None)]);
    redirect_suggestions(
        &mut suggestions,
        &covers,
        OwnAddress::latest(&endpoints).as_ref(),
    );
    let Suggestion::Unconditional { rule } = &suggestions[0].outcome else {
        panic!("{suggestions:?}");
    };
    assert_eq!(rule.redirected_to[0].name, "orders");
    assert_eq!(
        render(&rule.response, "mock:1"),
        r#"{"next":"http://mock:1/orders/orders?page=2"}"#
    );
}

#[test]
fn the_fuzz_check_holds_on_a_directory_of_the_backend() {
    let body = r#"{"self":"http://mimic.test/x","orders":"http://backend.test/orders","ns":"http://backend.test"}"#;
    let endpoints = [endpoint(
        "/",
        (0..3).map(|_| exchange(body, Some("mimic.test"))).collect(),
    )];
    assert_eq!(check(&endpoints), Ok(()));
}

#[test]
fn a_non_ascii_character_before_an_attribute_name_is_not_cut() {
    assert!(texts("orders¡internal:8080xmlns=\"http://").is_empty());
    assert!(texts("¡xmlns=\"http://ns.example/a\"").is_empty());
    assert_eq!(
        texts("é=\"http://ws.internal/a\""),
        ["http://ws.internal/a"]
    );
}
