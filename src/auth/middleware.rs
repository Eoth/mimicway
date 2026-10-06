use crate::auth::AuthConfig;
use crate::auth::keycloak::KeycloakClient;
use crate::i18n::tr;
use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub username: String,
    pub is_super_admin: bool,
}

impl AuthUser {
    pub fn anonymous() -> Self {
        Self {
            username: "anonymous".into(),
            is_super_admin: true,
        }
    }
}

pub async fn auth_middleware(
    auth_config: AuthConfig,
    keycloak: Option<KeycloakClient>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    let no_auth_paths = [
        "/api/health",
        "/api/auth/status",
        "/api/auth/login",
        "/api/auth/validate",
    ];
    // Only the management API is protected. Every other path is either the SPA shell, which must load without a
    // token to show the login screen, or traffic of the mocked/proxied services: that traffic comes from the
    // applications under test, which hold no Mimicway token, and a token required here would then be forwarded
    // to the real backend by the proxy.
    if !crate::server::validation::is_management_api_route(&path)
        || no_auth_paths.iter().any(|p| path == *p)
    {
        req.extensions_mut().insert(AuthUser::anonymous());
        return next.run(req).await;
    }

    if !auth_config.enabled {
        req.extensions_mut().insert(AuthUser::anonymous());
        return next.run(req).await;
    }

    let kc = match keycloak.as_ref() {
        Some(kc) => kc,
        None => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };

    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from);

    let Some(token) = token else {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({"error": tr("Missing token.", &[])})),
        )
            .into_response();
    };

    match kc.validate_token(&token).await {
        Ok(username) => {
            let is_super_admin = auth_config.is_super_admin(&username);
            req.extensions_mut().insert(AuthUser {
                username,
                is_super_admin,
            });
            next.run(req).await
        }
        // The details stay in the log: they would tell a caller how its token was judged, and expose
        // Keycloak's internal address when it is unreachable.
        Err(crate::auth::keycloak::AuthError::KeycloakUnavailable(detail)) => {
            tracing::warn!(error = %detail, "token not checked: Keycloak unavailable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(
                    serde_json::json!({"error": tr("Authentication service unavailable.", &[])}),
                ),
            )
                .into_response()
        }
        Err(e) => {
            tracing::debug!(error = %e, "token validation failed");
            (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({"error": tr("Invalid or expired token.", &[])})),
            )
                .into_response()
        }
    }
}

pub fn extract_user(extensions: &axum::http::Extensions) -> AuthUser {
    extensions
        .get::<AuthUser>()
        .cloned()
        .unwrap_or_else(AuthUser::anonymous)
}

