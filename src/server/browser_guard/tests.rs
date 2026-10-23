use super::*;
use crate::models::MockConfig;
use crate::server::test_support::{auth_disabled, mock_service, serve, temp_data_dir, test_state};

const UI: &str = "https://ui.example.com";
const EVIL: &str = "https://evil.example";

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, HeaderValue::from_str(value).unwrap());
    }
    map
}

fn refuses(guard: &BrowserGuard, method: &Method, path: &str, headers: &HeaderMap) -> bool {
    guard
        .refusal(method, &path.parse::<Uri>().unwrap(), headers)
        .is_some()
}

fn refuses_host(guard: &BrowserGuard, host: &str) -> bool {
    refuses(
        guard,
        &Method::GET,
        "/api/config",
        &headers(&[("host", host)]),
    )
}

#[test]
fn parses_a_comma_separated_list_and_ignores_blanks_and_trailing_slashes() {
    let guard = BrowserGuard::new(" https://ui.example.com/ , ,http://localhost:5173");
    assert!(guard.allows(&HeaderValue::from_static("https://ui.example.com")));
    assert!(guard.allows(&HeaderValue::from_static("http://localhost:5173")));
    assert!(!guard.allows(&HeaderValue::from_static("http://localhost:5174")));
    assert!(BrowserGuard::new("").allowed_origins.is_empty());
}

#[test]
fn refuses_cross_site_writes_to_the_api_only() {
    let guard = BrowserGuard::new(UI);
    let cross = headers(&[("sec-fetch-site", "cross-site"), ("origin", EVIL)]);
    assert!(refuses(&guard, &Method::POST, "/api/services", &cross));
    assert!(refuses(
        &guard,
        &Method::DELETE,
        "/api/config/reset",
        &cross
    ));
    assert!(refuses(
        &guard,
        &Method::PUT,
        "/api/config",
        &headers(&[("sec-fetch-site", "same-site")])
    ));
    // Reads, service traffic, same-origin calls and non-browser clients go through.
    assert!(!refuses(&guard, &Method::GET, "/api/config", &cross));
    assert!(!refuses(&guard, &Method::POST, "/orders/42", &cross));
    assert!(!refuses(
        &guard,
        &Method::POST,
        "/api/services",
        &headers(&[("sec-fetch-site", "same-origin")])
    ));
    assert!(!refuses(
        &guard,
        &Method::POST,
        "/api/services",
        &HeaderMap::new()
    ));
    // A listed origin may write from another site.
    let listed = headers(&[("sec-fetch-site", "cross-site"), ("origin", UI)]);
    assert!(!refuses(&guard, &Method::POST, "/api/services", &listed));
}

async fn spawn_app() -> String {
    let data_dir = temp_data_dir("browser-guard");
    let config = MockConfig {
        services: vec![mock_service("orders", "mocked")],
        groups: vec![],
    };
    let state = test_state(&data_dir, config, auth_disabled()).await;
    serve(crate::server::build_router_with(
        state,
        crate::server::ui_files::UiSource::Directory(data_dir.clone()),
        BrowserGuard::new(UI),
        crate::server::limits::Limits::default(),
    ))
    .await
}

async fn preflight(client: &reqwest::Client, url: String, origin: &str) -> Option<String> {
    let resp = client
        .request(Method::OPTIONS, url)
        .header("origin", origin)
        .header("access-control-request-method", "PUT")
        .send()
        .await
        .unwrap();
    resp.headers()
        .get("access-control-allow-origin")
        .map(|v| v.to_str().unwrap().to_string())
}

#[tokio::test]
async fn api_cors_is_limited_to_listed_origins_while_mocks_stay_open() {
    let root = spawn_app().await;
    let client = reqwest::Client::new();
    assert_eq!(
        preflight(&client, format!("{root}/api/config"), EVIL).await,
        None
    );
    assert_eq!(
        preflight(&client, format!("{root}/api/config"), UI)
            .await
            .as_deref(),
        Some(UI)
    );
    assert_eq!(
        preflight(&client, format!("{root}/orders/42"), EVIL)
            .await
            .as_deref(),
        Some(EVIL)
    );
}

