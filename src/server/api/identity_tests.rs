//! What traffic observation keeps follows the identity of each service (group, name) through every route that writes
//! the configuration. The routes come from the source of `routes()`, compared with the table below: a route added
//! tomorrow that writes, without a line here, fails `every_writing_route_is_in_the_table`. Each route of the table runs
//! once on a server where two proxy services are observed, each with a failed call kept (one in a group, one without);
//! after it, no call is kept and no switch is on for an identity that the route removed or brought in (a service
//! renamed, moved, deleted, a group renamed or deleted), nothing at all is kept after a replacement of the whole
//! configuration, and the services the route left as they were keep what they had.
//!
//! What it does not see: a route that writes the configuration through another store than `state.store` (none
//! exists), and a route outside `api::routes()` (the router adds only `GET /runtime-config.json`).
use crate::models::{Group, MockConfig, Service};
use crate::server::test_support::{auth_disabled, serve, temp_data_dir, test_state};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// The whole configuration replaced: nothing observed carries over.
    Replaces,
    /// Writes the configuration: must succeed, so that the case tests what it says.
    Writes,
    /// May write or not, may be refused: judged all the same.
    Other,
}

#[derive(Clone)]
enum Body {
    None,
    Json(Value),
    /// The configuration in place with one more service: an import that changes it.
    ConfigWithOneMore,
}

struct Case {
    method: &'static str,
    pattern: &'static str,
    path: &'static str,
    body: Body,
    kind: Kind,
}

fn proxy(name: &str, group: Option<&str>) -> Value {
    json!({
        "name": name, "listen_path": "", "real_target_url": "http://127.0.0.1:1", "is_mocked": false,
        "rewrite_directory_urls": false, "group_name": group, "wsdl_mode": "auto", "rules": []
    })
}

fn table() -> Vec<Case> {
    use Body::{ConfigWithOneMore, Json as J, None as N};
    use Kind::*;
    let case = |method, pattern, path, body, kind| Case {
        method,
        pattern,
        path,
        body,
        kind,
    };
    let group = |name: &str| json!({"name": name, "code": "tmaaa", "admins": [], "members": []});
    vec![
        case(
            "POST",
            "/auth/login",
            "/auth/login",
            J(json!({"username": "u", "password": "p"})),
            Other,
        ),
        case(
            "POST",
            "/auth/validate",
            "/auth/validate",
            J(json!({"token": "t"})),
            Other,
        ),
        case("PUT", "/config", "/config", ConfigWithOneMore, Replaces),
        case("DELETE", "/config/reset", "/config/reset", N, Replaces),
        case(
            "POST",
            "/config/restore/{filename}",
            "/config/restore/{filename}",
            N,
            Replaces,
        ),
        case(
            "POST",
            "/services",
            "/services",
            J(proxy("stock", None)),
            Writes,
        ),
        case(
            "PUT",
            "/services/{name}",
            "/services/orders",
            J(proxy("orders-renamed", None)),
            Writes,
        ),
        case("DELETE", "/services/{name}", "/services/orders", N, Writes),
        case(
            "PUT",
            "/services/{name}/toggle",
            "/services/orders/toggle",
            J(json!({"is_mocked": true})),
            Writes,
        ),
        case(
            "POST",
            "/services/{name}/ping",
            "/services/orders/ping",
            N,
            Other,
        ),
        case(
            "PUT",
            "/services/{name}/rules/reorder",
            "/services/orders/rules/reorder",
            J(json!({"order": []})),
            Other,
        ),
        case(
            "PUT",
            "/groups/{group}/services/{name}",
            "/groups/team-a/services/billing",
            J(proxy("billing-renamed", Some("team-a"))),
            Writes,
        ),
        case(
            "DELETE",
            "/groups/{group}/services/{name}",
            "/groups/team-a/services/billing",
            N,
            Writes,
        ),
        case(
            "PUT",
            "/groups/{group}/services/{name}/toggle",
            "/groups/team-a/services/billing/toggle",
            J(json!({"is_mocked": true})),
            Writes,
        ),
        case(
            "POST",
            "/groups/{group}/services/{name}/ping",
            "/groups/team-a/services/billing/ping",
            N,
            Other,
        ),
        case(
            "PUT",
            "/groups/{group}/services/{name}/rules/reorder",
            "/groups/team-a/services/billing/rules/reorder",
            J(json!({"order": []})),
            Other,
        ),
        case(
            "POST",
            "/script/validate",
            "/script/validate",
            J(json!({"script": "1"})),
            Other,
        ),
        case("POST", "/rule-test", "/rule-test", J(json!({})), Other),
        case(
            "POST",
            "/rule-conflicts",
            "/rule-conflicts",
            J(json!({})),
            Other,
        ),
        case(
            "POST",
            "/services/{name}/observe",
            "/services/orders/observe",
            N,
            Other,
        ),
        case(
            "DELETE",
            "/services/{name}/observe",
            "/services/orders/observe",
            N,
            Other,
        ),
        case(
            "POST",
            "/groups/{group}/services/{name}/observe",
            "/groups/team-a/services/billing/observe",
            N,
            Other,
        ),
        case(
            "DELETE",
            "/groups/{group}/services/{name}/observe",
            "/groups/team-a/services/billing/observe",
            N,
            Other,
        ),
        case(
            "POST",
            "/groups",
            "/groups",
            J(json!({"name": "team-b", "code": "tmbbb", "admins": [], "members": []})),
            Writes,
        ),
        case(
            "PUT",
            "/groups/{name}",
            "/groups/team-a",
            J(group("team-z")),
            Writes,
        ),
        case("DELETE", "/groups/{name}", "/groups/team-a", N, Writes),
        case(
            "PUT",
            "/groups/{name}/members",
            "/groups/team-a/members",
            J(json!({"admins": [], "members": ["bob"]})),
            Other,
        ),
        case(
            "POST",
            "/messaging/simulate",
            "/messaging/simulate",
            J(json!({"topic": "t", "payload": "p"})),
            Other,
        ),
        case(
            "POST",
            "/tcp/services",
            "/tcp/services",
            J(json!({})),
            Other,
        ),
        case(
            "PUT",
            "/tcp/services/{name}",
            "/tcp/services/x",
            J(json!({})),
            Other,
        ),
        case(
            "DELETE",
            "/tcp/services/{name}",
            "/tcp/services/x",
            N,
            Other,
        ),
    ]
}

