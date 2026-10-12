use super::*;
use std::collections::HashMap;

fn answer(status: u16, content_type: &str, body: &str) -> ObservedExchange {
    ObservedExchange::new(
        HashMap::new(),
        HashMap::new(),
        b"",
        None,
        status,
        HashMap::new(),
        body.as_bytes(),
        Some(content_type.into()),
    )
}

/// The patterns inferred for `paths`, each observed once with the answer `answer_for` gives it, with the paths each
/// pattern stands for.
fn patterns(
    paths: &[&str],
    answer_for: impl Fn(&str) -> ObservedExchange,
) -> Vec<(String, Vec<String>)> {
    let exchanges: Vec<Vec<ObservedExchange>> = paths.iter().map(|p| vec![answer_for(p)]).collect();
    let observed: Vec<ObservedPath> = paths
        .iter()
        .zip(&exchanges)
        .map(|(path, exchanges)| ObservedPath { path, exchanges })
        .collect();
    infer(&observed, &BTreeSet::new())
        .into_iter()
        .map(|route| {
            let members = route
                .members
                .iter()
                .map(|&m| paths[m].to_string())
                .collect();
            (route.pattern(), members)
        })
        .collect()
}

fn text(_: &str) -> ObservedExchange {
    answer(200, "text/plain", "ok")
}

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn identifiers_are_recognized_by_their_look() {
    for number in ["0", "42", "2026-10-12", "1.5", "12:30"] {
        assert_eq!(identifier(number), Some(Identifier::Number), "{number}");
    }
    assert_eq!(
        identifier("3f2b8c1e-9a4d-4e6f-8b2a-1c3d5e7f9a0b"),
        Some(Identifier::Uuid)
    );
    for hash in ["9f86d081", "9F86D081884C7D65", "5f2b0c1d9e8a7b6c5d4e3f2a"] {
        assert_eq!(identifier(hash), Some(Identifier::Hash), "{hash}");
    }
    for token in ["AbC123xyz_-9", "01HZX3K8M2N4P6Q8R0S2T4V6W8"] {
        assert_eq!(identifier(token), Some(Identifier::Token), "{token}");
    }
    for word in [
        "orders",
        "v1",
        "oauth2",
        "restOrders",
        "deadbeef",
        "-1",
        "1-",
        "",
        "a.b",
    ] {
        assert_eq!(identifier(word), None, "{word}");
    }
}

#[test]
fn a_value_the_matcher_would_read_as_a_parameter_cannot_stay_literal() {
    for unsafe_value in [":id", "%7Bid%7D", "{id}", "*", "%2A"] {
        assert!(!literal_safe(unsafe_value), "{unsafe_value}");
    }
    for safe in ["orders", "a%2Fb", "{open", "id}", "a:b"] {
        assert!(literal_safe(safe), "{safe}");
    }
    let routes = patterns(&["/files/:x"], text);
    assert_eq!(routes, vec![("/files/{id}".into(), owned(&["/files/:x"]))]);
}

#[test]
fn identifiers_become_parameters_even_when_seen_once() {
    assert_eq!(
        patterns(&["/service/9f86d081884c7d65/status"], text),
        vec![(
            "/service/{hash}/status".into(),
            owned(&["/service/9f86d081884c7d65/status"])
        )]
    );
}

#[test]
fn identifiers_of_several_kinds_share_one_parameter() {
    let routes = patterns(
        &[
            "/orders/42",
            "/orders/3f2b8c1e-9a4d-4e6f-8b2a-1c3d5e7f9a0b",
            "/orders/9f86d081",
        ],
        text,
    );
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].0, "/orders/{id}");
}

#[test]
fn words_with_answers_of_the_same_structure_become_one_parameter() {
    let lookup = |path: &str| {
        let name = path.rsplit('/').next().unwrap();
        answer(
            200,
            "text/xml",
            &format!("<lookup><url>http://backend/{name}</url></lookup>"),
        )
    };
    let routes = patterns(&["/directory/restOrders", "/directory/restStock"], lookup);
    assert_eq!(
        routes,
        vec![(
            "/directory/{name}".into(),
            owned(&["/directory/restOrders", "/directory/restStock"])
        )]
    );
}