#[tokio::test]
async fn a_cross_site_page_cannot_change_the_configuration() {
    let root = spawn_app().await;
    let client = reqwest::Client::new();
    let reset = |language: &'static str| {
        client
            .delete(format!("{root}/api/config/reset"))
            .header("origin", EVIL)
            .header("sec-fetch-site", "cross-site")
            .header("accept-language", language)
            .send()
    };
    for (language, reason) in [
        ("en-US", "Cross-site request refused"),
        ("fr-FR", "Requête inter-sites refusée"),
    ] {
        let resp = reset(language).await.unwrap();
        assert_eq!(resp.status().as_u16(), 403);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(
            body["error"].as_str().unwrap().starts_with(reason),
            "{body}"
        );
    }
    let services = client
        .get(format!("{root}/api/services"))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(
        services.as_array().map(Vec::len),
        Some(1),
        "nothing was reset"
    );

    let mocked = client
        .get(format!("{root}/orders/42"))
        .header("origin", EVIL)
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .unwrap();
    assert_eq!(mocked.text().await.unwrap(), "mocked");
}

// Browsers that predate Fetch Metadata (Safari before 16.4, older Firefox) send no Sec-Fetch-Site, but they do send
// Origin on a cross-origin form POST or fetch: the write is judged by it.
#[test]
fn without_sec_fetch_site_a_write_from_a_foreign_origin_is_refused() {
    let guard = BrowserGuard::new(UI);
    let from = |origin: &str, host: &str| headers(&[("origin", origin), ("host", host)]);
    for (origin, host) in [
        (EVIL, "localhost:7342"),
        ("http://localhost:3000", "localhost:7342"),
        ("https://localhost:7342", "localhost:7342:1"),
        ("null", "localhost:7342"),
        ("http://127.0.0.1:7342", "localhost:7342"),
    ] {
        assert!(
            refuses(
                &guard,
                &Method::POST,
                "/api/config/restore/x.yaml",
                &from(origin, host)
            ),
            "{origin} writing to {host}"
        );
    }
    for (origin, host) in [
        ("http://localhost:7342", "localhost:7342"),
        ("http://LOCALHOST:7342", "localhost:7342"),
        ("https://mimicway.example.com", "mimicway.example.com"),
        ("https://mimicway.example.com", "mimicway.example.com:443"),
        ("http://mimicway.example.com", "mimicway.example.com:80"),
        (UI, "api.example.com"),
    ] {
        assert!(
            !refuses(&guard, &Method::POST, "/api/services", &from(origin, host)),
            "{origin} writing to {host}"
        );
    }
    assert!(
        !refuses(
            &guard,
            &Method::POST,
            "/api/services",
            &headers(&[("host", "localhost:7342")])
        ),
        "a client without Origin is not a browser"
    );
    assert!(
        !refuses(
            &guard,
            &Method::GET,
            "/api/config",
            &from(EVIL, "localhost:7342")
        ),
        "reads are CORS's business"
    );
    assert!(
        !refuses(
            &guard,
            &Method::POST,
            "/orders/42",
            &from(EVIL, "localhost:7342")
        ),
        "service traffic stays open"
    );
}

#[test]
fn reads_host_names_as_browsers_send_them() {
    for (host, name) in [
        ("localhost:7342", "localhost"),
        (" LOCALHOST ", "localhost"),
        ("[::1]:7342", "::1"),
        ("[fd00::5]", "fd00::5"),
        ("Mimicway.Example.com.:443", "mimicway.example.com"),
        ("10.0.0.5", "10.0.0.5"),
        ("", ""),
    ] {
        assert_eq!(host_name(host), name, "{host}");
    }
}

#[test]
fn without_authentication_the_api_answers_only_names_a_page_cannot_rebind_on_any_interface() {
    for loopback_only in [true, false] {
        let guard = BrowserGuard::new("").with_host_check(None, loopback_only, false);
        for host in [
            "localhost:7342",
            "app.localhost:5173",
            "127.0.0.1:7342",
            "[::1]:7342",
            "192.168.1.5:7342",
            "[fd00::5]:7342",
        ] {
            assert!(
                !refuses_host(&guard, host),
                "{host}, loopback only: {loopback_only}"
            );
        }
        for host in ["evil.example:7342", "localhost.evil.example", "devbox"] {
            assert!(
                refuses_host(&guard, host),
                "{host}, loopback only: {loopback_only}"
            );
        }
        // A request that names no host is refused; mocked services answer any name.
        assert!(refuses(
            &guard,
            &Method::GET,
            "/api/config",
            &HeaderMap::new()
        ));
        let rebound = headers(&[("host", "evil.example:7342")]);
        assert!(!refuses(&guard, &Method::GET, "/orders/42", &rebound));
    }
}

