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

/// A node of a generated XML document: the tree that `xml_skeleton` must describe without being given it.
#[derive(Debug, Clone)]
enum XmlNode {
    Element {
        prefix: Option<&'static str>,
        name: String,
        self_closing: bool,
        children: Vec<XmlNode>,
    },
    Text,
    Comment,
    Instruction,
}

fn write_xml(nodes: &[XmlNode], out: &mut String) {
    for node in nodes {
        match node {
            XmlNode::Element {
                prefix,
                name,
                self_closing,
                children,
            } => {
                // An empty local name needs a prefix: `<p:>`.
                let prefix = if name.is_empty() {
                    prefix.or(Some("p"))
                } else {
                    *prefix
                };
                let qualified = prefix.map_or_else(|| name.clone(), |p| format!("{p}:{name}"));
                if *self_closing {
                    out.push_str(&format!("<{qualified} x=\"1\"/>"));
                } else {
                    out.push_str(&format!("<{qualified}>"));
                    write_xml(children, out);
                    out.push_str(&format!("</{qualified}>"));
                }
            }
            XmlNode::Text => out.push_str("t &amp; u"),
            XmlNode::Comment => out.push_str("<!-- <c> -->"),
            XmlNode::Instruction => out.push_str("<?p <c>?>"),
        }
    }
}

/// The paths of the elements from the top of the document, in the order their start tags come.
fn element_paths(nodes: &[XmlNode], ancestors: &mut Vec<String>, paths: &mut Vec<String>) {
    for node in nodes {
        if let XmlNode::Element { name, children, .. } = node {
            ancestors.push(name.clone());
            paths.push(ancestors.join("/"));
            element_paths(children, ancestors, paths);
            ancestors.pop();
        }
    }
}

/// Documents of a few elements whose names repeat at every level, with prefixes, self-closing elements, texts, comments
/// and processing instructions; some local names are empty (`<p:>`) or beyond ASCII, some are long (300 and 1,500
/// bytes, or within 8 bytes of `MAX_SKELETON`, so that some paths end exactly at the limit), and some elements wrap a
/// chain of up to 100 nested ones, deeper than the 90 levels that fit in `MAX_SKELETON` bytes. Not generated:
/// malformed documents (the first error is a written case), CDATA, long prefixes, more than `MAX_XML_EVENTS` events.
fn generated_documents() -> impl proptest::strategy::Strategy<Value = Vec<XmlNode>> {
    use proptest::prelude::*;
    let name = prop_oneof![
        12 => prop::sample::select(vec!["a", "b", "item", "été"]).prop_map(str::to_string),
        2 => Just(String::new()),
        3 => Just("l".repeat(300)),
        1 => Just("m".repeat(1500)),
        2 => (MAX_SKELETON - 8..=MAX_SKELETON).prop_map(|n| "w".repeat(n)),
    ];
    let prefix = prop::option::of(prop::sample::select(vec!["n", "soap"]));
    let element = |name, prefix, self_closing, children| XmlNode::Element {
        prefix,
        name,
        self_closing,
        children,
    };
    let leaf = prop_oneof![
        2 => Just(XmlNode::Text),
        1 => Just(XmlNode::Comment),
        1 => Just(XmlNode::Instruction),
        4 => (name.clone(), prefix.clone(), any::<bool>())
            .prop_map(move |(name, prefix, self_closing)| element(name, prefix, self_closing, vec![])),
    ];
    let node = leaf.prop_recursive(5, 48, 4, move |inner| {
        prop_oneof![
            3 => (name.clone(), prefix.clone(), prop::collection::vec(inner.clone(), 0..4))
                .prop_map(move |(name, prefix, children)| element(name, prefix, false, children)),
            1 => (name.clone(), 1..100usize, inner).prop_map(move |(name, levels, innermost)| {
                (0..levels).fold(innermost, |child, _| element(name.clone(), None, false, vec![child]))
            }),
        ]
    });
    prop::collection::vec(node, 1..3)
}

// The model is the old reading, which gathered every path before cutting, made to cut where the new one stops: the
// distinct paths in the order they come, as many of the first ones as fit, each with its line break, in
// `MAX_SKELETON` bytes. Both outcomes must come up in a share of the cases: a judge whose documents all fit would pass
// on a reading that never stops, and one whose documents never fit, on a reading that stops too soon.
#[test]
fn xml_skeleton_describes_the_first_paths_of_the_document_that_fit() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed, TestRunner};
    let config = Config {
        cases: 1024,
        rng_seed: RngSeed::Fixed(0x736b_656c_6574_6f6e),
        failure_persistence: None,
        ..Config::default()
    };
    let (cut, whole) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
    let result = TestRunner::new(config).run(&generated_documents(), |nodes| {
        let mut xml = String::new();
        write_xml(&nodes, &mut xml);
        let mut paths = Vec::new();
        element_paths(&nodes, &mut Vec::new(), &mut paths);
        let mut distinct: Vec<String> = Vec::new();
        for path in paths {
            if !distinct.contains(&path) {
                distinct.push(path);
            }
        }
        let mut size = 0;
        let fitting = distinct
            .iter()
            .take_while(|path| {
                size += path.len() + 1;
                size <= MAX_SKELETON
            })
            .count();
        let outcome = if fitting < distinct.len() {
            &cut
        } else {
            &whole
        };
        outcome.set(outcome.get() + 1);
        let mut expected: Vec<&String> = distinct[..fitting].iter().collect();
        expected.sort();
        let expected: String = expected
            .into_iter()
            .map(|path| format!("{path}\n"))
            .collect();
        prop_assert_eq!(xml_skeleton(&xml), expected, "{}", xml);
        Ok(())
    });
    result.unwrap();
    let (cut, whole) = (cut.get(), whole.get());
    assert!(
        cut >= 1024 / 4 && whole >= 1024 / 4,
        "cut at MAX_SKELETON {cut}, whole {whole}"
    );
}
