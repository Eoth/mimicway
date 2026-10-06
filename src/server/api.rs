use crate::auth::middleware::AuthUser;
use crate::auth::{can_access_service, can_manage_group, visible_services};
use crate::engine::matcher::{
    ConditionEvaluation, ConflictWinner, MatchEngine, OtherRuleConflictInput, RequestData,
    RuleConflictDraft, RuleTestInput,
};
use crate::engine::script::ScriptContext;
use crate::i18n::tr;
use crate::models::{ConditionGroup, Group, MockConfig, RuleAction, Service};
use crate::server::AppState;
use crate::server::request_log::LogEntry;
use crate::server::validation::{validate_backup_filename, validate_service};
use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete as delete_route, get, post, put};
use axum::{Json, Router};
use std::collections::HashMap;
use std::sync::Arc;

pub fn routes() -> Router<AppState> {
    let router = Router::new()
        .route("/health", get(health))
        .route("/auth/status", get(auth_status))
        .route("/auth/login", post(login))
        .route("/auth/validate", post(validate_token))
        .route("/auth/me", get(get_me))
        .route("/config", get(get_config).put(put_config))
        .route("/config/reset", delete_route(reset_config))
        .route("/config/backups", get(list_backups))
        .route("/config/restore/{filename}", post(restore_backup))
        .route("/services", get(list_services).post(create_service))
        .route(
            "/services/{name}",
            get(get_service).put(update_service).delete(delete_service),
        )
        .route("/services/{name}/toggle", put(toggle_service))
        .route("/services/{name}/ping", post(ping_service))
        .route("/services/{name}/rules/reorder", put(reorder_rules))
        .route(
            "/groups/{group}/services/{name}",
            get(get_service_grouped)
                .put(update_service_grouped)
                .delete(delete_service_grouped),
        )
        .route(
            "/groups/{group}/services/{name}/toggle",
            put(toggle_service_grouped),
        )
        .route(
            "/groups/{group}/services/{name}/ping",
            post(ping_service_grouped),
        )
        .route(
            "/groups/{group}/services/{name}/rules/reorder",
            put(reorder_rules_grouped),
        )
        .route("/script/validate", post(validate_script))
        .route("/rule-test", post(test_rule))
        .route("/rule-conflicts", post(check_rule_conflicts))
        .route("/logs", get(get_logs))
        .route("/observation/status", get(get_observation_status))
        .route(
            "/services/{name}/observe",
            post(observe_service).delete(unobserve_service),
        )
        .route(
            "/groups/{group}/services/{name}/observe",
            post(observe_service_grouped).delete(unobserve_service_grouped),
        )
        .route("/services/{name}/suggestions", get(get_service_suggestions))
        .route(
            "/groups/{group}/services/{name}/suggestions",
            get(get_service_suggestions_grouped),
        )
        .route("/groups", get(list_groups).post(create_group))
        .route(
            "/groups/{name}",
            get(get_group).put(update_group).delete(delete_group),
        )
        .route("/groups/{name}/members", put(update_group_members));

    #[cfg(feature = "messaging-kafka")]
    let router = router
        .route("/messaging/status", get(messaging_status))
        .route("/messaging/logs", get(get_messaging_logs))
        .route("/messaging/simulate", post(simulate_message));

    #[cfg(feature = "tcp-mock")]
    let router = router
        .route("/tcp/status", get(get_tcp_status))
        .route(
            "/tcp/services",
            get(list_tcp_services).post(create_tcp_service),
        )
        .route(
            "/tcp/services/{name}",
            put(update_tcp_service).delete(delete_tcp_service),
        );

    router
}

// --------------- Health ---------------

#[derive(serde::Serialize)]
struct HealthResponse {
    status: &'static str,
    write_queue: crate::store::WriteQueueStatus,
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        write_queue: state.store.writer_status(),
    })
}

// --------------- Auth ---------------

#[derive(serde::Serialize)]
struct AuthStatusResponse {
    enabled: bool,
    show_reset_button: bool,
}

async fn auth_status(State(state): State<AppState>) -> Json<AuthStatusResponse> {
    Json(AuthStatusResponse {
        enabled: state.auth_config.enabled,
        show_reset_button: state.auth_config.show_reset_button,
    })
}

#[derive(serde::Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(serde::Serialize)]
struct LoginResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    username: String,
    is_super_admin: bool,
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    if !state.auth_config.enabled {
        return Err(AppError::Validation(tr(
            "Authentication is not enabled.",
            &[],
        )));
    }

    let kc = state
        .keycloak
        .as_ref()
        .ok_or_else(|| AppError::Validation(tr("Authentication is not configured.", &[])))?;

    use crate::auth::keycloak::AuthError;
    let tokens = kc
        .login(&req.username, &req.password)
        .await
        .map_err(|e| match e {
            AuthError::InvalidCredentials => AppError::Unauthorized,
            other => {
                tracing::warn!(error = %other, "login: Keycloak unavailable");
                AppError::Unavailable
            }
        })?;
    // The identity is the one Keycloak put in the token (it normalizes user names), checked the way every API
    // call will check it: a token refused here (wrong KEYCLOAK_ISSUER or client) would be refused right after.
    let username = kc
        .validate_token(&tokens.access_token)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "login: Keycloak issued a token that Mimicway refuses (check KEYCLOAK_ISSUER and KEYCLOAK_CLIENT_ID)");
            match e {
                AuthError::KeycloakUnavailable(_) => AppError::Unavailable,
                _ => AppError::Unauthorized,
            }
        })?;

    let is_super_admin = state.auth_config.is_super_admin(&username);

    Ok(Json(LoginResponse {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        expires_in: tokens.expires_in,
        username,
        is_super_admin,
    }))
}

#[derive(serde::Deserialize)]
struct ValidateRequest {
    token: String,
}

async fn validate_token(
    State(state): State<AppState>,
    Json(req): Json<ValidateRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    if !state.auth_config.enabled {
        return Err(AppError::Validation(tr(
            "Authentication is not enabled.",
            &[],
        )));
    }

    let kc = state
        .keycloak
        .as_ref()
        .ok_or_else(|| AppError::Validation(tr("Authentication is not configured.", &[])))?;

    let username = kc.validate_token(&req.token).await.map_err(|e| match e {
        crate::auth::keycloak::AuthError::KeycloakUnavailable(_) => AppError::Unavailable,
        _ => AppError::Unauthorized,
    })?;

    let is_super_admin = state.auth_config.is_super_admin(&username);

    Ok(Json(LoginResponse {
        access_token: req.token,
        refresh_token: None,
        expires_in: 0,
        username,
        is_super_admin,
    }))
}

#[derive(serde::Serialize)]
struct MeResponse {
    username: String,
    is_super_admin: bool,
    groups: Vec<UserGroupInfo>,
}

#[derive(serde::Serialize)]
struct UserGroupInfo {
    name: String,
    role: String,
}

async fn get_me(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
) -> Json<MeResponse> {
    let config = state.store.snapshot().await;
    let mut groups = Vec::new();

    for g in &config.groups {
        if user.is_super_admin || g.admins.contains(&user.username) {
            groups.push(UserGroupInfo {
                name: g.name.clone(),
                role: "admin".into(),
            });
        } else if g.members.contains(&user.username) {
            groups.push(UserGroupInfo {
                name: g.name.clone(),
                role: "member".into(),
            });
        }
    }

    Json(MeResponse {
        username: user.username,
        is_super_admin: user.is_super_admin,
        groups,
    })
}

// --------------- Config ---------------

/// With authentication, a user who is not a super-admin gets only the groups they belong to and the services they
/// can access, as the list endpoints do: the full configuration exposed every other team's mocks and targets.
async fn get_config(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
) -> Json<Arc<MockConfig>> {
    let config = state.store.snapshot().await;
    if !state.auth_config.enabled || user.is_super_admin {
        return Json(config);
    }
    Json(Arc::new(MockConfig {
        services: visible_services(&user.username, false, &config),
        groups: config
            .groups
            .iter()
            .filter(|g| g.admins.contains(&user.username) || g.members.contains(&user.username))
            .cloned()
            .collect(),
    }))
}

async fn put_config(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Json(mut config): Json<MockConfig>,
) -> Result<Json<Arc<MockConfig>>, AppError> {
    require_super_admin(&user)?;

    for service in &config.services {
        if let Err(e) = validate_service(service) {
            tracing::warn!(service = %service.name, field = %e.field, reason = %e.message, "config rejected: invalid service");
            return Err(AppError::Validation(e.message));
        }
    }

    if let Some((service, group)) = config.unknown_group_references().first() {
        return Err(AppError::Validation(tr(
            "Service \"{0}\" refers to the group \"{1}\", which is not defined in this configuration.",
            &[service, group],
        )));
    }

    ensure_group_codes(&mut config.groups);

    state.store.replace(config).await.map_err(AppError::Store)?;
    Ok(Json(state.store.snapshot().await))
}

async fn reset_config(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
) -> Result<StatusCode, AppError> {
    require_super_admin(&user)?;

    state
        .store
        .backup_before_reset()
        .await
        .map_err(AppError::Store)?;

    tracing::info!(user = %user.username, "config reset: all services removed");
    state
        .store
        .replace(MockConfig::empty())
        .await
        .map_err(AppError::Store)?;
    // Observation state lives outside the configuration: without this, a service created again after the reset would
    // start out observed.
    state.observation.toggle.clear_all();
    Ok(StatusCode::NO_CONTENT)
}

async fn list_backups(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
) -> Result<Json<Vec<crate::store::BackupInfo>>, AppError> {
    require_super_admin(&user)?;
    let backups = state.store.list_backups().await.map_err(AppError::Store)?;
    Ok(Json(backups))
}