#[test]
fn listed_names_and_the_api_base_url_host_are_answered() {
    let guard = BrowserGuard {
        api_host: Some("api.example.com".into()),
        ..BrowserGuard::new("")
    }
    .with_host_check(Some(" mimicway.example.com, DevBox:7342 ,"), false, false);
    for host in [
        "mimicway.example.com:443",
        "devbox",
        "API.example.com",
        "localhost",
    ] {
        assert!(!refuses_host(&guard, host), "{host}");
    }
    assert!(refuses_host(&guard, "evil.example"));
    let unset = BrowserGuard {
        api_host: Some("api.example.com".into()),
        ..BrowserGuard::new("")
    }
    .with_host_check(None, false, false);
    assert_eq!(
        unset.host_names(),
        Some(&["api.example.com".to_string()][..])
    );
}

#[test]
fn any_name_is_answered_only_when_asked_or_beyond_loopback_with_authentication() {
    let any = |allowed: Option<&str>, loopback_only: bool, auth: bool| {
        BrowserGuard::new("")
            .with_host_check(allowed, loopback_only, auth)
            .host_names()
            .is_none()
    };
    assert!(any(Some("*"), true, false));
    assert!(any(Some(" * "), false, false));
    assert!(any(None, false, true));
    assert!(any(Some(""), false, true));
    assert!(!any(None, false, false));
    assert!(!any(None, true, true));
    assert!(!any(Some("mimicway.example.com"), false, true));
}

#[test]
fn the_http2_authority_is_checked_before_the_host_header() {
    let guard = BrowserGuard::new("").with_host_check(None, false, false);
    let check = |uri: &str, host: Option<&str>| {
        let headers = host.map_or_else(HeaderMap::new, |h| headers(&[("host", h)]));
        guard
            .refusal(&Method::GET, &uri.parse().unwrap(), &headers)
            .is_some()
    };
    assert!(check(
        "http://evil.example:7342/api/config",
        Some("localhost")
    ));
    assert!(check("http://user@evil.example/api/config", None));
    assert!(!check("http://localhost:7342/api/config", None));
    assert!(!check("http://[::1]:7342/api/config", Some("evil.example")));
}

#[tokio::test]
async fn a_rebound_name_cannot_read_the_configuration_of_a_server_open_to_the_network() {
    let data_dir = temp_data_dir("browser-guard-hosts");
    let config = MockConfig {
        services: vec![mock_service("orders", "mocked")],
        groups: vec![],
    };
    let state = test_state(&data_dir, config, auth_disabled()).await;
    let root = serve(crate::server::build_router_with(
        state,
        crate::server::ui_files::UiSource::Directory(data_dir.clone()),
        BrowserGuard::new("").with_host_check(None, false, false),
        crate::server::limits::Limits::default(),
    ))
    .await;
    let client = reqwest::Client::new();
    let get = |path: &str, host: &'static str, language: &'static str| {
        client
            .get(format!("{root}{path}"))
            .header("host", host)
            .header("accept-language", language)
            .send()
    };
    for (language, reason) in [
        (
            "en-US",
            "Request refused: this Mimicway does not answer to the name \"evil.example:7342\"",
        ),
        (
            "fr-FR",
            "Requête refusée : ce Mimicway ne répond pas au nom \"evil.example:7342\"",
        ),
    ] {
        let resp = get("/api/services", "evil.example:7342", language)
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 403);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert!(
            body["error"].as_str().unwrap().starts_with(reason),
            "{body}"
        );
    }
    let listed = get("/api/services", "10.1.2.3:7342", "en-US")
        .await
        .unwrap();
    assert_eq!(listed.status().as_u16(), 200);
    let mocked = get("/orders/42", "evil.example:7342", "en-US")
        .await
        .unwrap();
    assert_eq!(mocked.text().await.unwrap(), "mocked");
}

