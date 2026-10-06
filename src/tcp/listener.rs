// Raw TCP listening: one task per configured service, next to the HTTP server and never on its port (a binary
// protocol cannot be routed by URL path). TCP services live outside the Axum router, so no authentication, CORS or
// HTTP middleware applies: they mean nothing on a raw socket. Mock only, never a relay (see `tcp::mod`).
use crate::tcp::config::TcpService;
use crate::tcp::{hex, matcher};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

const READ_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_MAX_MESSAGE_SIZE: usize = 16 * 1024;

/// Largest first message read from a connection (`TCP_MOCK_MAX_MESSAGE_SIZE`, 16 KiB by default, like
/// `request_log::max_body_size`), so that a hostile client cannot grow memory before any matching happens.
pub fn max_message_size() -> usize {
    max_message_size_in(crate::settings::env)
}

fn max_message_size_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
    crate::settings::number(
        lookup,
        "TCP_MOCK_MAX_MESSAGE_SIZE",
        DEFAULT_MAX_MESSAGE_SIZE,
    )
}

/// Whether a TCP service could listen, as `GET /api/tcp/status` reports it, so that an operator sees a port
/// conflict without reading the log. Set when the listeners (re)start.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TcpServiceStatus {
    pub name: String,
    pub listen_port: u16,
    /// The address actually listened on (`BIND_ADDRESS` and the port), or the one that could not be.
    pub address: String,
    pub listening: bool,
    pub error: Option<String>,
}

/// Starts one accept loop per configured service on `bind_ip`. Each task holds the `TcpService` it serves, so a
/// connection reads no shared state.
///
/// A service whose port cannot be bound (taken, not permitted) is logged, reported in its status and skipped: it
/// never keeps the HTTP server or the other TCP services from starting.
pub async fn spawn_tcp_services(
    config: &crate::tcp::config::TcpConfig,
    bind_ip: std::net::IpAddr,
) -> (Vec<JoinHandle<()>>, Vec<TcpServiceStatus>) {
    let mut handles = Vec::with_capacity(config.services.len());
    let mut statuses = Vec::with_capacity(config.services.len());

    for service in &config.services {
        // The interface of the HTTP server (`BIND_ADDRESS`, loopback by default): a mock created on a workstation
        // must not be reachable from the network when the rest of Mimicway is not.
        let addr = std::net::SocketAddr::new(bind_ip, service.listen_port);
        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(
                    service = %service.name,
                    addr = %addr,
                    error = %e,
                    "tcp-mock: failed to bind, service skipped"
                );
                statuses.push(TcpServiceStatus {
                    name: service.name.clone(),
                    listen_port: service.listen_port,
                    address: addr.to_string(),
                    listening: false,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        let addr = listener.local_addr().unwrap_or(addr);
        tracing::info!(service = %service.name, addr = %addr, "tcp-mock: listening");
        statuses.push(TcpServiceStatus {
            name: service.name.clone(),
            listen_port: service.listen_port,
            address: addr.to_string(),
            listening: true,
            error: None,
        });

        let service = Arc::new(service.clone());
        handles.push(tokio::spawn(async move {
            loop {
                let (stream, peer) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::warn!(service = %service.name, error = %e, "tcp-mock: accept failed");
                        continue;
                    }
                };
                let service = service.clone();
                tokio::spawn(async move {
                    tracing::debug!(service = %service.name, peer = %peer, "tcp-mock: connection accepted");
                    handle_connection(stream, service).await;
                });
            }
        }));
    }

    (handles, statuses)
}

