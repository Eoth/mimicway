//! Changes of the raw TCP mocks: each is checked against the configuration the previous one left, and names are
//! stored as they are checked.
use crate::models::MockConfig;
use crate::server::test_support::{auth_disabled, serve, temp_data_dir, test_state};
use reqwest::StatusCode;
use serde_json::{Value, json};

async fn start() -> (String, reqwest::Client) {
    let data_dir = temp_data_dir("tcp-api");
    let state = test_state(&data_dir, MockConfig::empty(), auth_disabled()).await;
    let root = serve(crate::server::build_router(state, &data_dir)).await;
    (format!("{root}/api/tcp/services"), reqwest::Client::new())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .expect("free port")
}

fn tcp_service(name: &str, port: u16) -> Value {
    json!({
        "name": name,
        "listen_port": port,
        "rules": [{"name": "ping", "matcher": {"type": "Any"}, "response_hex": "706f6e67"}],
    })
}

async fn names(url: &str, client: &reqwest::Client) -> Vec<String> {
    let services: Vec<Value> = client.get(url).send().await.unwrap().json().await.unwrap();
    let mut names: Vec<_> = services
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn creations_at_the_same_time_are_all_kept_or_refused() {
    let (url, client) = start().await;
    // Each creation used to be checked against the same snapshot, then replace the whole configuration: the last
    // one to finish erased the others, which had been answered 201.
    let creations = (0..8).map(|i| {
        let (url, client) = (url.clone(), client.clone());
        let body = tcp_service(&format!("svc-{i}"), free_port());
        async move { client.post(&url).json(&body).send().await.unwrap().status() }
    });
    let statuses = futures_util::future::join_all(creations).await;
    let created = statuses
        .iter()
        .filter(|s| **s == StatusCode::CREATED)
        .count();
    assert_eq!(created, 8, "{statuses:?}");
    assert_eq!(names(&url, &client).await.len(), 8);
}

#[tokio::test]
async fn a_name_that_differs_only_by_surrounding_spaces_is_the_same_tcp_service() {
    let (url, client) = start().await;
    let status = |body: Value| {
        let (url, client) = (url.clone(), client.clone());
        async move { client.post(&url).json(&body).send().await.unwrap().status() }
    };
    assert_eq!(
        status(tcp_service(" svc ", free_port())).await,
        StatusCode::CREATED
    );
    assert_eq!(names(&url, &client).await, ["svc"]);
    assert_eq!(
        status(tcp_service("svc", free_port())).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        status(tcp_service("svc  ", free_port())).await,
        StatusCode::BAD_REQUEST
    );

    let renamed = client
        .put(format!("{url}/svc"))
        .json(&tcp_service("other ", free_port()))
        .send()
        .await
        .unwrap();
    assert_eq!(renamed.status(), StatusCode::OK);
    assert_eq!(names(&url, &client).await, ["other"]);
}

#[tokio::test]
async fn an_update_or_a_deletion_of_a_missing_tcp_service_changes_nothing() {
    let (url, client) = start().await;
    let created = client
        .post(&url)
        .json(&tcp_service("svc", free_port()))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), StatusCode::CREATED);
    let updated = client
        .put(format!("{url}/missing"))
        .json(&tcp_service("missing", free_port()))
        .send()
        .await
        .unwrap();
    assert_eq!(updated.status(), StatusCode::NOT_FOUND);
    let deleted = client
        .delete(format!("{url}/missing"))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::NOT_FOUND);
    assert_eq!(names(&url, &client).await, ["svc"]);
    let deleted = client.delete(format!("{url}/svc")).send().await.unwrap();
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert!(names(&url, &client).await.is_empty());
}
