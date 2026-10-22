//! Authorization of the management API with authentication enabled, through the real router and real tokens.
//! Users: `root` is a super-admin; `alice` administers `team-a`, `bob` is a member of it; `carol` administers
//! `team-b`.
use crate::auth::keycloak::KeycloakClient;
use crate::auth::test_realm::FakeRealm;
use crate::models::{Group, MockConfig, Service};
use crate::server::test_support::{mock_service, serve, temp_data_dir, test_state};
use reqwest::StatusCode;
use serde_json::{Value, json};

struct App {
    root: String,
    realm: FakeRealm,
    client: reqwest::Client,
}

fn group(name: &str, code: &str, admins: &[&str], members: &[&str]) -> Group {
    Group {
        name: name.into(),
        code: code.into(),
        admins: admins.iter().map(|s| s.to_string()).collect(),
        members: members.iter().map(|s| s.to_string()).collect(),
    }
}

fn grouped(name: &str, group: &str) -> Service {
    Service {
        group_name: Some(group.into()),
        ..mock_service(name, name)
    }
}

async fn start() -> App {
    let realm = FakeRealm::start().await;
    let auth_config = realm.auth_config(vec!["root".into()]);
    let config = MockConfig {
        services: vec![grouped("billing", "team-a"), grouped("stock", "team-b")],
        groups: vec![
            group("team-a", "tmaaa", &["alice"], &["bob"]),
            group("team-b", "tmbbb", &["carol"], &[]),
        ],
    };
    let data_dir = temp_data_dir("authz");
    let mut state = test_state(&data_dir, config, auth_config.clone()).await;
    state.keycloak = Some(KeycloakClient::new(auth_config));
    let root = serve(crate::server::build_router(state, &data_dir)).await;
    App {
        root,
        realm,
        client: reqwest::Client::new(),
    }
}

impl App {
    async fn call(
        &self,
        user: &str,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = self
            .client
            .request(method, format!("{}/api{path}", self.root))
            .bearer_auth(self.realm.token_for(user));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let resp = request.send().await.unwrap();
        let status = resp.status();
        (status, resp.json().await.unwrap_or(Value::Null))
    }

    async fn get(&self, user: &str, path: &str) -> (StatusCode, Value) {
        self.call(user, reqwest::Method::GET, path, None).await
    }

    async fn put(&self, user: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(user, reqwest::Method::PUT, path, Some(body))
            .await
    }

    async fn post(&self, user: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(user, reqwest::Method::POST, path, Some(body))
            .await
    }
}

fn service_json(name: &str, group: Option<&str>) -> Value {
    let mut service = serde_json::to_value(mock_service(name, name)).unwrap();
    service["group_name"] = json!(group);
    service
}

