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

// A text of 7 × 2^17 = 917,504 bytes, built by doubling: 17 copies, fast even in a debug build on a busy machine.
// Built with `pad`, one character at a time, it took most of the run's 500 ms there, and a test of the memory limit
// could stop at the time limit first.
const DOUBLED: &str = "let s = \"1234567\"; for i in 0..17 { s += s; }\n";

#[test]
fn a_replace_that_would_multiply_a_text_is_refused_before_it_is_built() {
    // Rhai's own replace built the whole result first: here a terabyte, which aborted the process (and would abort
    // this test binary if the checked version were not the one called).
    refused(
        &format!("{DOUBLED}let a = s; a.replace(\"1\", s); a"),
        "a text longer than 1048576 bytes",
    );
    // An empty pattern matches between every two characters.
    refused(
        &format!("{DOUBLED}let a = \"abc\"; a.replace(\"\", s); a"),
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
    // Control characters take six bytes each once escaped: 2^19 of them are within the limits, their JSON is not.
    let script = "let s = \"\\u0001\"; for i in 0..19 { s += s; } let a = [s]; to_json(a)";
    refused(script, "a text longer than 1048576 bytes");
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

    let deep = format!("<r>{}", "<i>".repeat(1_024));
    let error = run_with_body("parse_xml_items(request.body, \"r/i\")", &deep).unwrap_err();
    assert!(
        error.contains("an XML text nested more than 1024 elements deep"),
        "{error}"
    );
}

#[test]
fn a_run_stops_at_its_time_limit() {
    // Each call hashes 1 MiB: within the operation limit, the run would last seconds.
    let script = format!("{DOUBLED}let n = 0; loop {{ n += seeded_int(s, 0, 9); }}");
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
    let script = format!("{DOUBLED}let n = 0; loop {{ n += seeded_int(s, 0, 9); }}");
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
        &format!("{DOUBLED}let a = s + s; a"),
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

// Every way found to hold values outside the limits of each value: 30 texts of 0.9 MiB each time, about 1.6 times
// the memory budget of a run, which Rhai's own limits let through.
#[test]
fn values_held_outside_the_variables_stop_at_the_memory_budget() {
    let thirty = |each: &dyn Fn(usize) -> String| (0..30).map(each).collect::<Vec<_>>().join(", ");
    for script in [
        // The arguments of one call, all evaluated before the call fails.
        format!("{}nope({})", DOUBLED, thirty(&|i| format!("s + \"{i}\""))),
        // Map keys: Rhai's map limit counts entries, not the length of their keys.
        format!(
            "{}let m = #{{}}; for i in 0..30 {{ m[s + i] = 0; }} m.len()",
            DOUBLED
        ),
        // What `map` gathers before the result is checked.
        format!(
            "{}let a = []; for i in 0..30 {{ a.push(i); }} a.map(|x| s + x).len()",
            DOUBLED
        ),
        // Arguments curried into function pointers, which Rhai's limits do not count.
        format!(
            "fn f(x) {{ x }}\n{}let a = []; for i in 0..30 {{ a.push(Fn(\"f\").curry(s + i)); }} a.len()",
            DOUBLED
        ),
        // Values held by nested calls, one per level of a recursion (the text itself is shared, not copied).
        format!(
            "fn h(a, b) {{ 0 }}\nfn g(n, s) {{ if n > 0 {{ h(s + \"1\", g(n - 1, s)) }} else {{ 0 }} }}\n{}g(30, s)",
            DOUBLED
        ),
        // Variables, each within its limits.
        format!(
            "{}{}0",
            DOUBLED,
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

// Equivalent mutants (cargo-mutants, excluded in .cargo/mutants.toml):
// - `operations % 16 == 0` made `!= 0` in `apply`: the clock is then read at 15 operations out of 16 instead of 1, and a
//   run past its deadline still stops within 16 operations, as before; only the cost of reading the clock changes.
// - `register_split` made empty: Rhai's own `split`, `split_rev` and `to_chars` then answer, with the same results
//   (`split_family_answers_what_rhai_answers` compares them), and build every part before checking the size of the
//   array. The memory it takes is seen by `tests/script_budget.rs` (a 10 MiB body split into its parts: 640 MiB
//   before), an integration test that the gate does not run.

/// A text under the array limit once split, made of letters, separators and spaces, and the arguments of a call of
/// the `split` family: a text or character delimiter (empty, absent from the text or not), and a number of segments
/// or none (zero and negative counts included).
fn split_calls() -> impl proptest::strategy::Strategy<Value = (String, String, Option<i64>, bool)> {
    use proptest::prelude::*;
    (
        "[ab,é ]{0,12}",
        prop::sample::select(vec!["", ",", "ab", "é", ",,", "z"]),
        prop::option::of(-2i64..6),
        any::<bool>(),
    )
        .prop_map(|(text, delimiter, segments, reverse)| {
            (text, delimiter.to_string(), segments, reverse)
        })
}

fn texts(parts: rhai::Array) -> Vec<String> {
    parts.into_iter().map(|part| part.to_string()).collect()
}

// Rhai's own functions are the judge: `register_split` replaces them for the size of their result only. A text that
// a call with a delimiter returns whole is the one given, not a copy of it: a 1 MiB text without its delimiter is not
// held twice.
#[test]
fn split_family_answers_what_rhai_answers() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed, TestRunner};
    let mut ours = rhai::Engine::new();
    super::apply(&mut ours);
    let theirs = rhai::Engine::new();
    let config = Config {
        cases: 2048,
        rng_seed: RngSeed::Fixed(0x7370_6c69_745f_7276),
        failure_persistence: None,
        ..Config::default()
    };
    let whole = std::cell::Cell::new(0);
    let result =
        TestRunner::new(config).run(&split_calls(), |(text, delimiter, segments, reverse)| {
            let name = if reverse { "split_rev" } else { "split" };
            let mut calls = vec![format!("{name}(t, d)"), format!("{name}(t, c)")];
            if let Some(n) = segments {
                calls = vec![format!("{name}(t, d, {n})"), format!("{name}(t, c, {n})")];
            }
            if !reverse && segments.is_none() {
                calls.extend(["split(t)".to_string(), "to_chars(t)".to_string()]);
            }
            let given = rhai::ImmutableString::from(text.as_str());
            for call in calls {
                if call.contains(", c") && delimiter.chars().count() != 1 {
                    continue;
                }
                let mut scope = rhai::Scope::new();
                scope.push("t", given.clone());
                scope.push("d", delimiter.clone());
                scope.push("c", delimiter.chars().next().unwrap_or(','));
                let expected = theirs
                    .eval_with_scope::<rhai::Array>(&mut scope.clone(), &call)
                    .unwrap();
                let parts = ours
                    .eval_with_scope::<rhai::Array>(&mut scope, &call)
                    .unwrap();
                let shared = parts.len() == 1
                    && !text.is_empty()
                    && parts[0]
                        .clone()
                        .into_immutable_string()
                        .is_ok_and(|part| part.as_ptr() == given.as_ptr());
                let same_as_given =
                    parts.len() == 1 && parts[0].to_string() == text && !text.is_empty();
                whole.set(whole.get() + usize::from(same_as_given));
                prop_assert_eq!(
                    texts(parts),
                    texts(expected),
                    "{} on {:?}, d = {:?}",
                    call,
                    text,
                    delimiter
                );
                if same_as_given && (call.contains(", d") || call.contains(", c")) {
                    prop_assert!(shared, "{} on {:?} copied the text", call, text);
                }
            }
            Ok(())
        });
    result.unwrap();
    assert!(
        whole.get() >= 2048 / 8,
        "{} calls returned the text whole",
        whole.get()
    );
}

/// A replacement whose result is within a few bytes of the text limit, on either side: a pattern found `count` times
/// in a filler that never matches it, each match traded for `with`.
fn replacements_at_the_limit() -> impl proptest::strategy::Strategy<Value = (String, String, String)>
{
    use proptest::prelude::*;
    (
        prop::sample::select(vec!["a", "é", "ab", ""]),
        prop::sample::select(vec!["", "x", "xyz", "éé"]),
        0usize..3,
        -3i64..=3,
    )
        .prop_map(|(find, with, filler, offset)| {
            let target = (super::MAX_STRING_SIZE as i64 + offset) as usize;
            if find.is_empty() {
                // Matched between every two characters: `n` characters and `n + 1` substitutes.
                let n = (target - with.len()) / (1 + with.len());
                return ("b".repeat(n), find.to_string(), with.to_string());
            }
            let unit = with.len() + filler;
            // A match traded for nothing leaves the filler alone to make up the length.
            let (count, rest) = target
                .checked_div(unit)
                .map_or((1000, target), |count| (count, target % unit));
            let mut text = format!("{find}{}", "b".repeat(filler)).repeat(count);
            text.push_str(&"b".repeat(rest));
            (text, find.to_string(), with.to_string())
        })
}

// Rust's own `str::replace` is the judge: `replace` must give its result when it fits in a text, and refuse it
// otherwise, without building it.
#[test]
fn replace_refuses_exactly_the_results_longer_than_a_text() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, RngSeed, TestRunner};
    let config = Config {
        cases: 96,
        rng_seed: RngSeed::Fixed(0x7265_706c_6163_6521),
        failure_persistence: None,
        ..Config::default()
    };
    let (fitting, exact, over) = (
        std::cell::Cell::new(0),
        std::cell::Cell::new(0),
        std::cell::Cell::new(0),
    );
    let result = TestRunner::new(config).run(&replacements_at_the_limit(), |(text, find, with)| {
        let expected = text.replace(&find, &with);
        let mut replaced = rhai::ImmutableString::from(text.as_str());
        let outcome = super::replace(&mut replaced, &find, &with);
        let counter = match expected.len().cmp(&super::MAX_STRING_SIZE) {
            std::cmp::Ordering::Less => &fitting,
            std::cmp::Ordering::Equal => &exact,
            std::cmp::Ordering::Greater => &over,
        };
        counter.set(counter.get() + 1);
        if expected.len() <= super::MAX_STRING_SIZE {
            prop_assert!(outcome.is_ok(), "{} bytes refused", expected.len());
            prop_assert!(replaced.as_str() == expected, "the result differs");
        } else {
            prop_assert!(outcome.is_err(), "{} bytes accepted", expected.len());
            prop_assert!(replaced.as_str() == text, "the text changed");
        }
        Ok(())
    });
    result.unwrap();
    let counts = (fitting.get(), exact.get(), over.get());
    assert!(
        counts.0 >= 10 && counts.1 >= 5 && counts.2 >= 10,
        "under, at and over the limit: {counts:?}"
    );
}

// Rhai's own text form of a value is the judge: `display` must give it while it fits in a text, and refuse it
// otherwise. A text is written at once; an array, piece by piece.
#[test]
fn display_refuses_exactly_the_values_longer_than_a_text() {
    let limit = super::MAX_STRING_SIZE;
    let mut values: Vec<rhai::Dynamic> = Vec::new();
    for length in [limit - 1, limit, limit + 1] {
        values.push("t".repeat(length).into());
        // `["…", "…"]`: two items of `n` bytes take `2n + 8`.
        let n = (length - 8) / 2;
        let mut array: rhai::Array = vec!["a".repeat(n).into(), "b".repeat(length - 8 - n).into()];
        values.push(array.clone().into());
        array.push(rhai::Dynamic::UNIT);
        values.push(array.into());
    }
    let mut seen = [0usize; 2];
    for value in values {
        let expected = value.to_string();
        let shown = super::display(&value);
        seen[usize::from(expected.len() <= limit)] += 1;
        if expected.len() <= limit {
            assert_eq!(
                shown.ok().as_deref(),
                Some(expected.as_str()),
                "{} bytes",
                expected.len()
            );
        } else {
            assert!(shown.is_err(), "{} bytes accepted", expected.len());
        }
    }
    assert!(
        seen[0] >= 3 && seen[1] >= 3,
        "over and within the limit: {seen:?}"
    );
}

#[test]
fn a_run_takes_its_clock_away_when_it_ends() {
    assert_eq!(run("1 + 1").unwrap(), "2");
    assert!(super::RUN.with(std::cell::Cell::get).is_none());
    {
        let _clock = super::RunClock::start();
        assert!(super::RUN.with(std::cell::Cell::get).is_some());
    }
    assert!(super::RUN.with(std::cell::Cell::get).is_none());
}

#[test]
fn the_memory_budget_stops_a_run_holding_more_than_it() {
    let budget = super::MEMORY_BUDGET_MIB * 1024 * 1024;
    assert!(super::within_memory_budget(budget));
    assert!(!super::within_memory_budget(budget + 1));
}

// Rhai wraps an error in the calls it crossed only when it comes back through a native function that called the
// script, a closure given to `map`, and then once for each function: a limit is still named first, however deep, and
// any other error keeps Rhai's own text, which names the functions. A limit reached in functions of the script alone
// comes back unwrapped.
#[test]
fn errors_inside_functions_name_a_limit_first_and_keep_the_rest() {
    refused("[1].map(|x| { loop {} })", "more than 10000 operations");
    refused(
        "fn g() { [1].map(|x| { loop {} }) } fn f() { g() } f()",
        "more than 10000 operations",
    );
    let error = run("fn f() { throw \"boom\" } f()").unwrap_err();
    assert!(error.contains("boom") && error.contains("'f'"), "{error}");
    let error = run("fn g() { throw \"boom\" } fn f() { g() } f()").unwrap_err();
    assert!(
        error.contains("boom") && error.contains("'g'") && error.contains("'f'"),
        "{error}"
    );
}
