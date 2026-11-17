//! Runs the real binary with `REDACT_HEADERS` set, as an operator would, sends credentials to two observed proxy
//! services (one whose backend answers, one whose backend is down) and reads back every call they kept, by both
//! routes of the API: no answer holds a credential the requests carried, nor one the backend sent back. The setting is
//! read once per process, so only a process of its own can test it.
#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[path = "../src/test_ports.rs"]
mod test_ports;
use test_ports::reserve_port;

const SECRETS: [&str; 6] = [
    "secret-authorization",
    "secret-cookie",
    "secret-api-key",
    "secret-auth-token",
    "secret-extra-header",
    "secret-set-cookie",
];

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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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
    // (service, its group, where the application calls it, where the API reads it)
    let services = [
        (
            "alive",
            None,
            format!("{root}/alive/ws"),
            format!("{api}/services/alive"),
        ),
        (
            "down",
            Some("team"),
            format!("{root}/teamx/down/ws"),
            format!("{api}/groups/team/services/down"),
        ),
    ];
    for (name, group, _, service_api) in &services {
        let target = match *name {
            "alive" => format!("http://127.0.0.1:{backend_port}"),
            _ => "http://127.0.0.1:1".to_string(),
        };
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
            .post(format!("{service_api}/observe"))
            .send()
            .await
            .unwrap();
        assert_eq!(observed.status().as_u16(), 204, "{name}");
    }

    for (name, _, call_url, _) in &services {
        let relayed = client
            .post(call_url)
            .header("authorization", "Bearer secret-authorization")
            .header("cookie", "session=secret-cookie")
            .header("x-api-key", "secret-api-key")
            .header("x-auth-token", "secret-auth-token")
            .header("x-tenant-secret", "secret-extra-header")
            .header("content-type", "text/xml")
            .body("<call/>")
            .send()
            .await
            .unwrap();
        let expected = if *name == "alive" { 200 } else { 502 };
        assert_eq!(relayed.status().as_u16(), expected, "{name}");
    }

    let mut answers = Vec::new();
    for (name, _, _, service_api) in &services {
        let list = client
            .get(format!("{service_api}/observed-calls"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let calls: Vec<serde_json::Value> = serde_json::from_str(&list).unwrap();
        assert_eq!(calls.len(), 1, "{name}: {list}");
        let detail = client
            .get(format!("{service_api}/observed-calls/{}", calls[0]["id"]))
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
        answers.push(list);
        answers.push(detail);
    }
    let alive_detail: serde_json::Value = serde_json::from_str(&answers[1]).unwrap();
    assert_eq!(
        alive_detail["response"]["headers"]["set-cookie"],
        "[redacted]"
    );
    let down_detail: serde_json::Value = serde_json::from_str(&answers[3]).unwrap();
    assert_eq!(down_detail["failure"]["cause"], "unreachable");

    for answer in &answers {
        for secret in SECRETS {
            assert!(!answer.contains(secret), "{secret} in {answer}");
        }
    }

    let _ = std::fs::remove_dir_all(&data_dir);
}
