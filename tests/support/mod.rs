// The real binary started for a memory test, with a configuration of its own in a temporary directory, and the peak
// of its memory (VmHWM, Linux) read and reset between the requests a test compares.
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[path = "../../src/test_ports.rs"]
mod test_ports;
use test_ports::reserve_port;

pub struct Server {
    child: Child,
    pub base: String,
    data_dir: std::path::PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.data_dir);
    }
}

/// The binary serving `services` (as the configuration file writes them), with the variables of `env` set, once it
/// answers.
pub async fn start(services: serde_json::Value, env: &[(&str, &str)]) -> Server {
    let data_dir = std::env::temp_dir().join(format!("mimicway-memory-{}", fastrand::u64(..)));
    std::fs::create_dir_all(&data_dir).unwrap();
    let config = serde_json::json!({"groups": [], "services": services});
    // JSON is YAML: the configuration file takes it as it is.
    std::fs::write(data_dir.join("mock-config.yaml"), config.to_string()).unwrap();
    let port = reserve_port();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mimicway"));
    command
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
        .envs(env.iter().copied())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let server = Server {
        child: command.spawn().expect("start the binary"),
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

pub fn peak_kib(server: &Server) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{}/status", server.child.id()))
        .expect("the server is alive");
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
        .expect("VmHWM")
}

// The next peak read is that of what runs from now on (Linux 4.0 and later).
pub fn reset_peak(server: &Server) {
    std::fs::write(format!("/proc/{}/clear_refs", server.child.id()), "5")
        .expect("reset the memory peak of the server");
}
