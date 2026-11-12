//! Runs the real binary and sends it the scripts that cost the most for their size: one operation that would build a
//! huge result, a native function given a 10 MiB request body, values held outside the variables of a run, a loop
//! of slow operations. Each must stop at the limits of a script run (time, memory, size of a value) with the memory
//! peak of the process (VmHWM, Linux) no more than the memory budget of a run (16 MiB) and what one operation may add
//! before the check above that of the same request with a trivial script, and the time limit must end the slow one.
#![cfg(target_os = "linux")]

use std::time::{Duration, Instant};

mod support;
use support::{Server, reset_peak};

const BODY: usize = 10 * 1024 * 1024;
// The memory budget of a run, plus one operation past it (a text of 1 MiB built from another) and the rounding of
// the system allocator.
const BUDGET_MIB: u64 = 16 + 4;

// A rule of the mocked service: on a POST to its sub-path that meets every condition of `all_of`, it runs its
// script and answers 200 with `template`.
fn rule(
    name: &str,
    path: &str,
    script: &str,
    all_of: serde_json::Value,
    template: &str,
) -> serde_json::Value {
    serde_json::json!({
        "name": name, "method": "POST", "sub_path": format!("/{path}"), "action": "mock",
        "script": script, "conditions": {"all_of": all_of, "any_of": []},
        "response": {"status": 200, "headers": [], "body": [{"type": "Template", "template": template}]}
    })
}

// One mocked service whose rules each run one script on the request they match, by sub-path.
async fn start(rules: &[(&str, &str)]) -> Server {
    let rules = rules
        .iter()
        .map(|(path, script)| rule(path, path, script, serde_json::json!([]), "{{script}}"))
        .collect();
    start_rules(rules).await
}

async fn start_rules(rules: Vec<serde_json::Value>) -> Server {
    let service = serde_json::json!({
        "name": "s", "listen_path": "", "real_target_url": "", "is_mocked": true,
        "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto", "rules": rules
    });
    support::start(serde_json::json!([service]), &[]).await
}

fn peak_mib(server: &Server) -> u64 {
    support::peak_kib(server) / 1024
}

