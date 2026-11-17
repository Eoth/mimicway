// The fields read from a body are judged by the matcher of conditions itself: each field, written as the `Eq`
// condition a suggestion would write, must match the body it was read from, and the operation, written as the
// condition `Envelope/Body/<operation>` exists, must hold for that operation and no other. The generated documents
// carry namespace prefixes, CDATA sections, references, comments, repeated and self-closing elements, and SOAP
// envelopes with zero, one or several operations; the JSON ones, nested objects and arrays, escaped keys and numbers.
//
// What the judge does not see: a field that should have been read and was not (a missed field only costs a
// suggestion, never a wrong rule); the bounds are checked by the hand-written cases below.
use super::*;
use crate::engine::RequestData;
use crate::models::{Condition, ConditionGroup, Operator};
use proptest::prelude::*;
use std::cell::Cell;

fn matches(source: &ConditionSource, operator: Operator, body: &str) -> bool {
    let group = ConditionGroup {
        all_of: vec![Condition {
            source: source.clone(),
            operator,
        }],
        any_of: vec![],
    };
    let req = RequestData {
        query_params: Default::default(),
        headers: Default::default(),
        body: body.as_bytes().to_vec(),
        content_type: None,
        path_params: Default::default(),
        method: "POST".into(),
        remaining_path: String::new(),
    };
    MatchEngine::matches_group(&group, &req, None)
}

fn xpath(path: &str, value: &str) -> (ConditionSource, String) {
    (ConditionSource::XPath(path.into()), value.into())
}

fn pointer(path: &str, value: &str) -> (ConditionSource, String) {
    (ConditionSource::JsonPointer(path.into()), value.into())
}

#[test]
fn a_soap_call_gives_its_operation_and_the_texts_of_its_elements() {
    let body = r#"<?xml version="1.0"?>
<soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/" xmlns:o="urn:orders">
  <soap:Header><o:token>t-1</o:token></soap:Header>
  <soap:Body><o:GetStatus><o:ref><![CDATA[A&B]]></o:ref><o:note>x &amp; y</o:note><o:empty/></o:GetStatus></soap:Body>
</soap:Envelope>"#;
    let reading = read(body, false);
    assert_eq!(reading.operation.as_deref(), Some("GetStatus"));
    assert_eq!(
        reading.fields,
        vec![
            xpath("Envelope/Header/token", "t-1"),
            xpath("Envelope/Body/GetStatus/ref", "A&B"),
            xpath("Envelope/Body/GetStatus/note", "x & y"),
            xpath("Envelope/Body/GetStatus/empty", ""),
        ]
    );
}

#[test]
fn a_repeated_element_is_read_at_its_first_occurrence_only() {
    let body = "<Envelope><Body><Book><slot><date>d1</date></slot><slot><date>d2</date><type>t</type></slot>\
                </Book></Body></Envelope>";
    let reading = read(body, false);
    assert_eq!(
        reading.fields,
        vec![
            xpath("Envelope/Body/Book/slot/date", "d1"),
            // The first `type` is in the second slot: still the first element of its path.
            xpath("Envelope/Body/Book/slot/type", "t"),
        ]
    );
    for (source, value) in &reading.fields {
        assert!(
            matches(source, Operator::Eq(value.clone()), body),
            "{source:?}"
        );
    }
}

#[test]
fn an_element_that_holds_another_gives_no_field() {
    // Its XPath value would be `ab`, the text up to the first end tag inside it.
    let reading = read("<r><a>a<b>b</b>c</a></r>", false);
    assert_eq!(reading.fields, vec![xpath("r/a/b", "b")]);
}

#[test]
fn a_body_with_no_operation_or_several_names_none() {
    for body in [
        "<Envelope><Body/></Envelope>",
        "<Envelope><Body><A/><B/></Body></Envelope>",
        "<Envelope><Header><A/></Header></Envelope>",
        "<Root><Body><A/></Body></Root>",
        "<Envelope><Body><p:/></Body></Envelope>",
        "{\"Envelope\":{\"Body\":{\"A\":1}}}",
    ] {
        assert_eq!(read(body, false).operation, None, "{body}");
    }
    // The same operation twice is still one.
    let twice = read("<Envelope><Body><A>1</A><A>2</A></Body></Envelope>", false);
    assert_eq!(twice.operation.as_deref(), Some("A"));
}