async fn restore_backup(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(filename): Path<String>,
) -> Result<StatusCode, AppError> {
    require_super_admin(&user)?;

    if let Err(e) = validate_backup_filename(&filename) {
        tracing::warn!(filename = %filename, reason = %e.message, "restore rejected: invalid filename");
        return Err(AppError::Validation(e.message));
    }

    tracing::info!(user = %user.username, filename = %filename, "config restore requested");
    state
        .store
        .restore_from_backup(&filename)
        .await
        .map_err(|e| match e {
            crate::store::StoreError::NotFound(_) => AppError::NotFound,
            other => AppError::Store(other),
        })?;
    Ok(StatusCode::NO_CONTENT)
}

// --------------- Logs ---------------

#[derive(serde::Deserialize)]
struct LogsQuery {
    #[serde(default = "default_log_limit")]
    limit: usize,
}
fn default_log_limit() -> usize {
    50
}

/// Entries of the services the user can access only (all of them without authentication or for a super-admin):
/// captured requests carry other teams' traffic.
async fn get_logs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<LogsQuery>,
) -> Json<Vec<LogEntry>> {
    if !state.auth_config.enabled || user.is_super_admin {
        return Json(state.request_log.recent(q.limit));
    }
    let config = state.store.snapshot().await;
    let entries = state
        .request_log
        .recent(usize::MAX)
        .into_iter()
        .filter(|entry| {
            config.services.iter().any(|s| {
                service_matches(s, entry.group_name.as_deref(), &entry.service_name)
                    && can_access_service(&user.username, false, s, &config.groups)
            })
        })
        .take(q.limit)
        .collect();
    Json(entries)
}

// --------------- Traffic observation (service-level proxy) ---------------
// Turned on and off by a user, never automatically, and only for a pure proxy (is_mocked=false), the one path that
// streams traffic without capturing it. Same access rule as toggle and ping: a reversible action that changes no
// stored configuration.

#[derive(serde::Serialize, serde::Deserialize)]
struct ObservationStatusEntry {
    group_name: Option<String>,
    service_name: String,
}

async fn get_observation_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
) -> Json<Vec<ObservationStatusEntry>> {
    let config = state.store.snapshot().await;
    let entries = state
        .observation
        .toggle
        .active_services()
        .into_iter()
        .filter_map(|(group, name)| {
            let svc = config
                .services
                .iter()
                .find(|s| service_matches(s, group.as_deref(), &name))?;
            if state.auth_config.enabled
                && !can_access_service(&user.username, user.is_super_admin, svc, &config.groups)
            {
                return None;
            }
            Some(ObservationStatusEntry {
                group_name: group,
                service_name: name,
            })
        })
        .collect();
    Json(entries)
}

async fn observe_service_impl(
    state: AppState,
    user: AuthUser,
    group: Option<String>,
    name: String,
    enable: bool,
) -> Result<StatusCode, AppError> {
    let config = state.store.snapshot().await;
    let svc = config
        .services
        .iter()
        .find(|s| service_matches(s, group.as_deref(), &name))
        .ok_or(AppError::NotFound)?;

    if state.auth_config.enabled
        && !can_access_service(&user.username, user.is_super_admin, svc, &config.groups)
    {
        return Err(AppError::Forbidden);
    }

    if enable && svc.is_mocked {
        return Err(AppError::Validation(tr(
            "Traffic observation only applies to a service that is a pure proxy (is_mocked=false).",
            &[],
        )));
    }

    if enable {
        state.observation.toggle.enable(group.as_deref(), &name);
    } else {
        state.observation.toggle.disable(group.as_deref(), &name);
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn observe_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    observe_service_impl(state, user, None, name, true).await
}

async fn observe_service_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    observe_service_impl(state, user, Some(group), name, true).await
}

async fn unobserve_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    observe_service_impl(state, user, None, name, false).await
}

async fn unobserve_service_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    observe_service_impl(state, user, Some(group), name, false).await
}

/// Computed from `ObservationStore` at each call, so there is no suggestion state to keep in sync. Endpoints with
/// fewer observations than `suggestion::min_samples()` are left out.
async fn get_service_suggestions_impl(
    state: AppState,
    user: AuthUser,
    group: Option<String>,
    name: String,
) -> Result<Json<Vec<crate::server::suggestion::Suggestion>>, AppError> {
    let config = state.store.snapshot().await;
    let svc = config
        .services
        .iter()
        .find(|s| service_matches(s, group.as_deref(), &name))
        .ok_or(AppError::NotFound)?;

    if state.auth_config.enabled
        && !can_access_service(&user.username, user.is_super_admin, svc, &config.groups)
    {
        return Err(AppError::Forbidden);
    }

    let keys = state
        .observation
        .store
        .keys_for_service(group.as_deref(), &name);
    let suggestions = keys
        .into_iter()
        .filter_map(|key| {
            let observations = state.observation.store.observations(&key);
            crate::server::suggestion::suggest(&key.method, &key.sub_path, &observations)
        })
        .collect();
    Ok(Json(suggestions))
}

async fn get_service_suggestions(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<Json<Vec<crate::server::suggestion::Suggestion>>, AppError> {
    get_service_suggestions_impl(state, user, None, name).await
}

async fn get_service_suggestions_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
) -> Result<Json<Vec<crate::server::suggestion::Suggestion>>, AppError> {
    get_service_suggestions_impl(state, user, Some(group), name).await
}

// --------------- Messaging (Kafka) ---------------
// Only compiled with the "messaging-kafka" feature (see routes()).

#[cfg(feature = "messaging-kafka")]
#[derive(serde::Serialize)]
struct MessagingStatusResponse {
    /// Always true: the UI only needs to know that the route exists, which tells it the binary has the feature (a
    /// binary without it answers 404).
    available: bool,
}

#[cfg(feature = "messaging-kafka")]
async fn messaging_status() -> Json<MessagingStatusResponse> {
    Json(MessagingStatusResponse { available: true })
}

#[cfg(feature = "messaging-kafka")]
// Kafka is configured for the whole instance (brokers and topics come from the environment), not per group:
// its message log spans every team's services and a simulation publishes on the real reply topic. With
// authentication, both are reserved to super-admins.
async fn get_messaging_logs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Query(q): Query<LogsQuery>,
) -> Result<Json<Vec<crate::messaging::message_log::MessageLogEntry>>, AppError> {
    if state.auth_config.enabled {
        require_super_admin(&user)?;
    }
    Ok(Json(state.messaging.message_log.recent(q.limit)))
}

/// Runs a message through the same steps as the Kafka consumer (match, render, log, publish to the reply topic when
/// one is configured), without a Kafka producer: for testing messaging rules from the UI and in end-to-end tests.
#[cfg(feature = "messaging-kafka")]
#[derive(serde::Deserialize)]
struct SimulateMessageRequest {
    topic: String,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    payload: String,
}

#[cfg(feature = "messaging-kafka")]
async fn simulate_message(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Json(req): Json<SimulateMessageRequest>,
) -> Result<StatusCode, AppError> {
    if state.auth_config.enabled {
        require_super_admin(&user)?;
    }
    crate::messaging::consumer::process_message(
        &state.store,
        &state.messaging.message_log,
        state.messaging.reply_topic.as_deref(),
        &state.messaging.publisher,
        &req.topic,
        req.payload.as_bytes(),
        req.headers,
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

// --------------- Raw TCP mocks ---------------
// Only compiled with the "tcp-mock" feature. Reading the services shows their matchers and responses, and changing
// them opens or closes network ports: with authentication on, reading needs a signed-in user and every change a
// super-admin. A change is written to disk, then the affected listeners restart, without restarting the process.

#[cfg(feature = "tcp-mock")]
async fn get_tcp_status(State(state): State<AppState>) -> Json<Vec<crate::tcp::TcpServiceStatus>> {
    Json(state.tcp_runtime.statuses().await)
}

#[cfg(feature = "tcp-mock")]
async fn list_tcp_services(
    State(state): State<AppState>,
    Extension(_user): Extension<AuthUser>,
) -> Json<Vec<crate::tcp::config::TcpService>> {
    Json(state.tcp_runtime.snapshot_config().await.services)
}

#[cfg(feature = "tcp-mock")]
async fn create_tcp_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Json(service): Json<crate::tcp::config::TcpService>,
) -> Result<(StatusCode, Json<crate::tcp::config::TcpService>), AppError> {
    if state.auth_config.enabled {
        require_super_admin(&user)?;
    }

    let mut config = state.tcp_runtime.snapshot_config().await;

    if let Err(e) = crate::tcp::validation::validate_tcp_service(&service, &config.services) {
        tracing::warn!(service = %service.name, field = %e.field, reason = %e.message, "tcp service rejected");
        return Err(AppError::Validation(e.message));
    }

    config.services.push(service.clone());
    state
        .tcp_runtime
        .replace(config)
        .await
        .map_err(|e| AppError::Validation(e.to_string()))?;

    Ok((StatusCode::CREATED, Json(service)))
}

#[cfg(feature = "tcp-mock")]
async fn update_tcp_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
    Json(service): Json<crate::tcp::config::TcpService>,
) -> Result<Json<crate::tcp::config::TcpService>, AppError> {
    if state.auth_config.enabled {
        require_super_admin(&user)?;
    }

    let mut config = state.tcp_runtime.snapshot_config().await;
    let Some(idx) = config.services.iter().position(|s| s.name == name) else {
        return Err(AppError::NotFound);
    };

    // Leave the service being replaced out of the name and port checks, or a PUT that changes nothing would conflict
    // with itself.
    let others: Vec<_> = config
        .services
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .map(|(_, s)| s.clone())
        .collect();
    if let Err(e) = crate::tcp::validation::validate_tcp_service(&service, &others) {
        tracing::warn!(service = %name, field = %e.field, reason = %e.message, "tcp service rejected");
        return Err(AppError::Validation(e.message));
    }

    config.services[idx] = service.clone();
    state
        .tcp_runtime
        .replace(config)
        .await
        .map_err(|e| AppError::Validation(e.to_string()))?;

    Ok(Json(service))
}