async fn post(server: &Server, path: &str, body: String) -> Duration {
    let started = Instant::now();
    let response = reqwest::Client::new()
        .post(format!("{}/s/{path}", server.base))
        .body(body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{path}: the server did not answer ({e})"));
    assert_eq!(response.status().as_u16(), 200, "{path}");
    started.elapsed()
}

// 7 × 2^17 = 917,504 bytes by doubling: fast even in a debug build on a busy machine, where `pad` could use up the
// 500 ms of a run before the memory limit under test.
const DOUBLED: &str = "let s = \"1234567\"; for i in 0..17 { s += s; }";

#[tokio::test]
async fn one_operation_on_a_large_body_stays_within_the_budget_of_a_run() {
    let server = start(&[
        ("reference", "1"),
        (
            "replace",
            &format!("{DOUBLED} let a = s; a.replace(\"1\", s); a"),
        ),
        (
            "tag",
            "let a = []; for i in 0..999 { a.push(i) } xml_element(request.body, a)",
        ),
        ("json", "parse_json(request.body)"),
        ("xml", "parse_xml_items(request.body, \"r/i\")"),
        ("split", "split(request.body, \",\")"),
        (
            "slow",
            "let n = 0; for i in 0..5000 { n += parse_xml_items(request.body, \"q/z\").len(); } n",
        ),
    ])
    .await;

    let text = "x".repeat(BODY);
    post(&server, "reference", text.clone()).await;
    let reference = peak_mib(&server);

    // Before the limits: a terabyte asked for at once (the process aborted), 40 GB of repeated tags, 290 MiB and
    // 390 MiB of parsed values, 640 MiB of split parts.
    post(&server, "replace", String::new()).await;
    post(&server, "tag", text).await;
    post(&server, "json", format!("[{}0]", "0,".repeat(BODY / 2 - 2))).await;
    let items = format!("<r>{}</r>", "<i><a>x</a></i>".repeat(BODY / 16 - 1));
    post(&server, "xml", items.clone()).await;
    post(&server, "split", ",".repeat(BODY)).await;
    let peak = peak_mib(&server);
    assert!(
        peak <= reference + BUDGET_MIB,
        "memory peak {peak} MiB, {reference} MiB for the same request with a trivial script"
    );

    // Before the time limit: over a minute of scanning a 10 MiB body, on one worker thread. A debug build scans
    // slower, so the margin covers the one scan that runs past the deadline.
    let took = post(&server, "slow", items).await;
    assert!(
        took < Duration::from_secs(10),
        "the slow script ran {took:?}"
    );
}

// A server of its own, without request bodies: the 10 MiB requests above would hide a peak below theirs.
#[tokio::test]
async fn values_held_outside_the_variables_stay_within_the_memory_budget_of_a_run() {
    let server = start(&[
        ("reference", "1"),
        (
            "arguments",
            &format!(
                "{DOUBLED} nope({})",
                (0..60).map(|i| format!("s + \"{i}\"")).collect::<Vec<_>>().join(", ")
            ),
        ),
        ("keys", &format!("{DOUBLED} let m = #{{}}; for i in 0..60 {{ m[s + i] = 0; }} m.len()")),
        (
            "gathered",
            &format!("{DOUBLED} let a = []; for i in 0..60 {{ a.push(i); }} a.map(|x| s + x).len()"),
        ),
        (
            "curried",
            &format!(
                "fn f(x) {{ x }}\n{DOUBLED} let a = []; for i in 0..60 {{ a.push(Fn(\"f\").curry(s + i)); }} a.len()"
            ),
        ),
        (
            "nested",
            &format!(
                "fn h(a, b) {{ 0 }}\nfn g(n, s) {{ if n > 0 {{ h(s + \"1\", g(n - 1, s)) }} else {{ 0 }} }}\n{DOUBLED} g(30, s)"
            ),
        ),
        (
            "variables",
            &format!(
                "{DOUBLED} {}0",
                (0..60).map(|i| format!("let v{i} = s + \"{i}\";\n")).collect::<String>()
            ),
        ),
    ])
    .await;

    post(&server, "reference", String::new()).await;
    let reference = peak_mib(&server);
    // Before the memory budget, measured on a release build: 1.7 GB of arguments (stopped by the deadline), 330 MiB
    // of map keys, 1 GB gathered by map(), 850 MiB of curried pointers, 265 MiB held by nested calls, 75 MiB of
    // variables.
    for path in [
        "arguments",
        "keys",
        "gathered",
        "curried",
        "nested",
        "variables",
    ] {
        post(&server, path, String::new()).await;
        let peak = peak_mib(&server);
        assert!(
            peak <= reference + BUDGET_MIB,
            "{path}: memory peak {peak} MiB, {reference} MiB for a trivial script"
        );
    }
}

// A server of its own, without scripts that hold memory: the peaks above would hide what the readers of XML keep for
// each open element of a body (the elements they follow along the path, the names the reader checks end tags
// against). Every body is 10 MiB of start tags never closed, nested to a depth of 1, 4, 16... 4^10, then one element
// per three bytes, each name as long as the size allows: the shallow ones hold the most bytes of names, the deep ones
// the most elements.
#[tokio::test]
async fn a_deeply_nested_xml_body_stays_within_the_memory_budget_of_a_request() {
    let without_script = |mut rule: serde_json::Value| {
        rule["script"] = serde_json::Value::Null;
        rule
    };
    let never_met = serde_json::json!([
        {"source": {"type": "XPath", "key": "a/a/b"}, "operator": {"type": "Exists"}}
    ]);
    let server = start_rules(vec![
        // A script holds copies of the body that a condition or a variable never holds, which would hide 30 MiB of
        // what they keep: they are compared with a request that runs no script either.
        without_script(rule("bare", "bare", "", serde_json::json!([]), "ok")),
        // An XPath condition that no body below meets, and the rule that answers then.
        without_script(rule("condition", "condition", "", never_met, "ok")),
        without_script(rule(
            "otherwise",
            "condition",
            "",
            serde_json::json!([]),
            "ok",
        )),
        without_script(rule(
            "template",
            "template",
            "",
            serde_json::json!([]),
            "{{xpath.a/a/b}}",
        )),
        rule(
            "reference",
            "reference",
            "1",
            serde_json::json!([]),
            "{{script}}",
        ),
        rule(
            "script",
            "script",
            "parse_xml_items(request.body, \"a/a/b\").len()",
            serde_json::json!([]),
            "{{script}}",
        ),
        rule(
            "holding",
            "holding",
            &format!(
                "{DOUBLED} {}parse_xml_items(request.body, \"a/a/b\").len() + v0.len()",
                (0..8)
                    .map(|i| format!("let v{i} = s + \"{i}\"; "))
                    .collect::<String>()
            ),
            serde_json::json!([]),
            "{{script}}",
        ),
    ])
    .await;

    post(&server, "bare", "x".repeat(BODY)).await;
    let bare = peak_mib(&server);
    reset_peak(&server);
    post(&server, "reference", "x".repeat(BODY)).await;
    let scripted = peak_mib(&server);
    let depths = (0..=10).map(|k| 4usize.pow(k)).chain([BODY / 3]);
    let mut cases: Vec<(&str, usize, u64)> = depths
        .flat_map(|depth| {
            [
                ("condition", depth, bare),
                ("template", depth, bare),
                ("script", depth, scripted),
            ]
        })
        .collect();
    // The reference with a script holds copies of its body that `parse_xml_items` no longer needs when it reads it:
    // before the depth limit, the 30 MiB it kept for the deepest body stayed within the budget 3 runs out of 5. A
    // script that holds 8 MiB of texts while it calls it adds them to the peak of the call (+21 MiB became +28).
    cases.push(("holding", BODY / 3, scripted));
    let mut over = Vec::new();
    for (path, depth, reference) in cases {
        let name = "a".repeat(BODY / depth - 2);
        reset_peak(&server);
        post(&server, path, format!("<{name}>").repeat(depth)).await;
        let peak = peak_mib(&server);
        if peak > reference + BUDGET_MIB {
            over.push(format!("{path}, depth {depth}: {peak} MiB for {reference}"));
        }
    }
    assert!(
        over.is_empty(),
        "memory peaks over the same request with no script or a trivial one, + {BUDGET_MIB} MiB: {over:?}"
    );
}
