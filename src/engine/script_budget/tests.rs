use crate::engine::script::{ScriptContext, ScriptEngine};
use std::collections::HashMap;
use std::time::{Duration, Instant};

fn run(script: &str) -> Result<String, String> {
    run_with_body(script, "")
}

fn run_with_body(script: &str, body: &str) -> Result<String, String> {
    let context = ScriptContext {
        body: body.to_string(),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path_params: HashMap::new(),
    };
    ScriptEngine::new()
        .execute(script, &context)
        .map(|result| result.value)
}

fn refused(script: &str, limit: &str) {
    let error = run(script).expect_err(script);
    assert!(error.contains(limit), "{script}: {error}");
}

// A text of 1 MiB minus `short` bytes, built in one operation.
fn big(short: usize) -> String {
    format!(
        "let s = \"x\"; s.pad({}, \"x\");\n",
        super::MAX_STRING_SIZE - short
    )
}

#[test]
fn a_replace_that_would_multiply_a_text_is_refused_before_it_is_built() {
    // Rhai's own replace built the whole result first: here a terabyte, which aborted the process (and would abort
    // this test binary if the checked version were not the one called).
    refused(
        &format!("{}let a = s; a.replace(\"x\", s); a", big(8)),
        "a text longer than 1048576 bytes",
    );
    // An empty pattern matches between every two characters.
    refused(
        &format!("{}let a = \"abc\"; a.replace(\"\", s); a", big(8)),
        "a text longer than 1048576 bytes",
    );
}

#[test]
fn replace_keeps_its_results_for_every_kind_of_pattern() {
    assert_eq!(
        run("let a = \"a-b-c\"; a.replace(\"-\", \"+\"); a").unwrap(),
        "a+b+c"
    );
    assert_eq!(
        run("let a = \"a-b-c\"; a.replace(\"-\", '+'); a").unwrap(),
        "a+b+c"
    );
    assert_eq!(
        run("let a = \"a-b-c\"; a.replace('-', \"--\"); a").unwrap(),
        "a--b--c"
    );
    assert_eq!(
        run("let a = \"a-b-c\"; a.replace('-', 'é'); a").unwrap(),
        "aébéc"
    );
    assert_eq!(
        run("let a = \"ab\"; a.replace(\"\", \".\"); a").unwrap(),
        ".a.b."
    );
    assert_eq!(run("let a = \"\"; a.replace(\"\", \".\"); a").unwrap(), "");
}

#[test]
fn split_keeps_its_results() {
    for (script, expected) in [
        ("\"a b  c\".split()", "[\"a\", \"b\", \"c\"]"),
        ("\"a,b,c\".split(\",\")", "[\"a\", \"b\", \"c\"]"),
        ("\"a,b,c\".split(\",\", 2)", "[\"a\", \"b,c\"]"),
        ("\"a,b,c\".split(',')", "[\"a\", \"b\", \"c\"]"),
        ("\"a,b,c\".split(',', 2)", "[\"a\", \"b,c\"]"),
        ("\"a,b,c\".split_rev(\",\")", "[\"c\", \"b\", \"a\"]"),
        ("\"a,b,c\".split_rev(\",\", 2)", "[\"c\", \"a,b\"]"),
        ("\"a,b,c\".split_rev(',')", "[\"c\", \"b\", \"a\"]"),
        ("\"a,b,c\".split_rev(',', 2)", "[\"c\", \"a,b\"]"),
        ("\"abc\".split(\"\")", "[\"\", \"a\", \"b\", \"c\", \"\"]"),
        ("\"abc\".split(\"-\")", "[\"abc\"]"),
        ("\"\".split(\",\")", "[\"\"]"),
        ("\"abc\".split(1)", "[\"a\", \"bc\"]"),
        ("\"ab\".to_chars()", "['a', 'b']"),
    ] {
        assert_eq!(run(script).unwrap(), expected, "{script}");
    }
}

#[test]
fn split_stops_at_the_array_limit_before_building_every_part() {
    let script = "let s = \",\"; s.pad(5000, \",\");";
    for call in [
        "s.split(\",\")",
        "s.split(',')",
        "s.split(\",\", 5000)",
        "s.split(',', 5000)",
        "s.split_rev(\",\")",
        "s.split_rev(',')",
        "s.split_rev(\",\", 5000)",
        "s.split_rev(',', 5000)",
        "s.split(\"\")",
        "s.to_chars()",
    ] {
        refused(
            &format!("{script} {call}"),
            "an array of more than 1000 items",
        );
    }
    refused(
        "let s = \"a\"; s.pad(4000, \" a\"); s.split()",
        "an array of more than 1000 items",
    );
}

#[test]
fn split_on_a_request_body_larger_than_a_text_stops_at_the_array_limit() {
    let body = ",".repeat(10 * 1024 * 1024);
    let error = run_with_body("split(request.body, \",\")", &body).unwrap_err();
    assert!(
        error.contains("an array of more than 1000 items"),
        "{error}"
    );
}

