//! Traffic suggestions through the real router and a real proxied backend: calls observed, the suggestions the API
//! returns, then the suggested rules saved and the same calls replayed in mock mode, with no backend left to answer.
use crate::models::{MockConfig, Service};
use crate::server::test_support::{auth_disabled, serve, temp_data_dir, test_state};
use serde_json::{Value, json};

struct App {
    root: String,
    client: reqwest::Client,
}

async fn start(services: Vec<Service>) -> App {
    let data_dir = temp_data_dir("suggestions");
    let config = MockConfig {
        services,
        groups: vec![],
    };
    let state = test_state(&data_dir, config, auth_disabled()).await;
    App {
        root: serve(crate::server::build_router(state, &data_dir)).await,
        client: reqwest::Client::new(),
    }
}

/// A pure proxy service in front of `target`.
fn proxied(name: &str, listen_path: &str, target: &str) -> Service {
    serde_json::from_value(json!({
        "name": name, "listen_path": listen_path, "real_target_url": target, "is_mocked": false,
        "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto", "rules": []
    }))
    .unwrap()
}

impl App {
    async fn observe(&self, service: &str) {
        let resp = self
            .client
            .post(format!("{}/api/services/{service}/observe", self.root))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);
    }

    /// `GET` on a service URL: the status and the body.
    async fn call(&self, path: &str) -> (u16, String) {
        let resp = self
            .client
            .get(format!("{}{path}", self.root))
            .send()
            .await
            .unwrap();
        (resp.status().as_u16(), resp.text().await.unwrap())
    }

    async fn suggestions(&self, service: &str) -> Vec<Value> {
        self.client
            .get(format!("{}/api/services/{service}/suggestions", self.root))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    /// Saves `rules` on the service as the rule form would (name given, the rest as suggested), switches it to mock
    /// mode and removes its target: what answers afterwards can only be the rules.
    async fn save_as_mock(&self, service: &str, rules: &[&Value]) {
        let url = format!("{}/api/services/{service}", self.root);
        let mut svc: Value = self
            .client
            .get(&url)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        svc["rules"] = rules
            .iter()
            .enumerate()
            .map(|(i, rule)| {
                json!({
                    "name": format!("suggested-{i}"), "method": rule["method"], "sub_path": rule["sub_path"],
                    "action": "mock", "pre_script": null, "script": null, "post_script": null,
                    "conditions": {"all_of": rule["condition"].as_object().map_or(vec![], |_| vec![rule["condition"].clone()]), "any_of": []},
                    "response": rule["response"],
                })
            })
            .collect();
        svc["is_mocked"] = json!(true);
        svc["real_target_url"] = json!("");
        let resp = self.client.put(&url).json(&svc).send().await.unwrap();
        assert!(resp.status().is_success(), "{}", resp.text().await.unwrap());
    }
}

/// A backend on a free local port, answering with `app`; returns its root URL.
async fn backend(app: axum::Router) -> String {
    serve(app).await
}

#[tokio::test]
async fn a_rule_suggested_under_a_listen_path_matches_the_calls_it_comes_from() {
    // The listen path `/v1/*` takes `/v1` off every call before a rule's sub-path is matched. Observations were keyed by
    // the whole path after the service name, so the suggested rule (`/v1/orders`) never matched the calls it came from.
    let target = backend(axum::Router::new().fallback(|| async { "order list" })).await;
    let app = start(vec![proxied("shop", "/v1/*", &target)]).await;
    app.observe("shop").await;
    for _ in 0..crate::server::suggestion::min_samples() {
        assert_eq!(
            app.call("/shop/v1/orders").await,
            (200, "order list".into())
        );
    }

    let suggestions = app.suggestions("shop").await;
    assert_eq!(suggestions.len(), 1, "{suggestions:?}");
    let rule = &suggestions[0]["rule"];
    assert_eq!(rule["sub_path"], "/orders");

    app.save_as_mock("shop", &[rule]).await;
    assert_eq!(
        app.call("/shop/v1/orders").await,
        (200, "order list".into())
    );
}
