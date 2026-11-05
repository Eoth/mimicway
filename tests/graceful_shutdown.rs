//! Runs the real binary and stops it the way Kubernetes, Docker and systemd do (SIGTERM): the process must drain
//! its pending configuration writes and exit cleanly instead of being killed by the signal.
#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[path = "../src/test_ports.rs"]
mod test_ports;
use test_ports::reserve_port;

#[tokio::test]
async fn sigterm_drains_pending_writes_and_exits_cleanly() {
    let data_dir = std::env::temp_dir().join(format!("mimicway-sigterm-{}", fastrand::u64(..)));
    std::fs::create_dir_all(&data_dir).unwrap();
    let port = reserve_port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mimicway"))
        .env("DATA_PATH", &data_dir)
        .env("STATIC_DIR", &data_dir)
        .env("PORT", port.to_string())
        .env("BIND_ADDRESS", "127.0.0.1")
        .env_remove("AUTH_ENABLED")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the binary");

    let base = format!("http://127.0.0.1:{port}/api");
    let client = reqwest::Client::new();
    let started = Instant::now();
    while client.get(format!("{base}/health")).send().await.is_err() {
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the server never answered"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let service = serde_json::json!({
        "name": "drained", "listen_path": "", "real_target_url": "", "is_mocked": true,
        "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto", "rules": []
    });
    let created = client
        .post(format!("{base}/services"))
        .json(&service)
        .send()
        .await
        .unwrap();
    assert_eq!(created.status().as_u16(), 201);

    let killed = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(killed.success());
    let status = child.wait().unwrap();
    assert_eq!(
        status.code(),
        Some(0),
        "SIGTERM must trigger the graceful shutdown, got {status:?}"
    );

    let yaml = std::fs::read_to_string(data_dir.join("mock-config.yaml")).unwrap();
    assert!(
        yaml.contains("drained"),
        "the last mutation must be on disk after the drain"
    );
    let _ = std::fs::remove_dir_all(&data_dir);
}
