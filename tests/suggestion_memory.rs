//! Runs the real binary as an observed proxy in front of a backend, then asks it for the rules suggested from what it
//! saw. The XML bodies that cost the most to describe for their size must add no more than their own size, give or
//! take the rounding of the allocator, to the memory peak (VmHWM, Linux) of the suggestion request, above that of the
//! same request on a plain XML body of the same size: at the default size of an observed body (16 KiB) and at the
//! largest one observed (10 MiB, the most an exchange may have to be captured).
#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

mod support;
use support::{Server, peak_kib, reset_peak};

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;
// Bodies kept whole by observation; above it, an exchange is not captured at all (`observation::max_buffer_size`).
const LARGEST: usize = 10 * MIB;
// The rounding of the system allocator, and what else the request holds for a body that is not the same.
const MARGIN_KIB: u64 = 1024;

/// A backend that answers each path with the XML body given for it, on a thread of its own: its port.
fn backend(bodies: HashMap<String, String>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bodies = Arc::new(bodies);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let path = request_line.split(' ').nth(1).unwrap_or_default();
            let body = bodies.get(path).map(String::as_str).unwrap_or_default();
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
    });
    port
}

/// Parents whose names fill half the body, over as many short and distinct self-closing children as the reader may
/// meet: the path of each child is as long as the names of all the parents together.
fn long_parents(size: usize, letter: char) -> String {
    const PARENTS: usize = 63;
    let child = |i: usize| format!("<c{i}/>");
    let mut children = String::new();
    let mut i = 0;
    while children.len() + child(i).len() <= size / 2 && i < 4096 - PARENTS {
        children.push_str(&child(i));
        i += 1;
    }
    let name = letter
        .to_string()
        .repeat((size - children.len()) / PARENTS - 2);
    let mut body = format!("<{name}>").repeat(PARENTS);
    body.push_str(&children);
    body.push_str(&"x".repeat(size - body.len()));
    body
}

/// A body of the given size, of a shape of its own for each letter.
type Body = fn(usize, char) -> String;

/// One element whose name is the whole body.
fn one_name(size: usize, letter: char) -> String {
    format!("<{}>", letter.to_string().repeat(size - 2))
}

/// A document of one element and its text, whose description costs nothing.
fn plain(size: usize, letter: char) -> String {
    format!("<{letter}>{}</{letter}>", "x".repeat(size - 7))
}

#[tokio::test]
async fn the_xml_bodies_that_cost_the_most_to_describe_add_at_most_their_own_size_to_a_suggestion()
{
    let cases: [(&str, usize, Body); 6] = [
        ("plain-16k", 16 * KIB - 4, plain),
        ("name-16k", 16 * KIB - 4, one_name),
        ("parents-16k", 16 * KIB - 4, long_parents),
        ("plain-10m", LARGEST, plain),
        ("name-10m", LARGEST, one_name),
        ("parents-10m", LARGEST, long_parents),
    ];
    // Two literal paths of plain words under one service: suggestions describe the shape of their responses to know
    // whether they are one route with a parameter. Each answers a body of its own shape, or of no shape that could be
    // described, so they stay two routes in every case; called twice each, below the three exchanges a route needs,
    // they get no rule, which would hold copies of their bodies longer than describing them does and hide its peak.
    // A third path, called three times, gets one: the request goes to its end.
    let mut bodies = HashMap::new();
    for (service, size, body) in &cases {
        for (word, letter) in [("alpha", 'a'), ("beta", 'b')] {
            let body = body(*size, letter);
            assert_eq!(body.len(), *size, "{service}");
            bodies.insert(format!("/{service}/doc/{word}"), body);
        }
        bodies.insert(format!("/{service}/ping"), "<ok/>".to_string());
    }
    let port = backend(bodies);
    let services: Vec<serde_json::Value> = cases
        .iter()
        .map(|(service, _, _)| {
            serde_json::json!({
                "name": service, "listen_path": "", "real_target_url": format!("http://127.0.0.1:{port}/{service}"),
                "is_mocked": false, "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto",
                "rules": []
            })
        })
        .collect();
    let largest = LARGEST.to_string();
    let server = support::start(
        serde_json::Value::Array(services),
        &[("TRAFFIC_OBSERVATION_MAX_BODY_SIZE", &largest)],
    )
    .await;

    for (service, _, _) in &cases {
        let response = client()
            .post(format!("{}/api/services/{service}/observe", server.base))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 204, "{service}");
        for path in ["doc/alpha", "doc/beta", "doc/alpha", "doc/beta"] {
            call(&server, service, path).await;
        }
        for _ in 0..3 {
            call(&server, service, "ping").await;
        }
    }

    // The first suggestion request of each size leaves what the process keeps once it has served one.
    suggestions(&server, "plain-16k", 1).await;
    suggestions(&server, "plain-10m", 1).await;
    let mut peaks = HashMap::new();
    let mut report = Vec::new();
    for (service, _, _) in &cases {
        reset_peak(&server);
        let before = peak_kib(&server);
        suggestions(&server, service, 1).await;
        let peak = peak_kib(&server);
        report.push(format!("{service}: {before} -> {peak} KiB"));
        // Shown when the test fails, and by `--nocapture`: a server that dies on a case shows the peaks before it.
        eprintln!("{}", report[report.len() - 1]);
        peaks.insert(*service, peak);
    }
    let mut over = Vec::new();
    for (service, size, _) in &cases {
        let reference = if *size == LARGEST {
            peaks["plain-10m"]
        } else {
            peaks["plain-16k"]
        };
        if peaks[service] > reference + (*size / KIB) as u64 + MARGIN_KIB {
            over.push(*service);
        }
    }
    assert!(
        over.is_empty(),
        "peaks over the plain body of the same size, + the body's size + {MARGIN_KIB} KiB: {over:?} ({report:?})"
    );

    // The bodies measured were observed: one more call of each path makes their routes suggested.
    for (service, _, _) in &cases {
        call(&server, service, "doc/alpha").await;
        call(&server, service, "doc/beta").await;
        suggestions(&server, service, 3).await;
    }
}

/// A client that fails a request the server takes too long to answer, rather than wait for it.
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap()
}

/// A call of `path` through the observed service, answered in full by the backend.
async fn call(server: &Server, service: &str, path: &str) {
    let response = client()
        .get(format!("{}/{service}/{path}", server.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200, "{service}/{path}");
    let length = response.content_length();
    let body = response.bytes().await.unwrap();
    assert_eq!(Some(body.len() as u64), length, "{service}/{path}");
}

/// The suggestions for `service`, which must be `count`.
async fn suggestions(server: &Server, service: &str, count: usize) {
    let response = client()
        .get(format!(
            "{}/api/services/{service}/suggestions",
            server.base
        ))
        .send()
        .await
        .unwrap_or_else(|e| panic!("{service}: the server did not answer ({e})"));
    assert_eq!(response.status().as_u16(), 200, "{service}");
    let suggestions: Vec<serde_json::Value> = response.json().await.unwrap();
    assert_eq!(suggestions.len(), count, "{service}: {suggestions:?}");
}
