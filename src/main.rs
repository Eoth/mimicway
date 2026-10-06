// Starts Mimicway: reads the environment, then serves the library's router (lib.rs holds the server itself).
// No unsafe Rust, tests included (lib.rs says why).
#![forbid(unsafe_code)]

use mimicway::auth::AuthConfig;
use mimicway::auth::keycloak::KeycloakClient;
use mimicway::engine::ProxyClient;
use mimicway::engine::script::ScriptEngine;
use mimicway::server::browser_guard::BrowserGuard;
use mimicway::server::ping::PingCache;
use mimicway::server::request_log::RequestLog;
use mimicway::server::{AppState, build_router_with};
use mimicway::store::MockStore;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// Reads the configuration from the environment, loads the stored mocks, builds the proxy, the Keycloak client (when
// authentication is on) and the script engine, then serves HTTP until SIGTERM or Ctrl+C.
#[tokio::main]
async fn main() {
    let (filter, legacy_log_filter) = log_filter(std::env::var("RUST_LOG").ok().as_deref());
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_new(&filter)
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER)),
        )
        .init();
    if legacy_log_filter {
        tracing::warn!(
            "RUST_LOG names light_mock, the former name of this program: read as mimicway, please update it"
        );
    }

    let bind_ip = bind_address().unwrap_or_else(|message| exit_with(&message));

    let auth_config = AuthConfig::from_env().unwrap_or_else(|message| exit_with(&message));

    let data_dir = MockStore::data_path();
    let store = MockStore::load_or_init(&data_dir)
        .await
        .unwrap_or_else(|e| {
            exit_with(&format!(
                "cannot load the configuration from {} (DATA_PATH): {e}",
                data_dir.display()
            ))
        });

    let ui = mimicway::server::ui_files::UiSource::from_env();

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(7342);

    let keycloak = if auth_config.enabled {
        tracing::info!(
            keycloak_url = %auth_config.keycloak_url,
            realm = %auth_config.realm,
            "auth enabled, connecting to Keycloak"
        );
        Some(KeycloakClient::new(auth_config.clone()))
    } else {
        tracing::info!("auth disabled");
        None
    };

    #[cfg(feature = "messaging-kafka")]
    let messaging = {
        let kafka_config = mimicway::messaging::KafkaConfig::from_env();
        let message_log = mimicway::messaging::message_log::MessageLog::new();
        let publisher = if kafka_config.enabled {
            tracing::info!(
                topic = %kafka_config.listen_topic,
                brokers = ?kafka_config.brokers,
                "messaging: Kafka enabled, starting consumer"
            );
            mimicway::messaging::consumer::spawn(
                kafka_config.clone(),
                store.clone(),
                message_log.clone(),
            )
        } else {
            tracing::info!("messaging: Kafka disabled (KAFKA_ENABLED=false)");
            mimicway::messaging::consumer::Publisher::None
        };
        mimicway::messaging::MessagingState {
            message_log,
            reply_topic: kafka_config.reply_topic,
            publisher,
        }
    };

    // Raw TCP mocks listen on their own ports, next to the HTTP server, and are reconfigured live through the API
    // (TcpRuntime::replace). Their tasks are detached: on shutdown only HTTP is drained, and open TCP connections are
    // cut, since a binary protocol has no generic way to end a session cleanly from the server side anyway.
    #[cfg(feature = "tcp-mock")]
    let tcp_runtime = mimicway::tcp::TcpRuntime::load_and_spawn(&data_dir, bind_ip).await;

    let state = AppState {
        store,
        proxy: ProxyClient::new(),
        seq_counters: Arc::new(RwLock::new(HashMap::new())),
        request_log: RequestLog::new(),
        auth_config,
        keycloak,
        script_engine: ScriptEngine::new(),
        ping_cache: PingCache::new(),
        observation: mimicway::server::observation::ObservationState::new(),
        #[cfg(feature = "messaging-kafka")]
        messaging,
        #[cfg(feature = "tcp-mock")]
        tcp_runtime,
    };

    let store_for_shutdown = state.store.clone();
    let guard = BrowserGuard::from_env().with_loopback_hosts_only(bind_ip.is_loopback());
    tracing::info!(ui = %ui.describe(), "serving the UI");
    let app = build_router_with(state, ui, guard);
    let addr = std::net::SocketAddr::new(bind_ip, port);

    if bind_ip.is_loopback() {
        tracing::info!(
            addr = %addr,
            "Mimicway listening on this machine only (set BIND_ADDRESS=0.0.0.0 to accept remote connections)"
        );
    } else {
        tracing::info!(addr = %addr, "Mimicway listening");
    }

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| {
            exit_with(&format!(
                "cannot listen on {addr}: {e} (another process may use the port; see PORT and BIND_ADDRESS)"
            ))
        });

    if let Err(e) = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
    {
        tracing::error!(error = %e, "server stopped on an error");
    }

    // Changes are written to disk in the background: drain that queue before exiting, so a normal stop keeps the last
    // changes. A SIGKILL or a crash can still lose the writes still queued.
    tracing::info!("draining pending config writes before exit");
    store_for_shutdown.flush().await;
}

const DEFAULT_LOG_FILTER: &str = "mimicway=info";

/// The log filter: `RUST_LOG`, else Mimicway's informational messages. A filter written for the former name of the
/// program (`light_mock=debug`) keeps working after the rename; the flag says it was translated, to report it.
fn log_filter(rust_log: Option<&str>) -> (String, bool) {
    match rust_log.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.contains("light_mock") => {
            (value.replace("light_mock", "mimicway"), true)
        }
        Some(value) => (value.to_string(), false),
        None => (DEFAULT_LOG_FILTER.to_string(), false),
    }
}

/// Stops the start with a message meant for whoever configures the process, instead of a panic and its trace.
fn exit_with(message: &str) -> ! {
    eprintln!("Mimicway cannot start: {message}");
    std::process::exit(2)
}

// Loopback by default: without authentication (the default), a server reachable from the network lets anyone on it
// rewrite the mocks and use the proxy rules. Containers set BIND_ADDRESS=0.0.0.0, the network being theirs.
fn bind_address() -> Result<std::net::IpAddr, String> {
    let raw = std::env::var("BIND_ADDRESS").unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(std::net::Ipv4Addr::LOCALHOST.into());
    }
    raw.parse().map_err(|_| {
        format!(
            "BIND_ADDRESS must be an IP address such as 127.0.0.1, 0.0.0.0 or ::, got \"{raw}\""
        )
    })
}

// Kubernetes, Docker and systemd stop a process with SIGTERM, not SIGINT: listening to Ctrl+C alone let every
// pod stop kill the process before the write-behind drain above, losing the last queued configuration writes.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %e, "cannot listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(e) => {
                tracing::error!(error = %e, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
    tracing::info!("shutdown signal received");
}

// The crate root resolves `mod tests;` to src/tests.rs; the tests of this file sit next to it instead.
#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