#[cfg(feature = "tcp-mock")]
async fn delete_tcp_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    if state.auth_config.enabled {
        require_super_admin(&user)?;
    }

    let mut config = state.tcp_runtime.snapshot_config().await;
    let before = config.services.len();
    config.services.retain(|s| s.name != name);
    if config.services.len() == before {
        return Err(AppError::NotFound);
    }

    state
        .tcp_runtime
        .replace(config)
        .await
        .map_err(|e| AppError::Validation(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

// --------------- Services ---------------

async fn list_services(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
) -> Json<Vec<Service>> {
    let config = state.store.snapshot().await;
    Json(visible_services(
        &user.username,
        user.is_super_admin,
        &config,
    ))
}

// A service is identified by its group and its name: two groups may each hold a service of the same name, and the
// ungrouped services (`group` = None) form one more such namespace.
fn service_matches(s: &Service, group: Option<&str>, name: &str) -> bool {
    s.name == name && s.group_name.as_deref() == group
}

async fn get_service_impl(
    state: AppState,
    user: AuthUser,
    group: Option<String>,
    name: String,
) -> Result<Json<Service>, AppError> {
    let config = state.store.snapshot().await;
    let service = config
        .services
        .iter()
        .find(|s| service_matches(s, group.as_deref(), &name))
        .ok_or(AppError::NotFound)?;

    if state.auth_config.enabled
        && !can_access_service(&user.username, user.is_super_admin, service, &config.groups)
    {
        return Err(AppError::Forbidden);
    }

    Ok(Json(service.clone()))
}

async fn get_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<Json<Service>, AppError> {
    get_service_impl(state, user, None, name).await
}

async fn get_service_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
) -> Result<Json<Service>, AppError> {
    get_service_impl(state, user, Some(group), name).await
}

/// Whether `user` may put a service in `group`: anyone without authentication; with it, a super-admin anywhere
/// and a group admin in that group only. The ungrouped scope belongs to super-admins.
fn check_service_scope(
    state: &AppState,
    user: &AuthUser,
    cfg: &MockConfig,
    group: Option<&str>,
) -> Result<(), AppError> {
    if !state.auth_config.enabled || user.is_super_admin {
        return Ok(());
    }
    let Some(group) = group else {
        return Err(AppError::Forbidden);
    };
    match cfg.groups.iter().find(|g| g.name == group) {
        Some(g) if can_manage_group(&user.username, false, g) => Ok(()),
        Some(_) => Err(AppError::Forbidden),
        None => Err(unknown_group(group)),
    }
}

fn unknown_group(group: &str) -> AppError {
    AppError::Validation(tr("The group \"{0}\" does not exist.", &[&group]))
}

/// Whether `service` can take its (group, name) slot in `cfg`, the service currently at `replacing` excepted:
/// the group must exist, the name must be free in that group, and an ungrouped service cannot be named like a
/// group code, since `/{code}/...` would then route to two places.
fn check_service_slot(
    cfg: &MockConfig,
    service: &Service,
    replacing: Option<(Option<&str>, &str)>,
) -> Result<(), AppError> {
    let group = service.group_name.as_deref();
    if let Some(group) = group
        && !cfg.groups.iter().any(|g| g.name == group)
    {
        return Err(unknown_group(group));
    }
    let taken = cfg.services.iter().any(|s| {
        service_matches(s, group, &service.name)
            && replacing.is_none_or(|(g, n)| !service_matches(s, g, n))
    });
    if taken {
        return Err(AppError::Conflict(tr(
            "A service named \"{0}\" already exists in this group.",
            &[&service.name],
        )));
    }
    if group.is_none()
        && let Some(g) = cfg
            .groups
            .iter()
            .find(|g| g.code.eq_ignore_ascii_case(&service.name))
    {
        return Err(AppError::Conflict(tr(
            "\"{0}\" is the URL code of the group \"{1}\": an ungrouped service cannot use it as its name.",
            &[&service.name, &g.name],
        )));
    }
    Ok(())
}

async fn create_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Json(service): Json<Service>,
) -> Result<(StatusCode, Json<Service>), AppError> {
    if let Err(e) = validate_service(&service) {
        tracing::warn!(service = %service.name, field = %e.field, reason = %e.message, "service rejected");
        return Err(AppError::Validation(e.message));
    }

    let updated = state
        .store
        .try_update(|cfg| {
            check_service_scope(&state, &user, cfg, service.group_name.as_deref())?;
            check_service_slot(cfg, &service, None)?;
            cfg.services.push(service.clone());
            Ok(())
        })
        .await
        .map_err(AppError::Store)??;

    updated
        .services
        .iter()
        .find(|s| service_matches(s, service.group_name.as_deref(), &service.name))
        .cloned()
        .map(|s| (StatusCode::CREATED, Json(s)))
        .ok_or(AppError::NotFound)
}

async fn update_service_impl(
    state: AppState,
    user: AuthUser,
    group: Option<String>,
    name: String,
    service: Service,
) -> Result<Json<Service>, AppError> {
    if let Err(e) = validate_service(&service) {
        tracing::warn!(service = %name, field = %e.field, reason = %e.message, "service rejected");
        return Err(AppError::Validation(e.message));
    }

    let updated = state
        .store
        .try_update(|cfg| {
            let index = cfg
                .services
                .iter()
                .position(|s| service_matches(s, group.as_deref(), &name))
                .ok_or(AppError::NotFound)?;
            if state.auth_config.enabled
                && !can_access_service(
                    &user.username,
                    user.is_super_admin,
                    &cfg.services[index],
                    &cfg.groups,
                )
            {
                return Err(AppError::Forbidden);
            }
            // Moving a service needs the right to create it where it goes, or a group member could push
            // services into groups (and URL prefixes) they do not belong to.
            if service.group_name != cfg.services[index].group_name {
                check_service_scope(&state, &user, cfg, service.group_name.as_deref())?;
            }
            check_service_slot(cfg, &service, Some((group.as_deref(), &name)))?;
            cfg.services[index] = service.clone();
            Ok(())
        })
        .await
        .map_err(AppError::Store)??;

    if service.group_name != group || service.name != name {
        state.observation.toggle.disable(group.as_deref(), &name);
    }
    updated
        .services
        .iter()
        .find(|s| service_matches(s, service.group_name.as_deref(), &service.name))
        .cloned()
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn update_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
    Json(service): Json<Service>,
) -> Result<Json<Service>, AppError> {
    update_service_impl(state, user, None, name, service).await
}

async fn update_service_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
    Json(service): Json<Service>,
) -> Result<Json<Service>, AppError> {
    update_service_impl(state, user, Some(group), name, service).await
}

async fn delete_service_impl(
    state: AppState,
    user: AuthUser,
    group: Option<String>,
    name: String,
) -> Result<StatusCode, AppError> {
    if state.auth_config.enabled {
        require_super_admin(&user)?;
    }

    let updated = state
        .store
        .update(|cfg| {
            cfg.services
                .retain(|s| !service_matches(s, group.as_deref(), &name));
        })
        .await
        .map_err(AppError::Store)?;

    if updated
        .services
        .iter()
        .any(|s| service_matches(s, group.as_deref(), &name))
    {
        Err(AppError::NotFound)
    } else {
        // Observation state lives outside the configuration: without this, a service created again under the same group
        // and name would start out observed.
        state.observation.toggle.disable(group.as_deref(), &name);
        Ok(StatusCode::NO_CONTENT)
    }
}

async fn delete_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    delete_service_impl(state, user, None, name).await
}

async fn delete_service_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    delete_service_impl(state, user, Some(group), name).await
}

#[derive(serde::Deserialize)]
struct TogglePayload {
    is_mocked: bool,
}

async fn toggle_service_impl(
    state: AppState,
    user: AuthUser,
    group: Option<String>,
    name: String,
    payload: TogglePayload,
) -> Result<Json<Service>, AppError> {
    {
        let config = state.store.snapshot().await;
        let svc = config
            .services
            .iter()
            .find(|s| service_matches(s, group.as_deref(), &name))
            .ok_or(AppError::NotFound)?;

        if state.auth_config.enabled
            && !can_access_service(&user.username, user.is_super_admin, svc, &config.groups)
        {
            return Err(AppError::Forbidden);
        }
    }

    let updated = state
        .store
        .update(|cfg| {
            if let Some(svc) = cfg
                .services
                .iter_mut()
                .find(|s| service_matches(s, group.as_deref(), &name))
            {
                svc.is_mocked = payload.is_mocked;
            }
        })
        .await
        .map_err(AppError::Store)?;

    updated
        .services
        .iter()
        .find(|s| service_matches(s, group.as_deref(), &name))
        .cloned()
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn toggle_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
    Json(payload): Json<TogglePayload>,
) -> Result<Json<Service>, AppError> {
    toggle_service_impl(state, user, None, name, payload).await
}

async fn toggle_service_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
    Json(payload): Json<TogglePayload>,
) -> Result<Json<Service>, AppError> {
    toggle_service_impl(state, user, Some(group), name, payload).await
}

async fn ping_service_impl(
    state: AppState,
    user: AuthUser,
    group: Option<String>,
    name: String,
) -> Result<Json<crate::engine::PingStatus>, AppError> {
    let target_url = {
        let config = state.store.snapshot().await;
        let svc = config
            .services
            .iter()
            .find(|s| service_matches(s, group.as_deref(), &name))
            .ok_or(AppError::NotFound)?;

        if state.auth_config.enabled
            && !can_access_service(&user.username, user.is_super_admin, svc, &config.groups)
        {
            return Err(AppError::Forbidden);
        }

        svc.real_target_url.clone()
    };

    // Keyed by group and name, like the services: two services of the same name in two groups have their own
    // targets. The unit separator cannot appear in a group or service name.
    let cache_key = format!("{}\u{1f}{name}", group.as_deref().unwrap_or(""));
    if let Some(cached) = state
        .ping_cache
        .get_fresh(&cache_key, crate::server::ping::PING_TTL_MS)
    {
        return Ok(Json(cached));
    }

    let status = state.proxy.ping(&target_url).await;
    state.ping_cache.set(&cache_key, status.clone());
    Ok(Json(status))
}