#[test]
fn words_with_answers_of_different_structures_stay_literal() {
    let resource = |path: &str| match path {
        "/api/orders" => answer(200, "application/json", r#"[{"orderId":1,"total":3.5}]"#),
        _ => answer(
            200,
            "application/json",
            r#"[{"name":"Ada","email":"a@b.c"}]"#,
        ),
    };
    let routes = patterns(&["/api/orders", "/api/customers"], resource);
    assert_eq!(
        routes,
        vec![
            ("/api/customers".into(), owned(&["/api/customers"])),
            ("/api/orders".into(), owned(&["/api/orders"])),
        ]
    );
}

#[test]
fn words_answered_in_plain_text_stay_literal() {
    // Nothing to compare in a text body: `/health` and `/version` are not the same resource.
    let routes = patterns(&["/health", "/version"], text);
    assert_eq!(routes.len(), 2);
}

#[test]
fn json_values_do_not_change_the_structure_but_keys_do() {
    let a = answer(
        200,
        "application/json",
        r#"{"id":1,"tags":["x"],"owner":{"name":"A"}}"#,
    );
    let b = answer(
        200,
        "application/json; charset=utf-8",
        r#"{"owner":{"name":"B"},"tags":["y","z"],"id":2}"#,
    );
    let c = answer(
        200,
        "application/json",
        r#"{"id":1,"tags":["x"],"owner":{"nom":"A"}}"#,
    );
    assert_eq!(shape(&a), shape(&b));
    assert_ne!(shape(&a), shape(&c));
}

#[test]
fn xml_structure_ignores_namespace_prefixes_and_text() {
    let a = answer(
        200,
        "text/xml",
        "<s:Envelope><s:Body><r><v>1</v></r></s:Body></s:Envelope>",
    );
    let b = answer(
        200,
        "text/xml",
        "<soap:Envelope><soap:Body><r><v>2</v></r></soap:Body></soap:Envelope>",
    );
    let c = answer(
        200,
        "text/xml",
        "<s:Envelope><s:Body><r><w>1</w></r></s:Body></s:Envelope>",
    );
    assert_eq!(shape(&a), shape(&b));
    assert_ne!(shape(&a), shape(&c));
    assert_eq!(
        xml_skeleton("<a><b/><c>t</c></a>"),
        "a\na/b\na/c\n",
        "elements in full, self-closing ones too"
    );
    assert_eq!(
        xml_skeleton("<a><b></c></a>"),
        "a\na/b\n",
        "stops at the first error"
    );
}

#[test]
fn a_literal_comes_before_a_parameter_at_the_same_place() {
    let routes = patterns(&["/users/7", "/users/me", "/users/8"], text);
    let order: Vec<&str> = routes.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(order, vec!["/users/me", "/users/{id}"]);
}

#[test]
fn paths_of_different_lengths_are_different_routes() {
    let routes = patterns(&["/orders/1", "/orders/1/lines", "/orders/2/lines"], text);
    let order: Vec<&str> = routes.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(order, vec!["/orders/{id}", "/orders/{id}/lines"]);
}

#[test]
fn the_root_route_comes_last() {
    // Its rule has no sub-path: it would match every path if it came first.
    let routes = patterns(&["", "/a"], text);
    let order: Vec<&str> = routes.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(order, vec!["/a", ""]);
}

#[test]
fn parameter_names_are_unique_in_a_pattern() {
    let routes = patterns(&["/a/1/2", "/a/3/4"], text);
    assert_eq!(routes[0].0, "/a/{id}/{id2}");
    let reserved = BTreeSet::from(["id".to_string()]);
    let exchanges = vec![text("")];
    let observed = [ObservedPath {
        path: "/a/1",
        exchanges: &exchanges,
    }];
    assert_eq!(infer(&observed, &reserved)[0].pattern(), "/a/{id2}");
}

#[test]
fn parameter_values_are_decoded_like_the_matcher_decodes_them() {
    let exchanges = vec![text("")];
    let observed = [ObservedPath {
        path: "/files/2026-10-12/caf%C3%A9",
        exchanges: &exchanges,
    }];
    let route = &infer(&observed, &BTreeSet::new())[0];
    assert_eq!(route.pattern(), "/files/{id}/caf%C3%A9");
    assert_eq!(
        route.params_of("/files/2026-10-12/caf%C3%A9"),
        vec![("id".to_string(), "2026-10-12".to_string())]
    );
    assert!(
        crate::engine::matcher::match_path(&route.pattern(), "/files/2026-10-13/café").is_some()
    );
}

#[test]
fn overlong_paths_are_left_out() {
    let long = "/a".repeat(MAX_SEGMENTS + 1);
    let routes = patterns(&[long.as_str(), "/b"], text);
    assert_eq!(routes, vec![("/b".into(), owned(&["/b"]))]);
}
