use super::*;
use crate::models::MockConfig;
use crate::server::limits::Limits;
use crate::server::request_log::RequestLog;
use crate::server::test_support;
use futures_util::StreamExt;
use std::time::Instant;

/// A mocked service `feed` whose `GET /log/{run}` rule answers `events`.
fn feed(events: serde_json::Value) -> MockConfig {
    let service = serde_json::from_value(serde_json::json!({
        "name": "feed", "listen_path": "", "real_target_url": "", "is_mocked": true,
        "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto",
        "rules": [{
            "name": "log", "method": "GET", "sub_path": "/log/{run}", "action": "mock", "script": null,
            "conditions": {"all_of": [], "any_of": []},
            "response": {"status": 200, "headers": [], "body": [], "events": events}
        }]
    }))
    .unwrap();
    MockConfig {
        services: vec![service],
        groups: vec![],
    }
}

async fn start(config: MockConfig, limits: Limits) -> (String, RequestLog) {
    let dir = test_support::temp_data_dir("events");
    let state = test_support::test_state(&dir, config, test_support::auth_disabled()).await;
    let log = state.request_log.clone();
    let app = crate::server::build_router_with(
        state,
        crate::server::ui_files::UiSource::Directory(dir),
        crate::server::browser_guard::BrowserGuard::new(""),
        limits,
    );
    (test_support::serve(app).await, log)
}

/// The events read until the body ends or fails, each with the time it arrived after the response headers, and the
/// transport error that ended the body, if any.
async fn read(resp: reqwest::Response) -> (Vec<(String, Duration)>, Option<reqwest::Error>) {
    let opened = Instant::now();
    let mut chunks = resp.bytes_stream();
    let mut pending = String::new();
    let mut events = Vec::new();
    while let Some(chunk) = chunks.next().await {
        match chunk {
            Ok(bytes) => {
                pending.push_str(std::str::from_utf8(&bytes).unwrap());
                while let Some(end) = pending.find("\n\n") {
                    events.push((pending[..end + 1].to_string(), opened.elapsed()));
                    pending.drain(..end + 2);
                }
            }
            Err(e) => return (events, Some(e)),
        }
    }
    assert!(
        pending.is_empty(),
        "the stream ended inside an event: {pending:?}"
    );
    (events, None)
}

#[tokio::test]
async fn a_client_receives_the_events_in_order_after_their_delays_then_the_end() {
    let (base, log) = start(
        feed(serde_json::json!({"events": [
            {"data": "pulling image"},
            {"data": "run {{path.run}}\ncopied 3\r\nfailed: 0", "delay_ms": 150},
            {"event": "endofstream", "data": "endofstream", "delay_ms": 150}
        ]})),
        Limits::default(),
    )
    .await;

    let resp = reqwest::get(format!("{base}/feed/log/42")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    assert_eq!(resp.headers()["cache-control"], "no-cache");
    let (events, error) = read(resp).await;

    assert!(
        error.is_none(),
        "a stream that ends is not an error: {error:?}"
    );
    let texts: Vec<&str> = events.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "data: pulling image\n",
            "data: run 42\ndata: copied 3\ndata: failed: 0\n",
            "event: endofstream\ndata: endofstream\n",
        ]
    );
    let gaps: Vec<Duration> = events.windows(2).map(|w| w[1].1 - w[0].1).collect();
    assert!(
        gaps.iter().all(|gap| *gap >= Duration::from_millis(140)),
        "each event waits its delay after the previous one: {gaps:?}"
    );
    assert!(
        events[0].1 < Duration::from_millis(100),
        "the first event has no delay"
    );

    let entries = log.recent(10);
    assert_eq!(entries.len(), 1, "a stream is logged once");
    let logged = serde_json::to_value(&entries[0]).unwrap();
    assert_eq!(logged["rule_matched"], "log");
    assert_eq!(logged["stream"]["outcome"], "ended");
    assert_eq!(logged["stream"]["planned"], 3);
    assert_eq!(logged["stream"]["sent_count"], 3);
    assert_eq!(
        logged["stream"]["sent"][1]["data"],
        "run 42\ncopied 3\r\nfailed: 0"
    );
    assert_eq!(logged["stream"]["sent"][2]["event"], "endofstream");
}

