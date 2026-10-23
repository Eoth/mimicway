use super::*;
use crate::models::MockConfig;
use crate::server::test_support::{
    auth_disabled, mock_service, serve as serve_app, temp_data_dir, test_state,
};

static FILES: FileTable = &[
    ("assets/index-abc123.js", b"console.log('ui');"),
    ("favicon.svg", b"<svg/>"),
    ("index.html", b"<!doctype html><title>Mimicway</title>"),
];

fn get(path: &str) -> Response {
    serve(FILES, &Method::GET, &path.parse::<Uri>().unwrap())
}

fn header(response: &Response, name: axum::http::HeaderName) -> &str {
    response.headers()[name].to_str().unwrap()
}

#[test]
fn static_dir_wins_then_the_embedded_ui_then_the_default_directory() {
    assert!(matches!(
        UiSource::choose(Some("/srv/ui".into()), FILES),
        UiSource::Directory(dir) if dir == std::path::Path::new("/srv/ui")
    ));
    assert!(matches!(UiSource::choose(None, FILES), UiSource::Embedded(files) if files.len() == 3));
    assert!(matches!(
        UiSource::choose(Some("  ".into()), FILES),
        UiSource::Embedded(_)
    ));
    assert!(matches!(
        UiSource::choose(None, &[]),
        UiSource::Directory(dir) if dir == std::path::Path::new("./frontend/dist")
    ));
}

#[test]
fn the_root_gives_index_html_revalidated_on_each_visit() {
    let response = get("/");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, CONTENT_TYPE), "text/html; charset=utf-8");
    assert_eq!(header(&response, CACHE_CONTROL), "no-cache");
}

#[test]
fn fingerprinted_assets_are_cached_for_good() {
    let response = get("/assets/index-abc123.js");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, CONTENT_TYPE),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        header(&response, CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(header(&get("/favicon.svg"), CONTENT_TYPE), "image/svg+xml");
}

#[test]
fn only_files_of_the_table_are_served() {
    for path in [
        "/missing.js",
        "/assets/",
        "/../Cargo.toml",
        "/%2e%2e/Cargo.toml",
        "/assets/../index.html",
    ] {
        assert_eq!(get(path).status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[test]
fn only_get_and_head_are_answered() {
    let uri: Uri = "/".parse().unwrap();
    assert_eq!(
        serve(FILES, &Method::POST, &uri).status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    let head = serve(FILES, &Method::HEAD, &uri);
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(header(&head, CONTENT_TYPE), "text/html; charset=utf-8");
}

#[tokio::test]
async fn the_embedded_ui_is_served_with_the_ui_protections_and_leaves_the_rest_alone() {
    let data_dir = temp_data_dir("ui-files");
    let config = MockConfig {
        services: vec![mock_service("orders", "mocked")],
        groups: vec![],
    };
    let state = test_state(&data_dir, config, auth_disabled()).await;
    let root = serve_app(crate::server::build_router_with(
        state,
        UiSource::Embedded(FILES),
        crate::server::browser_guard::BrowserGuard::new(""),
        crate::server::limits::Limits::default(),
    ))
    .await;
    let client = reqwest::Client::new();

    let page = client.get(format!("{root}/")).send().await.unwrap();
    assert_eq!(page.status().as_u16(), 200);
    assert!(page.headers().contains_key("content-security-policy"));
    assert!(
        page.text()
            .await
            .unwrap()
            .contains("<title>Mimicway</title>")
    );

    let health = client
        .get(format!("{root}/api/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status().as_u16(), 200);
    let mocked = client
        .get(format!("{root}/orders/42"))
        .send()
        .await
        .unwrap();
    assert_eq!(mocked.text().await.unwrap(), "mocked");
    let missing = client
        .get(format!("{root}/nothing-here.js"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status().as_u16(), 404);
}