/// One connection: read one message, match it, answer if a rule matches, close. No loop over several exchanges (see
/// `tcp::mod`): enough for a simple request and response. Without a matching rule, the connection just closes.
async fn handle_connection(mut stream: TcpStream, service: Arc<TcpService>) {
    let mut buf = vec![0u8; max_message_size()];
    let n = match tokio::time::timeout(READ_TIMEOUT, stream.read(&mut buf)).await {
        Ok(Ok(0)) => return,
        Ok(Ok(n)) => n,
        Ok(Err(e)) => {
            tracing::debug!(service = %service.name, error = %e, "tcp-mock: read failed");
            return;
        }
        Err(_) => {
            tracing::debug!(service = %service.name, "tcp-mock: read timeout, closing");
            return;
        }
    };
    let data = &buf[..n];

    let Some(rule) = matcher::match_rule(&service.rules, data) else {
        tracing::debug!(service = %service.name, "tcp-mock: no rule matched, closing");
        return;
    };

    match hex::decode(&rule.response_hex) {
        Ok(response) => {
            if let Err(e) = stream.write_all(&response).await {
                tracing::debug!(
                    service = %service.name, rule = %rule.name, error = %e,
                    "tcp-mock: mock write failed"
                );
            }
        }
        Err(e) => {
            tracing::error!(
                service = %service.name, rule = %rule.name, error = %e,
                "tcp-mock: invalid response_hex in matched rule, closing without reply"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::LOOPBACK;
    use crate::tcp::config::{TcpMatcher, TcpRule};

    fn svc(rules: Vec<TcpRule>) -> Arc<TcpService> {
        Arc::new(TcpService {
            name: "svc".into(),
            listen_port: 0,
            rules,
        })
    }

    #[tokio::test]
    async fn mock_rule_replies_with_configured_bytes() {
        let service = svc(vec![TcpRule {
            name: "hello".into(),
            matcher: TcpMatcher::Any,
            response_hex: hex::encode(b"pong"),
        }]);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, service).await;
        });

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut resp = [0u8; 4];
        client.read_exact(&mut resp).await.unwrap();
        assert_eq!(&resp, b"pong");
    }

    #[tokio::test]
    async fn no_matching_rule_closes_connection_without_reply() {
        let service = svc(vec![TcpRule {
            name: "specific".into(),
            matcher: TcpMatcher::Prefix(hex::encode(b"X")),
            response_hex: hex::encode(b"nope-should-not-see-this"),
        }]);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, service).await;
        });

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut resp = Vec::new();
        let n = tokio::time::timeout(Duration::from_millis(300), client.read_to_end(&mut resp))
            .await
            .unwrap_or(Ok(0))
            .unwrap_or(0);
        assert_eq!(n, 0);
        assert!(resp.is_empty());
    }

    #[tokio::test]
    async fn no_rules_at_all_closes_connection_without_reply() {
        let service = svc(vec![]);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            handle_connection(stream, service).await;
        });

        let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let mut resp = Vec::new();
        let n = tokio::time::timeout(Duration::from_millis(300), client.read_to_end(&mut resp))
            .await
            .unwrap_or(Ok(0))
            .unwrap_or(0);
        assert_eq!(n, 0);
    }

    #[test]
    fn max_message_size_defaults_to_16kb() {
        assert_eq!(max_message_size_in(crate::settings::vars(&[])), 16 * 1024);
    }

    #[test]
    fn max_message_size_from_env() {
        let lookup = crate::settings::vars(&[("TCP_MOCK_MAX_MESSAGE_SIZE", "64")]);
        assert_eq!(max_message_size_in(lookup), 64);
    }

    #[tokio::test]
    async fn spawn_reports_listening_true_for_a_free_port() {
        // Port 0 lets the OS pick a free port, so it cannot express "port taken" (the next test reserves one first).
        let config = crate::tcp::config::TcpConfig {
            services: vec![TcpService {
                name: "free-port".into(),
                listen_port: 0,
                rules: vec![],
            }],
        };
        let (_handles, statuses) = spawn_tcp_services(&config, LOOPBACK).await;
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].listening);
        assert!(statuses[0].error.is_none());
    }

    #[tokio::test]
    async fn listens_on_the_bind_address_only() {
        let config = crate::tcp::config::TcpConfig {
            services: vec![TcpService {
                name: "local-only".into(),
                listen_port: 0,
                rules: vec![],
            }],
        };
        let (_handles, statuses) = spawn_tcp_services(&config, LOOPBACK).await;
        assert!(
            statuses[0].address.starts_with("127.0.0.1:"),
            "a loopback-only Mimicway must not expose its TCP mocks: {:?}",
            statuses[0]
        );
    }

    #[tokio::test]
    async fn spawn_reports_listening_false_when_port_already_taken() {
        // Reserve a real port on the same address first, for a conflict that does not depend on which ports happen to be
        // free on the machine.
        let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let taken_port = reserved.local_addr().unwrap().port();

        let config = crate::tcp::config::TcpConfig {
            services: vec![TcpService {
                name: "taken-port".into(),
                listen_port: taken_port,
                rules: vec![],
            }],
        };
        let (handles, statuses) = spawn_tcp_services(&config, LOOPBACK).await;
        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].listening);
        assert!(statuses[0].error.is_some());
        // No listener task for the service that failed.
        assert!(handles.is_empty());

        drop(reserved);
    }
}