async fn ping_service(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<Json<crate::engine::PingStatus>, AppError> {
    ping_service_impl(state, user, None, name).await
}

async fn ping_service_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
) -> Result<Json<crate::engine::PingStatus>, AppError> {
    ping_service_impl(state, user, Some(group), name).await
}

#[derive(serde::Deserialize)]
struct ReorderPayload {
    order: Vec<String>,
}

async fn reorder_rules_impl(
    state: AppState,
    user: AuthUser,
    group: Option<String>,
    name: String,
    payload: ReorderPayload,
) -> Result<Json<Service>, AppError> {
    {
        let config = state.store.snapshot().await;
        let svc = config
            .services
            .iter()
            .find(|s| service_matches(s, group.as_deref(), &name))
            .ok_or(AppError::NotFound)?;

        if state.auth_config.enabled
            && !can_access_service(&user.username, user.is_super_admin, svc, &config.groups)
        {
            return Err(AppError::Forbidden);
        }
    }

    let updated = state
        .store
        .update(|cfg| {
            if let Some(svc) = cfg
                .services
                .iter_mut()
                .find(|s| service_matches(s, group.as_deref(), &name))
            {
                let mut reordered = Vec::with_capacity(svc.rules.len());
                for rule_name in &payload.order {
                    if let Some(pos) = svc.rules.iter().position(|r| &r.name == rule_name) {
                        reordered.push(svc.rules.remove(pos));
                    }
                }
                reordered.append(&mut svc.rules);
                svc.rules = reordered;
            }
        })
        .await
        .map_err(AppError::Store)?;

    updated
        .services
        .iter()
        .find(|s| service_matches(s, group.as_deref(), &name))
        .cloned()
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn reorder_rules(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
    Json(payload): Json<ReorderPayload>,
) -> Result<Json<Service>, AppError> {
    reorder_rules_impl(state, user, None, name, payload).await
}

async fn reorder_rules_grouped(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path((group, name)): Path<(String, String)>,
    Json(payload): Json<ReorderPayload>,
) -> Result<Json<Service>, AppError> {
    reorder_rules_impl(state, user, Some(group), name, payload).await
}

// --------------- Groups ---------------

async fn list_groups(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
) -> Json<Vec<Group>> {
    let config = state.store.snapshot().await;
    if user.is_super_admin {
        return Json(config.groups.clone());
    }
    let visible: Vec<Group> = config
        .groups
        .iter()
        .filter(|g| g.admins.contains(&user.username) || g.members.contains(&user.username))
        .cloned()
        .collect();
    Json(visible)
}

async fn get_group(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<Json<Group>, AppError> {
    let config = state.store.snapshot().await;
    let group = config
        .groups
        .iter()
        .find(|g| g.name == name)
        .ok_or(AppError::NotFound)?;

    if state.auth_config.enabled
        && !user.is_super_admin
        && !group.admins.contains(&user.username)
        && !group.members.contains(&user.username)
    {
        return Err(AppError::Forbidden);
    }

    Ok(Json(group.clone()))
}

/// Normalizes `group` (trimmed name, lowercase code, a generated code when none is given) and checks that it fits
/// in `cfg`, the group currently named `replacing` excepted: a non-empty name and a 5-character alphanumeric code,
/// both unique, and a code that no ungrouped service uses as its name (`/{code}/...` would route to both).
fn check_group_identity(
    cfg: &MockConfig,
    mut group: Group,
    replacing: Option<&str>,
) -> Result<Group, AppError> {
    group.name = group.name.trim().to_string();
    if group.name.is_empty() {
        return Err(AppError::Validation(tr("The group name is required.", &[])));
    }
    let others: Vec<&Group> = cfg
        .groups
        .iter()
        .filter(|g| Some(g.name.as_str()) != replacing)
        .collect();
    if others
        .iter()
        .any(|g| g.name.eq_ignore_ascii_case(&group.name))
    {
        return Err(AppError::Conflict(tr(
            "A group named \"{0}\" already exists.",
            &[&group.name],
        )));
    }
    group.code = if group.code.trim().is_empty() {
        let existing: Vec<String> = cfg.groups.iter().map(|g| g.code.clone()).collect();
        crate::server::codegen::generate_code(&group.name, &existing)
    } else {
        group.code.trim().to_lowercase()
    };
    if group.code.len() != 5 || !group.code.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return Err(AppError::Validation(tr(
            "A group code is exactly 5 letters or digits.",
            &[],
        )));
    }
    if others
        .iter()
        .any(|g| g.code.eq_ignore_ascii_case(&group.code))
    {
        return Err(AppError::Conflict(tr(
            "The code \"{0}\" is already used by another group.",
            &[&group.code],
        )));
    }
    if let Some(s) = cfg
        .services
        .iter()
        .find(|s| s.group_name.is_none() && s.name.eq_ignore_ascii_case(&group.code))
    {
        return Err(AppError::Conflict(tr(
            "The code \"{0}\" is the name of the ungrouped service \"{1}\": choose another code.",
            &[&group.code, &s.name],
        )));
    }
    Ok(group)
}

async fn create_group(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Json(mut group): Json<Group>,
) -> Result<(StatusCode, Json<Group>), AppError> {
    if !group.admins.contains(&user.username) {
        group.admins.push(user.username.clone());
    }

    let mut created_name = String::new();
    let updated = state
        .store
        .try_update(|cfg| {
            let group = check_group_identity(cfg, group, None)?;
            created_name = group.name.clone();
            cfg.groups.push(group);
            Ok(())
        })
        .await
        .map_err(AppError::Store)??;

    updated
        .groups
        .iter()
        .find(|g| g.name == created_name)
        .cloned()
        .map(|g| (StatusCode::CREATED, Json(g)))
        .ok_or(AppError::NotFound)
}

async fn update_group(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
    Json(group): Json<Group>,
) -> Result<Json<Group>, AppError> {
    let mut new_name = String::new();
    let updated = state
        .store
        .try_update(|cfg| {
            let index = cfg
                .groups
                .iter()
                .position(|g| g.name == name)
                .ok_or(AppError::NotFound)?;
            if state.auth_config.enabled
                && !can_manage_group(&user.username, user.is_super_admin, &cfg.groups[index])
            {
                return Err(AppError::Forbidden);
            }
            let group = check_group_identity(cfg, group, Some(&name))?;
            // Services point to their group by name: a rename left them in a group that no longer existed.
            for service in cfg.services.iter_mut() {
                if service.group_name.as_deref() == Some(name.as_str()) {
                    service.group_name = Some(group.name.clone());
                }
            }
            new_name = group.name.clone();
            cfg.groups[index] = group;
            Ok(())
        })
        .await
        .map_err(AppError::Store)??;

    updated
        .groups
        .iter()
        .find(|g| g.name == new_name)
        .cloned()
        .map(Json)
        .ok_or(AppError::NotFound)
}

async fn delete_group(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
) -> Result<StatusCode, AppError> {
    {
        let config = state.store.snapshot().await;
        let existing = config
            .groups
            .iter()
            .find(|g| g.name == name)
            .ok_or(AppError::NotFound)?;
        if state.auth_config.enabled
            && !can_manage_group(&user.username, user.is_super_admin, existing)
        {
            return Err(AppError::Forbidden);
        }
    }

    state
        .store
        .update(|cfg| {
            for svc in cfg.services.iter_mut() {
                if svc.group_name.as_deref() == Some(&name) {
                    svc.group_name = None;
                }
            }
            cfg.groups.retain(|g| g.name != name);
        })
        .await
        .map_err(AppError::Store)?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
struct UpdateMembersPayload {
    #[serde(default)]
    admins: Vec<String>,
    #[serde(default)]
    members: Vec<String>,
}

async fn update_group_members(
    State(state): State<AppState>,
    Extension(user): Extension<AuthUser>,
    Path(name): Path<String>,
    Json(payload): Json<UpdateMembersPayload>,
) -> Result<Json<Group>, AppError> {
    {
        let config = state.store.snapshot().await;
        let group = config
            .groups
            .iter()
            .find(|g| g.name == name)
            .ok_or(AppError::NotFound)?;

        if state.auth_config.enabled
            && !can_manage_group(&user.username, user.is_super_admin, group)
        {
            return Err(AppError::Forbidden);
        }
    }

    let updated = state
        .store
        .update(|cfg| {
            if let Some(g) = cfg.groups.iter_mut().find(|g| g.name == name) {
                g.admins = payload.admins.clone();
                g.members = payload.members.clone();
            }
        })
        .await
        .map_err(AppError::Store)?;

    updated
        .groups
        .iter()
        .find(|g| g.name == name)
        .cloned()
        .map(Json)
        .ok_or(AppError::NotFound)
}

// --------------- Helpers & Errors ---------------

// --------------- Script validation ---------------

#[derive(serde::Deserialize)]
struct ValidateScriptRequest {
    script: String,
}

#[derive(serde::Serialize)]
struct ValidateScriptResponse {
    valid: bool,
    error: Option<String>,
}

async fn validate_script(
    State(state): State<AppState>,
    Json(req): Json<ValidateScriptRequest>,
) -> Json<ValidateScriptResponse> {
    match state.script_engine.validate(&req.script) {
        Ok(()) => Json(ValidateScriptResponse {
            valid: true,
            error: None,
        }),
        Err(e) => Json(ValidateScriptResponse {
            valid: false,
            error: Some(e),
        }),
    }
}

// --------------- Rule tester (read-only replay) ---------------
//
// Stateless: the UI sends the rule being edited, saved or not, and a request captured earlier (CapturedRequest);
// the handler rebuilds the request, evaluates the rule on it (MatchEngine::evaluate_rule_test) and runs its scripts.
// Nothing is stored, nothing is sent over the network. Any signed-in user may call it: it reads no service from the
// configuration, and the captured request it replays is one the caller already received from /api/logs.

#[derive(serde::Deserialize, Default)]
struct RuleTestCapturedRequest {
    method: String,
    remaining_path: String,
    #[serde(default)]
    path_params: HashMap<String, String>,
    #[serde(default)]
    query_params: HashMap<String, String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    body_truncated: bool,
    #[serde(default)]
    content_type: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct RuleTestRequest {
    method: String,
    sub_path: Option<String>,
    conditions: ConditionGroup,
    // Optional, for UIs that do not send them. The scripts run under the production conditions (the rule matches and
    // is not a proxy rule, see run_rule_script in intercept.rs) and against the captured request itself, never an empty
    // one, so that a script such as `parse_json(request.body).len()` is not reported as failing for lack of a body.
    #[serde(default)]
    action: RuleAction,
    #[serde(default)]
    pre_script: Option<String>,
    #[serde(default)]
    script: Option<String>,
    #[serde(default)]
    post_script: Option<String>,
    request: RuleTestCapturedRequest,
}

#[derive(serde::Serialize)]
struct ScriptExecutionError {
    slot: &'static str,
    message: String,
}

// What a script returned (value and fields, see ScriptResult). A script can run without error and still produce
// something its author did not expect (a misspelled key, a nested path that {{script.field}} cannot reach): the
// tester shows the result so that this is seen before the rule is saved.
#[derive(serde::Serialize)]
struct ScriptExecutionResult {
    slot: &'static str,
    value: String,
    fields: HashMap<String, String>,
}

#[derive(serde::Serialize)]
struct RuleTestResponse {
    method_matches: bool,
    sub_path_matches: bool,
    path_params: HashMap<String, String>,
    overall_matched: bool,
    body_truncated: bool,
    all_of: Vec<ConditionEvaluation>,
    any_of: Vec<ConditionEvaluation>,
    // Script errors, replayed against the captured request; empty when the rule does not match (scripts do not run
    // then, as in production) or has no script. In production a failing script never blocks the response: the error
    // only reaches the server log and the template gets an empty result. This field is where a user sees it.
    script_errors: Vec<ScriptExecutionError>,
    // What the scripts that ran without error returned. A script slot appears either here or in script_errors.
    script_results: Vec<ScriptExecutionResult>,
}

async fn test_rule(
    State(state): State<AppState>,
    Extension(_user): Extension<AuthUser>,
    Json(payload): Json<RuleTestRequest>,
) -> Json<RuleTestResponse> {
    let body_truncated = payload.request.body_truncated;
    let req = RequestData {
        query_params: payload.request.query_params,
        headers: payload.request.headers,
        body: payload.request.body.into_bytes(),
        content_type: payload.request.content_type,
        path_params: payload.request.path_params,
        method: payload.request.method,
        remaining_path: payload.request.remaining_path,
    };

    let outcome = MatchEngine::evaluate_rule_test(
        RuleTestInput {
            method: &payload.method,
            sub_path: &payload.sub_path,
            conditions: &payload.conditions,
        },
        &req,
    );

    // As in production: scripts run only when the rule matches and is not a proxy rule (a proxied request renders no
    // template).
    let mut script_errors = Vec::new();
    let mut script_results = Vec::new();
    if outcome.overall_matched && payload.action != RuleAction::Proxy {
        let script_ctx = ScriptContext {
            body: String::from_utf8_lossy(&req.body).into_owned(),
            headers: req.headers.clone(),
            query_params: req.query_params.clone(),
            path_params: outcome.path_params.clone(),
        };
        for (slot, script) in [
            ("pre_script", &payload.pre_script),
            ("script", &payload.script),
            ("post_script", &payload.post_script),
        ] {
            if let Some(code) = script {
                match state.script_engine.execute(code, &script_ctx) {
                    Err(message) => script_errors.push(ScriptExecutionError { slot, message }),
                    Ok(result) => script_results.push(ScriptExecutionResult {
                        slot,
                        value: result.value,
                        fields: result.fields,
                    }),
                }
            }
        }
    }

    Json(RuleTestResponse {
        method_matches: outcome.method_matches,
        sub_path_matches: outcome.sub_path_matches,
        path_params: outcome.path_params,
        overall_matched: outcome.overall_matched,
        body_truncated,
        all_of: outcome.group.all_of,
        any_of: outcome.group.any_of,
        script_errors,
        script_results,
    })
}

// --------------- Rule conflict detection (when saving) ---------------
//
// Stateless, like the rule tester: the UI sends the rule about to be saved, the service's other rules in their
// current order and the position the new rule will take; MatchEngine::find_rule_conflicts says which overlaps
// would hide one rule behind another. Informative only: it changes nothing, and any signed-in user may call it.

#[derive(serde::Deserialize)]
struct RuleConflictDraftRequest {
    method: String,
    sub_path: Option<String>,
    conditions: ConditionGroup,
}

#[derive(serde::Deserialize)]
struct OtherRuleConflictRequest {
    name: String,
    method: String,
    sub_path: Option<String>,
    conditions: ConditionGroup,
}

#[derive(serde::Deserialize)]
struct RuleConflictsRequest {
    draft: RuleConflictDraftRequest,
    other_rules: Vec<OtherRuleConflictRequest>,
    draft_position: usize,
}

#[derive(serde::Serialize)]
struct RuleConflictResponseItem {
    other_rule_name: String,
    winner: ConflictWinner,
}

#[derive(serde::Serialize)]
struct RuleConflictsResponse {
    conflicts: Vec<RuleConflictResponseItem>,
}

async fn check_rule_conflicts(
    Extension(_user): Extension<AuthUser>,
    Json(payload): Json<RuleConflictsRequest>,
) -> Json<RuleConflictsResponse> {
    let draft = RuleConflictDraft {
        method: &payload.draft.method,
        sub_path: &payload.draft.sub_path,
        conditions: &payload.draft.conditions,
    };
    let other_rules: Vec<OtherRuleConflictInput> = payload
        .other_rules
        .iter()
        .map(|r| OtherRuleConflictInput {
            name: &r.name,
            method: &r.method,
            sub_path: &r.sub_path,
            conditions: &r.conditions,
        })
        .collect();

    let conflicts = MatchEngine::find_rule_conflicts(&draft, &other_rules, payload.draft_position);

    Json(RuleConflictsResponse {
        conflicts: conflicts
            .into_iter()
            .map(|c| RuleConflictResponseItem {
                other_rule_name: c.other_rule_name,
                winner: c.winner,
            })
            .collect(),
    })
}

fn ensure_group_codes(groups: &mut [Group]) {
    let mut existing_codes: Vec<String> = groups
        .iter()
        .filter(|g| !g.code.trim().is_empty())
        .map(|g| g.code.clone())
        .collect();
    for group in groups.iter_mut() {
        if group.code.trim().is_empty() {
            let code = crate::server::codegen::generate_code(&group.name, &existing_codes);
            group.code = code.clone();
            existing_codes.push(code);
        }
    }
}

fn require_super_admin(user: &AuthUser) -> Result<(), AppError> {
    if !user.is_super_admin {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

enum AppError {
    Store(crate::store::StoreError),
    NotFound,
    Validation(String),
    Conflict(String),
    Unauthorized,
    Forbidden,
    Unavailable,
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        match self {
            AppError::Store(e) => {
                tracing::error!(error = %e, "store error");
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
            }
            AppError::NotFound => StatusCode::NOT_FOUND.into_response(),
            AppError::Validation(msg) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response(),
            AppError::Conflict(msg) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response(),
            AppError::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "error": tr("Invalid credentials.", &[]) })),
            )
                .into_response(),
            AppError::Forbidden => (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "error": tr("Access denied.", &[]) })),
            )
                .into_response(),
            AppError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(
                    serde_json::json!({ "error": tr("Authentication service unavailable.", &[]) }),
                ),
            )
                .into_response(),
        }
    }
}

