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

    /// A management API call (`path` after `/api`) that must succeed.
    async fn api(&self, method: reqwest::Method, path: &str, body: Option<Value>) {
        let mut request = self
            .client
            .request(method, format!("{}/api{path}", self.root));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let resp = request.send().await.unwrap();
        assert!(resp.status().is_success(), "{path}: {}", resp.status());
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

/// A proxied service observed through three calls that would give a suggestion.
async fn observed_three_times(app: &App, service: &str) {
    app.observe(service).await;
    for _ in 0..crate::server::suggestion::min_samples() {
        assert_eq!(app.call(&format!("/{service}/orders")).await.0, 200);
    }
    assert_eq!(app.suggestions(service).await.len(), 1);
}

#[tokio::test]
async fn observing_again_starts_from_nothing() {
    // Suggestions read the traffic observed since observation was last turned on.
    let target = backend(axum::Router::new().fallback(|| async { "order list" })).await;
    let app = start(vec![proxied("shop", "", &target)]).await;
    observed_three_times(&app, "shop").await;

    app.api(reqwest::Method::DELETE, "/services/shop/observe", None)
        .await;
    app.observe("shop").await;
    assert!(app.suggestions("shop").await.is_empty());
}

#[tokio::test]
async fn a_service_created_again_does_not_inherit_the_traffic_of_a_deleted_one() {
    // The observed calls outlived their service: a new service of the same name, maybe in front of another backend,
    // was suggested rules from the deleted one's traffic.
    let target = backend(axum::Router::new().fallback(|| async { "order list" })).await;
    let service = proxied("shop", "", &target);
    let app = start(vec![service.clone()]).await;
    observed_three_times(&app, "shop").await;

    app.api(reqwest::Method::DELETE, "/services/shop", None)
        .await;
    app.api(reqwest::Method::POST, "/services", Some(json!(service)))
        .await;
    app.observe("shop").await;
    assert!(app.suggestions("shop").await.is_empty());
}

#[tokio::test]
async fn a_reset_forgets_every_observed_call() {
    let target = backend(axum::Router::new().fallback(|| async { "order list" })).await;
    let service = proxied("shop", "", &target);
    let app = start(vec![service.clone()]).await;
    observed_three_times(&app, "shop").await;

    app.api(reqwest::Method::DELETE, "/config/reset", None)
        .await;
    app.api(reqwest::Method::POST, "/services", Some(json!(service)))
        .await;
    app.observe("shop").await;
    assert!(app.suggestions("shop").await.is_empty());
}

#[tokio::test]
async fn a_directory_called_once_per_name_gives_one_rule_that_answers_any_name() {
    // A service directory: `/service/{hash}/{name}` answers the address of the named service. Each name is called once:
    // no literal path reaches the three samples a suggestion waits for, the route does, and since the answers differ
    // only by the name they repeat, one rule covers every name, including one never observed.
    async fn lookup(
        axum::extract::Path((_hash, name)): axum::extract::Path<(String, String)>,
    ) -> axum::response::Response {
        let body = format!("<lookup><url>http://backend.internal:8080/ws/{name}</url></lookup>");
        ([("content-type", "text/xml")], body).into_response()
    }
    use axum::response::IntoResponse;
    let target =
        backend(axum::Router::new().route("/service/{hash}/{name}", axum::routing::get(lookup)))
            .await;
    let app = start(vec![proxied("directory", "", &target)]).await;
    app.observe("directory").await;
    for name in ["restOrders", "restStock", "restBilling"] {
        let (status, _) = app
            .call(&format!("/directory/service/9f86d081884c7d65/{name}"))
            .await;
        assert_eq!(status, 200);
    }

    let suggestions = app.suggestions("directory").await;
    assert_eq!(suggestions.len(), 1, "{suggestions:?}");
    let s = &suggestions[0];
    assert_eq!(s["outcome"], "Echo");
    assert_eq!(s["sub_path"], "/service/{hash}/{name}");
    assert_eq!(s["path_count"], 3);
    assert_eq!(
        s["rule"]["echo"],
        json!({"type": "PathParam", "key": "name"})
    );

    app.save_as_mock("directory", &[&s["rule"]]).await;
    assert_eq!(
        app.call("/directory/service/0123456789abcdef/restInvoices")
            .await,
        (
            200,
            "<lookup><url>http://backend.internal:8080/ws/restInvoices</url></lookup>".into()
        )
    );
}

#[tokio::test]
async fn a_directory_rule_sends_the_application_to_the_service_in_front_of_the_backend_it_names() {
    // The directory answers the backend's address. Once a proxy service stands in front of that backend, the suggested
    // rule answers Mimicway's address of that service instead, and following it reaches the backend through Mimicway.
    let orders = backend(axum::Router::new().fallback(|| async { "order 1" })).await;
    let body = format!(r#"{{"orders":"{orders}/orders/1"}}"#);
    let directory = backend(axum::Router::new().fallback(move || {
        let body = body.clone();
        async move { body }
    }))
    .await;
    let app = start(vec![proxied("directory", "", &directory)]).await;
    app.observe("directory").await;
    for _ in 0..3 {
        assert_eq!(app.call("/directory/lookup").await.0, 200);
    }

    let linked: Vec<Value> = app
        .client
        .get(format!(
            "{}/api/services/directory/linked-backends",
            app.root
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(linked.len(), 1, "{linked:?}");
    assert_eq!(linked[0]["origin"], json!(orders));
    assert_eq!(linked[0]["served_by"], json!([]));
    assert_eq!(linked[0]["draft"]["real_target_url"], json!(orders));
    assert_eq!(linked[0]["draft"]["name"], json!("backend-127-0-0-1"));

    app.api(
        reqwest::Method::POST,
        "/services",
        Some(serde_json::to_value(proxied("backend-127-0-0-1", "", &orders)).unwrap()),
    )
    .await;
    let suggestions = app.suggestions("directory").await;
    let rule = &suggestions[0]["rule"];
    assert_eq!(
        rule["redirected_to"],
        json!([{ "name": "backend-127-0-0-1", "group_name": null }])
    );

    app.save_as_mock("directory", &[rule]).await;
    let (status, answer) = app.call("/directory/lookup").await;
    assert_eq!(status, 200);
    let mimicway = app.root.trim_start_matches("http://");
    assert_eq!(
        answer,
        format!(r#"{{"orders":"http://{mimicway}/backend-127-0-0-1/orders/1"}}"#)
    );
    assert_eq!(
        app.call("/backend-127-0-0-1/orders/1").await,
        (200, "order 1".to_string())
    );
}