#[tokio::test]
async fn a_service_moves_only_where_its_editor_may_create_services() {
    let app = start().await;
    let path = "/groups/team-a/services/billing";
    // A member edits the service but cannot move it, an admin of team-a cannot push it into team-b nor out of
    // any group.
    let (status, _) = app
        .put("bob", path, service_json("billing", Some("team-a")))
        .await;
    assert_eq!(status, StatusCode::OK);
    for (user, target) in [
        ("bob", Some("team-b")),
        ("alice", Some("team-b")),
        ("alice", None),
    ] {
        let (status, _) = app.put(user, path, service_json("billing", target)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{user} -> {target:?}");
    }
    let (status, _) = app
        .put("root", path, service_json("billing", Some("team-b")))
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_rename_cannot_take_the_name_of_another_service_of_the_group() {
    let app = start().await;
    let (status, _) = app
        .post(
            "alice",
            "/services",
            service_json("invoices", Some("team-a")),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = app
        .put(
            "alice",
            "/groups/team-a/services/invoices",
            service_json("billing", Some("team-a")),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn services_cannot_join_an_undefined_group() {
    let app = start().await;
    let (status, _) = app
        .post("root", "/services", service_json("x", Some("ghost")))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn group_codes_and_ungrouped_service_names_never_collide() {
    let app = start().await;
    let (status, _) = app
        .post("root", "/services", service_json("tmaaa", None))
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "ungrouped service named like a group code"
    );
    let (status, _) = app
        .post("root", "/services", service_json("orders", None))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = app
        .post(
            "root",
            "/groups",
            json!({"name": "team-c", "code": "orders", "admins": [], "members": []}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "6 characters");
    let (status, _) = app
        .post("root", "/services", service_json("ordrs", None))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = app
        .post(
            "root",
            "/groups",
            json!({"name": "team-c", "code": "ordrs", "admins": [], "members": []}),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "group code named like an ungrouped service"
    );
}

#[tokio::test]
async fn updating_a_group_validates_it_and_keeps_its_services() {
    let app = start().await;
    let renamed =
        json!({"name": "team-alpha", "code": "alpha", "admins": ["alice"], "members": ["bob"]});
    let (status, _) = app.put("alice", "/groups/team-a", renamed).await;
    assert_eq!(status, StatusCode::OK);
    let (status, service) = app.get("bob", "/groups/team-alpha/services/billing").await;
    assert_eq!(status, StatusCode::OK, "the service followed its group");
    assert_eq!(service["group_name"], "team-alpha");

    let (status, _) = app
        .put(
            "alice",
            "/groups/team-alpha",
            json!({"name": "team-alpha", "code": "tmbbb", "admins": ["alice"], "members": []}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "code of team-b");
    let (status, _) = app
        .put(
            "alice",
            "/groups/team-alpha",
            json!({"name": " ", "code": "alpha", "admins": ["alice"], "members": []}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = app
        .put(
            "carol",
            "/groups/team-alpha",
            json!({"name": "mine", "code": "mines", "admins": ["carol"], "members": []}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_configuration_and_the_log_only_show_what_the_user_can_access() {
    let app = start().await;
    let (_, config) = app.get("bob", "/config").await;
    let names: Vec<&str> = config["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["billing"]);
    assert_eq!(config["groups"].as_array().unwrap().len(), 1);
    let (_, everything) = app.get("root", "/config").await;
    assert_eq!(everything["services"].as_array().unwrap().len(), 2);

    let traffic = app
        .client
        .get(format!("{}/tmaaa/billing/anything", app.root))
        .header("authorization", "Bearer team-a-secret")
        .send()
        .await
        .unwrap();
    assert_eq!(traffic.text().await.unwrap(), "billing");
    let (_, carol_log) = app.get("carol", "/logs").await;
    assert_eq!(carol_log.as_array().unwrap().len(), 0);
    let (_, bob_log) = app.get("bob", "/logs").await;
    assert_eq!(bob_log.as_array().unwrap().len(), 1);
    assert_eq!(bob_log[0]["group_name"], "team-a");
}

#[tokio::test]
async fn concurrent_creations_of_one_service_create_it_once() {
    let app = start().await;
    let attempts =
        (0..10).map(|_| app.post("root", "/services", service_json("once", Some("team-a"))));
    let statuses: Vec<StatusCode> = futures_util::future::join_all(attempts)
        .await
        .into_iter()
        .map(|(status, _)| status)
        .collect();
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CREATED)
            .count(),
        1,
        "{statuses:?}"
    );
    let (_, config) = app.get("root", "/config").await;
    let count = config["services"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["name"] == "once")
        .count();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn creating_returns_the_service_of_the_requested_group() {
    let app = start().await;
    let mut other = service_json("stock", Some("team-a"));
    other["real_target_url"] = json!("");
    let (status, created) = app.post("root", "/services", other).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["group_name"], "team-a", "not the team-b namesake");
}

#[cfg(feature = "messaging-kafka")]
#[tokio::test]
async fn kafka_log_and_simulation_are_reserved_to_super_admins() {
    let app = start().await;
    let message = json!({"topic": "orders", "payload": "{}", "headers": {}});
    for user in ["alice", "bob"] {
        assert_eq!(
            app.get(user, "/messaging/logs").await.0,
            StatusCode::FORBIDDEN,
            "{user}"
        );
        let (status, _) = app.post(user, "/messaging/simulate", message.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{user}");
    }
    assert_eq!(app.get("root", "/messaging/logs").await.0, StatusCode::OK);
    let (status, _) = app.post("root", "/messaging/simulate", message).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[cfg(feature = "tcp-mock")]
#[tokio::test]
async fn raw_tcp_mocks_are_reserved_to_super_admins() {
    let app = start().await;
    let tcp_service = |name: &str| {
        json!({
            "name": name,
            "listen_port": 0,
            "rules": [{"name": "ping", "matcher": {"type": "Any"}, "response_hex": "706f6e67"}],
        })
    };
    let (status, _) = app
        .post("root", "/tcp/services", tcp_service("heartbeat"))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    for user in ["alice", "bob"] {
        assert_eq!(
            app.get(user, "/tcp/services").await.0,
            StatusCode::FORBIDDEN,
            "{user}"
        );
        assert_eq!(
            app.get(user, "/tcp/status").await.0,
            StatusCode::FORBIDDEN,
            "{user}"
        );
        let (status, _) = app.post(user, "/tcp/services", tcp_service("beacon")).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{user}");
        let (status, _) = app
            .put(user, "/tcp/services/heartbeat", tcp_service("heartbeat"))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{user}");
        let (status, _) = app
            .call(
                user,
                reqwest::Method::DELETE,
                "/tcp/services/heartbeat",
                None,
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{user}");
    }
    let (status, services) = app.get("root", "/tcp/services").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(services[0]["name"], "heartbeat");
    assert_eq!(app.get("root", "/tcp/status").await.0, StatusCode::OK);
    let (status, _) = app
        .put("root", "/tcp/services/heartbeat", tcp_service("heartbeat"))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = app
        .call(
            "root",
            reqwest::Method::DELETE,
            "/tcp/services/heartbeat",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}