#[test]
fn a_cut_body_gives_nothing() {
    let body = "<Envelope><Body><A><id>1</id></A></Body></Envelope>";
    assert_eq!(read(body, true), BodyReading::default());
    assert_eq!(read("{\"a\":1}", true), BodyReading::default());
}

#[test]
fn a_json_body_gives_its_scalars_at_any_depth() {
    let body = r#"{"order":{"type":"express","lines":[{"sku":"A-1","qty":2}],"paid":true,"note":null},
                   "a/b~c":1.5,"":"root"}"#;
    let reading = read(body, false);
    assert_eq!(reading.operation, None);
    let mut fields = reading.fields;
    fields.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    let mut expected = vec![
        pointer("/order/type", "express"),
        pointer("/order/lines/0/sku", "A-1"),
        pointer("/order/lines/0/qty", "2"),
        pointer("/order/paid", "true"),
        pointer("/a~1b~0c", "1.5"),
        pointer("/", "root"),
    ];
    expected.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    assert_eq!(fields, expected);
    for (source, value) in &fields {
        assert!(
            matches(source, Operator::Eq(value.clone()), body),
            "{source:?}"
        );
    }
}

#[test]
fn a_json_body_with_a_key_written_twice_gives_no_field() {
    // The condition reads the last one: `/a/b` would find nothing there.
    assert_eq!(read(r#"{"a":{"b":1},"a":{"c":2}}"#, false).fields, vec![]);
    assert_eq!(read(r#"{"a":1} trailing"#, false).fields, vec![]);
}

#[test]
fn the_bounds_hold() {
    // Fields: the first ones only.
    let many: String = (0..2 * MAX_BODY_FIELDS)
        .map(|i| format!("<f{i}>{i}</f{i}>"))
        .collect();
    let fields = read(&format!("<r>{many}</r>"), false).fields;
    assert_eq!(fields.len(), MAX_BODY_FIELDS);
    assert_eq!(fields[0], xpath("r/f0", "0"));
    let many: Vec<String> = (0..2 * MAX_BODY_FIELDS)
        .map(|i| format!("\"k{i}\":{i}"))
        .collect();
    assert_eq!(
        read(&format!("{{{}}}", many.join(",")), false).fields.len(),
        MAX_BODY_FIELDS
    );

    // Depth: a value at `MAX_FIELD_DEPTH` is read, one below is not.
    let nested = |depth: usize| {
        let open: String = (1..depth).map(|i| format!("<e{i}>")).collect();
        let close: String = (1..depth).rev().map(|i| format!("</e{i}>")).collect();
        format!("{open}<v>x</v>{close}")
    };
    assert_eq!(read(&nested(MAX_FIELD_DEPTH), false).fields.len(), 1);
    assert_eq!(read(&nested(MAX_FIELD_DEPTH + 1), false).fields.len(), 0);
    let json_nested = |depth: usize| format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    assert_eq!(read(&json_nested(MAX_FIELD_DEPTH), false).fields.len(), 1);
    assert_eq!(
        read(&json_nested(MAX_FIELD_DEPTH + 1), false).fields.len(),
        0
    );

    // Values and names: no longer than their bound.
    let value = |n: usize| format!("<r><v>{}</v></r>", "x".repeat(n));
    assert_eq!(read(&value(MAX_FIELD_VALUE), false).fields.len(), 1);
    assert_eq!(read(&value(MAX_FIELD_VALUE + 1), false).fields.len(), 0);
    let name = |n: usize| format!("<r><{0}>x</{0}></r>", "n".repeat(n));
    assert_eq!(read(&name(MAX_FIELD_PATH - 2), false).fields.len(), 1);
    assert_eq!(read(&name(MAX_FIELD_PATH - 1), false).fields.len(), 0);
    let long_operation = format!(
        "<Envelope><Body><{0}/></Body></Envelope>",
        "o".repeat(MAX_FIELD_PATH + 1)
    );
    assert_eq!(read(&long_operation, false).operation, None);
    let json_value = |n: usize| format!("{{\"v\":\"{}\"}}", "x".repeat(n));
    assert_eq!(read(&json_value(MAX_FIELD_VALUE), false).fields.len(), 1);
    assert_eq!(
        read(&json_value(MAX_FIELD_VALUE + 1), false).fields.len(),
        0
    );
}

#[test]
fn reading_stops_where_the_xpath_reader_stops() {
    // A malformed part: what came before is kept, what comes after is not read.
    let reading = read("<r><a>1</a><b>2</c><d>3</d></r>", false);
    assert_eq!(reading.fields, vec![xpath("r/a", "1")]);
    // An element nested past the limit of the XML readers ends the reading. The reader of conditions finds no
    // `Envelope/Body/A` there (it stops at that element, before any end tag): no operation either.
    let deep = format!(
        "<Envelope><Body><A>{}</A><B/></Body></Envelope>",
        "<x>".repeat(XML_DEPTH_LIMIT)
    );
    let reading = read(&deep, false);
    assert_eq!(reading.operation, None);
    for operation in ["A", "B"] {
        assert!(!matches(
            &ConditionSource::XPath(format!("Envelope/Body/{operation}")),
            Operator::Exists,
            &deep
        ));
    }
}

// --- Generated documents

#[derive(Debug, Clone)]
enum Node {
    Element {
        prefix: Option<&'static str>,
        name: &'static str,
        children: Vec<Node>,
    },
    Text(&'static str),
    CData(&'static str),
    Comment,
}

const NAMES: &[&str] = &["Envelope", "Body", "Header", "ref", "item", "a"];
const OPERATIONS: &[&str] = &["GetStatus", "Book", "Ping"];
const TEXTS: &[&str] = &[
    "abc",
    "x &amp; y",
    "&#233;t&#xE9;",
    "1",
    "",
    "a\r\nb",
    "&lt;q&gt;",
];
const CDATA: &[&str] = &["a&b<c", "]]", "x"];

fn arb_node() -> impl Strategy<Value = Node> {
    let leaf = prop_oneof![
        prop::sample::select(TEXTS).prop_map(Node::Text),
        prop::sample::select(CDATA).prop_map(Node::CData),
        Just(Node::Comment),
        (prop::option::of(Just("p")), prop::sample::select(NAMES)).prop_map(|(prefix, name)| {
            Node::Element {
                prefix,
                name,
                children: vec![],
            }
        }),
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        (
            prop::option::of(prop::sample::select(vec!["p", "soap"])),
            prop::sample::select(NAMES),
            prop::collection::vec(inner, 0..4),
        )
            .prop_map(|(prefix, name, children)| Node::Element {
                prefix,
                name,
                children,
            })
    })
}

fn element(prefix: Option<&'static str>, name: &'static str, children: Vec<Node>) -> Node {
    Node::Element {
        prefix,
        name,
        children,
    }
}

/// A SOAP envelope with a header or not, and zero to three operations (the same one sometimes repeated), each holding
/// generated content; or any generated tree.
fn arb_document() -> impl Strategy<Value = Node> {
    let operation = (
        prop::sample::select(OPERATIONS),
        prop::collection::vec(arb_node(), 0..4),
    )
        .prop_map(|(name, children)| element(Some("o"), name, children));
    let envelope = (
        prop::option::of(Just("soap")),
        prop::option::of(prop::collection::vec(arb_node(), 0..3)),
        prop::collection::vec(operation, 0..3),
    )
        .prop_map(|(prefix, header, operations)| {
            let mut children: Vec<Node> = header
                .map(|h| element(prefix, "Header", h))
                .into_iter()
                .collect();
            children.push(element(prefix, "Body", operations));
            element(prefix, "Envelope", children)
        });
    prop_oneof![3 => envelope, 1 => arb_node()]
}

fn write(node: &Node, out: &mut String, root: bool) {
    match node {
        Node::Element {
            prefix,
            name,
            children,
        } => {
            let qualified = match prefix {
                Some(p) => format!("{p}:{name}"),
                None => name.to_string(),
            };
            let namespaces = if root {
                " xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:p=\"urn:p\" xmlns:o=\"urn:o\""
            } else {
                ""
            };
            if children.is_empty() {
                out.push_str(&format!("<{qualified}{namespaces}/>"));
            } else {
                out.push_str(&format!("<{qualified}{namespaces}>"));
                for child in children {
                    write(child, out, false);
                }
                out.push_str(&format!("</{qualified}>"));
            }
        }
        Node::Text(text) => out.push_str(text),
        Node::CData(text) => out.push_str(&format!("<![CDATA[{text}]]>")),
        Node::Comment => out.push_str("<!-- c -->"),
    }
}

#[test]
fn every_xml_field_and_operation_matches_the_body_it_was_read_from() {
    use proptest::test_runner::TestRunner;
    let config = crate::test_seed::config(1024, 0x626f_6479_5f78_6d6c);
    let (with_fields, with_operation, unclear) = (Cell::new(0), Cell::new(0), Cell::new(0));
    let result = TestRunner::new(config).run(&arb_document(), |document| {
        let mut body = String::new();
        write(&document, &mut body, true);
        let reading = read(&body, false);
        with_fields.set(with_fields.get() + usize::from(!reading.fields.is_empty()));
        for (source, value) in &reading.fields {
            prop_assert!(
                matches(source, Operator::Eq(value.clone()), &body),
                "{:?} = {:?} in {}",
                source,
                value,
                body
            );
        }
        let present: Vec<&str> = OPERATIONS
            .iter()
            .chain(NAMES)
            .copied()
            .filter(|name| {
                let path = ConditionSource::XPath(format!("Envelope/Body/{name}"));
                matches(&path, Operator::Exists, &body)
            })
            .collect();
        match &reading.operation {
            Some(name) => {
                with_operation.set(with_operation.get() + 1);
                prop_assert_eq!(present, vec![name.as_str()], "{}", body);
            }
            None => {
                unclear.set(unclear.get() + usize::from(present.len() > 1));
                prop_assert!(present.len() != 1, "{:?} in {}", present, body);
            }
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
    assert!(
        with_fields.get() >= 256 && with_operation.get() >= 256 && unclear.get() >= 64,
        "{with_fields:?} {with_operation:?} {unclear:?}"
    );
}

fn arb_json() -> impl Strategy<Value = serde_json::Value> {
    use serde_json::{Value, json};
    let leaf = prop_oneof![
        prop::sample::select(vec!["a", "", "x y", "é", "a/b"]).prop_map(|s| json!(s)),
        any::<i32>().prop_map(|n| json!(n)),
        prop::sample::select(vec![1.5f64, -0.25, 1e3, 12345678.9]).prop_map(|f| json!(f)),
        any::<bool>().prop_map(|b| json!(b)),
        Just(Value::Null),
    ];
    leaf.prop_recursive(5, 48, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(
                prop::sample::select(vec!["id", "type", "a/b", "~x", "", "items"]),
                inner,
                0..4
            )
            .prop_map(|map| Value::Object(
                map.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
            )),
        ]
    })
}

#[test]
fn every_json_field_matches_the_body_it_was_read_from() {
    use proptest::test_runner::TestRunner;
    let config = crate::test_seed::config(1024, 0x626f_6479_5f6a_736e);
    let (with_fields, nested) = (Cell::new(0), Cell::new(0));
    let result = TestRunner::new(config).run(&arb_json(), |document| {
        let body = document.to_string();
        let reading = read(&body, false);
        with_fields.set(with_fields.get() + usize::from(!reading.fields.is_empty()));
        let deep = reading.fields.iter().any(|(source, _)| {
            matches!(source, ConditionSource::JsonPointer(p) if p.matches('/').count() > 1)
        });
        nested.set(nested.get() + usize::from(deep));
        for (source, value) in &reading.fields {
            prop_assert!(
                matches(source, Operator::Eq(value.clone()), &body),
                "{:?} = {:?} in {}",
                source,
                value,
                body
            );
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
    assert!(
        with_fields.get() >= 256 && nested.get() >= 128,
        "{with_fields:?} {nested:?}"
    );
}
