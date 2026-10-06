use crate::engine::matcher::match_path;
use crate::engine::script::{ScriptContext, ScriptEngine, ScriptResult};
use crate::engine::{MatchEngine, RequestData, TemplateContext, apply_chaos_and_render};
use crate::models::{RuleAction, Service, WsdlMode};
use crate::server::AppState;
use crate::server::request_log::CapturedRequest;
use crate::server::validation::is_internal_route;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;

/// Runs one of a rule's three script slots (pre_script, script, post_script). The slots are independent: same
/// context, no chaining (see `Rule`). `None` when the slot is empty. A runtime error is logged and gives an empty
/// result: a broken script never blocks the response.
fn run_rule_script(
    engine: &ScriptEngine,
    rule_name: &str,
    slot: &str,
    script: &Option<String>,
    ctx: &ScriptContext,
) -> Option<ScriptResult> {
    let script = script.as_ref()?;
    match engine.execute(script, ctx) {
        Ok(result) => Some(result),
        Err(e) => {
            tracing::warn!(rule = rule_name, slot, error = %e, "script execution failed");
            Some(ScriptResult::default())
        }
    }
}

// Runs on every incoming request: internal routes (UI, API) pass through; otherwise the service is found from the
// path, then the request is either proxied (pure proxy service) or matched against the rules in order, first match
// wins, and mocked or proxied as that rule says.
pub async fn intercept_layer(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    if is_internal_route(&path) {
        tracing::trace!(path = %path, "internal route protected, skipping intercept");
        return next.run(req).await;
    }
    if crate::server::validation::has_dot_segment(&path) {
        return (
            StatusCode::BAD_REQUEST,
            "Paths with '.' or '..' segments are refused.",
        )
            .into_response();
    }

    let config = state.store.snapshot().await;

    let matched = config.services.iter().find_map(|s| {
        let group_code = s.group_name.as_ref().and_then(|gn| {
            config
                .groups
                .iter()
                .find(|g| &g.name == gn)
                .map(|g| g.code.clone())
                .filter(|c| !c.trim().is_empty())
        });
        let effective = build_effective_pattern(group_code.as_deref(), &s.name, &s.listen_path);
        match_path(&effective, &path)
            .map(|(params, remaining)| (s.clone(), params, remaining, group_code))
    });

    match matched {
        Some((service, path_params, remaining, group_code)) => {
            let mut response = handle_service(
                &state,
                &service,
                &path,
                path_params,
                remaining,
                group_code,
                req,
            )
            .await;
            response
                .extensions_mut()
                .insert(crate::server::browser_guard::ServiceResponse);
            response
        }
        None => next.run(req).await,
    }
}

fn build_effective_pattern(group_code: Option<&str>, name: &str, listen_path: &str) -> String {
    let lp = listen_path.trim().trim_start_matches('/');
    let base = match group_code {
        Some(code) => format!("/{code}/{name}"),
        None => format!("/{name}"),
    };
    if lp.is_empty() || lp == "*" {
        format!("{base}/*")
    } else if lp.ends_with('*') || lp.contains('{') {
        format!("{base}/{lp}")
    } else {
        format!("{base}/{lp}/*")
    }
}

// More arguments than clippy's default allows; they belong in a request context struct.
#[allow(clippy::too_many_arguments)]
async fn do_proxy(
    state: &AppState,
    service: &Service,
    path: &str,
    method_str: &str,
    context: &str,
    group_code: Option<&str>,
    req: Request<Body>,
    captured: Option<CapturedRequest>,
    observe: bool,
) -> Response {
    // A purely mocked service has no target: never try to proxy to an empty URL. Saving refuses a pure proxy without a
    // target, but a proxy rule can still match on a service that became purely mocked later (saving only warns), so
    // answer with a clear error here rather than a confusing one (invalid URL, DNS failure).
    if service.real_target_url.trim().is_empty() {
        tracing::warn!(
            service_key = %service.name, method = %method_str, path = %path,
            context = %context, mode = "proxy-blocked",
            "proxy attempted on a purely-mocked service (no target configured)"
        );
        let message = "This service is purely mocked (no target configured): the request cannot be forwarded.";
        state.request_log.log_proxy(
            service,
            method_str,
            path,
            "(no target configured)",
            StatusCode::BAD_GATEWAY.as_u16(),
            captured,
        );
        return (StatusCode::BAD_GATEWAY, message).into_response();
    }

    let prefix = match group_code {
        Some(code) => format!("/{}/{}", code, service.name),
        None => format!("/{}", service.name),
    };
    let proxy_path = path.strip_prefix(&prefix).unwrap_or(path);
    let target = format!(
        "{}/{}",
        service.real_target_url.trim_end_matches('/'),
        proxy_path.trim_start_matches('/')
    );
    tracing::info!(
        service_key = %service.name,
        method = %method_str,
        path = %path,
        mode = "proxy",
        context = %context,
        target = %crate::server::redaction::redact_url_credentials(&target),
        "proxy forwarding"
    );
    if observe {
        return do_proxy_observed(
            state, service, path, method_str, proxy_path, &target, req, captured,
        )
        .await;
    }

    match state
        .proxy
        .forward(&service.real_target_url, proxy_path, req)
        .await
    {
        Ok(resp) => {
            let status = resp.status().as_u16();
            state
                .request_log
                .log_proxy(service, method_str, path, &target, status, captured);
            resp
        }
        Err(status) => {
            state.request_log.log_proxy(
                service,
                method_str,
                path,
                &target,
                status.as_u16(),
                captured,
            );
            status.into_response()
        }
    }
}

/// The same relay as `do_proxy`'s default branch, plus a capture for traffic observation (`server::observation`).
/// Only called for a service a user chose to observe. `ProxyClient::forward_with_capture` falls back to plain
/// streaming whenever a capture would not be safe (size unknown or too large): the relayed traffic is unchanged,
/// only that exchange is not observed.
// More arguments than clippy's default allows, as for do_proxy; they belong in a request context struct.
#[allow(clippy::too_many_arguments)]
async fn do_proxy_observed(
    state: &AppState,
    service: &Service,
    path: &str,
    method_str: &str,
    proxy_path: &str,
    target: &str,
    req: Request<Body>,
    captured: Option<CapturedRequest>,
) -> Response {
    let max_buffer = crate::server::observation::max_buffer_size();
    match state
        .proxy
        .forward_with_capture(&service.real_target_url, proxy_path, req, max_buffer)
        .await
    {
        Ok((resp, capture)) => {
            let status = resp.status().as_u16();
            state
                .request_log
                .log_proxy(service, method_str, path, target, status, captured);
            if let Some(raw) = capture {
                // Keyed by group name, like the API that enables observation and lists suggestions (the
                // group code only exists in service URLs).
                let key = crate::server::observation::ObservationKey {
                    group_name: service.group_name.clone(),
                    service_name: service.name.clone(),
                    method: method_str.to_string(),
                    sub_path: proxy_path.to_string(),
                };
                let exchange = crate::server::observation::ObservedExchange::new(
                    raw.request_query_params,
                    raw.request_headers,
                    &raw.request_body,
                    raw.request_content_type,
                    raw.response_status,
                    raw.response_headers,
                    &raw.response_body,
                    raw.response_content_type,
                );
                state.observation.store.record(key, exchange);
            }
            resp
        }
        Err(status) => {
            state.request_log.log_proxy(
                service,
                method_str,
                path,
                target,
                status.as_u16(),
                captured,
            );
            status.into_response()
        }
    }
}

