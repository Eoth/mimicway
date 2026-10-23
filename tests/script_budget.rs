//! Runs the real binary and sends it the scripts that cost the most for their size: one operation that would build a
//! huge result, a native function given a 10 MiB request body, values held outside the variables of a run, a loop
//! of slow operations. Each must stop at the limits of a script run (time, memory, size of a value) with the memory
//! peak of the process (VmHWM, Linux) no more than the memory budget of a run (16 MiB) and what one operation may add
//! before the check above that of the same request with a trivial script, and the time limit must end the slow one.
#![cfg(target_os = "linux")]

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BODY: usize = 10 * 1024 * 1024;
// The memory budget of a run, plus one operation past it (a text of 1 MiB built from another) and the rounding of
// the system allocator.
const BUDGET_MIB: u64 = 16 + 4;

struct Server {
    child: Child,
    base: String,
    data_dir: std::path::PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

// One mocked service whose rules each run one script on the request they match, by sub-path.
async fn start(rules: &[(&str, &str)]) -> Server {
    let data_dir =
        std::env::temp_dir().join(format!("mimicway-script-budget-{}", fastrand::u64(..)));
    std::fs::create_dir_all(&data_dir).unwrap();
    let rules: Vec<_> = rules
        .iter()
        .map(|(path, script)| {
            serde_json::json!({
                "name": path, "method": "POST", "sub_path": format!("/{path}"), "action": "mock",
                "script": script, "conditions": {"all_of": [], "any_of": []},
                "response": {"status": 200, "headers": [], "body": [{"type": "Template", "template": "{{script}}"}]}
            })
        })
        .collect();
    let config = serde_json::json!({
        "groups": [],
        "services": [{
            "name": "s", "listen_path": "", "real_target_url": "", "is_mocked": true,
            "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto", "rules": rules
        }]
    });
    // JSON is YAML: the configuration file takes it as it is.
    std::fs::write(data_dir.join("mock-config.yaml"), config.to_string()).unwrap();
    let port = free_port();
    let child = Command::new(env!("CARGO_BIN_EXE_mimicway"))
        .env("DATA_PATH", &data_dir)
        .env("STATIC_DIR", &data_dir)
        .env("PORT", port.to_string())
        .env("BIND_ADDRESS", "127.0.0.1")
        .env_remove("AUTH_ENABLED")
        // The peak must count the memory held at once, not what glibc keeps: by default each thread allocates from an
        // arena of its own and keeps large blocks a request freed, so the same request handled by another worker
        // thread raised the peak by up to 40 MiB. One arena, and large blocks given back to the system when freed.
        .env("MALLOC_ARENA_MAX", "1")
        .env("MALLOC_MMAP_THRESHOLD_", "131072")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the binary");
    let server = Server {
        child,
        base: format!("http://127.0.0.1:{port}"),
        data_dir,
    };
    let client = reqwest::Client::new();
    let started = Instant::now();
    while client
        .get(format!("{}/api/health", server.base))
        .send()
        .await
        .is_err()
    {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the server never answered"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    server
}

fn peak_mib(server: &Server) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{}/status", server.child.id()))
        .expect("the server is alive");
    let kib: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
        .expect("VmHWM");
    kib / 1024
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
