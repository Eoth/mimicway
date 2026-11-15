// Equivalent mutants (cargo-mutants, excluded in .cargo/mutants.toml):
// - `expecting` written empty: serde_json reads it only to word an error, which `value_at` turns into `None` without
//   reading it.
// - `visit_bool`, `visit_i64`, `visit_u64`, `visit_f64`, `visit_str` and `visit_unit` returning `Default::default()`:
//   they return `Ok(None)`, which is the default. They cannot be removed: serde's own ones refuse the value, and a
//   pointer that goes through a scalar before a later duplicate key holds what it points to would then find nothing
//   (`a_scalar_before_a_later_duplicate_key_does_not_end_the_search`).
use super::*;
use proptest::prelude::*;

/// What the matcher and the templates computed before: the whole document, then `Value::pointer`.
fn reference(body: &[u8], pointer: &str) -> Option<Value> {
    serde_json::from_slice::<Value>(body)
        .ok()?
        .pointer(pointer)
        .cloned()
}

fn same(body: &str, pointer: &str) {
    assert_eq!(
        value_at(body.as_bytes(), pointer),
        reference(body.as_bytes(), pointer),
        "{pointer} in {body}"
    );
}

#[test]
fn finds_what_value_pointer_finds() {
    let body = r#"{"user":{"id":7,"role":"admin","tags":["a","b",{"x":null}]},"a/b":1,"m~n":2,"":3,"list":[]}"#;
    for pointer in [
        "",
        "/user",
        "/user/id",
        "/user/role",
        "/user/tags/1",
        "/user/tags/2/x",
        "/user/tags/3",
        "/user/tags/01",
        "/user/tags/+1",
        "/user/tags/-1",
        "/user/id/deeper",
        "/a~1b",
        "/m~0n",
        "/",
        "/list/0",
        "/missing",
        "user",
    ] {
        same(body, pointer);
    }
}

#[test]
fn the_last_of_duplicate_keys_wins_as_in_a_parsed_document() {
    same(r#"{"id":1,"id":2}"#, "/id");
    same(r#"{"a":{"b":1},"a":{"c":2}}"#, "/a/b");
    same(r#"{"a":{"b":1},"a":{"c":2}}"#, "/a/c");
}

#[test]
fn a_scalar_before_a_later_duplicate_key_does_not_end_the_search() {
    for scalar in ["null", "true", "-7", "7", "1.5", r#""text""#] {
        same(&format!(r#"{{"a":{scalar},"a":{{"x":1}}}}"#), "/a/x");
        same(&format!(r#"{{"a":{{"x":1}},"a":{scalar}}}"#), "/a/x");
        same(&format!(r#"{{"a":[{scalar}],"a":[{{"x":1}}]}}"#), "/a/0/x");
    }
}

/// Every token of up to three characters among digits, signs, a space and a letter, as an index of an array of twelve
/// items (so that `10` and `11` are there), and as a key of an object whose keys are those same tokens.
#[test]
fn every_short_token_is_read_as_value_pointer_reads_it() {
    let alphabet = ['0', '1', '9', '+', '-', ' ', 'a'];
    let mut tokens = vec![String::new()];
    for length in 1..=3 {
        let mut all = vec![String::new()];
        for _ in 0..length {
            all = all
                .iter()
                .flat_map(|prefix| alphabet.iter().map(move |c| format!("{prefix}{c}")))
                .collect();
        }
        tokens.extend(all);
    }
    let array = Value::Array((0..12).map(|i| Value::from(i * 10)).collect());
    let object = Value::Object(
        tokens
            .iter()
            .enumerate()
            .map(|(i, token)| (token.clone(), Value::from(i)))
            .collect(),
    );
    let body = serde_json::to_string(&serde_json::json!({ "list": array, "map": object })).unwrap();
    let mut found = 0;
    for token in &tokens {
        for pointer in [format!("/list/{token}"), format!("/map/{token}")] {
            same(&body, &pointer);
            found += usize::from(value_at(body.as_bytes(), &pointer).is_some());
        }
    }
    // Every key, and the indexes the alphabet writes without a sign or a leading zero: 0, 1, 9, 10 and 11.
    assert_eq!(found, tokens.len() + 5);
}

#[test]
fn a_body_that_is_not_json_has_no_value() {
    for body in [
        "",
        "{",
        r#"{"id":1} trailing"#,
        "not json",
        r#"{"id":"\x"}"#,
        "[1,]",
    ] {
        same(body, "/id");
        same(body, "");
    }
}

#[test]
fn a_deep_document_fails_as_the_parser_does() {
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    same(&deep, "/0/0");
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        (-1e6f64..1e6).prop_map(Value::from),
        "[a-c/~0-9]{0,3}".prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::vec(("[a-c/~0-9]{0,2}", inner), 0..4)
                .prop_map(|entries| Value::Object(entries.into_iter().collect())),
        ]
    })
}

proptest! {
    #[test]
    fn agrees_with_value_pointer_on_any_document(doc in json_value(), pointer in "(/[a-c/~0-9+]{0,2}){0,4}") {
        let body = serde_json::to_vec(&doc).unwrap();
        prop_assert_eq!(value_at(&body, &pointer), reference(&body, &pointer));
    }

    #[test]
    fn agrees_with_value_pointer_on_any_text(body in "[\\[\\]{}\":,0-9a-c ]{0,24}", pointer in "(/[a-c0-9]{0,2}){0,3}") {
        prop_assert_eq!(value_at(body.as_bytes(), &pointer), reference(body.as_bytes(), &pointer));
    }
}