async fn handle_service(
    state: &AppState,
    service: &Service,
    path: &str,
    path_params: HashMap<String, String>,
    remaining: String,
    group_code: Option<String>,
    req: Request<Body>,
) -> Response {
    let method_str = req.method().to_string();
    let gc = group_code.as_deref();

    if !service.is_mocked {
        // Service-level proxy: the request is streamed, nothing is buffered, so the request log keeps no details for the
        // rule tester on this path. The one opt-in exception is observation: when a user turned it on for this service,
        // `do_proxy` captures bounded copies to suggest mock rules (`server::observation`).
        let observe = state
            .observation
            .toggle
            .is_enabled(service.group_name.as_deref(), &service.name);
        return do_proxy(
            state,
            service,
            path,
            &method_str,
            "service-level",
            gc,
            req,
            None,
            observe,
        )
        .await;
    }

    if is_wsdl_request(req.uri().query()) {
        match service.wsdl_mode {
            WsdlMode::Mock => {
                tracing::info!(
                    service_key = %service.name, method = %method_str, path = %path,
                    "WSDL request, mode=mock, applying rules"
                );
            }
            WsdlMode::Auto | WsdlMode::Proxy => {
                tracing::info!(
                    service_key = %service.name, method = %method_str, path = %path,
                    mode = "proxy", context = "wsdl-bypass",
                    "WSDL request, bypassing mock rules"
                );
                return do_proxy(
                    state,
                    service,
                    path,
                    &method_str,
                    "wsdl-bypass",
                    gc,
                    req,
                    None,
                    false,
                )
                .await;
            }
        }
    }

    let uri = req.uri().clone();
    let query_params = extract_query_params(uri.query());
    let headers = extract_headers(req.headers());
    let content_type = headers.get("content-type").cloned();

    let body_bytes = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    let request_data = RequestData {
        query_params,
        headers,
        body: body_bytes.to_vec(),
        content_type,
        path_params: path_params.clone(),
        method: method_str.clone(),
        remaining_path: remaining,
    };

    // The request is already fully buffered for matching (`request_data`), so keeping its details for the rule tester
    // costs one copy, not a new capture (see CapturedRequest in request_log.rs).
    let captured = Some(CapturedRequest::from_request_data(&request_data));

    let matched = MatchEngine::first_match(&service.rules, &request_data);

    let Some((rule, sub_params)) = matched else {
        tracing::warn!(
            service_key = %service.name, method = %method_str, path = %path,
            mode = "no-rule",
            "no matching rule, returning 404"
        );
        state
            .request_log
            .log_no_rule(service, &method_str, path, captured);
        // Tell a purely mocked service apart from a service with a target that only lacks a rule, to point the diagnosis
        // in the right direction.
        let message = if service.real_target_url.trim().is_empty() {
            "No rule matches this request: this service is purely mocked (no target configured)."
        } else {
            "No rule matches this request for this service."
        };
        return (StatusCode::NOT_FOUND, message).into_response();
    };

    if rule.action == RuleAction::Proxy {
        tracing::info!(
            service_key = %service.name, method = %method_str, path = %path,
            rule = %rule.name, mode = "proxy",
            "rule matched with action=proxy"
        );
        let proxy_req = rebuild_request_for_proxy(&method_str, &uri, &request_data, &body_bytes);
        return do_proxy(
            state,
            service,
            path,
            &method_str,
            &format!("rule:{}", rule.name),
            gc,
            proxy_req,
            captured,
            false,
        )
        .await;
    }

    let path_segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let seq = state.next_seq(&service.name);

    let mut merged_params = path_params;
    merged_params.extend(sub_params);

    // The three slots run independently (same context, no chaining, see `Rule`); a failing one is logged and never
    // blocks the response.
    let script_ctx = ScriptContext {
        body: String::from_utf8_lossy(&request_data.body).into_owned(),
        headers: request_data.headers.clone(),
        query_params: request_data.query_params.clone(),
        path_params: merged_params.clone(),
    };
    let pre_script_result = run_rule_script(
        &state.script_engine,
        &rule.name,
        "pre_script",
        &rule.pre_script,
        &script_ctx,
    );
    let script_result = run_rule_script(
        &state.script_engine,
        &rule.name,
        "script",
        &rule.script,
        &script_ctx,
    );
    let post_script_result = run_rule_script(
        &state.script_engine,
        &rule.name,
        "post_script",
        &rule.post_script,
        &script_ctx,
    );

    let ctx = TemplateContext {
        path_params: &merged_params,
        query_params: &request_data.query_params,
        headers: &request_data.headers,
        request_body: &request_data.body,
        seq_counter: seq,
        script_result: script_result.as_ref(),
        pre_script_result: pre_script_result.as_ref(),
        post_script_result: post_script_result.as_ref(),
    };

    match apply_chaos_and_render(&rule.response, &path_segments, &ctx).await {
        Ok((status, resp_headers, body)) => {
            tracing::info!(
                service_key = %service.name,
                method = %method_str,
                path = %path,
                mode = "mock",
                rule = %rule.name,
                status = %status.as_u16(),
                "request handled"
            );
            state.request_log.log_mock(
                service,
                &method_str,
                path,
                &rule.name,
                status.as_u16(),
                captured,
            );
            let mut response = axum::http::Response::builder().status(status);
            for (name, value) in &resp_headers {
                if let Ok(hv) = HeaderValue::from_str(value) {
                    response = response.header(name.as_str(), hv);
                }
            }
            response
                .body(Body::from(body))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(status) => {
            state.request_log.log_mock(
                service,
                &method_str,
                path,
                &rule.name,
                status.as_u16(),
                captured,
            );
            (status, "chaos error injected").into_response()
        }
    }
}

fn rebuild_request_for_proxy(
    method: &str,
    uri: &axum::http::Uri,
    data: &RequestData,
    body_bytes: &[u8],
) -> Request<Body> {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri.clone());
    for (k, v) in &data.headers {
        if let Ok(hv) = HeaderValue::from_str(v) {
            builder = builder.header(k.as_str(), hv);
        }
    }
    builder
        .body(Body::from(body_bytes.to_vec()))
        .unwrap_or_else(|_| Request::new(Body::empty()))
}