#[tokio::test]
async fn a_stream_cut_by_its_rule_reaches_the_client_as_a_transport_error() {
    let (base, log) = start(
        feed(serde_json::json!({"cut_at": 1, "events": [
            {"data": "pulling image"},
            {"data": "never sent", "delay_ms": 100},
            {"event": "endofstream", "data": "endofstream"}
        ]})),
        Limits::default(),
    )
    .await;

    let resp = reqwest::get(format!("{base}/feed/log/7")).await.unwrap();
    let (events, error) = read(resp).await;

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "data: pulling image\n");
    let error = error.expect("a cut stream fails, it does not end");
    assert!(error.is_body() || error.is_decode(), "{error:?}");
    assert!(events[0].1 < Duration::from_millis(100));

    let logged = serde_json::to_value(&log.recent(1)[0]).unwrap();
    assert_eq!(logged["stream"]["outcome"], "cut");
    assert_eq!(logged["stream"]["planned"], 2);
    assert_eq!(logged["stream"]["sent_count"], 1);
}

#[tokio::test]
async fn a_stream_past_the_open_streams_is_refused_until_one_ends() {
    let (base, log) = start(
        feed(serde_json::json!({"events": [{"data": "slow", "delay_ms": 400}]})),
        Limits::default().with_streams(1),
    )
    .await;
    let url = format!("{base}/feed/log/1");

    let first = reqwest::get(&url).await.unwrap();
    let refused = reqwest::get(&url).await.unwrap();
    assert_eq!(refused.status().as_u16(), 503);
    assert_eq!(refused.headers()["retry-after"], "1");

    let (events, error) = read(first).await;
    assert!(error.is_none());
    assert_eq!(events.len(), 1);
    // The place is given back once the body is dropped, just after its end reached the client.
    let mut again = reqwest::get(&url).await.unwrap();
    for _ in 0..20 {
        if again.status().as_u16() == 200 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        again = reqwest::get(&url).await.unwrap();
    }
    assert_eq!(again.status().as_u16(), 200);

    let statuses: Vec<u16> = log.recent(10).iter().map(|e| e.status).collect();
    assert!(
        statuses.contains(&503),
        "the refusal is logged: {statuses:?}"
    );
}

#[tokio::test]
async fn a_client_that_leaves_is_logged_as_gone_and_frees_its_place() {
    let (base, log) = start(
        feed(serde_json::json!({"events": [
            {"data": "first"},
            {"data": "second", "delay_ms": 200},
            {"data": "third", "delay_ms": 200}
        ]})),
        Limits::default().with_streams(1),
    )
    .await;
    let url = format!("{base}/feed/log/1");

    let mut chunks = reqwest::get(&url).await.unwrap().bytes_stream();
    let first = chunks.next().await.unwrap().unwrap();
    assert_eq!(&first[..], b"data: first\n\n");
    drop(chunks);

    // The server sees the client gone at the latest when it writes the next event.
    let mut outcome = serde_json::Value::Null;
    for _ in 0..50 {
        outcome = serde_json::to_value(&log.recent(1)[0]).unwrap()["stream"]["outcome"].clone();
        if outcome == "client-gone" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(outcome, "client-gone");
    assert_eq!(reqwest::get(&url).await.unwrap().status().as_u16(), 200);
}

#[tokio::test]
async fn events_that_render_past_one_mebibyte_are_not_sent() {
    let big = "x".repeat(600 * 1024);
    let (base, _log) = start(
        feed(serde_json::json!({"events": [{"data": big}, {"data": big}]})),
        Limits::default(),
    )
    .await;
    let resp = reqwest::get(format!("{base}/feed/log/1")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 500);
}

#[tokio::test]
async fn the_rule_headers_are_rendered_and_override_the_stream_defaults() {
    let service = serde_json::json!({"events": [{"data": "{{header.x-run}}"}]});
    let mut config = feed(service);
    config.services[0].rules[0].response.headers = vec![
        crate::models::HeaderEntry {
            name: "Content-Type".into(),
            value: "text/event-stream; charset=utf-8".into(),
        },
        crate::models::HeaderEntry {
            name: "X-Run".into(),
            value: "{{path.run}}".into(),
        },
    ];
    let (base, _log) = start(config, Limits::default()).await;
    let resp = reqwest::Client::new()
        .get(format!("{base}/feed/log/9"))
        .header("x-run", "from the request")
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.headers()["content-type"],
        "text/event-stream; charset=utf-8"
    );
    assert_eq!(resp.headers()["x-run"], "9");
    assert_eq!(resp.text().await.unwrap(), "data: from the request\n\n");
}

#[test]
fn every_line_break_of_the_data_starts_a_data_line_and_a_name_stays_on_one_line() {
    assert_eq!(
        frame(Some("a\nb"), "one\r\ntwo\rthree\nfour"),
        Bytes::from("event: a b\ndata: one\ndata: two\ndata: three\ndata: four\n\n")
    );
    assert_eq!(frame(None, ""), Bytes::from("data: \n\n"));
}
