//! Builders for the tests that run the real router on a real socket, so that they exercise the middleware stack
//! exactly as it runs in production.
use crate::auth::AuthConfig;
use crate::models::{MockConfig, Service};
use crate::server::AppState;
use axum::Router;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// A fixture with a service in a group it does not define does not run like production (no group code, so
/// another URL): it once hid the fact that traffic observation never worked for grouped services.
pub(crate) fn assert_consistent(config: &MockConfig) {
    let dangling = config.unknown_group_references();
    assert!(
        dangling.is_empty(),
        "test fixture refers to undefined groups: {dangling:?}"
    );
}

/// Directory of the current test run: `<temp>/mimicway-tests/<pid>-<start time>`. A test cannot know when the
/// servers it started stop writing, so its directory is not removed by the test itself; instead the first test of
/// each run removes the runs older than an hour. Tests used to leave one directory per test in the temporary
/// folder, thousands after a few days.
static RUN_DIR: std::sync::LazyLock<PathBuf> = std::sync::LazyLock::new(|| {
    let root = std::env::temp_dir().join("mimicway-tests");
    if let Ok(runs) = std::fs::read_dir(&root) {
        let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for run in runs.flatten() {
            let stale = run
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|modified| modified < hour_ago);
            if stale {
                let _ = std::fs::remove_dir_all(run.path());
            }
        }
    }
    let started = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    root.join(format!("{}-{started}", std::process::id()))
});

/// A fresh, empty directory for one test, under the directory of the current run.
pub(crate) fn temp_data_dir(prefix: &str) -> PathBuf {
    let dir = RUN_DIR.join(format!("{prefix}-{}", fastrand::u64(..)));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(crate) fn auth_disabled() -> AuthConfig {
    AuthConfig {
        enabled: false,
        keycloak_url: String::new(),
        realm: String::new(),
        client_id: String::new(),
        super_admins: vec![],
        issuer: String::new(),
        show_reset_button: false,
    }
}

/// An ungrouped, mocked service whose single `GET` rule answers `body` on any path.
pub(crate) fn mock_service(name: &str, body: &str) -> Service {
    serde_json::from_value(serde_json::json!({
        "name": name, "listen_path": "", "real_target_url": "", "is_mocked": true,
        "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto",
        "rules": [{
            "name": "any", "method": "GET", "sub_path": null, "action": "mock",
            "pre_script": null, "script": null, "post_script": null,
            "conditions": {"all_of": [], "any_of": []},
            "response": {"status": 200, "headers": [], "body": [{"type": "Literal", "value": body}]}
        }]
    }))
    .unwrap()
}

pub(crate) async fn test_state(
    data_dir: &std::path::Path,
    config: MockConfig,
    auth_config: AuthConfig,
) -> AppState {
    assert_consistent(&config);
    let store = crate::store::MockStore::new(data_dir.join("mock-config.yaml"));
    store.replace(config).await.unwrap();
    store.flush().await;
    AppState {
        store,
        proxy: crate::engine::ProxyClient::new(),
        seq_counters: Arc::new(RwLock::new(HashMap::new())),
        request_log: crate::server::request_log::RequestLog::new(),
        auth_config,
        keycloak: None,
        script_engine: crate::engine::script::ScriptEngine::new(),
        ping_cache: crate::server::ping::PingCache::new(),
        observation: crate::server::observation::ObservationState::new(),
        #[cfg(feature = "messaging-kafka")]
        messaging: crate::messaging::MessagingState {
            message_log: crate::messaging::message_log::MessageLog::new(),
            reply_topic: None,
            publisher: crate::messaging::consumer::Publisher::None,
        },
        #[cfg(feature = "tcp-mock")]
        tcp_runtime: crate::tcp::TcpRuntime::load_and_spawn(data_dir, crate::tcp::LOOPBACK).await,
    }
}

/// Serves `app` on a free local port and returns its root URL (`http://127.0.0.1:<port>`).
pub(crate) async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{port}")
}

/// A fake HTTP target on a free local port: sends its port through `ready`, answers the one request it receives with
/// an empty 200, and returns that request as raw text (request line, headers, body). What a proxy sends is then checked
/// without an HTTP parser in between.
///
/// The request is read up to the end its headers announce (Content-Length, or the last chunk), never up to a pause:
/// TCP may deliver the body long after the headers on a loaded machine. The deadline only stops a test that would hang.
pub(crate) async fn capture_one_raw_request(ready: tokio::sync::oneshot::Sender<u16>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    ready.send(listener.local_addr().unwrap().port()).unwrap();
    let (mut stream, _) = listener.accept().await.unwrap();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while !request_is_complete(&buf) {
        match tokio::time::timeout_at(deadline, stream.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
            _ => break,
        }
    }
    let _ = stream
        .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
        .await;
    String::from_utf8_lossy(&buf).into_owned()
}

/// Whether `buf` holds a whole HTTP/1.1 request: headers, then the body they announce.
fn request_is_complete(buf: &[u8]) -> bool {
    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
    let body = &buf[end + 4..];
    let header = |name: &str| {
        head.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':').map(str::trim))
    };
    if header("transfer-encoding").is_some_and(|value| value.contains("chunked")) {
        return body == b"0\r\n\r\n" || body.ends_with(b"\r\n0\r\n\r\n");
    }
    let length = header("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    body.len() >= length
}

#[cfg(test)]
mod tests {
    use super::capture_one_raw_request;
    use tokio::io::AsyncWriteExt;

    // Sends `parts` to the fake target, pausing between them as a loaded machine or network would, and returns what
    // the target captured.
    async fn capture_sent_in_parts(parts: &[&[u8]]) -> String {
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let target = tokio::spawn(capture_one_raw_request(ready_tx));
        let port = ready_rx.await.unwrap();
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
            // A target that stopped reading too early has closed the connection: what it captured says so.
            let _ = client.write_all(part).await;
        }
        target.await.unwrap()
    }

    #[tokio::test]
    async fn a_body_that_arrives_after_the_headers_is_captured() {
        let raw = capture_sent_in_parts(&[
            b"POST /a HTTP/1.1\r\nhost: target\r\ncontent-length: 12\r\n\r\n",
            b"payload-",
            b"body",
        ])
        .await;
        assert!(raw.ends_with("\r\n\r\npayload-body"), "{raw}");
    }

    #[tokio::test]
    async fn a_chunked_body_is_captured_up_to_its_last_chunk() {
        let raw = capture_sent_in_parts(&[
            b"POST /a HTTP/1.1\r\nhost: target\r\ntransfer-encoding: chunked\r\n\r\n",
            b"c\r\npayload-body\r\n",
            b"0\r\n\r\n",
        ])
        .await;
        assert!(
            raw.ends_with("\r\n\r\nc\r\npayload-body\r\n0\r\n\r\n"),
            "{raw}"
        );
    }
}