#[cfg(test)]
mod authz_tests;

#[cfg(test)]
mod tests {
    use super::*;

    // require_super_admin() is what stands between any user and reset_config or restore_backup.
    #[test]
    fn require_super_admin_rejects_non_admin() {
        let user = AuthUser {
            username: "bob".into(),
            is_super_admin: false,
        };
        assert!(matches!(
            require_super_admin(&user),
            Err(AppError::Forbidden)
        ));
    }

    #[test]
    fn require_super_admin_accepts_admin() {
        let user = AuthUser {
            username: "alice".into(),
            is_super_admin: true,
        };
        assert!(require_super_admin(&user).is_ok());
    }

    // service_matches() alone decides which service a scoped handler acts on (get, update, delete, toggle, ping,
    // reorder): names are unique per group, not globally.
    fn svc_named(name: &str, group: Option<&str>) -> Service {
        Service {
            name: name.into(),
            listen_path: "".into(),
            real_target_url: "http://example.com".into(),
            is_mocked: true,
            rewrite_directory_urls: false,
            group_name: group.map(|g| g.to_string()),
            wsdl_mode: crate::models::WsdlMode::default(),
            rules: vec![],
        }
    }

    #[test]
    fn service_matches_same_name_same_group() {
        let s = svc_named("foo", Some("team-a"));
        assert!(service_matches(&s, Some("team-a"), "foo"));
    }

    #[test]
    fn service_matches_same_name_different_group_does_not_match() {
        let s = svc_named("foo", Some("team-a"));
        assert!(!service_matches(&s, Some("team-b"), "foo"));
    }

    #[test]
    fn service_matches_ungrouped_is_its_own_scope() {
        let grouped = svc_named("foo", Some("team-a"));
        let ungrouped = svc_named("foo", None);
        assert!(!service_matches(&grouped, None, "foo"));
        assert!(service_matches(&ungrouped, None, "foo"));
    }

