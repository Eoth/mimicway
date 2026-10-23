// The live state of the TCP mocks: configuration, listener tasks and their status, behind one lock. Simpler than
// MockStore: no write-behind and no backups, changes being rare and small (see `tcp::config`).
//
// `try_update()` is the only way to change it (used by the API, server/api.rs): under the write lock, it checks the
// change against the current configuration, writes the file atomically (temporary file, then rename, as MockStore
// does), then stops the old listeners before starting the new ones, so the TCP mocks change without restarting the
// process or touching the HTTP server.
use crate::tcp::config::TcpConfig;
use crate::tcp::listener::{TcpServiceStatus, spawn_tcp_services};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

struct Inner {
    config: TcpConfig,
    handles: Vec<JoinHandle<()>>,
    statuses: Vec<TcpServiceStatus>,
}

#[derive(Clone)]
pub struct TcpRuntime {
    data_dir: PathBuf,
    bind_ip: std::net::IpAddr,
    inner: Arc<RwLock<Inner>>,
}

impl TcpRuntime {
    /// Loads `{data_dir}/tcp-config.yaml` (missing or unreadable means empty, see `TcpConfig::load`) and starts its
    /// listeners on `bind_ip`. Called once at startup (main.rs).
    pub async fn load_and_spawn(data_dir: &Path, bind_ip: std::net::IpAddr) -> Self {
        let config = TcpConfig::load(data_dir);
        let (handles, statuses) = spawn_tcp_services(&config, bind_ip).await;
        Self {
            data_dir: data_dir.to_path_buf(),
            bind_ip,
            inner: Arc::new(RwLock::new(Inner {
                config,
                handles,
                statuses,
            })),
        }
    }

    pub async fn snapshot_config(&self) -> TcpConfig {
        self.inner.read().await.config.clone()
    }

    pub async fn statuses(&self) -> Vec<TcpServiceStatus> {
        self.inner.read().await.statuses.clone()
    }

    /// Applies `change` to a copy of the configuration; when it accepts, writes the result and restarts every listener
    /// (no per-service diff: there are a handful, and a full restart is cheap and simpler to get right). Check, write and
    /// restart run under the write lock: two changes at the same time are each checked against what the other left,
    /// not both against one snapshot, where the second replaced the first without a word. A refusal changes nothing.
    /// Each old task is aborted AND awaited: awaiting an aborted task returns only once it and its `TcpListener` are
    /// dropped, so the port is free before it is bound again.
    pub async fn try_update<T, E>(
        &self,
        change: impl FnOnce(&mut TcpConfig) -> Result<T, E>,
    ) -> std::io::Result<Result<T, E>> {
        let mut guard = self.inner.write().await;
        let mut new_config = guard.config.clone();
        let accepted = match change(&mut new_config) {
            Ok(accepted) => accepted,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let yaml = serde_yaml::to_string(&new_config).map_err(std::io::Error::other)?;
        Self::write_to_disk(&TcpConfig::config_file(&self.data_dir), &yaml)?;

        for handle in guard.handles.drain(..) {
            handle.abort();
            let _ = handle.await;
        }
        let (handles, statuses) = spawn_tcp_services(&new_config, self.bind_ip).await;
        guard.config = new_config;
        guard.handles = handles;
        guard.statuses = statuses;
        Ok(Ok(accepted))
    }

    #[cfg(test)]
    async fn replace(&self, new_config: TcpConfig) -> std::io::Result<()> {
        self.try_update(|config| {
            *config = new_config;
            Ok::<(), std::convert::Infallible>(())
        })
        .await
        .map(|_| ())
    }

    fn write_to_disk(path: &Path, yaml: &str) -> std::io::Result<()> {
        let parent = path
            .parent()
            .expect("tcp-config.yaml always has a parent directory (DATA_PATH)");
        std::fs::create_dir_all(parent)?;
        let tmp_path = parent.join(".tcp-config.yaml.tmp");
        std::fs::write(&tmp_path, yaml.as_bytes())?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::config::{TcpMatcher, TcpRule, TcpService};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    fn temp_dir() -> PathBuf {
        let dir = crate::server::test_support::temp_data_dir("tcp-runtime-test");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn load_and_spawn_starts_empty_with_no_config_file() {
        let dir = temp_dir();
        let runtime = TcpRuntime::load_and_spawn(&dir, crate::tcp::LOOPBACK).await;
        assert!(runtime.snapshot_config().await.services.is_empty());
        assert!(runtime.statuses().await.is_empty());
    }

    #[tokio::test]
    async fn replace_persists_config_to_disk() {
        let dir = temp_dir();
        let runtime = TcpRuntime::load_and_spawn(&dir, crate::tcp::LOOPBACK).await;

        let config = TcpConfig {
            services: vec![TcpService {
                name: "svc".into(),
                listen_port: 0,
                rules: vec![],
            }],
        };
        runtime.replace(config.clone()).await.unwrap();

        let on_disk = std::fs::read_to_string(TcpConfig::config_file(&dir)).unwrap();
        let parsed: TcpConfig = serde_yaml::from_str(&on_disk).unwrap();
        assert_eq!(parsed, config);
    }

    #[tokio::test]
    async fn replace_updates_live_statuses() {
        let dir = temp_dir();
        let runtime = TcpRuntime::load_and_spawn(&dir, crate::tcp::LOOPBACK).await;
        assert!(runtime.statuses().await.is_empty());

        let config = TcpConfig {
            services: vec![TcpService {
                name: "svc".into(),
                listen_port: 0,
                rules: vec![],
            }],
        };
        runtime.replace(config).await.unwrap();

        let statuses = runtime.statuses().await;
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].listening);
    }

    #[tokio::test]
    async fn replace_on_same_port_frees_it_before_rebinding() {
        // The risky case of replace(): a service replaced without changing its port must bind again, which requires the old
        // listener to have released the port ("address already in use" otherwise).
        let dir = temp_dir();
        let runtime = TcpRuntime::load_and_spawn(&dir, crate::tcp::LOOPBACK).await;

        // A real port (not 0), so that the second replace() reuses exactly the same one.
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let config = TcpConfig {
            services: vec![TcpService {
                name: "svc".into(),
                listen_port: port,
                rules: vec![TcpRule {
                    name: "r1".into(),
                    matcher: TcpMatcher::Any,
                    response_hex: crate::tcp::hex::encode(b"v1"),
                }],
            }],
        };
        runtime.replace(config).await.unwrap();
        assert!(runtime.statuses().await[0].listening);

        // Same port, another rule: binding again must work.
        let config2 = TcpConfig {
            services: vec![TcpService {
                name: "svc".into(),
                listen_port: port,
                rules: vec![TcpRule {
                    name: "r2".into(),
                    matcher: TcpMatcher::Any,
                    response_hex: crate::tcp::hex::encode(b"v2"),
                }],
            }],
        };
        runtime.replace(config2).await.unwrap();
        let statuses = runtime.statuses().await;
        assert!(
            statuses[0].listening,
            "rebind on the same port should succeed: {statuses:?}"
        );

        // The NEW rule answers: the new listener took over, the old one is gone.
        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut resp = [0u8; 2];
        client.read_exact(&mut resp).await.unwrap();
        assert_eq!(&resp, b"v2");
    }
}
