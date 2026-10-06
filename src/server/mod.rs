// The Axum router and its middleware, in the order a request goes through them (see `build_router_with`).
//   api.rs           management API handlers (/api/...), with the authorization of each endpoint
//   intercept.rs     service traffic: matching, mock rendering, proxying
//   browser_guard.rs what a web page may do with the management API (CORS, cross-site writes, DNS rebinding)
//   validation.rs    names, reserved routes and paths, before anything is stored or forwarded
//   request_log.rs   the last 200 requests, in memory, credentials redacted (redaction.rs)
mod api;
pub mod browser_guard;
pub(crate) mod codegen;
mod intercept;
pub mod observation;
pub mod ping;
pub mod redaction;
pub mod request_log;
pub mod suggestion;
pub mod ui_files;
pub mod validation;

use crate::auth::AuthConfig;
use crate::auth::keycloak::KeycloakClient;
use crate::engine::ProxyClient;
use crate::engine::script::ScriptEngine;
use crate::store::MockStore;
use axum::Router;
use ping::PingCache;
use request_log::RequestLog;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct AppState {
    pub store: MockStore,
    pub proxy: ProxyClient,
    pub seq_counters: Arc<RwLock<HashMap<String, Arc<AtomicU64>>>>,
    pub request_log: RequestLog,
    pub auth_config: AuthConfig,
    pub keycloak: Option<KeycloakClient>,
    pub script_engine: ScriptEngine,
    pub ping_cache: PingCache,
    pub observation: observation::ObservationState,
    #[cfg(feature = "messaging-kafka")]
    pub messaging: crate::messaging::MessagingState,
    #[cfg(feature = "tcp-mock")]
    pub tcp_runtime: crate::tcp::TcpRuntime,
}