fn is_wsdl_request(query: Option<&str>) -> bool {
    query
        .map(|q| {
            q.split('&').any(|part| {
                let key = part.split('=').next().unwrap_or("");
                key.eq_ignore_ascii_case("wsdl")
            })
        })
        .unwrap_or(false)
}

fn extract_query_params(query: Option<&str>) -> HashMap<String, String> {
    query
        .map(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

fn extract_headers(headers: &axum::http::HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|v| (name.as_str().to_string(), v.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::matcher::match_path;
    use crate::models::{
        BodyFragment, Condition, ConditionGroup, ConditionSource, HeaderEntry, MockConfig,
        MockResponse, Operator, Rule,
    };
    use crate::store::MockStore;

    #[test]
    fn effective_pattern() {
        assert_eq!(
            build_effective_pattern(None, "insee", "/v4/sirene/{siret}"),
            "/insee/v4/sirene/{siret}"
        );
        assert_eq!(build_effective_pattern(None, "svc-a", "/*"), "/svc-a/*");
        assert_eq!(
            build_effective_pattern(None, "svc", "/api/v4"),
            "/svc/api/v4/*",
            "a listen_path without wildcard or parameter gets an implicit /*"
        );
    }

    #[test]
    fn namespace_match() {
        let pattern = build_effective_pattern(None, "insee", "/v4/sirene/{siret}");
        let r = match_path(&pattern, "/insee/v4/sirene/44306184100047");
        assert!(r.is_some());
        let (params, remaining) = r.unwrap();
        assert_eq!(params.get("siret").unwrap(), "44306184100047");
        assert_eq!(remaining, "");
    }

    #[test]
    fn namespace_no_collision() {
        let p1 = build_effective_pattern(None, "svc-a", "/users/*");
        let p2 = build_effective_pattern(None, "svc-b", "/users/*");
        assert!(match_path(&p1, "/svc-a/users/42").is_some());
        assert!(match_path(&p1, "/svc-b/users/42").is_none());
        assert!(match_path(&p2, "/svc-b/users/42").is_some());
    }

    #[test]
    fn namespace_wildcard_remaining() {
        let pattern = build_effective_pattern(None, "api", "/*");
        let r = match_path(&pattern, "/api/foo/bar");
        assert!(r.is_some());
        let (_, remaining) = r.unwrap();
        assert_eq!(remaining, "/foo/bar");
    }

    #[test]
    fn proxy_path_strips_service_prefix_only() {
        let path = "/insee/v4/sirene/44306184100047";
        let service_name = "insee";
        let prefix = format!("/{}", service_name);
        let proxy_path = path.strip_prefix(&prefix).unwrap_or(path);
        assert_eq!(proxy_path, "/v4/sirene/44306184100047");
    }

    #[test]
    fn proxy_path_wildcard_service() {
        let path = "/api/users/42";
        let service_name = "api";
        let prefix = format!("/{}", service_name);
        let proxy_path = path.strip_prefix(&prefix).unwrap_or(path);
        assert_eq!(proxy_path, "/users/42");
    }

    #[test]
    fn proxy_path_root_only() {
        let path = "/svc";
        let service_name = "svc";
        let prefix = format!("/{}", service_name);
        let proxy_path = path.strip_prefix(&prefix).unwrap_or(path);
        assert_eq!(proxy_path, "");
    }

    #[test]
    fn proxy_path_preserves_deep_business_path() {
        let path = "/myservice/api/v2/resources/123/details";
        let service_name = "myservice";
        let prefix = format!("/{}", service_name);
        let proxy_path = path.strip_prefix(&prefix).unwrap_or(path);
        assert_eq!(proxy_path, "/api/v2/resources/123/details");
    }

    #[test]
    fn non_wildcard_remaining_is_empty() {
        let pattern = build_effective_pattern(None, "insee", "/v4/sirene/{siret}");
        let r = match_path(&pattern, "/insee/v4/sirene/44306184100047");
        assert!(r.is_some());
        let (_, remaining) = r.unwrap();
        assert_eq!(remaining, "");
    }

    #[test]
    fn extract_query_params_works() {
        let params = extract_query_params(Some("a=1&b=hello"));
        assert_eq!(params.get("a").unwrap(), "1");
        assert_eq!(params.get("b").unwrap(), "hello");
    }

    #[test]
    fn extract_query_params_none() {
        assert!(extract_query_params(None).is_empty());
    }

    #[test]
    fn extract_headers_works() {
        let mut map = axum::http::HeaderMap::new();
        map.insert("x-test", HeaderValue::from_static("value"));
        let result = extract_headers(&map);
        assert_eq!(result.get("x-test").unwrap(), "value");
    }

    // --- Security tests ---

    #[test]
    fn empty_pattern_never_matches() {
        assert!(match_path("", "/").is_none());
        assert!(match_path("", "/foo").is_none());
        assert!(match_path("/", "/").is_none());
        assert!(match_path("//", "/any").is_none());
    }

    #[test]
    fn empty_listen_path_cannot_hijack_root() {
        let pattern = build_effective_pattern(None, "hijacker", "");
        assert_eq!(pattern, "/hijacker/*");
        assert!(
            match_path(&pattern, "/").is_none(),
            "service with empty listen_path must not capture /"
        );
        assert!(match_path(&pattern, "/hijacker/any").is_some());
    }

    #[test]
    fn slash_listen_path_cannot_hijack_root() {
        let pattern = build_effective_pattern(None, "hijacker", "/");
        assert_eq!(pattern, "/hijacker/*");
        assert!(
            match_path(&pattern, "/").is_none(),
            "service with listen_path '/' must not capture /"
        );
    }

    #[test]
    fn service_cannot_match_internal_api_routes() {
        assert!(is_internal_route("/api/services"));
        assert!(is_internal_route("/api/config"));
        assert!(is_internal_route("/api/logs"));
        assert!(is_internal_route("/"));
        assert!(is_internal_route("/index.html"));
        assert!(is_internal_route("/assets/main.js"));
    }

    #[test]
    fn user_service_routes_not_internal() {
        assert!(!is_internal_route("/insee/v4/sirene/123"));
        assert!(!is_internal_route("/my-svc/foo/bar"));
        assert!(!is_internal_route("/users/42"));
    }

    #[test]
    fn empty_listen_path_produces_catchall() {
        let p = build_effective_pattern(None, "svc", "");
        assert_eq!(p, "/svc/*");
        assert!(match_path(&p, "/").is_none(), "must not capture root");
        assert!(match_path(&p, "/svc/foo").is_some());
        assert!(match_path(&p, "/svc/foo/bar").is_some());
        assert!(match_path(&p, "/other").is_none());
    }

    #[test]
    fn service_matches_any_method() {
        let pattern = build_effective_pattern(None, "svc", "/v1/*");
        assert!(
            match_path(&pattern, "/svc/v1/test").is_some(),
            "service matching is path-only, no method check"
        );
    }

    #[test]
    fn group_code_prefixes_url() {
        let pattern = build_effective_pattern(Some("qtr01"), "insee", "/v4/*");
        assert_eq!(pattern, "/qtr01/insee/v4/*");
        assert!(match_path(&pattern, "/qtr01/insee/v4/sirene").is_some());
        assert!(match_path(&pattern, "/insee/v4/sirene").is_none());
    }

    #[test]
    fn group_code_catchall() {
        let pattern = build_effective_pattern(Some("abc01"), "svc", "");
        assert_eq!(pattern, "/abc01/svc/*");
        assert!(match_path(&pattern, "/abc01/svc/foo").is_some());
        assert!(match_path(&pattern, "/svc/foo").is_none());
    }

    // --- WSDL bypass tests ---

    #[test]
    fn wsdl_query_detected() {
        assert!(is_wsdl_request(Some("wsdl")));
        assert!(is_wsdl_request(Some("WSDL")));
        assert!(is_wsdl_request(Some("Wsdl")));
        assert!(is_wsdl_request(Some("wsdl=")));
        assert!(is_wsdl_request(Some("foo=bar&wsdl")));
        assert!(is_wsdl_request(Some("WSDL&other=1")));
    }

    #[test]
    fn non_wsdl_query_ignored() {
        assert!(!is_wsdl_request(None));
        assert!(!is_wsdl_request(Some("")));
        assert!(!is_wsdl_request(Some("foo=bar")));
        assert!(!is_wsdl_request(Some("wsdlx=true")));
    }

    fn empty_script_ctx() -> ScriptContext {
        ScriptContext {
            body: String::new(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            path_params: HashMap::new(),
        }
    }

    #[test]
    fn run_rule_script_returns_none_when_no_script() {
        let engine = ScriptEngine::new();
        let result = run_rule_script(&engine, "my-rule", "pre_script", &None, &empty_script_ctx());
        assert!(result.is_none());
    }

    #[test]
    fn run_rule_script_executes_and_returns_result() {
        let engine = ScriptEngine::new();
        let script = Some(r#""hello""#.to_string());
        let result = run_rule_script(&engine, "my-rule", "script", &script, &empty_script_ctx());
        assert_eq!(result.unwrap().value, "hello");
    }

    #[test]
    fn run_rule_script_soft_fails_on_invalid_script() {
        let engine = ScriptEngine::new();
        let script = Some("this is not valid rhai (((".to_string());
        let result = run_rule_script(
            &engine,
            "my-rule",
            "post_script",
            &script,
            &empty_script_ctx(),
        );
        // Never None, never a panic: an empty ScriptResult instead.
        assert_eq!(result.unwrap().value, "");
    }

    #[test]
    fn pre_script_and_post_script_are_independent_slots() {
        let engine = ScriptEngine::new();
        let pre = Some(r#""PRE""#.to_string());
        let post = Some(r#""POST""#.to_string());
        let ctx = empty_script_ctx();

        let pre_result = run_rule_script(&engine, "r", "pre_script", &pre, &ctx);
        let post_result = run_rule_script(&engine, "r", "post_script", &post, &ctx);
        let script_result = run_rule_script(&engine, "r", "script", &None, &ctx);

        assert_eq!(pre_result.unwrap().value, "PRE");
        assert_eq!(post_result.unwrap().value, "POST");
        assert!(script_result.is_none());
    }

    // --- The proxy forwards query parameters, custom headers, the method and the body untouched. A fake TCP target
    // behind the real router reads the raw request it receives, which checks the whole pipeline (intercept layer,
    // do_proxy, handle_service), not only ProxyClient::forward() (tested on its own in engine/proxy.rs).

    fn temp_dir_for_intercept_test() -> std::path::PathBuf {
        let dir = crate::server::test_support::temp_data_dir("intercept-test");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    use crate::server::test_support::capture_one_raw_request;

    fn disabled_auth_config() -> crate::auth::AuthConfig {
        crate::auth::AuthConfig {
            enabled: false,
            keycloak_url: String::new(),
            realm: String::new(),
            client_id: String::new(),
            super_admins: vec![],
            issuer: String::new(),
            show_reset_button: false,
        }
    }

    #[tokio::test]
    async fn proxy_end_to_end_preserves_query_headers_method_and_body() {
        // 1. A fake target that records the raw request.
        let (target_ready_tx, target_ready_rx) = tokio::sync::oneshot::channel();
        let target_server = tokio::spawn(capture_one_raw_request(target_ready_tx));
        let target_port = target_ready_rx.await.unwrap();

        // 2. A pure proxy service (is_mocked=false: straight to do_proxy, no rule evaluated).
        let data_dir = temp_dir_for_intercept_test();
        let store = MockStore::new(data_dir.join("mock-config.yaml"));
        store
            .replace(MockConfig {
                services: vec![Service {
                    name: "upstream".into(),
                    listen_path: "".into(),
                    real_target_url: format!("http://127.0.0.1:{target_port}"),
                    is_mocked: false,
                    rewrite_directory_urls: false,
                    group_name: None,
                    wsdl_mode: WsdlMode::default(),
                    rules: vec![],
                }],
                groups: vec![],
            })
            .await
            .unwrap();
        store.flush().await;

        // 3. The production router on a real port.
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
            seq_counters: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
            request_log: crate::server::request_log::RequestLog::new(),
            auth_config: disabled_auth_config(),
            keycloak: None,
            script_engine: ScriptEngine::new(),
            ping_cache: crate::server::ping::PingCache::new(),
            observation: crate::server::observation::ObservationState::new(),
            #[cfg(feature = "messaging-kafka")]
            messaging,
            #[cfg(feature = "tcp-mock")]
            tcp_runtime,
        };
        let request_log_handle = state.request_log.clone();
        let app = crate::server::build_router(state, &data_dir);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // 4. A POST with query parameters, a custom header and a body, sent to Mimicway (not to the target).
        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{server_port}/upstream/foo/bar?a=1&b=two"
            ))
            .header("x-custom-header", "custom-value")
            .body("payload-body")
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success() || resp.status().as_u16() == 200);

        let raw = target_server.await.unwrap();
        let request_line = raw.lines().next().unwrap_or("");
        assert!(
            request_line.starts_with("POST "),
            "HTTP method not preserved: {request_line}"
        );
        assert!(
            request_line.contains("/foo/bar?a=1&b=two"),
            "query parameters or path lost in the proxied request: {request_line}"
        );
        assert!(
            raw.to_lowercase().contains("x-custom-header: custom-value"),
            "custom header missing from the proxied request:\n{raw}"
        );
        assert!(
            raw.contains("payload-body"),
            "request body missing from the proxied request:\n{raw}"
        );

        // Service-level proxy: streamed without buffering, so no details are kept for the rule tester.
        let logged = request_log_handle.recent(1);
        assert_eq!(logged.len(), 1);
        assert!(logged[0].captured.is_none());

        std::fs::remove_dir_all(&data_dir).ok();
    }

    #[tokio::test]
    async fn rule_level_proxy_action_preserves_query_headers_method_and_body() {
        // The same check through the other proxy path: a mocked service whose rule says proxy, which rebuilds the request
        // (rebuild_request_for_proxy) instead of forwarding the original one. The target must see no difference.
        let (target_ready_tx, target_ready_rx) = tokio::sync::oneshot::channel();
        let target_server = tokio::spawn(capture_one_raw_request(target_ready_tx));
        let target_port = target_ready_rx.await.unwrap();

        let data_dir = temp_dir_for_intercept_test();
        let store = MockStore::new(data_dir.join("mock-config.yaml"));
        store
            .replace(MockConfig {
                services: vec![Service {
                    name: "upstream2".into(),
                    listen_path: "".into(),
                    real_target_url: format!("http://127.0.0.1:{target_port}"),
                    is_mocked: true,
                    rewrite_directory_urls: false,
                    group_name: None,
                    wsdl_mode: WsdlMode::default(),
                    rules: vec![Rule {
                        name: "proxy-rule".into(),
                        method: "POST".into(),
                        sub_path: None,
                        action: RuleAction::Proxy,
                        pre_script: None,
                        script: None,
                        post_script: None,
                        response_mode: None,
                        conditions: ConditionGroup::default(),
                        response: MockResponse {
                            status: 200,
                            headers: vec![],
                            body: vec![],
                            chaos: None,
                        },
                    }],
                }],
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
            seq_counters: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
            request_log: crate::server::request_log::RequestLog::new(),
            auth_config: disabled_auth_config(),
            keycloak: None,
            script_engine: ScriptEngine::new(),
            ping_cache: crate::server::ping::PingCache::new(),
            observation: crate::server::observation::ObservationState::new(),
            #[cfg(feature = "messaging-kafka")]
            messaging,
            #[cfg(feature = "tcp-mock")]
            tcp_runtime,
        };
        let request_log_handle = state.request_log.clone();
        let app = crate::server::build_router(state, &data_dir);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "http://127.0.0.1:{server_port}/upstream2/foo/bar?a=1&b=two"
            ))
            .header("x-custom-header", "custom-value")
            .body("payload-body")
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success() || resp.status().as_u16() == 200);

        let raw = target_server.await.unwrap();
        let request_line = raw.lines().next().unwrap_or("");
        assert!(
            request_line.starts_with("POST "),
            "HTTP method not preserved (rule-level proxy): {request_line}"
        );
        assert!(
            request_line.contains("/foo/bar?a=1&b=two"),
            "query params/chemin non preserves (rule-level proxy): {request_line}"
        );
        assert!(
            raw.to_lowercase().contains("x-custom-header: custom-value"),
            "custom header missing (rule-level proxy):\n{raw}"
        );
        assert!(
            raw.contains("payload-body"),
            "request body missing (rule-level proxy):\n{raw}"
        );

        // Rule-level proxy: the request was buffered to evaluate the rules, so its details are kept, unlike the
        // service-level proxy above.
        let logged = request_log_handle.recent(1);
        assert_eq!(logged.len(), 1);
        let captured = logged[0]
            .captured
            .as_ref()
            .expect("a rule-level proxy keeps the request details");
        assert_eq!(captured.query_params.get("a").unwrap(), "1");
        assert_eq!(
            captured.headers.get("x-custom-header").unwrap(),
            "custom-value"
        );
        assert_eq!(captured.body, "payload-body");

        std::fs::remove_dir_all(&data_dir).ok();
    }

    #[tokio::test]
    async fn mock_response_captures_request_detail_in_log() {
        // The mock path keeps the request details for the rule tester too.
        let data_dir = temp_dir_for_intercept_test();
        let store = MockStore::new(data_dir.join("mock-config.yaml"));
        store
            .replace(MockConfig {
                services: vec![Service {
                    name: "mocksvc".into(),
                    listen_path: "/{id}/*".into(),
                    real_target_url: "http://unused.invalid".into(),
                    is_mocked: true,
                    rewrite_directory_urls: false,
                    group_name: None,
                    wsdl_mode: WsdlMode::default(),
                    rules: vec![Rule {
                        name: "mock-rule".into(),
                        method: "GET".into(),
                        sub_path: None,
                        action: RuleAction::Mock,
                        pre_script: None,
                        script: None,
                        post_script: None,
                        response_mode: None,
                        conditions: ConditionGroup::default(),
                        response: MockResponse {
                            status: 200,
                            headers: vec![],
                            body: vec![BodyFragment::Literal { value: "ok".into() }],
                            chaos: None,
                        },
                    }],
                }],
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
            seq_counters: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
            request_log: crate::server::request_log::RequestLog::new(),
            auth_config: disabled_auth_config(),
            keycloak: None,
            script_engine: ScriptEngine::new(),
            ping_cache: crate::server::ping::PingCache::new(),
            observation: crate::server::observation::ObservationState::new(),
            #[cfg(feature = "messaging-kafka")]
            messaging,
            #[cfg(feature = "tcp-mock")]
            tcp_runtime,
        };
        let request_log_handle = state.request_log.clone();
        let app = crate::server::build_router(state, &data_dir);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let resp = client
            .get(format!(
                "http://127.0.0.1:{server_port}/mocksvc/42/rest?foo=bar"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);

        let logged = request_log_handle.recent(1);
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].mode, "mock");
        let captured = logged[0]
            .captured
            .as_ref()
            .expect("a mocked request keeps its details");
        assert_eq!(captured.path_params.get("id").unwrap(), "42");
        assert_eq!(captured.query_params.get("foo").unwrap(), "bar");
        assert!(!captured.body_truncated);

        std::fs::remove_dir_all(&data_dir).ok();
    }

    // --- Purely mocked services (no real_target_url). One helper starts the production router for a given
    // configuration, since the three cases below only differ by it.
    async fn spawn_test_server(
        config: MockConfig,
    ) -> (
        u16,
        crate::server::request_log::RequestLog,
        std::path::PathBuf,
    ) {
        let data_dir = temp_dir_for_intercept_test();
        crate::server::test_support::assert_consistent(&config);
        let store = MockStore::new(data_dir.join("mock-config.yaml"));
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
            seq_counters: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
            request_log: crate::server::request_log::RequestLog::new(),
            auth_config: disabled_auth_config(),
            keycloak: None,
            script_engine: ScriptEngine::new(),
            ping_cache: crate::server::ping::PingCache::new(),
            observation: crate::server::observation::ObservationState::new(),
            #[cfg(feature = "messaging-kafka")]
            messaging,
            #[cfg(feature = "tcp-mock")]
            tcp_runtime,
        };
        let request_log_handle = state.request_log.clone();
        let app = crate::server::build_router(state, &data_dir);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (server_port, request_log_handle, data_dir)
    }

    fn purely_mocked_service(rules: Vec<Rule>) -> Service {
        Service {
            name: "nocible".into(),
            listen_path: "".into(),
            real_target_url: "".into(),
            is_mocked: true,
            rewrite_directory_urls: false,
            group_name: None,
            wsdl_mode: WsdlMode::default(),
            rules,
        }
    }

    #[tokio::test]
    async fn no_rule_match_message_mentions_purely_mocked_when_target_empty() {
        // A POST-only rule, so that a GET reaches the "no rule matches" path.
        let rule = Rule {
            name: "post-only".into(),
            method: "POST".into(),
            sub_path: None,
            action: RuleAction::Mock,
            pre_script: None,
            script: None,
            post_script: None,
            response_mode: None,
            conditions: ConditionGroup::default(),
            response: MockResponse {
                status: 200,
                headers: vec![],
                body: vec![],
                chaos: None,
            },
        };
        let (port, request_log_handle, data_dir) = spawn_test_server(MockConfig {
            services: vec![purely_mocked_service(vec![rule])],
            groups: vec![],
        })
        .await;

        let resp = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/nocible/anything"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 404);
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("purely mocked"),
            "the message must say there is no target, got: {body}"
        );

        let logged = request_log_handle.recent(1);
        assert_eq!(logged[0].mode, "no-rule");

        std::fs::remove_dir_all(&data_dir).ok();
    }

    #[tokio::test]
    async fn no_rule_match_message_is_generic_when_target_configured() {
        let rule = Rule {
            name: "post-only".into(),
            method: "POST".into(),
            sub_path: None,
            action: RuleAction::Mock,
            pre_script: None,
            script: None,
            post_script: None,
            response_mode: None,
            conditions: ConditionGroup::default(),
            response: MockResponse {
                status: 200,
                headers: vec![],
                body: vec![],
                chaos: None,
            },
        };
        let mut service = purely_mocked_service(vec![rule]);
        service.name = "avecible".into();
        service.real_target_url = "http://unused.invalid".into();
        let (port, _log, data_dir) = spawn_test_server(MockConfig {
            services: vec![service],
            groups: vec![],
        })
        .await;

        let resp = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/avecible/anything"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 404);
        let body = resp.text().await.unwrap();
        assert!(
            !body.contains("purely mocked"),
            "a service with a target must not get the purely mocked message, got: {body}"
        );

        std::fs::remove_dir_all(&data_dir).ok();
    }

    #[tokio::test]
    async fn service_level_proxy_with_empty_target_returns_clear_error() {
        // Saving refuses a pure proxy without a target, but a YAML file edited by hand can hold one: the runtime guard of
        // do_proxy is the last line of defense.
        let mut service = purely_mocked_service(vec![]);
        service.is_mocked = false;
        let (port, request_log_handle, data_dir) = spawn_test_server(MockConfig {
            services: vec![service],
            groups: vec![],
        })
        .await;

        let resp = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/nocible/anything"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 502);
        let body = resp.text().await.unwrap();
        assert!(body.contains("purely mocked") && body.contains("cannot be forwarded"));

        let logged = request_log_handle.recent(1);
        assert_eq!(logged[0].mode, "proxy");
        assert_eq!(logged[0].status, 502);

        std::fs::remove_dir_all(&data_dir).ok();
    }

    #[tokio::test]
    async fn rule_level_proxy_action_with_empty_target_returns_clear_error() {
        // A service made purely mocked later can still hold a proxy rule (saving only warns): if it matches, the request
        // must never reach a proxy call to an empty URL.
        let rule = Rule {
            name: "stale-proxy-rule".into(),
            method: "GET".into(),
            sub_path: None,
            action: RuleAction::Proxy,
            pre_script: None,
            script: None,
            post_script: None,
            response_mode: None,
            conditions: ConditionGroup::default(),
            response: MockResponse {
                status: 200,
                headers: vec![],
                body: vec![],
                chaos: None,
            },
        };
        let (port, _log, data_dir) = spawn_test_server(MockConfig {
            services: vec![purely_mocked_service(vec![rule])],
            groups: vec![],
        })
        .await;

        let resp = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/nocible/anything"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 502);

        std::fs::remove_dir_all(&data_dir).ok();
    }

    // --- Repeating response items per request item (parse_json/to_json/parse_xml_items/xml_element), end to end, for
    // 2, 1 and 0 items, in JSON and in SOAP. The scripts are the ones of docs/en/rhai-scripts.md, so that copying the
    // documented example gives the documented result.
    fn json_repetition_rule() -> Rule {
        Rule {
            name: "calcul-devis".into(),
            method: "POST".into(),
            sub_path: None,
            action: RuleAction::Mock,
            pre_script: None,
            script: Some(
                r#"
                    let req = parse_json(request.body);
                    let lines = req.lines;
                    let out = [];
                    for line in lines {
                        let unit_price = seeded_int(line.sku, 10, 500);
                        out.push(#{
                            sku: line.sku,
                            qty: line.qty,
                            unitPrice: unit_price,
                            lineTotal: unit_price * line.qty
                        });
                    }
                    #{
                        count: out.len(),
                        lines_json: to_json(out)
                    }
                "#
                .into(),
            ),
            post_script: None,
            response_mode: None,
            conditions: ConditionGroup::default(),
            response: MockResponse {
                status: 200,
                headers: vec![HeaderEntry {
                    name: "Content-Type".into(),
                    value: "application/json".into(),
                }],
                body: vec![BodyFragment::Template {
                    template: r#"{"count":{{script.count}},"lines":{{script.lines_json}}}"#.into(),
                }],
                chaos: None,
            },
        }
    }

    #[tokio::test]
    async fn json_repetition_pattern_builds_one_response_item_per_request_item() {
        let mut service = purely_mocked_service(vec![json_repetition_rule()]);
        service.name = "devis".into();
        let (port, _log, data_dir) = spawn_test_server(MockConfig {
            services: vec![service],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/devis/calcul");

        // 2 items: two lines, each built from the item at the same position.
        let resp = client
            .post(&url)
            .json(&serde_json::json!({"lines": [
                {"sku": "REF-001", "qty": 3},
                {"sku": "REF-002", "qty": 1},
            ]}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["count"], 2);
        let lines = body["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["sku"], "REF-001");
        assert_eq!(lines[0]["qty"], 3);
        assert_eq!(lines[1]["sku"], "REF-002");
        assert_eq!(lines[1]["qty"], 1);
        // The same SKU always gives the same unitPrice (seeded_int), checked again below with REF-001 alone.
        let ref001_price = lines[0]["unitPrice"].as_i64().unwrap();
        assert_eq!(lines[0]["lineTotal"], ref001_price * 3);

        // 1 item, the same SKU as above: the same unitPrice.
        let resp = client
            .post(&url)
            .json(&serde_json::json!({"lines": [{"sku": "REF-001", "qty": 9}]}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["count"], 1);
        let lines = body["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["unitPrice"].as_i64().unwrap(), ref001_price);
        assert_eq!(lines[0]["lineTotal"], ref001_price * 9);

        // 0 items: an empty array in the response, no error.
        let resp = client
            .post(&url)
            .json(&serde_json::json!({"lines": []}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["count"], 0);
        assert!(body["lines"].as_array().unwrap().is_empty());

        std::fs::remove_dir_all(&data_dir).ok();
    }

    fn xml_repetition_rule() -> Rule {
        Rule {
            name: "calcul-totaux".into(),
            method: "POST".into(),
            sub_path: None,
            action: RuleAction::Mock,
            pre_script: None,
            script: Some(
                r#"
                    let articles = parse_xml_items(request.body, "Envelope/Body/GetOrderTotalsRequest/articles/article");
                    let out = "";
                    for a in articles {
                        let unit_price = seeded_int(a.sku, 10, 500);
                        let qty = parse_int(a.qty);
                        out += xml_element("article", #{
                            sku: a.sku,
                            qty: a.qty,
                            unitPrice: unit_price,
                            lineTotal: unit_price * qty
                        });
                    }
                    #{
                        count: articles.len(),
                        articles_xml: out
                    }
                "#
                .into(),
            ),
            post_script: None,
            response_mode: None,
            conditions: ConditionGroup::default(),
            response: MockResponse {
                status: 200,
                headers: vec![HeaderEntry { name: "Content-Type".into(), value: "text/xml".into() }],
                body: vec![BodyFragment::Template {
                    template: r#"<?xml version="1.0"?><soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body><GetOrderTotalsResponse><count>{{script.count}}</count><articles>{{script.articles_xml}}</articles></GetOrderTotalsResponse></soap:Body></soap:Envelope>"#.into(),
                }],
                chaos: None,
            },
        }
    }

    fn soap_request(articles_xml: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><soap:Envelope xmlns:soap="http://schemas.xmlsoap.org/soap/envelope/"><soap:Body><GetOrderTotalsRequest><articles>{articles_xml}</articles></GetOrderTotalsRequest></soap:Body></soap:Envelope>"#
        )
    }

    #[tokio::test]
    async fn xml_repetition_pattern_builds_one_response_item_per_request_item() {
        let mut service = purely_mocked_service(vec![xml_repetition_rule()]);
        service.name = "commande-soap".into();
        let (port, _log, data_dir) = spawn_test_server(MockConfig {
            services: vec![service],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/commande-soap/totaux");

        // 2 items
        let body_xml = soap_request(
            "<article><sku>REF-001</sku><qty>3</qty></article><article><sku>REF-002</sku><qty>1</qty></article>",
        );
        let resp = client
            .post(&url)
            .header("Content-Type", "text/xml")
            .body(body_xml)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let text = resp.text().await.unwrap();
        assert_eq!(text.matches("<article>").count(), 2);
        assert!(text.contains("<count>2</count>"));
        assert!(text.contains("<sku>REF-001</sku>"));
        assert!(text.contains("<sku>REF-002</sku>"));
        // lineTotal = unitPrice * qty; REF-001 has qty=3: read unitPrice to check it.
        let unit_price_pos = text.find("<sku>REF-001</sku>").unwrap();
        let after = &text[unit_price_pos..];
        let up_start = after.find("<unitPrice>").unwrap() + "<unitPrice>".len();
        let up_end = after.find("</unitPrice>").unwrap();
        let ref001_price: i64 = after[up_start..up_end].parse().unwrap();
        assert!(text.contains(&format!("<lineTotal>{}</lineTotal>", ref001_price * 3)));

        // 1 item, the same SKU REF-001: the same unitPrice (seeded_int).
        let body_xml = soap_request("<article><sku>REF-001</sku><qty>9</qty></article>");
        let resp = client
            .post(&url)
            .header("Content-Type", "text/xml")
            .body(body_xml)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let text = resp.text().await.unwrap();
        assert_eq!(text.matches("<article>").count(), 1);
        assert!(text.contains("<count>1</count>"));
        assert!(text.contains(&format!("<unitPrice>{ref001_price}</unitPrice>")));
        assert!(text.contains(&format!("<lineTotal>{}</lineTotal>", ref001_price * 9)));

        // 0 items: an empty <articles></articles>, no error.
        let body_xml = soap_request("");
        let resp = client
            .post(&url)
            .header("Content-Type", "text/xml")
            .body(body_xml)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("<count>0</count>"));
        assert!(text.contains("<articles></articles>"));

        std::fs::remove_dir_all(&data_dir).ok();
    }

    // --- A lookup table keyed by a path parameter, with an XML response: the documented script (a `#{...}` map,
    // `.contains(key)`, `mapping[key]`, an if/else fallback) works end to end, fallback included, so copying the
    // example from docs/en/rhai-scripts.md works.
    fn service_lookup_rule() -> Rule {
        Rule {
            name: "lookup-service".into(),
            method: "GET".into(),
            sub_path: Some("/lookup/{name}".into()),
            action: RuleAction::Mock,
            pre_script: None,
            script: Some(
                r#"
                    let mapping = #{
                        "billing": "svc-billing-042",
                        "orders": "svc-orders-017"
                    };
                    let name = request.path.name;
                    if mapping.contains(name) {
                        #{ id: mapping[name], found: "true" }
                    } else {
                        #{ id: "unknown", found: "false" }
                    }
                "#
                .into(),
            ),
            post_script: None,
            response_mode: None,
            conditions: ConditionGroup::default(),
            response: MockResponse {
                status: 200,
                headers: vec![HeaderEntry { name: "Content-Type".into(), value: "text/xml".into() }],
                body: vec![BodyFragment::Template {
                    template: r#"<?xml version="1.0"?><serviceLookup><name>{{path.name}}</name><id>{{script.id}}</id><found>{{script.found}}</found></serviceLookup>"#.into(),
                }],
                chaos: None,
            },
        }
    }

    #[tokio::test]
    async fn map_lookup_by_path_param_returns_correct_target_and_falls_back_for_unknown_key() {
        let mut service = purely_mocked_service(vec![service_lookup_rule()]);
        service.name = "annuaire".into();
        let (port, _log, data_dir) = spawn_test_server(MockConfig {
            services: vec![service],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();

        // A key of the table: found.
        let resp = client
            .get(format!("http://127.0.0.1:{port}/annuaire/lookup/billing"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("<name>billing</name>"));
        assert!(text.contains("<id>svc-billing-042</id>"));
        assert!(text.contains("<found>true</found>"));

        // A key missing from the table: the else branch answers, which a script calling a missing function never reached.
        let resp = client
            .get(format!(
                "http://127.0.0.1:{port}/annuaire/lookup/nonexistent"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("<name>nonexistent</name>"));
        assert!(text.contains("<id>unknown</id>"));
        assert!(text.contains("<found>false</found>"));

        std::fs::remove_dir_all(&data_dir).ok();
    }

    // --- An XPath condition on a SOAP body with namespaces, and a value copied from the request into the response:
    // "Envelope/Body/recherche" (no namespace prefix, see MatchEngine::local_name) picks the right operation even with
    // an empty <Header></Header> written in full before <Body> (see MatchEngine::walk_xml), and the extraction script
    // (parse_xml_items) copies the request's Siret into the response.
    fn soap_condition_rules() -> Vec<Rule> {
        vec![
            Rule {
                name: "operation-recherche".into(),
                method: "POST".into(),
                sub_path: None,
                action: RuleAction::Mock,
                pre_script: None,
                script: Some(
                    r#"
                        let items = parse_xml_items(request.body, "Envelope/Body/recherche");
                        let siret = if items.len() > 0 { items[0].Siret } else { "" };
                        #{ siret: siret }
                    "#
                    .into(),
                ),
                post_script: None,
                response_mode: None,
                conditions: ConditionGroup {
                    all_of: vec![Condition {
                        source: ConditionSource::XPath("Envelope/Body/recherche".into()),
                        operator: Operator::Exists,
                    }],
                    any_of: vec![],
                },
                response: MockResponse {
                    status: 200,
                    headers: vec![HeaderEntry { name: "Content-Type".into(), value: "text/xml".into() }],
                    body: vec![BodyFragment::Template {
                        template: r#"<?xml version="1.0"?><rechercheResponse><siret>{{script.siret}}</siret></rechercheResponse>"#.into(),
                    }],
                    chaos: None,
                },
            },
            Rule {
                name: "operation-mode".into(),
                method: "POST".into(),
                sub_path: None,
                action: RuleAction::Mock,
                pre_script: None,
                script: None,
                post_script: None,
                response_mode: None,
                conditions: ConditionGroup {
                    all_of: vec![Condition {
                        source: ConditionSource::XPath("Envelope/Body/mode".into()),
                        operator: Operator::Exists,
                    }],
                    any_of: vec![],
                },
                response: MockResponse {
                    status: 200,
                    headers: vec![HeaderEntry { name: "Content-Type".into(), value: "text/xml".into() }],
                    body: vec![BodyFragment::Template {
                        template: r#"<?xml version="1.0"?><modeResponse><ok>true</ok></modeResponse>"#.into(),
                    }],
                    chaos: None,
                },
            },
        ]
    }

    #[tokio::test]
    async fn soap_xpath_condition_routes_by_operation_and_extracts_value_into_response() {
        let mut service = purely_mocked_service(soap_condition_rules());
        service.name = "annuaire-soap".into();
        let (port, _log, data_dir) = spawn_test_server(MockConfig {
            services: vec![service],
            groups: vec![],
        })
        .await;
        let client = reqwest::Client::new();
        let url = format!("http://127.0.0.1:{port}/annuaire-soap/service");

        // The "recherche" operation with a full Header before Body: the condition matches and the Siret comes back as is.
        let body_recherche = r#"<SOAP:Envelope><SOAP-ENV:Header></SOAP-ENV:Header><SOAP-ENV:Body><ns3:recherche><ns3:Nom>Test</ns3:Nom><ns3:Siret>12345678901234</ns3:Siret></ns3:recherche></SOAP-ENV:Body></SOAP:Envelope>"#;
        let resp = client
            .post(&url)
            .header("Content-Type", "text/xml")
            .body(body_recherche)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let text = resp.text().await.unwrap();
        assert!(
            text.contains("<siret>12345678901234</siret>"),
            "the request's Siret must come back in the response, got: {text}"
        );

        // The "mode" operation, same envelope: the "recherche" rule does not match, "mode" answers instead.
        let body_mode = r#"<SOAP:Envelope><SOAP-ENV:Header></SOAP-ENV:Header><SOAP-ENV:Body><ns3:mode><ns3:Valeur>test</ns3:Valeur></ns3:mode></SOAP-ENV:Body></SOAP:Envelope>"#;
        let resp = client
            .post(&url)
            .header("Content-Type", "text/xml")
            .body(body_mode)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("<modeResponse>"));
        assert!(!text.contains("<siret>"));

        std::fs::remove_dir_all(&data_dir).ok();
    }

    #[tokio::test]
    async fn dot_segments_never_reach_the_proxied_backend() {
        use crate::server::test_support::{auth_disabled, serve, temp_data_dir, test_state};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let backend = axum::Router::new().fallback(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async { "backend" }
        });
        let backend_url = serve(backend).await;
        let service: Service = serde_json::from_value(serde_json::json!({
            "name": "svc", "listen_path": "", "real_target_url": format!("{backend_url}/base/"),
            "is_mocked": false, "rewrite_directory_urls": false, "group_name": null,
            "wsdl_mode": "auto", "rules": []
        }))
        .unwrap();
        let data_dir = temp_data_dir("dot-segments");
        let config = crate::models::MockConfig {
            services: vec![service],
            groups: vec![],
        };
        let state = test_state(&data_dir, config, auth_disabled()).await;
        let root = serve(crate::server::build_router(state, &data_dir)).await;
        let addr = root.trim_start_matches("http://").to_string();

        // Raw requests: an HTTP client library would resolve the dot segments before sending.
        for path in ["/svc/../admin", "/svc/%2e%2e/%2E%2E/admin", "/svc/./x"] {
            let mut stream = tokio::net::TcpStream::connect(&addr).await.unwrap();
            let request =
                format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
            stream.write_all(request.as_bytes()).await.unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with("HTTP/1.1 400"), "{path}: {response}");
        }
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let ok = reqwest::get(format!("{root}/svc/orders")).await.unwrap();
        assert_eq!(ok.text().await.unwrap(), "backend");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
