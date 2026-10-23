//! One service per (group, name), through every way the API changes the configuration: the interface lists services
//! keyed by name within their group, and two services in the same slot crash it, while routing only ever reaches
//! the first one.
use crate::models::{Group, MockConfig, Service};
use crate::server::test_support::{auth_disabled, mock_service, serve, temp_data_dir, test_state};
use reqwest::StatusCode;
use serde_json::Value;

fn group(name: &str, code: &str) -> Group {
    Group {
        name: name.into(),
        code: code.into(),
        admins: vec![],
        members: vec![],
    }
}

fn grouped(name: &str, group: &str) -> Service {
    Service {
        group_name: Some(group.into()),
        ..mock_service(name, name)
    }
}

struct App {
    api: String,
    client: reqwest::Client,
}

async fn start(config: MockConfig) -> App {
    let data_dir = temp_data_dir("uniqueness");
    let state = test_state(&data_dir, config, auth_disabled()).await;
    let root = serve(crate::server::build_router(state, &data_dir)).await;
    App {
        api: format!("{root}/api"),
        client: reqwest::Client::new(),
    }
}

impl App {
    async fn send(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> StatusCode {
        let mut request = self.client.request(method, format!("{}{path}", self.api));
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send().await.unwrap().status()
    }

    async fn config(&self) -> MockConfig {
        let resp = self
            .client
            .get(format!("{}/config", self.api))
            .send()
            .await
            .unwrap();
        resp.json().await.unwrap()
    }

    async fn slots(&self) -> Vec<(Option<String>, String)> {
        let mut slots: Vec<_> = self
            .config()
            .await
            .services
            .into_iter()
            .map(|s| (s.group_name, s.name))
            .collect();
        slots.sort();
        slots
    }
}

fn config_with(services: Vec<Service>, groups: Vec<Group>) -> Value {
    serde_json::to_value(MockConfig { services, groups }).unwrap()
}

#[tokio::test]
async fn an_import_with_two_services_in_one_slot_is_refused_and_changes_nothing() {
    let app = start(MockConfig::empty()).await;
    let body = config_with(
        vec![grouped("users", "team-a"), grouped("users", "team-a")],
        vec![group("team-a", "tmaaa")],
    );
    let status = app.send(reqwest::Method::PUT, "/config", Some(body)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        app.slots().await.is_empty(),
        "a refused import must not replace the configuration"
    );

    let ungrouped = config_with(
        vec![mock_service("users", "a"), mock_service("users", "b")],
        vec![],
    );
    let status = app
        .send(reqwest::Method::PUT, "/config", Some(ungrouped))
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_import_keeps_namesakes_of_different_groups() {
    let app = start(MockConfig::empty()).await;
    let body = config_with(
        vec![
            grouped("users", "team-a"),
            grouped("users", "team-b"),
            mock_service("users", "c"),
        ],
        vec![group("team-a", "tmaaa"), group("team-b", "tmbbb")],
    );
    let status = app.send(reqwest::Method::PUT, "/config", Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(app.slots().await.len(), 3);
}

#[tokio::test]
async fn an_import_obeys_the_group_rules_of_the_api() {
    let app = start(MockConfig::empty()).await;
    let same_name = config_with(
        vec![],
        vec![group("team-a", "tmaaa"), group("Team-A", "tmbbb")],
    );
    assert_eq!(
        app.send(reqwest::Method::PUT, "/config", Some(same_name))
            .await,
        StatusCode::CONFLICT,
        "group names are unique whatever the case"
    );
    let same_code = config_with(
        vec![],
        vec![group("team-a", "tmaaa"), group("team-b", "TMAAA")],
    );
    assert_eq!(
        app.send(reqwest::Method::PUT, "/config", Some(same_code))
            .await,
        StatusCode::CONFLICT,
        "two groups with one URL code would share their URLs"
    );
    let bad_code = config_with(vec![], vec![group("team-a", "a/b")]);
    assert_eq!(
        app.send(reqwest::Method::PUT, "/config", Some(bad_code))
            .await,
        StatusCode::BAD_REQUEST,
        "a group code is a URL segment: 5 letters or digits"
    );
    let named_like_a_code = config_with(
        vec![mock_service("tmaaa", "x")],
        vec![group("team-a", "tmaaa")],
    );
    assert_eq!(
        app.send(reqwest::Method::PUT, "/config", Some(named_like_a_code))
            .await,
        StatusCode::CONFLICT,
        "/tmaaa/... cannot route to an ungrouped service and to a group"
    );
    assert!(app.config().await.groups.is_empty());
}

#[tokio::test]
async fn an_import_names_its_services_as_the_api_does() {
    let app = start(MockConfig::empty()).await;
    let body = config_with(vec![mock_service(" users ", "x")], vec![]);
    assert_eq!(
        app.send(reqwest::Method::PUT, "/config", Some(body)).await,
        StatusCode::OK
    );
    assert_eq!(app.slots().await, vec![(None, "users".to_string())]);
}

#[tokio::test]
async fn a_name_that_differs_only_by_surrounding_spaces_is_the_same_service() {
    let app = start(MockConfig {
        services: vec![grouped("users", "team-a")],
        groups: vec![group("team-a", "tmaaa")],
    })
    .await;
    let padded = serde_json::to_value(grouped(" users ", "team-a")).unwrap();
    assert_eq!(
        app.send(reqwest::Method::POST, "/services", Some(padded))
            .await,
        StatusCode::CONFLICT
    );

    let other = serde_json::to_value(grouped("orders ", "team-a")).unwrap();
    assert_eq!(
        app.send(reqwest::Method::POST, "/services", Some(other))
            .await,
        StatusCode::CREATED
    );
    let renamed = serde_json::to_value(grouped("users\t", "team-a")).unwrap();
    assert_eq!(
        app.send(
            reqwest::Method::PUT,
            "/groups/team-a/services/orders",
            Some(renamed)
        )
        .await,
        StatusCode::CONFLICT,
        "the service was stored under its trimmed name, and cannot be renamed onto another one"
    );
    assert_eq!(
        app.slots().await,
        vec![
            (Some("team-a".to_string()), "orders".to_string()),
            (Some("team-a".to_string()), "users".to_string())
        ]
    );
}

#[tokio::test]
async fn deleting_a_group_never_leaves_two_ungrouped_namesakes() {
    let app = start(MockConfig {
        services: vec![
            grouped("users", "team-a"),
            grouped("orders", "team-a"),
            mock_service("users", "x"),
        ],
        groups: vec![group("team-a", "tmaaa")],
    })
    .await;
    assert_eq!(
        app.send(reqwest::Method::DELETE, "/groups/team-a", None)
            .await,
        StatusCode::CONFLICT
    );
    let config = app.config().await;
    assert_eq!(
        config.groups.len(),
        1,
        "the group stays when its deletion is refused"
    );
    assert_eq!(config.services.len(), 3);

    assert_eq!(
        app.send(
            reqwest::Method::DELETE,
            "/groups/team-a/services/users",
            None
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.send(reqwest::Method::DELETE, "/groups/team-a", None)
            .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        app.slots().await,
        vec![(None, "orders".to_string()), (None, "users".to_string())]
    );
}

#[tokio::test]
async fn deleting_a_group_never_gives_an_ungrouped_service_the_name_of_a_group_code() {
    let app = start(MockConfig {
        services: vec![grouped("tmbbb", "team-a")],
        groups: vec![group("team-a", "tmaaa"), group("team-b", "tmbbb")],
    })
    .await;
    assert_eq!(
        app.send(reqwest::Method::DELETE, "/groups/team-a", None)
            .await,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn rules_stay_unique_within_a_service_through_an_import() {
    let app = start(MockConfig::empty()).await;
    let mut service = mock_service("users", "x");
    let mut copy = service.rules[0].clone();
    copy.name = "ANY".into();
    service.rules.push(copy);
    let body = config_with(vec![service], vec![]);
    assert_eq!(
        app.send(reqwest::Method::PUT, "/config", Some(body)).await,
        StatusCode::BAD_REQUEST
    );
}
