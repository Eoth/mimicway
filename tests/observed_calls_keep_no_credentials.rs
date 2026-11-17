//! Runs the real binary with `REDACT_HEADERS` set, as an operator would, and sends credentials through four observed
//! proxy services: with and without a group, each once with a backend that answers and once with one that is down,
//! their targets carrying `user:password@`. Every call is read back by the route the service has (four routes: list and
//! one call, with and without a group): no answer holds a credential the requests carried (headers, a sensitive header
//! sent twice, a header whose value is not ASCII) nor one the backend set or the target held; and the output of the
//! binary (stdout and stderr, at its default level of log) holds none of them, nor the secret of the query.
//!
//! What it does not see: the logs at the levels `debug` and `trace`, which name what the HTTP libraries do; and a
//! credential in the query or a body, which observation keeps as sent (the guide says so).
#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[path = "../src/test_ports.rs"]
mod test_ports;
use test_ports::reserve_port;

/// Credentials that no answer of the API and no line of the log may hold.
const SECRETS: [&str; 9] = [
    "secret-authorization",
    "secret-cookie",
    "secret-api-key-first",
    "secret-api-key-second",
    "secret-auth-token",
    "secret-extra-header",
    "secret-set-cookie",
    "secret-userinfo",
    "secret-non-ascii",
];
/// Kept as sent by observation (the guide says so), never written to the log.
const QUERY_SECRET: &str = "secret-query";

/// A backend that answers every request with a session cookie of its own and a known size, so that the exchange is
/// captured.
async fn backend() -> u16 {
    async fn answer() -> axum::response::Response {
        axum::response::Response::builder()
            .header("set-cookie", "session=secret-set-cookie")
            .header("content-type", "text/xml")
            .body(axum::body::Body::from("<ok/>"))
            .unwrap()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new().fallback(answer);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

/// Stops the server however the test ends, a failed assertion included.
struct Server(std::process::Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn no_answer_about_observed_calls_holds_a_credential() {
    let backend_port = backend().await;
    let data_dir = std::env::temp_dir().join(format!("mimicway-calls-{}", fastrand::u64(..)));
    std::fs::create_dir_all(&data_dir).unwrap();
    let output = data_dir.join("output.log");
    let log = std::fs::File::create(&output).unwrap();
    let port = reserve_port();
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_mimicway"))
            .env("DATA_PATH", &data_dir)
            .env("STATIC_DIR", &data_dir)
            .env("PORT", port.to_string())
            .env("BIND_ADDRESS", "127.0.0.1")
            .env("REDACT_HEADERS", "X-Tenant-Secret")
            .env_remove("AUTH_ENABLED")
            .env_remove("PROXY_ALLOWED_TARGETS")
            .env_remove("RUST_LOG")
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .expect("start the binary"),
    );

    let root = format!("http://127.0.0.1:{port}");
    let api = format!("{root}/api");
    let client = reqwest::Client::new();
    let started = Instant::now();
    while client.get(format!("{api}/health")).send().await.is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the server never answered"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let group = serde_json::json!({"name": "team", "code": "teamx", "admins": [], "members": []});
    let created = client
        .post(format!("{api}/groups"))
        .json(&group)
        .send()
        .await
        .unwrap();
    assert!(created.status().is_success(), "{}", created.status());

    let alive = format!("http://user:secret-userinfo@127.0.0.1:{backend_port}");
    let down = "http://user:secret-userinfo@127.0.0.1:1".to_string();
    // (service, its group, its target, where the application calls it, where the API reads it, status expected)
    let services = [
        ("alive", None, &alive, "", "/services/alive", 200),
        ("down", None, &down, "", "/services/down", 502),
        (
            "alive-g",
            Some("team"),
            &alive,
            "/teamx",
            "/groups/team/services/alive-g",
            200,
        ),
        (
            "down-g",
            Some("team"),
            &down,
            "/teamx",
            "/groups/team/services/down-g",
            502,
        ),
    ];
    for (name, group, target, _, service_api, _) in &services {
        let service = serde_json::json!({
            "name": name, "listen_path": "", "real_target_url": target, "is_mocked": false,
            "rewrite_directory_urls": false, "group_name": group, "wsdl_mode": "auto", "rules": []
        });
        let created = client
            .post(format!("{api}/services"))
            .json(&service)
            .send()
            .await
            .unwrap();
        assert_eq!(created.status().as_u16(), 201, "{name}");
        let observed = client
            .post(format!("{api}{service_api}/observe"))
            .send()
            .await
            .unwrap();
        assert_eq!(observed.status().as_u16(), 204, "{name}");
    }

    for (name, _, _, prefix, _, expected) in &services {
        let mut headers = reqwest::header::HeaderMap::new();
        let mut add = |name: &'static str, value: &[u8]| {
            headers.append(
                name,
                reqwest::header::HeaderValue::from_bytes(value).unwrap(),
            );
        };
        add("authorization", b"Bearer secret-authorization");
        add("cookie", b"session=secret-cookie");
        add("x-api-key", b"secret-api-key-first");
        add("x-api-key", b"secret-api-key-second");
        add("x-auth-token", b"secret-auth-token");
        add("x-tenant-secret", b"secret-extra-header");
        add("x-note", b"secret-non-ascii-\xe9t\xe9");
        add("content-type", b"text/xml");
        let relayed = client
            .post(format!("{root}{prefix}/{name}/ws?api_key={QUERY_SECRET}"))
            .headers(headers)
            .body("<call/>")
            .send()
            .await
            .unwrap();
        assert_eq!(relayed.status().as_u16(), *expected, "{name}");
    }

    let mut answers = Vec::new();
    for (name, _, _, _, service_api, expected) in &services {
        let list = client
            .get(format!("{api}{service_api}/observed-calls"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let calls: Vec<serde_json::Value> = serde_json::from_str(&list).unwrap();
        assert_eq!(calls.len(), 1, "{name}: {list}");
        let detail = client
            .get(format!(
                "{api}{service_api}/observed-calls/{}",
                calls[0]["id"]
            ))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&detail).unwrap();
        for header in [
            "authorization",
            "cookie",
            "x-api-key",
            "x-auth-token",
            "x-tenant-secret",
        ] {
            assert_eq!(
                parsed["request"]["headers"][header], "[redacted]",
                "{name}: {header}"
            );
        }
        assert_eq!(parsed["request"]["body"]["text"], "<call/>", "{name}");
        if *expected == 200 {
            assert_eq!(
                parsed["response"]["headers"]["set-cookie"], "[redacted]",
                "{name}"
            );
        } else {
            assert_eq!(parsed["failure"]["cause"], "unreachable", "{name}");
            assert_eq!(
                parsed["failure"]["target"], "http://127.0.0.1:1/ws",
                "{name}"
            );
        }
        answers.push((name.to_string(), list));
        answers.push((name.to_string(), detail));
    }
    for (name, answer) in &answers {
        for secret in SECRETS {
            assert!(!answer.contains(secret), "{name}: {secret} in {answer}");
        }
    }

    // What the binary wrote while relaying, failures included.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let written = std::fs::read_to_string(&output).unwrap();
    assert!(written.contains("proxy"), "the log was read: {written}");
    for secret in SECRETS.iter().chain([&QUERY_SECRET]) {
        assert!(!written.contains(secret), "{secret} in the log:\n{written}");
    }

    let _ = std::fs::remove_dir_all(&data_dir);
}