impl AppState {
    pub fn next_seq(&self, service_name: &str) -> u64 {
        {
            let counters = self.seq_counters.read().unwrap();
            if let Some(counter) = counters.get(service_name) {
                return counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let mut counters = self.seq_counters.write().unwrap();
        let counter = counters
            .entry(service_name.to_string())
            .or_insert_with(|| Arc::new(AtomicU64::new(0)));
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

// Body of `GET /runtime-config.json`, see `runtime_config_handler`.
#[derive(serde::Serialize)]
struct RuntimeConfig {
    api_base_url: String,
}

// Tells the UI where the management API lives (`API_BASE_URL`), for deployments that route `/api` to another origin
// than the UI's files. Empty means the UI's own origin, which is the default and covers one process serving both.
//
// It is served outside `/api`, next to index.html, so that the UI can always fetch it from where it was loaded, and
// without authentication, since the UI reads it before knowing whether anyone is signed in. The value is read from
// the environment at each request rather than baked into the UI at build time: one image serves every environment.
async fn runtime_config_handler() -> axum::Json<RuntimeConfig> {
    axum::Json(runtime_config(crate::settings::env))
}

fn runtime_config(lookup: impl Fn(&str) -> Option<String>) -> RuntimeConfig {
    let api_base_url = lookup("API_BASE_URL")
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .to_string();
    RuntimeConfig { api_base_url }
}

/// The production router with the default browser guard (no extra CORS origin, all host names accepted), for
/// tests; `main` builds its guard from the configuration.
#[cfg(test)]
pub fn build_router(state: AppState, static_dir: &std::path::Path) -> Router {
    build_router_with(
        state,
        ui_files::UiSource::Directory(static_dir.to_path_buf()),
        browser_guard::BrowserGuard::new(""),
    )
}

pub fn build_router_with(
    state: AppState,
    ui: ui_files::UiSource,
    guard: browser_guard::BrowserGuard,
) -> Router {
    let cors = guard.cors_layer();
    let api_routes = api::routes();

    let auth_config = state.auth_config.clone();
    let keycloak = state.keycloak.clone();

    let router = Router::new()
        .route(
            "/runtime-config.json",
            axum::routing::get(runtime_config_handler),
        )
        .nest("/api", api_routes);
    let router = match ui {
        ui_files::UiSource::Directory(dir) => {
            router.fallback_service(ServeDir::new(dir).append_index_html_on_directories(true))
        }
        ui_files::UiSource::Embedded(files) => router.fallback(
            move |method: axum::http::Method, uri: axum::http::Uri| async move {
                ui_files::serve(files, &method, &uri)
            },
        ),
    };
    router
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            intercept::intercept_layer,
        ))
        .layer(axum::middleware::from_fn(move |req, next| {
            crate::auth::middleware::auth_middleware(
                auth_config.clone(),
                keycloak.clone(),
                req,
                next,
            )
        }))
        .layer(axum::middleware::from_fn_with_state(
            guard.clone(),
            browser_guard::management_api_guard,
        ))
        .layer(axum::middleware::from_fn_with_state(
            guard,
            browser_guard::security_headers,
        ))
        .layer(axum::middleware::from_fn(crate::i18n::language_scope))
        .with_state(state)
        .layer(cors)
}

#[cfg(test)]
pub(crate) mod test_support;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::vars;

    #[test]
    fn runtime_config_defaults_to_empty_when_env_unset() {
        assert_eq!(runtime_config(vars(&[])).api_base_url, "");
    }

    #[test]
    fn runtime_config_returns_configured_value() {
        let config = runtime_config(vars(&[("API_BASE_URL", "https://api.example.com")]));
        assert_eq!(config.api_base_url, "https://api.example.com");
    }

    #[test]
    fn runtime_config_trims_trailing_slash_and_whitespace() {
        let config = runtime_config(vars(&[("API_BASE_URL", "  https://api.example.com/  ")]));
        assert_eq!(config.api_base_url, "https://api.example.com");
    }

    async fn spawn_test_app(auth_config: crate::auth::AuthConfig) -> String {
        let data_dir = crate::server::test_support::temp_data_dir("servermod-test");
        std::fs::create_dir_all(&data_dir).unwrap();
        let store = crate::store::MockStore::new(data_dir.join("mock-config.yaml"));
        store
            .replace(crate::models::MockConfig {
                services: vec![],
                groups: vec![],
            })
            .await
            .unwrap();
        store.flush().await;

        #[cfg(feature = "messaging-kafka")]
        let messaging = crate::messaging::MessagingState {
            message_log: crate::messaging::message_log::MessageLog::new(),
            reply_topic: None,
            publisher: crate::messaging::consumer::Publisher::None,
        };
        #[cfg(feature = "tcp-mock")]
        let tcp_runtime =
            crate::tcp::TcpRuntime::load_and_spawn(&data_dir, crate::tcp::LOOPBACK).await;
        let state = AppState {
            store,
            proxy: crate::engine::ProxyClient::new(),
            seq_counters: Arc::new(RwLock::new(HashMap::new())),
            request_log: RequestLog::new(),
            auth_config,
            keycloak: None,
            script_engine: crate::engine::script::ScriptEngine::new(),
            ping_cache: PingCache::new(),
            observation: observation::ObservationState::new(),
            #[cfg(feature = "messaging-kafka")]
            messaging,
            #[cfg(feature = "tcp-mock")]
            tcp_runtime,
        };
        let app = build_router(state, &data_dir);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://127.0.0.1:{port}")
    }

    #[tokio::test]
    async fn runtime_config_route_accessible_without_token_when_auth_enabled() {
        // Authentication on with no Keycloak client makes every protected route fail closed (500); this one must
        // still answer without a token, since the UI reads it before anyone signs in.
        let auth_config = crate::auth::AuthConfig {
            enabled: true,
            keycloak_url: "http://127.0.0.1:1".into(),
            realm: "test-realm".into(),
            client_id: "mimicway".into(),
            super_admins: vec![],
            issuer: String::new(),
            show_reset_button: false,
        };
        let base = spawn_test_app(auth_config).await;
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{base}/runtime-config.json"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body["api_base_url"].is_string(), "{body}");
    }

    #[tokio::test]
    async fn runtime_config_route_not_intercepted_as_a_mock_service() {
        // Through the real router: the interception layer lets it pass (is_internal_route) even when services exist.
        let auth_config = crate::auth::AuthConfig {
            enabled: false,
            keycloak_url: String::new(),
            realm: String::new(),
            client_id: String::new(),
            super_admins: vec![],
            issuer: String::new(),
            show_reset_button: false,
        };
        let base = spawn_test_app(auth_config).await;
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{base}/runtime-config.json"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["api_base_url"], "");
    }
}
