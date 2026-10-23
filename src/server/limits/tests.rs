use super::*;
use crate::models::{MockConfig, Service};
use crate::server::test_support::{auth_disabled, mock_service, temp_data_dir, test_state};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;

/// A mocked service `slow` whose rule waits `delay_ms` before answering, next to `fast` (no delay).
fn services(delay_ms: u64) -> Vec<Service> {
    let mut slow = mock_service("slow", "late");
    slow.rules[0].method = "POST".into();
    slow.rules[0].response.chaos = Some(crate::models::ChaosConfig {
        delay_ms: Some(delay_ms),
        delay_min_ms: None,
        delay_max_ms: None,
        error_rate: None,
        error_status: 500,
    });
    let mut fast = mock_service("fast", "ok");
    fast.rules[0].method = "POST".into();
    vec![slow, fast]
}

/// Serves the production router through `serve`, as `main` does, and returns its address.
async fn start(limits: Limits, services: Vec<Service>) -> std::net::SocketAddr {
    let data_dir = temp_data_dir("limits");
    let config = MockConfig {
        services,
        groups: vec![],
    };
    let state = test_state(&data_dir, config, auth_disabled()).await;
    let app = crate::server::build_router_with(
        state,
        crate::server::ui_files::UiSource::Directory(data_dir.clone()),
        crate::server::browser_guard::BrowserGuard::new(""),
        limits.clone(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { serve(listener, app, &limits, std::future::pending()).await });
    addr
}

fn quick(requests: usize, body_bytes: usize) -> Limits {
    Limits::new(requests, body_bytes).with_timing(
        Duration::from_millis(300),
        Duration::from_millis(300),
        1024 * 1024,
    )
}

/// Reads until the server closes the connection or `within` elapses; `None` if it is still open then.
async fn read_until_closed(stream: &mut TcpStream, within: Duration) -> Option<String> {
    let mut received = Vec::new();
    let mut chunk = [0u8; 4096];
    let deadline = Instant::now() + within;
    loop {
        match tokio::time::timeout_at(deadline, stream.read(&mut chunk)).await {
            Ok(Ok(0)) | Ok(Err(_)) => return Some(String::from_utf8_lossy(&received).into_owned()),
            Ok(Ok(n)) => received.extend_from_slice(&chunk[..n]),
            Err(_) => return None,
        }
    }
}

fn status_of(response: &str) -> Option<u16> {
    response.split(' ').nth(1)?.parse().ok()
}

#[tokio::test]
async fn a_connection_that_trickles_its_headers_is_closed_after_the_timeout() {
    let addr = start(quick(16, 1 << 20), services(0)).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let started = Instant::now();
    let request = b"GET /api/health HTTP/1.1\r\nhost: localhost\r\nx-padding: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n\r\n";
    let mut closed = false;
    for byte in request.iter().take(request.len() - 2) {
        if stream.write_all(&[*byte]).await.is_err() {
            closed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        if started.elapsed() > Duration::from_secs(3) {
            break;
        }
    }
    if !closed {
        closed = read_until_closed(&mut stream, Duration::from_secs(2))
            .await
            .is_some();
    }
    assert!(
        closed,
        "a client that never finishes its headers must lose its connection"
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "closed after the 300 ms timeout, not at the end of the trickle: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn an_idle_keep_alive_connection_is_closed_after_the_timeout() {
    let addr = start(quick(16, 1 << 20), services(0)).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET /api/health HTTP/1.1\r\nhost: localhost\r\n\r\n")
        .await
        .unwrap();
    let response = read_until_closed(&mut stream, Duration::from_secs(2)).await;
    assert!(
        response
            .as_deref()
            .is_some_and(|r| r.starts_with("HTTP/1.1 200")),
        "answered, then closed once idle: {response:?}"
    );
}

#[tokio::test]
async fn a_trickled_body_is_cut_with_408() {
    let addr = start(quick(16, 1 << 20), services(0)).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"POST /fast/x HTTP/1.1\r\nhost: localhost\r\ncontent-length: 1000\r\n\r\n")
        .await
        .unwrap();
    let started = Instant::now();
    let (mut read_half, mut write_half) = stream.into_split();
    let writer = tokio::spawn(async move {
        for _ in 0..200 {
            if write_half.write_all(b"a").await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let mut received = Vec::new();
    let _ =
        tokio::time::timeout(Duration::from_secs(3), read_half.read_to_end(&mut received)).await;
    writer.abort();
    let response = String::from_utf8_lossy(&received).into_owned();
    assert_eq!(status_of(&response), Some(408), "{response}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_body_sent_at_once_is_read_whatever_the_rate() {
    let addr = start(quick(16, 1 << 20), services(0)).await;
    let body = vec![b'x'; 200 * 1024];
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/fast/x"))
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn the_request_over_the_limit_is_refused_at_once_while_those_in_flight_complete() {
    let addr = start(quick(2, 1 << 20), services(600)).await;
    let client = reqwest::Client::new();
    let slow = |client: reqwest::Client| async move {
        client
            .post(format!("http://{addr}/slow/x"))
            .send()
            .await
            .unwrap()
            .status()
    };
    let first = tokio::spawn(slow(client.clone()));
    let second = tokio::spawn(slow(client.clone()));
    tokio::time::sleep(Duration::from_millis(200)).await;

    let started = Instant::now();
    let refused = client
        .post(format!("http://{addr}/fast/x"))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 503);
    assert_eq!(refused.headers().get("retry-after").unwrap(), "1");
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "refused at once, not queued"
    );

    let health = client
        .get(format!("http://{addr}/api/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        health.status(),
        200,
        "an orchestrator's probe is never refused"
    );

    assert_eq!(first.await.unwrap(), 200);
    assert_eq!(second.await.unwrap(), 200);
    let after = client
        .post(format!("http://{addr}/fast/x"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        after.status(),
        200,
        "the permits come back with the responses"
    );
}

#[tokio::test]
async fn a_body_over_the_memory_budget_is_refused_before_it_is_read() {
    let addr = start(quick(16, 1000), services(0)).await;
    let client = reqwest::Client::new();
    let over = client
        .post(format!("http://{addr}/fast/x"))
        .body(vec![b'x'; 2000])
        .send()
        .await
        .unwrap();
    assert_eq!(over.status(), 503);
    let within = client
        .post(format!("http://{addr}/fast/x"))
        .body(vec![b'x'; 900])
        .send()
        .await
        .unwrap();
    assert_eq!(within.status(), 200);
}

#[tokio::test]
async fn bodies_held_at_once_share_the_budget() {
    let addr = start(quick(16, 1000), services(600)).await;
    let client = reqwest::Client::new();
    let held = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .post(format!("http://{addr}/slow/x"))
                .body(vec![b'x'; 800])
                .send()
                .await
                .unwrap()
                .status()
        }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    let refused = client
        .post(format!("http://{addr}/fast/x"))
        .body(vec![b'x'; 300])
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 503, "800 bytes held + 300 > 1000");
    assert_eq!(held.await.unwrap(), 200);
    let after = client
        .post(format!("http://{addr}/fast/x"))
        .body(vec![b'x'; 300])
        .send()
        .await
        .unwrap();
    assert_eq!(
        after.status(),
        200,
        "the share comes back with the response"
    );
}

#[tokio::test]
async fn a_body_of_unknown_length_is_counted_as_it_arrives() {
    let addr = start(quick(16, 1000), services(0)).await;
    let chunks: Vec<Result<Vec<u8>, std::io::Error>> =
        (0..4).map(|_| Ok(vec![b'x'; 400])).collect();
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/fast/x"))
        .body(reqwest::Body::wrap_stream(futures_util::stream::iter(
            chunks,
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
}

#[tokio::test]
async fn a_body_larger_than_a_service_reads_is_refused_with_413() {
    let addr = start(quick(16, usize::MAX), services(0)).await;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/fast/x"))
        .body(vec![b'x'; MAX_SERVICE_BODY + 1])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 413);
}

#[tokio::test]
async fn a_management_api_body_counts_in_the_budget() {
    let addr = start(quick(16, 1000), vec![]).await;
    let service = serde_json::to_vec(&mock_service(&"n".repeat(1200), "x")).unwrap();
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/api/services"))
        .header("content-type", "application/json")
        .body(service)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 503);
}

#[tokio::test]
async fn a_body_streamed_to_a_backend_holds_no_share_of_the_budget() {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let target = tokio::spawn(crate::server::test_support::capture_one_raw_request(
        ready_tx,
    ));
    let port = ready_rx.await.unwrap();
    let mut proxied = mock_service("proxied", "x");
    proxied.is_mocked = false;
    proxied.real_target_url = format!("http://127.0.0.1:{port}");
    let addr = start(quick(16, 1000), vec![proxied]).await;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/proxied/upload"))
        .body(vec![b'x'; 5000])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(target.await.unwrap().ends_with(&"x".repeat(5000)));
}

#[test]
fn the_limits_come_from_the_environment_with_safe_fallbacks() {
    use crate::settings::vars;
    let limits = Limits::from_lookup(vars(&[]));
    assert_eq!(
        limits.requests.available_permits(),
        DEFAULT_MAX_REQUESTS_IN_FLIGHT
    );
    assert_eq!(
        limits.body_bytes.available_permits(),
        DEFAULT_MAX_BUFFERED_BODY_SIZE
    );
    let limits = Limits::from_lookup(vars(&[
        ("MAX_REQUESTS_IN_FLIGHT", "8"),
        ("MAX_BUFFERED_BODY_SIZE", "4096"),
    ]));
    assert_eq!(limits.requests.available_permits(), 8);
    assert_eq!(limits.body_bytes.available_permits(), 4096);
    let limits = Limits::from_lookup(vars(&[
        ("MAX_REQUESTS_IN_FLIGHT", "0"),
        ("MAX_BUFFERED_BODY_SIZE", "lots"),
    ]));
    assert_eq!(
        limits.requests.available_permits(),
        1,
        "zero would refuse everything"
    );
    assert_eq!(
        limits.body_bytes.available_permits(),
        DEFAULT_MAX_BUFFERED_BODY_SIZE
    );
}