    #[test]
    fn service_matches_different_name_never_matches() {
        let s = svc_named("foo", Some("team-a"));
        assert!(!service_matches(&s, Some("team-a"), "bar"));
    }

    // --- The real router on a real port, called with reqwest: a regression such as deleting a same-named service of
    // another group must fail `cargo test`, not only the browser suite.
    async fn spawn_test_app(config: MockConfig) -> String {
        crate::server::test_support::assert_consistent(&config);
        let data_dir = crate::server::test_support::temp_data_dir("api-test");
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
        let state = AppState {
            store,
            proxy: crate::engine::ProxyClient::new(),
            seq_counters: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            request_log: crate::server::request_log::RequestLog::new(),
            auth_config: crate::auth::AuthConfig {
                enabled: false,
                keycloak_url: String::new(),
                realm: String::new(),
                client_id: String::new(),
                super_admins: vec![],
                issuer: String::new(),
                show_reset_button: false,
            },
            keycloak: None,
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

    fn groups_named(names: &[&str]) -> Vec<Group> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| Group {
                name: name.to_string(),
                code: format!("grp{i:02}"),
                admins: vec![],
                members: vec![],
            })
            .collect()
    }

    fn ambiguous_services() -> Vec<Service> {
        vec![
            svc_named("shared-name", Some("team-a")),
            svc_named("shared-name", Some("team-b")),
            svc_named("shared-name", None),
        ]
    }

    #[tokio::test]
    async fn get_service_grouped_route_returns_only_the_matching_group() {
        let base = spawn_test_app(MockConfig {
            services: ambiguous_services(),
            groups: groups_named(&["team-a", "team-b"]),
        })
        .await;
        let client = reqwest::Client::new();

        let via_group_a = client
            .get(format!("{base}/groups/team-a/services/shared-name"))
            .send()
            .await
            .unwrap();
        assert_eq!(via_group_a.status(), 200);
        let svc: Service = via_group_a.json().await.unwrap();
        assert_eq!(svc.group_name.as_deref(), Some("team-a"));

        let via_flat = client
            .get(format!("{base}/services/shared-name"))
            .send()
            .await
            .unwrap();
        assert_eq!(via_flat.status(), 200);
        let svc: Service = via_flat.json().await.unwrap();
        assert_eq!(
            svc.group_name, None,
            "the route without a group must resolve to the ungrouped service, not to the first one found"
        );
    }