#[test]
fn parse_json_stops_at_the_first_item_entry_or_byte_over_a_limit() {
    let items = format!("[{}0]", "0,".repeat(1_000));
    let error = run_with_body("parse_json(request.body)", &items).unwrap_err();
    assert!(
        error.contains("an array of more than 1000 items"),
        "{error}"
    );

    // Every key counts, repeated or not: a document of a million keys `a` is not a map of one entry.
    let entries = format!("{{{}\"a\":0}}", "\"a\":0,".repeat(500));
    let error = run_with_body("parse_json(request.body)", &entries).unwrap_err();
    assert!(error.contains("a map of more than 500 entries"), "{error}");

    let text = format!("[\"{}\", \"x\"]", "y".repeat(super::MAX_STRING_SIZE));
    let error = run_with_body("parse_json(request.body)", &text).unwrap_err();
    assert!(
        error.contains("a text longer than 1048576 bytes"),
        "{error}"
    );

    // Keys count with the text, so that a long key does not escape the limit.
    let key = format!(
        "{{\"{}\": 0, \"b\": \"x\"}}",
        "k".repeat(super::MAX_STRING_SIZE)
    );
    let error = run_with_body("parse_json(request.body)", &key).unwrap_err();
    assert!(
        error.contains("a text longer than 1048576 bytes"),
        "{error}"
    );
}

#[test]
fn parse_json_reads_what_it_read_before() {
    let body = r#"{"n": 1, "big": 18446744073709551615, "f": 1.5, "t": true, "z": null, "a": [1, "x"], "o": {"k": "v"}}"#;
    let script = "let v = parse_json(request.body); \
        `${v.n}|${type_of(v.big)}|${v.f}|${v.t}|${v.z == ()}|${v.a[1]}|${v.o.k}`";
    assert_eq!(
        run_with_body(script, body).unwrap(),
        "1|f64|1.5|true|true|x|v"
    );
    assert_eq!(
        run_with_body("parse_json(request.body) == ()", "{not json").unwrap(),
        "true"
    );
    assert_eq!(
        run_with_body("parse_json(request.body) == ()", "[1] trailing").unwrap(),
        "true"
    );
    let exactly = format!("[{}0]", "0,".repeat(999));
    assert_eq!(
        run_with_body("parse_json(request.body).len()", &exactly).unwrap(),
        "1000"
    );
}

#[test]
fn to_json_stops_at_the_text_limit() {
    // Control characters take six bytes each once escaped: the value is within the limits, its JSON is not.
    let script = format!(
        "{}let a = [s]; to_json(a)",
        big(8).replace("\"x\")", "\"\\u0001\")")
    );
    refused(&script, "a text longer than 1048576 bytes");
    assert_eq!(
        run("to_json(#{a: [1, 2.5, \"x\", ()], b: true})").unwrap(),
        "{\"a\":[1,2.5,\"x\",null],\"b\":true}"
    );
}

#[test]
fn xml_element_stops_at_the_text_limit_however_often_a_tag_repeats() {
    let script = "let tag = \"t\"; tag.pad(2000, \"t\"); let a = []; for i in 0..999 { a.push(i) } xml_element(tag, a)";
    refused(script, "a text longer than 1048576 bytes");
    // A tag taken from the request body, which no text limit bounds.
    let error = run_with_body(
        "xml_element(request.body, [1, 2])",
        &"b".repeat(2 * 1024 * 1024),
    )
    .unwrap_err();
    assert!(
        error.contains("a text longer than 1048576 bytes"),
        "{error}"
    );
}

#[test]
fn parse_xml_items_stops_at_the_limits_of_a_value() {
    let items = format!("<r>{}</r>", "<i></i>".repeat(1_001));
    let error = run_with_body("parse_xml_items(request.body, \"r/i\")", &items).unwrap_err();
    assert!(
        error.contains("an array of more than 1000 items"),
        "{error}"
    );

    let fields = format!("<r>{}</r>", "<i><a>1</a></i>".repeat(501));
    let error = run_with_body("parse_xml_items(request.body, \"r/i\")", &fields).unwrap_err();
    assert!(error.contains("a map of more than 500 entries"), "{error}");

    let text = format!(
        "<r><i><a>{}</a></i></r>",
        "x".repeat(super::MAX_STRING_SIZE + 1)
    );
    let error = run_with_body("parse_xml_items(request.body, \"r/i\")", &text).unwrap_err();
    assert!(
        error.contains("a text longer than 1048576 bytes"),
        "{error}"
    );
}