/// Every (method, path) of `routes()` that is not a GET, read from its source.
fn writing_routes() -> BTreeSet<(String, String)> {
    let source = include_str!("../api.rs");
    let start = source
        .find("pub fn routes() -> Router<AppState> {")
        .expect("routes()");
    let end = start
        + source[start..]
            .find("\n    router\n}")
            .expect("the end of routes()");
    let body = &source[start..end];
    let mut routes = BTreeSet::new();
    for part in body.split(".route(").skip(1) {
        let path = part.split('"').nth(1).expect("a path");
        // The methods of this route: up to the next route, or the end.
        for (word, method) in [
            ("post(", "POST"),
            ("put(", "PUT"),
            ("delete(", "DELETE"),
            ("delete_route(", "DELETE"),
        ] {
            let found = part
                .match_indices(word)
                .any(|(i, _)| !part[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_'));
            if found {
                routes.insert((method.to_string(), path.to_string()));
            }
        }
    }
    routes
}

#[test]
fn every_writing_route_is_in_the_table() {
    let in_table: BTreeSet<(String, String)> = table()
        .iter()
        .map(|c| (c.method.to_string(), c.pattern.to_string()))
        .collect();
    assert_eq!(writing_routes(), in_table);
}

type Identity = (Option<String>, String);

fn identities(config: &MockConfig) -> BTreeSet<Identity> {
    config
        .services
        .iter()
        .map(|s| (s.group_name.clone(), s.name.clone()))
        .collect()
}

fn service(value: Value) -> Service {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn no_call_outlives_the_identity_of_its_service_whatever_the_route() {
    for case in table() {
        let config = MockConfig {
            services: vec![
                service(proxy("orders", None)),
                service(proxy("billing", Some("team-a"))),
            ],
            groups: vec![Group {
                name: "team-a".into(),
                code: "tmaaa".into(),
                admins: vec![],
                members: vec![],
            }],
        };
        let data_dir = temp_data_dir("identity");
        let state = test_state(&data_dir, config.clone(), auth_disabled()).await;
        let store = state.store.clone();
        let observation = state.observation.clone();
        let root = serve(crate::server::build_router(state, &data_dir)).await;
        let client = reqwest::Client::new();
        let case_name = format!("{} {}", case.method, case.path);

        // A backup to restore: a change written over the file in place.
        let mut changed = config.clone();
        changed.services.push(service(proxy("spare", None)));
        store.replace(changed).await.unwrap();
        store.flush().await;
        store.replace(config.clone()).await.unwrap();
        store.flush().await;
        let backup = store.list_backups().await.unwrap()[0].filename.clone();

        for (observe, call) in [
            ("/services/orders/observe", "/orders/x"),
            (
                "/groups/team-a/services/billing/observe",
                "/tmaaa/billing/x",
            ),
        ] {
            let on = client
                .post(format!("{root}/api{observe}"))
                .send()
                .await
                .unwrap();
            assert_eq!(on.status().as_u16(), 204, "{case_name}: {observe}");
            let relayed = client
                .post(format!("{root}{call}"))
                .body("<call/>")
                .send()
                .await
                .unwrap();
            assert_eq!(relayed.status().as_u16(), 502, "{case_name}: {call}");
        }
        let before = store.snapshot().await;
        for (group, name) in identities(&before) {
            assert_eq!(
                observation.store.calls(group.as_deref(), &name).len(),
                1,
                "{case_name}: {name}"
            );
        }

        let path = case.path.replace("{filename}", &backup);
        let mut request = client.request(case.method.parse().unwrap(), format!("{root}/api{path}"));
        request = match &case.body {
            Body::None => request,
            Body::Json(value) => request.json(value),
            Body::ConfigWithOneMore => {
                let mut value = serde_json::to_value(&*before).unwrap();
                value["services"]
                    .as_array_mut()
                    .unwrap()
                    .push(proxy("stock", None));
                request.json(&value)
            }
        };
        let status = request.send().await.unwrap().status();
        if case.kind != Kind::Other {
            assert!(status.is_success(), "{case_name}: {status}");
        }

        let after = store.snapshot().await;
        let (old, new) = (identities(&before), identities(&after));
        let stopped_by_the_user = case.path.ends_with("/observe") && case.method == "DELETE";
        for identity in old.union(&new) {
            let (group, name) = (identity.0.as_deref(), identity.1.as_str());
            let changed =
                case.kind == Kind::Replaces || !old.contains(identity) || !new.contains(identity);
            let calls = observation.store.calls(group, name).len();
            let observed = observation.toggle.is_enabled(group, name);
            if changed {
                assert_eq!(
                    (calls, observed),
                    (0, false),
                    "{case_name}: {identity:?} kept after it changed"
                );
            } else if !(stopped_by_the_user && case.path.contains(name)) {
                assert_eq!(
                    (calls, observed),
                    (1, true),
                    "{case_name}: {identity:?} lost while it stayed"
                );
            }
        }
    }
}