// Each branch of the middleware, through the real router on a real port, as it runs in production.
// Valid tokens are real JWTs signed by the fake realm of `auth::test_realm`, which publishes its keys like
// Keycloak does.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::test_realm::FakeRealm;

    fn enabled_auth_config(keycloak_url: String, super_admins: Vec<String>) -> AuthConfig {
        AuthConfig {
            enabled: true,
            keycloak_url,
            realm: "test-realm".into(),
            client_id: "mimicway".into(),
            super_admins,
            issuer: String::new(),
            show_reset_button: false,
        }
    }

    fn disabled_auth_config() -> AuthConfig {
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

    async fn spawn_test_app(auth_config: AuthConfig, keycloak: Option<KeycloakClient>) -> String {
        spawn_test_app_with_config(auth_config, keycloak, crate::models::MockConfig::empty()).await
    }

    async fn spawn_test_app_with_config(
        auth_config: AuthConfig,
        keycloak: Option<KeycloakClient>,
        config: crate::models::MockConfig,
    ) -> String {
        let data_dir = crate::server::test_support::temp_data_dir("auth-mw-test");
        std::fs::create_dir_all(&data_dir).unwrap();
        let store = crate::store::MockStore::new(data_dir.join("mock-config.yaml")).unwrap();
        store.replace(config).await.unwrap();
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
        let state = crate::server::AppState {
            store,
            proxy: crate::engine::ProxyClient::new(),
            seq_counters: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            request_log: crate::server::request_log::RequestLog::new(),
            auth_config,
            keycloak,
            script_engine: crate::engine::script::ScriptEngine::new(),
            ping_cache: crate::server::ping::PingCache::new(),
            observation: crate::server::observation::ObservationState::new(),
            #[cfg(feature = "messaging-kafka")]
            messaging,
            #[cfg(feature = "tcp-mock")]
            tcp_runtime,
        };
        let app = crate::server::build_router(state, &data_dir);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://127.0.0.1:{port}/api")
    }

    #[tokio::test]
    async fn missing_token_rejected_on_protected_route() {
        // No Authorization header on a protected route: 401, without asking Keycloak anything.
        let realm = FakeRealm::start().await;
        let auth_config = realm.auth_config(vec![]);
        let keycloak = Some(KeycloakClient::new(auth_config.clone()));
        let base = spawn_test_app(auth_config, keycloak).await;

        let client = reqwest::Client::new();
        let resp = client.get(format!("{base}/services")).send().await.unwrap();
        assert_eq!(resp.status().as_u16(), 401);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["error"], "Missing token.");
    }

    #[tokio::test]
    async fn non_bearer_scheme_treated_as_missing_token() {
        // Another scheme than Bearer (Basic, for instance) counts as no token.
        let realm = FakeRealm::start().await;
        let auth_config = realm.auth_config(vec![]);
        let keycloak = Some(KeycloakClient::new(auth_config.clone()));
        let base = spawn_test_app(auth_config, keycloak).await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{base}/services"))
            .header("authorization", "Basic dXNlcjpwYXNz")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 401);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["error"], "Missing token.");
    }

    #[tokio::test]
    async fn invalid_token_rejected() {
        // An invalid token: 401, without saying why.
        let realm = FakeRealm::start().await;
        let auth_config = realm.auth_config(vec![]);
        let keycloak = Some(KeycloakClient::new(auth_config.clone()));
        let base = spawn_test_app(auth_config, keycloak).await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{base}/services"))
            .header("authorization", "Bearer wrong-token")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 401);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["error"], "Invalid or expired token.");
    }

    #[tokio::test]
    async fn unreachable_keycloak_answers_503_without_internal_details() {
        let auth_config = enabled_auth_config("http://127.0.0.1:1".into(), vec![]);
        let keycloak = Some(KeycloakClient::new(auth_config.clone()));
        let base = spawn_test_app(auth_config, keycloak).await;
        let token = crate::auth::test_realm::SigningKey::generate("k1")
            .sign(&serde_json::json!({"preferred_username": "alice"}));
        let resp = reqwest::Client::new()
            .get(format!("{base}/services"))
            .header("authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 503);
        assert!(!resp.text().await.unwrap().contains("127.0.0.1"));
    }

    #[tokio::test]
    async fn valid_token_injects_authuser_readable_by_next_handler() {
        // A valid token: the handlers receive the user it names (GET /api/auth/me echoes it back).
        let realm = FakeRealm::start().await;
        let auth_config = realm.auth_config(vec!["alice".into()]);
        let keycloak = Some(KeycloakClient::new(auth_config.clone()));
        let base = spawn_test_app(auth_config, keycloak).await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{base}/auth/me"))
            .header(
                "authorization",
                format!("Bearer {}", realm.token_for("alice")),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["username"], "alice");
        assert_eq!(body["is_super_admin"], true);
    }

    #[tokio::test]
    async fn bypass_routes_accessible_without_any_token() {
        // The routes open without a token answer even with no Keycloak client at all, which proves they are let
        // through before anything depends on Keycloak (they would answer 500 otherwise).
        let auth_config = enabled_auth_config("http://127.0.0.1:1".into(), vec![]);
        let base = spawn_test_app(auth_config, None).await;
        let client = reqwest::Client::new();

        let health = client.get(format!("{base}/health")).send().await.unwrap();
        assert_eq!(
            health.status().as_u16(),
            200,
            "/api/health must not require a token"
        );

        let status = client
            .get(format!("{base}/auth/status"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            status.status().as_u16(),
            200,
            "/api/auth/status must not require a token"
        );

        // Login and validate reach their handler, which then fails for another reason (no Keycloak client): what
        // matters is the absence of the middleware's 401.
        let login = client
            .post(format!("{base}/auth/login"))
            .json(&serde_json::json!({"username": "u", "password": "p"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            login.status().as_u16(),
            400,
            "/api/auth/login must not require a token (it then fails on the missing Keycloak client, not on the token)"
        );

        let validate = client
            .post(format!("{base}/auth/validate"))
            .json(&serde_json::json!({"token": "whatever"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            validate.status().as_u16(),
            400,
            "/api/auth/validate must not require a token"
        );
    }

    #[tokio::test]
    async fn static_asset_routes_accessible_without_any_token() {
        // The UI's own files load without a token, or a browser would never get the login screen. The test serves
        // an empty directory, so the files answer 404: what matters is that the middleware does not answer 401.
        let auth_config = enabled_auth_config("http://127.0.0.1:1".into(), vec![]);
        let base = spawn_test_app(auth_config, None).await;
        let root = base.trim_end_matches("/api");
        let client = reqwest::Client::new();

        for path in ["/", "/index.html", "/assets/main.js", "/favicon.ico"] {
            let resp = client.get(format!("{root}{path}")).send().await.unwrap();
            assert_ne!(
                resp.status().as_u16(),
                401,
                "the UI file {path} must never require a token"
            );
        }
    }

    #[tokio::test]
    async fn static_asset_bypass_does_not_widen_api_protection() {
        // Letting the UI's files through never opens another /api route.
        let realm = FakeRealm::start().await;
        let auth_config = realm.auth_config(vec![]);
        let keycloak = Some(KeycloakClient::new(auth_config.clone()));
        let base = spawn_test_app(auth_config, keycloak).await;
        let client = reqwest::Client::new();

        let resp = client.get(format!("{base}/services")).send().await.unwrap();
        assert_eq!(
            resp.status().as_u16(),
            401,
            "/api/services must stay protected although the UI files are open"
        );
    }

    #[tokio::test]
    async fn auth_disabled_injects_anonymous_super_admin() {
        // Authentication off: every route, /api/auth/me included, gets the anonymous super-admin.
        let base = spawn_test_app(disabled_auth_config(), None).await;
        let client = reqwest::Client::new();
        let resp = client.get(format!("{base}/auth/me")).send().await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["username"], "anonymous");
        assert_eq!(body["is_super_admin"], true);
    }

    #[tokio::test]
    async fn misconfigured_auth_without_keycloak_fails_closed() {
        // Authentication on but no Keycloak client (an inconsistent state): fail closed with 500, before even
        // looking for a token, rather than let the request through.
        let auth_config = enabled_auth_config("http://127.0.0.1:1".into(), vec![]);
        let base = spawn_test_app(auth_config, None).await;
        let client = reqwest::Client::new();
        let resp = client.get(format!("{base}/services")).send().await.unwrap();
        assert_eq!(resp.status().as_u16(), 500);
    }

    #[tokio::test]
    async fn mocked_service_traffic_needs_no_token_while_the_api_still_does() {
        // The applications under test call the mocks with their own credentials (or none): requiring a Mimicway
        // token there broke every mock as soon as authentication was enabled, and would have forwarded that token
        // to the real backend on proxied rules.
        let service: crate::models::Service = serde_json::from_value(serde_json::json!({
            "name": "orders", "listen_path": "", "real_target_url": "", "is_mocked": true,
            "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto",
            "rules": [{
                "name": "list", "method": "GET", "sub_path": null, "action": "mock",
                "pre_script": null, "script": null, "post_script": null,
                "conditions": {"all_of": [], "any_of": []},
                "response": {"status": 200, "headers": [], "body": [{"type": "Literal", "value": "mocked"}]}
            }]
        }))
        .unwrap();
        let realm = FakeRealm::start().await;
        let auth_config = realm.auth_config(vec![]);
        let keycloak = Some(KeycloakClient::new(auth_config.clone()));
        let config = crate::models::MockConfig {
            services: vec![service],
            groups: vec![],
        };
        let base = spawn_test_app_with_config(auth_config, keycloak, config).await;
        let root = base.trim_end_matches("/api");
        let client = reqwest::Client::new();

        let mocked = client
            .get(format!("{root}/orders/list"))
            .send()
            .await
            .unwrap();
        assert_eq!(mocked.status().as_u16(), 200);
        assert_eq!(mocked.text().await.unwrap(), "mocked");

        for api in ["/api/services", "/API/services"] {
            let resp = client.get(format!("{root}{api}")).send().await.unwrap();
            assert_ne!(resp.status().as_u16(), 200, "{api} must stay protected");
        }
    }
}