#[test]
fn a_run_stops_at_its_time_limit() {
    // Each call hashes 1 MiB: within the operation limit, the run would last seconds.
    let script = format!("{}let n = 0; loop {{ n += seeded_int(s, 0, 9); }}", big(0));
    let started = Instant::now();
    let error = run(&script).unwrap_err();
    assert!(error.contains("longer than 500 ms"), "{error}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn the_time_limit_does_not_outlive_its_run() {
    let engine = ScriptEngine::new();
    let context = ScriptContext {
        body: String::new(),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path_params: HashMap::new(),
    };
    let script = format!("{}let n = 0; loop {{ n += seeded_int(s, 0, 9); }}", big(0));
    assert!(engine.execute(&script, &context).is_err());
    // The same thread runs the next script with a fresh deadline, not the one left by the previous run.
    assert_eq!(engine.execute("40 + 2", &context).unwrap().value, "42");
}

#[test]
fn the_other_limits_are_named_in_their_errors() {
    refused("loop {}", "more than 10000 operations");
    let variables: String = (0..super::MAX_VARIABLES)
        .map(|i| format!("let v{i} = {i};\n"))
        .collect();
    refused(&variables, "more than 64 variables at once");
    refused(
        "fn f(n) { f(n + 1) } f(0)",
        "more than 32 nested function calls",
    );
    refused(
        &format!("{}let a = s + s; a", big(8)),
        "a text longer than 1048576 bytes",
    );
    refused(
        "let a = []; for i in 0..1001 { a.push(i) } a",
        "an array of more than 1000 items",
    );
    refused(
        "let m = #{}; for i in 0..501 { m[`k${i}`] = i } m",
        "a map of more than 500 entries",
    );
    // Inside a function of the script, the limit is still what the message says first.
    refused("fn f() { loop {} } f()", "more than 10000 operations");
}

#[test]
fn an_expression_nested_too_deep_is_refused_with_its_limit() {
    let script = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    let expected = "nested more than 64 levels deep (32 in a function)";
    assert!(run(&script).unwrap_err().contains(expected));
    assert!(
        ScriptEngine::new()
            .validate(&script)
            .unwrap_err()
            .contains(expected)
    );
}

#[test]
fn a_result_longer_than_a_text_is_refused() {
    let error = run_with_body("request.body", &"b".repeat(2 * 1024 * 1024)).unwrap_err();
    assert!(
        error.contains("a text longer than 1048576 bytes"),
        "{error}"
    );
}

// Every way found to hold values outside the limits of each value: 30 texts of 1 MiB each time, about twice the
// memory budget of a run, which Rhai's own limits let through.
#[test]
fn values_held_outside_the_variables_stop_at_the_memory_budget() {
    let thirty = |each: &dyn Fn(usize) -> String| (0..30).map(each).collect::<Vec<_>>().join(", ");
    for script in [
        // The arguments of one call, all evaluated before the call fails.
        format!("{}nope({})", big(8), thirty(&|i| format!("s + \"{i}\""))),
        // Map keys: Rhai's map limit counts entries, not the length of their keys.
        format!(
            "{}let m = #{{}}; for i in 0..30 {{ m[s + i] = 0; }} m.len()",
            big(8)
        ),
        // What `map` gathers before the result is checked.
        format!(
            "{}let a = []; for i in 0..30 {{ a.push(i); }} a.map(|x| s + x).len()",
            big(8)
        ),
        // Arguments curried into function pointers, which Rhai's limits do not count.
        format!(
            "fn f(x) {{ x }}\n{}let a = []; for i in 0..30 {{ a.push(Fn(\"f\").curry(s + i)); }} a.len()",
            big(8)
        ),
        // Values held by nested calls, one per level of a recursion (the text itself is shared, not copied).
        format!(
            "fn h(a, b) {{ 0 }}\nfn g(n, s) {{ if n > 0 {{ h(s + \"1\", g(n - 1, s)) }} else {{ 0 }} }}\n{}g(30, s)",
            big(8)
        ),
        // Variables, each within its limits.
        format!(
            "{}{}0",
            big(8),
            (0..30)
                .map(|i| format!("let v{i} = s + \"{i}\";\n"))
                .collect::<String>()
        ),
    ] {
        refused(&script, "more than 16 MiB of memory");
    }
}

#[test]
fn a_run_within_its_memory_budget_is_not_stopped() {
    // A response built from a large request: 200 objects (400 entries, near the 500 Rhai allows across a value) and
    // close to 1 MiB of text, parsed then written back.
    let body = format!(
        "[{}]",
        (0..200)
            .map(|i| format!("{{\"id\": {i}, \"name\": \"{}\"}}", "n".repeat(5000)))
            .collect::<Vec<_>>()
            .join(",")
    );
    let script = "let items = parse_json(request.body); let out = []; \
        for item in items { out.push(#{ id: item.id, size: item.name.len() }); } to_json(out).len()";
    assert!(
        run_with_body(script, &body)
            .unwrap()
            .parse::<usize>()
            .unwrap()
            > 200
    );
}