    #[tokio::test]
    async fn delete_ambiguous_service_only_removes_the_targeted_group() {
        let base = spawn_test_app(MockConfig {
            services: ambiguous_services(),
            groups: groups_named(&["team-a", "team-b"]),
        })
        .await;
        let client = reqwest::Client::new();

        let del = client
            .delete(format!("{base}/groups/team-a/services/shared-name"))
            .send()
            .await
            .unwrap();
        assert_eq!(del.status(), 204);

        let still_team_b = client
            .get(format!("{base}/groups/team-b/services/shared-name"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            still_team_b.status(),
            200,
            "the service of team-b must not be deleted"
        );

        let still_ungrouped = client
            .get(format!("{base}/services/shared-name"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            still_ungrouped.status(),
            200,
            "the ungrouped service must not be deleted"
        );

        let gone_team_a = client
            .get(format!("{base}/groups/team-a/services/shared-name"))
            .send()
            .await
            .unwrap();
        assert_eq!(gone_team_a.status(), 404);
    }

    #[tokio::test]
    async fn update_via_flat_route_does_not_touch_grouped_namesakes() {
        let base = spawn_test_app(MockConfig {
            services: ambiguous_services(),
            groups: groups_named(&["team-a", "team-b"]),
        })
        .await;
        let client = reqwest::Client::new();

        let mut updated = svc_named("shared-name", None);
        updated.real_target_url = "http://changed.example.com".into();
        let put = client
            .put(format!("{base}/services/shared-name"))
            .json(&updated)
            .send()
            .await
            .unwrap();
        assert_eq!(put.status(), 200);

        let team_a = client
            .get(format!("{base}/groups/team-a/services/shared-name"))
            .send()
            .await
            .unwrap()
            .json::<Service>()
            .await
            .unwrap();
        assert_eq!(
            team_a.real_target_url, "http://example.com",
            "team-a must not change through a PUT on the ungrouped route"
        );
    }

    // --- test_rule(): the stateless rule tester ---

    fn anon_user() -> AuthUser {
        AuthUser::anonymous()
    }

    // A minimal state to call test_rule() directly: only the script engine matters to these tests.
    async fn test_state() -> AppState {
        let data_dir = crate::server::test_support::temp_data_dir("scripttest");
        std::fs::create_dir_all(&data_dir).unwrap();
        let store = crate::store::MockStore::new(data_dir.join("mock-config.yaml")).unwrap();
        store
            .replace(MockConfig {
                services: vec![],
                groups: vec![],
            })
            .await
            .unwrap();

        #[cfg(feature = "messaging-kafka")]
        let messaging = crate::messaging::MessagingState {
            message_log: crate::messaging::message_log::MessageLog::new(),
            reply_topic: None,
            publisher: crate::messaging::consumer::Publisher::None,
        };
        #[cfg(feature = "tcp-mock")]
        let tcp_runtime =
            crate::tcp::TcpRuntime::load_and_spawn(&data_dir, crate::tcp::LOOPBACK).await;
        AppState {
            store,
            proxy: crate::engine::ProxyClient::new(),
            seq_counters: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
            request_log: crate::server::request_log::RequestLog::new(),
            auth_config: crate::auth::AuthConfig {
                enabled: false,
                keycloak_url: String::new(),
                realm: String::new(),
                client_id: String::new(),
                super_admins: vec![],
                issuer: String::new(),
                show_reset_button: false,
            },
            keycloak: None,
            script_engine: crate::engine::script::ScriptEngine::new(),
            ping_cache: crate::server::ping::PingCache::new(),
            observation: crate::server::observation::ObservationState::new(),
            #[cfg(feature = "messaging-kafka")]
            messaging,
            #[cfg(feature = "tcp-mock")]
            tcp_runtime,
        }
    }

    fn empty_captured(method: &str, remaining_path: &str) -> RuleTestCapturedRequest {
        RuleTestCapturedRequest {
            method: method.into(),
            remaining_path: remaining_path.into(),
            path_params: HashMap::new(),
            query_params: HashMap::new(),
            headers: HashMap::new(),
            body: String::new(),
            body_truncated: false,
            content_type: None,
        }
    }

    #[tokio::test]
    async fn test_rule_nominal_match_via_path_param() {
        let mut captured = empty_captured("GET", "/orders/42");
        captured.path_params.clear();
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: Some("/orders/{id}".into()),
            conditions: ConditionGroup {
                all_of: vec![crate::models::Condition {
                    source: crate::models::ConditionSource::PathParam("id".into()),
                    operator: crate::models::Operator::Eq("42".into()),
                }],
                any_of: vec![],
            },
            request: captured,
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(result.method_matches);
        assert!(result.sub_path_matches);
        assert!(result.overall_matched);
        assert_eq!(result.path_params.get("id").unwrap(), "42");
        assert!(result.all_of[0].matched);
        assert!(result.script_errors.is_empty());
    }

    #[tokio::test]
    async fn test_rule_reports_cross_source_hint_for_misplaced_query_param() {
        let mut captured = empty_captured("GET", "/orders/42");
        captured.path_params.insert("id".into(), "42".into());
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup {
                all_of: vec![crate::models::Condition {
                    source: crate::models::ConditionSource::QueryParam("id".into()),
                    operator: crate::models::Operator::Eq("42".into()),
                }],
                any_of: vec![],
            },
            request: captured,
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(!result.overall_matched);
        assert!(!result.all_of[0].matched);
        let hint = result.all_of[0].hint.as_deref().unwrap();
        assert!(hint.contains("path parameter"));
    }

    #[tokio::test]
    async fn test_rule_method_mismatch_reported() {
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            request: empty_captured("POST", "/anything"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(!result.method_matches);
        assert!(!result.overall_matched);
    }

    #[tokio::test]
    async fn test_rule_propagates_body_truncated_flag() {
        let mut captured = empty_captured("GET", "/x");
        captured.body_truncated = true;
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            request: captured,
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(result.body_truncated);
    }

    // --- test_rule(): script errors ---
    //
    // In production a failing script only reaches the server log; the tester is where a user sees it (`script_errors`).

    #[tokio::test]
    async fn test_rule_reports_script_execution_error_when_rule_matches() {
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            // A function that does not exist: compiling the script (/api/script/validate) accepts it, only running it fails.
            script: Some("totally_undefined_fn(1, 2)".into()),
            request: empty_captured("GET", "/x"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(result.overall_matched);
        assert_eq!(result.script_errors.len(), 1);
        assert_eq!(result.script_errors[0].slot, "script");
        assert!(
            result.script_errors[0]
                .message
                .contains("totally_undefined_fn")
        );
    }

    #[tokio::test]
    async fn test_rule_reports_errors_for_all_three_script_slots_independently() {
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            pre_script: Some("broken_pre()".into()),
            script: Some("\"ok\"".into()),
            post_script: Some("broken_post()".into()),
            request: empty_captured("GET", "/x"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(result.overall_matched);
        let slots: Vec<&str> = result.script_errors.iter().map(|e| e.slot).collect();
        assert_eq!(slots, vec!["pre_script", "post_script"]);
    }

    #[tokio::test]
    async fn test_rule_no_script_execution_attempted_when_rule_does_not_match() {
        let payload = RuleTestRequest {
            method: "POST".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            script: Some("totally_undefined_fn(1, 2)".into()),
            request: empty_captured("GET", "/x"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(
            !result.overall_matched,
            "GET is not POST: the rule must not match"
        );
        assert!(
            result.script_errors.is_empty(),
            "a script never runs for a rule that does not match, as in production"
        );
    }

    #[tokio::test]
    async fn test_rule_no_script_execution_attempted_for_proxy_action() {
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            action: crate::models::RuleAction::Proxy,
            script: Some("totally_undefined_fn(1, 2)".into()),
            request: empty_captured("GET", "/x"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(result.overall_matched);
        assert!(
            result.script_errors.is_empty(),
            "un proxy ne rend jamais de template donc n'execute jamais de script, meme en production"
        );
    }

    #[tokio::test]
    async fn test_rule_valid_scripts_report_no_error() {
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            script: Some(r#"#{ greeting: "hi" }"#.into()),
            request: empty_captured("GET", "/x"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(result.overall_matched);
        assert!(result.script_errors.is_empty());
    }

    // --- test_rule(): what a script returned ---
    //
    // A script can run without error and still return something unexpected (a misspelled key, a nested object that a
    // template path cannot reach); `script_results` shows what it produced.

    #[tokio::test]
    async fn test_rule_reports_successful_script_result_fields() {
        // The reported case: an object picked from a list of cities and returned as is; the tester shows each field.
        let mut captured = empty_captured("GET", "/quote/44306184100047");
        captured
            .path_params
            .insert("siret".into(), "44306184100047".into());
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: Some("/quote/{siret}".into()),
            conditions: ConditionGroup::default(),
            script: Some(
                r#"
                    let villes = [
                        #{ name: "Paris", cp: "75000", insee: "75056" },
                        #{ name: "Lyon", cp: "69000", insee: "69123" }
                    ];
                    seeded_pick(request.path.siret, villes)
                "#
                .into(),
            ),
            request: captured,
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(result.overall_matched);
        assert!(result.script_errors.is_empty());
        assert_eq!(result.script_results.len(), 1);
        let script_result = &result.script_results[0];
        assert_eq!(script_result.slot, "script");
        assert!(
            !script_result.fields.is_empty(),
            "the fields of the picked city must be shown"
        );
        assert!(script_result.fields.contains_key("name"));
        assert!(script_result.fields.contains_key("cp"));
        assert!(script_result.fields.contains_key("insee"));
    }

    #[tokio::test]
    async fn test_rule_script_result_exposes_nested_object_as_valid_json_field() {
        // The picked object nested under a key, as soon as a script returns several things: the tester shows that field as
        // JSON, which makes it plain that a nested path such as {{script.city.name}} does not exist (only "city" does).
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            script: Some(
                r#"
                    let ville = #{ name: "Lyon", cp: "69000" };
                    #{ ville: ville, id: "fixed-id" }
                "#
                .into(),
            ),
            request: empty_captured("GET", "/x"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert_eq!(result.script_results.len(), 1);
        let fields = &result.script_results[0].fields;
        assert_eq!(fields.get("id").unwrap(), "fixed-id");
        let ville_json: serde_json::Value =
            serde_json::from_str(fields.get("ville").unwrap()).unwrap();
        assert_eq!(ville_json["name"], "Lyon");
    }

    #[tokio::test]
    async fn test_rule_script_results_empty_when_rule_does_not_match() {
        let payload = RuleTestRequest {
            method: "POST".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            script: Some(r#"#{ greeting: "hi" }"#.into()),
            request: empty_captured("GET", "/x"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert!(!result.overall_matched);
        assert!(result.script_results.is_empty());
    }

    #[tokio::test]
    async fn test_rule_script_results_and_errors_are_mutually_exclusive_per_slot() {
        let payload = RuleTestRequest {
            method: "GET".into(),
            sub_path: None,
            conditions: ConditionGroup::default(),
            pre_script: Some("broken_pre()".into()),
            script: Some(r#"#{ ok: "yes" }"#.into()),
            request: empty_captured("GET", "/x"),
            ..Default::default()
        };
        let Json(result) = test_rule(
            State(test_state().await),
            Extension(anon_user()),
            Json(payload),
        )
        .await;
        assert_eq!(result.script_errors.len(), 1);
        assert_eq!(result.script_errors[0].slot, "pre_script");
        assert_eq!(result.script_results.len(), 1);
        assert_eq!(result.script_results[0].slot, "script");
        assert_eq!(result.script_results[0].fields.get("ok").unwrap(), "yes");
    }

    // --- check_rule_conflicts (POST /api/rule-conflicts) tests ---

    fn header_eq_cond(key: &str, val: &str) -> crate::models::Condition {
        crate::models::Condition {
            source: crate::models::ConditionSource::Header(key.into()),
            operator: crate::models::Operator::Eq(val.into()),
        }
    }

    #[tokio::test]
    async fn check_rule_conflicts_reports_identical_conditions() {
        let payload = RuleConflictsRequest {
            draft: RuleConflictDraftRequest {
                method: "GET".into(),
                sub_path: None,
                conditions: ConditionGroup {
                    all_of: vec![header_eq_cond("x-env", "prod")],
                    any_of: vec![],
                },
            },
            other_rules: vec![OtherRuleConflictRequest {
                name: "existing-rule".into(),
                method: "GET".into(),
                sub_path: None,
                conditions: ConditionGroup {
                    all_of: vec![header_eq_cond("x-env", "prod")],
                    any_of: vec![],
                },
            }],
            draft_position: 1,
        };
        let Json(result) = check_rule_conflicts(Extension(anon_user()), Json(payload)).await;
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].other_rule_name, "existing-rule");
        assert_eq!(result.conflicts[0].winner, ConflictWinner::Other);
    }

    #[tokio::test]
    async fn check_rule_conflicts_no_conflict_for_disjoint_conditions() {
        let payload = RuleConflictsRequest {
            draft: RuleConflictDraftRequest {
                method: "GET".into(),
                sub_path: None,
                conditions: ConditionGroup {
                    all_of: vec![header_eq_cond("x-env", "prod")],
                    any_of: vec![],
                },
            },
            other_rules: vec![OtherRuleConflictRequest {
                name: "staging-rule".into(),
                method: "GET".into(),
                sub_path: None,
                conditions: ConditionGroup {
                    all_of: vec![header_eq_cond("x-env", "staging")],
                    any_of: vec![],
                },
            }],
            draft_position: 1,
        };
        let Json(result) = check_rule_conflicts(Extension(anon_user()), Json(payload)).await;
        assert!(result.conflicts.is_empty());
    }

    #[tokio::test]
    async fn check_rule_conflicts_reports_subset_conditions_with_draft_winner() {
        // The draft moves before the other rule (edited in place at index 0), so the draft is the one that would win.
        let payload = RuleConflictsRequest {
            draft: RuleConflictDraftRequest {
                method: "POST".into(),
                sub_path: None,
                conditions: ConditionGroup::default(),
            },
            other_rules: vec![OtherRuleConflictRequest {
                name: "more-specific-rule".into(),
                method: "POST".into(),
                sub_path: None,
                conditions: ConditionGroup {
                    all_of: vec![header_eq_cond("x-env", "prod")],
                    any_of: vec![],
                },
            }],
            draft_position: 0,
        };
        let Json(result) = check_rule_conflicts(Extension(anon_user()), Json(payload)).await;
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(result.conflicts[0].winner, ConflictWinner::Draft);
    }

    // --- Traffic observation (service-level proxy), authentication off.

    #[tokio::test]
    async fn observe_service_enables_toggle_for_proxy_service() {
        let svc = Service {
            is_mocked: false,
            ..svc_named("proxy-svc", None)
        };
        let base = spawn_test_app(MockConfig {
            services: vec![svc],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{base}/services/proxy-svc/observe"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let status = client
            .get(format!("{base}/observation/status"))
            .send()
            .await
            .unwrap()
            .json::<Vec<ObservationStatusEntry>>()
            .await
            .unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].service_name, "proxy-svc");
        assert_eq!(status[0].group_name, None);
    }

    #[tokio::test]
    async fn observe_service_rejects_mocked_service() {
        let svc = svc_named("mocked-svc", None); // svc_named builds a mocked service
        let base = spawn_test_app(MockConfig {
            services: vec![svc],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{base}/services/mocked-svc/observe"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unobserve_service_disables_toggle() {
        let svc = Service {
            is_mocked: false,
            ..svc_named("proxy-svc", None)
        };
        let base = spawn_test_app(MockConfig {
            services: vec![svc],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        client
            .post(format!("{base}/services/proxy-svc/observe"))
            .send()
            .await
            .unwrap();
        let resp = client
            .delete(format!("{base}/services/proxy-svc/observe"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let status = client
            .get(format!("{base}/observation/status"))
            .send()
            .await
            .unwrap()
            .json::<Vec<ObservationStatusEntry>>()
            .await
            .unwrap();
        assert!(status.is_empty());
    }

    #[tokio::test]
    async fn observe_service_grouped_scopes_by_group() {
        let svc = Service {
            is_mocked: false,
            ..svc_named("shared-name", Some("team-a"))
        };
        let base = spawn_test_app(MockConfig {
            services: vec![svc],
            groups: groups_named(&["team-a"]),
        })
        .await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{base}/groups/team-a/services/shared-name/observe"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let status = client
            .get(format!("{base}/observation/status"))
            .send()
            .await
            .unwrap()
            .json::<Vec<ObservationStatusEntry>>()
            .await
            .unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].group_name.as_deref(), Some("team-a"));
    }

    #[tokio::test]
    async fn every_shipped_example_imports_cleanly() {
        // The files of examples/ are what a newcomer imports first: each one must pass the same checks as an
        // import through the UI (PUT /api/config).
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let config: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let base = spawn_test_app(MockConfig::empty()).await;
            let resp = reqwest::Client::new()
                .put(format!("{base}/config"))
                .json(&config)
                .send()
                .await
                .unwrap();
            let status = resp.status();
            assert_eq!(
                status,
                StatusCode::OK,
                "{}: {}",
                path.display(),
                resp.text().await.unwrap()
            );
            checked += 1;
        }
        assert!(checked > 0);
    }

    #[tokio::test]
    async fn put_config_refuses_a_service_in_an_undefined_group() {
        let base = spawn_test_app(MockConfig::empty()).await;
        let config = MockConfig {
            services: vec![svc_named("orphan", Some("ghost"))],
            groups: vec![],
        };
        let resp = reqwest::Client::new()
            .put(format!("{base}/config"))
            .json(&config)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(body["error"].as_str().unwrap().contains("ghost"));
    }

    #[tokio::test]
    async fn observation_works_for_a_service_of_a_real_group() {
        // Groups are addressed by name in the API and by code in service URLs: the observation was enabled
        // under the name and looked up under the code, so it never started for a grouped service.
        let target_app = axum::Router::new().fallback(|| async { "from-backend" });
        let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_port = target_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(target_listener, target_app).await.unwrap();
        });
        let svc = Service {
            is_mocked: false,
            real_target_url: format!("http://127.0.0.1:{target_port}"),
            ..svc_named("billing", Some("team-a"))
        };
        let group = Group {
            name: "team-a".into(),
            code: "tma01".into(),
            admins: vec![],
            members: vec![],
        };
        let base = spawn_test_app(MockConfig {
            services: vec![svc],
            groups: vec![group],
        })
        .await;
        let client = reqwest::Client::new();
        let observe = client
            .post(format!("{base}/groups/team-a/services/billing/observe"))
            .send()
            .await
            .unwrap();
        assert_eq!(observe.status(), StatusCode::NO_CONTENT);

        let proxy_base = base.trim_end_matches("/api");
        for _ in 0..crate::server::suggestion::min_samples() {
            let resp = client
                .get(format!("{proxy_base}/tma01/billing/invoices"))
                .send()
                .await
                .unwrap();
            assert_eq!(resp.text().await.unwrap(), "from-backend");
        }

        let suggestions: Vec<serde_json::Value> = client
            .get(format!("{base}/groups/team-a/services/billing/suggestions"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(suggestions.len(), 1, "{suggestions:?}");
        assert_eq!(suggestions[0]["outcome"], "Unconditional");
    }

    #[tokio::test]
    async fn suggestions_end_to_end_through_real_proxy_traffic() {
        // A real target answering 200 for id=1 and 404 for id=2 on the same method and path: legitimate variance, explained
        // here by the `id` query parameter.
        async fn target(
            axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
        ) -> axum::response::Response {
            match params.get("id").map(String::as_str) {
                Some("1") => (StatusCode::OK, "found").into_response(),
                _ => (StatusCode::NOT_FOUND, "missing").into_response(),
            }
        }
        let target_app = axum::Router::new().route("/{*rest}", axum::routing::any(target));
        let target_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_port = target_listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(target_listener, target_app).await.unwrap();
        });

        let svc = Service {
            is_mocked: false,
            real_target_url: format!("http://127.0.0.1:{target_port}"),
            ..svc_named("proxy-svc", None)
        };
        let base = spawn_test_app(MockConfig {
            services: vec![svc],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        client
            .post(format!("{base}/services/proxy-svc/observe"))
            .send()
            .await
            .unwrap();

        let proxy_base = base.trim_end_matches("/api");
        for _ in 0..2 {
            client
                .get(format!("{proxy_base}/proxy-svc/orders?id=1"))
                .send()
                .await
                .unwrap();
            client
                .get(format!("{proxy_base}/proxy-svc/orders?id=2"))
                .send()
                .await
                .unwrap();
        }

        let suggestions: Vec<serde_json::Value> = client
            .get(format!("{base}/services/proxy-svc/suggestions"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0]["outcome"], "Conditional");
        let rules = suggestions[0]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        let statuses: Vec<u64> = rules
            .iter()
            .map(|r| r["response"]["status"].as_u64().unwrap())
            .collect();
        assert!(statuses.contains(&200));
        assert!(statuses.contains(&404));
    }

    #[tokio::test]
    async fn observe_service_unknown_name_returns_not_found() {
        let base = spawn_test_app(MockConfig {
            services: vec![],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        let resp = client
            .post(format!("{base}/services/does-not-exist/observe"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn deleting_a_service_clears_its_observation_toggle() {
        let svc = Service {
            is_mocked: false,
            ..svc_named("proxy-svc", None)
        };
        let base = spawn_test_app(MockConfig {
            services: vec![svc.clone()],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        client
            .post(format!("{base}/services/proxy-svc/observe"))
            .send()
            .await
            .unwrap();
        client
            .delete(format!("{base}/services/proxy-svc"))
            .send()
            .await
            .unwrap();

        // A service created again under the same name does not inherit the deleted one's observation.
        client
            .post(format!("{base}/services"))
            .json(&svc)
            .send()
            .await
            .unwrap();
        let status = client
            .get(format!("{base}/observation/status"))
            .send()
            .await
            .unwrap()
            .json::<Vec<ObservationStatusEntry>>()
            .await
            .unwrap();
        assert!(status.is_empty());
    }

    #[tokio::test]
    async fn resetting_config_clears_all_observation_toggles() {
        let svc = Service {
            is_mocked: false,
            ..svc_named("proxy-svc", None)
        };
        let base = spawn_test_app(MockConfig {
            services: vec![svc.clone()],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        client
            .post(format!("{base}/services/proxy-svc/observe"))
            .send()
            .await
            .unwrap();
        client
            .delete(format!("{base}/config/reset"))
            .send()
            .await
            .unwrap();

        client
            .post(format!("{base}/services"))
            .json(&svc)
            .send()
            .await
            .unwrap();
        let status = client
            .get(format!("{base}/observation/status"))
            .send()
            .await
            .unwrap()
            .json::<Vec<ObservationStatusEntry>>()
            .await
            .unwrap();
        assert!(status.is_empty());
    }

    // --- Raw TCP mock CRUD ("tcp-mock" feature), authentication off; who may change TCP mocks is covered by the
    // authorization tests.

    #[cfg(feature = "tcp-mock")]
    #[tokio::test]
    async fn tcp_services_crud_roundtrip() {
        use crate::tcp::config::{TcpMatcher, TcpRule, TcpService};

        let base = spawn_test_app(MockConfig {
            services: vec![],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        let created = client
            .post(format!("{base}/tcp/services"))
            .json(&TcpService {
                name: "heartbeat".into(),
                listen_port: 0,
                rules: vec![TcpRule {
                    name: "ping".into(),
                    matcher: TcpMatcher::Any,
                    response_hex: "706f6e67".into(),
                }],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);

        let list: Vec<TcpService> = client
            .get(format!("{base}/tcp/services"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "heartbeat");

        let updated = client
            .put(format!("{base}/tcp/services/heartbeat"))
            .json(&TcpService {
                name: "heartbeat".into(),
                listen_port: 0,
                rules: vec![TcpRule {
                    name: "ping2".into(),
                    matcher: TcpMatcher::Any,
                    response_hex: "706f6e6732".into(),
                }],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(updated.status(), StatusCode::OK);
        let updated_service: TcpService = updated.json().await.unwrap();
        assert_eq!(updated_service.rules[0].name, "ping2");

        let deleted = client
            .delete(format!("{base}/tcp/services/heartbeat"))
            .send()
            .await
            .unwrap();
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

        let list_after: Vec<TcpService> = client
            .get(format!("{base}/tcp/services"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(list_after.is_empty());
    }

    #[cfg(feature = "tcp-mock")]
    #[tokio::test]
    async fn tcp_service_create_rejects_duplicate_port() {
        use crate::tcp::config::{TcpMatcher, TcpRule, TcpService};

        let base = spawn_test_app(MockConfig {
            services: vec![],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        let service = |name: &str| TcpService {
            name: name.into(),
            listen_port: 19999,
            rules: vec![TcpRule {
                name: "r".into(),
                matcher: TcpMatcher::Any,
                response_hex: String::new(),
            }],
        };

        let first = client
            .post(format!("{base}/tcp/services"))
            .json(&service("svc-a"))
            .send()
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::CREATED);

        let second = client
            .post(format!("{base}/tcp/services"))
            .json(&service("svc-b"))
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::BAD_REQUEST);
    }

    #[cfg(feature = "tcp-mock")]
    #[tokio::test]
    async fn tcp_service_update_missing_returns_404() {
        use crate::tcp::config::TcpService;

        let base = spawn_test_app(MockConfig {
            services: vec![],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        let resp = client
            .put(format!("{base}/tcp/services/does-not-exist"))
            .json(&TcpService {
                name: "does-not-exist".into(),
                listen_port: 0,
                rules: vec![],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(feature = "tcp-mock")]
    #[tokio::test]
    async fn tcp_service_delete_missing_returns_404() {
        let base = spawn_test_app(MockConfig {
            services: vec![],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        let resp = client
            .delete(format!("{base}/tcp/services/does-not-exist"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[cfg(feature = "tcp-mock")]
    #[tokio::test]
    async fn tcp_service_created_via_api_is_immediately_reachable_over_raw_tcp() {
        // TcpRuntime::replace() really restarts the listeners: create a service through the API, then connect to its port
        // and read the mocked answer, without restarting anything in between.
        use crate::tcp::config::{TcpMatcher, TcpRule, TcpService};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let base = spawn_test_app(MockConfig {
            services: vec![],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        // The service needs a port that can be connected to: reserve a free one.
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let created = client
            .post(format!("{base}/tcp/services"))
            .json(&TcpService {
                name: "live-check".into(),
                listen_port: port,
                rules: vec![TcpRule {
                    name: "always".into(),
                    matcher: TcpMatcher::Any,
                    response_hex: crate::tcp::hex::encode(b"live"),
                }],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);

        let mut tcp_client = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        tcp_client.write_all(b"ping").await.unwrap();
        let mut resp = [0u8; 4];
        tcp_client.read_exact(&mut resp).await.unwrap();
        assert_eq!(&resp, b"live");
    }
}