#[tokio::test]
async fn mimicway_pages_carry_security_headers_and_service_responses_only_the_sandbox() {
    let root = spawn_app().await;
    let client = reqwest::Client::new();
    for path in ["/api/health", "/runtime-config.json", "/index.html"] {
        let resp = client.get(format!("{root}{path}")).send().await.unwrap();
        let headers = resp.headers();
        let csp = headers["content-security-policy"].to_str().unwrap();
        assert!(csp.contains("frame-ancestors 'none'"), "{path}: {csp}");
        assert!(csp.contains("script-src 'self'"), "{path}: {csp}");
        assert!(!csp.contains("sandbox"), "{path}: {csp}");
        assert_eq!(headers["x-content-type-options"], "nosniff", "{path}");
        assert_eq!(headers["x-frame-options"], "DENY", "{path}");
    }
    let mocked = client
        .get(format!("{root}/orders/42"))
        .send()
        .await
        .unwrap();
    assert_eq!(policies(mocked.headers()), [SERVICE_RESPONSE_POLICY]);
    for name in [
        "x-frame-options",
        "x-content-type-options",
        "referrer-policy",
    ] {
        assert!(
            mocked.headers().get(name).is_none(),
            "{name} added to a mock response"
        );
    }
    assert_eq!(mocked.text().await.unwrap(), "mocked");
}

fn policies(headers: &HeaderMap) -> Vec<&str> {
    headers
        .get_all("content-security-policy")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect()
}

#[test]
fn the_service_sandbox_never_gives_the_interface_origin_back() {
    let flags: Vec<&str> = SERVICE_RESPONSE_POLICY.split_whitespace().collect();
    assert_eq!(flags[0], "sandbox");
    assert!(flags.contains(&"allow-scripts"));
    for flag in [
        "allow-same-origin",
        "allow-popups-to-escape-sandbox",
        "allow-top-navigation",
    ] {
        assert!(!flags.contains(&flag), "{flag}");
    }
}

/// A backend that answers every request with an HTML page and a policy of its own.
async fn spawn_html_backend() -> String {
    serve(axum::Router::new().fallback(|| async {
        (
            [
                ("content-type", "text/html"),
                ("content-security-policy", "img-src 'none'"),
            ],
            "<script>parent</script>",
        )
    }))
    .await
}

#[tokio::test]
async fn mocked_and_proxied_pages_keep_their_own_policy_next_to_the_sandbox() {
    let backend = spawn_html_backend().await;
    let mut page = mock_service("page", "<script>x</script>");
    page.rules[0].response.headers = serde_json::from_value(serde_json::json!([
        {"name": "Content-Type", "value": "text/html"},
        {"name": "Content-Security-Policy", "value": "img-src 'none'"}
    ]))
    .unwrap();
    let proxied: crate::models::Service = serde_json::from_value(serde_json::json!({
        "name": "web", "listen_path": "", "real_target_url": backend, "is_mocked": false,
        "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto", "rules": []
    }))
    .unwrap();
    let data_dir = temp_data_dir("browser-guard-sandbox");
    let config = MockConfig {
        services: vec![page, proxied],
        groups: vec![],
    };
    let state = test_state(&data_dir, config, auth_disabled()).await;
    let root = serve(crate::server::build_router_with(
        state,
        crate::server::ui_files::UiSource::Directory(data_dir.clone()),
        BrowserGuard::new(""),
        crate::server::limits::Limits::default(),
    ))
    .await;
    for path in ["/page/x", "/web/x"] {
        let resp = reqwest::get(format!("{root}{path}")).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200, "{path}");
        assert_eq!(
            policies(resp.headers()),
            ["img-src 'none'", SERVICE_RESPONSE_POLICY],
            "{path}"
        );
        assert!(resp.text().await.unwrap().starts_with("<script>"), "{path}");
    }
}

#[test]
fn the_ui_may_call_the_configured_api_origin() {
    let guard = BrowserGuard {
        api_origin: Some("https://api.example.com".into()),
        ..BrowserGuard::new("")
    };
    assert!(
        guard
            .content_security_policy()
            .contains("connect-src 'self' https://api.example.com;")
    );
    assert!(
        BrowserGuard::new("")
            .content_security_policy()
            .contains("connect-src 'self';")
    );
}
