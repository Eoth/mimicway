//! Workloads of the scale test (scripts/scale-test.mjs), compiled with the "scale-probe" feature only, never in the
//! shipped binary. Each family builds an input of size n, then runs the work production does on it inside `measured`,
//! the one function whose instructions Callgrind counts (`--toggle-collect`). The driver runs every family at n, 2n
//! and 4n and is red when the count grows faster than the growth written next to the family.
//!
//! Requests go through the production router, middleware included, without a socket: a step that grows with the
//! configuration anywhere on a request's way (finding the service, cloning it, matching, rendering, logging) counts.
use crate::engine::MatchEngine;
use crate::engine::matcher::{OtherRuleConflictInput, RuleConflictDraft};
use crate::models::{ConditionGroup, MockConfig};
use crate::server::observation::{ObservationKey, ObservedExchange};
use crate::server::{AppState, browser_guard, limits, ui_files};
use crate::store::MockStore;
use axum::body::Body;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use tower::ServiceExt;

/// How the count of a family may grow with n. The driver allows the growth from n to 4n that the class gives, times
/// the tolerance it states once.
#[derive(Debug, Clone, Copy)]
pub enum Growth {
    Linear,
    /// n log n: a sort, or an ordered map, when a measure shows it. A family whose count grows linearly stays linear, so
    /// that its limit stays tight.
    LinearLog,
}

impl Growth {
    pub fn name(self) -> &'static str {
        match self {
            Growth::Linear => "linear",
            Growth::LinearLog => "n log n",
        }
    }
}

pub struct Family {
    pub name: &'static str,
    /// The smallest size; the driver also runs 2n and 4n.
    pub n: usize,
    pub growth: Growth,
    pub about: &'static str,
    /// Settings the driver puts in the probe's environment, read by production code as at startup (`crate::settings`).
    pub env: &'static [(&'static str, &'static str)],
    run: fn(usize),
}

pub const FAMILIES: &[Family] = &[
    Family {
        name: "rules",
        n: 800,
        growth: Growth::Linear,
        about: "one request through the router, matched by the last of n rules of its service",
        env: &[],
        run: rules,
    },
    Family {
        name: "services",
        n: 400,
        growth: Growth::Linear,
        about: "one request through the router, for the last of n services, each in a group of its own",
        env: &[],
        run: services,
    },
    Family {
        name: "requests",
        n: 50,
        growth: Growth::Linear,
        about: "n requests through the router against the same 20 rules, each one logged (4n stays within the log's 200 entries)",
        env: &[],
        run: requests,
    },
    Family {
        name: "conflicts",
        n: 200,
        growth: Growth::Linear,
        about: "conflict search for a draft rule among n rules of its service",
        env: &[],
        run: conflicts,
    },
    Family {
        name: "load",
        n: 50,
        growth: Growth::Linear,
        about: "loading a configuration file of n services of 4 rules",
        env: &[],
        run: load,
    },
    Family {
        name: "save",
        n: 50,
        growth: Growth::Linear,
        about: "saving one change to a configuration of n services of 4 rules, backup and disk write included",
        env: &[],
        run: save,
    },
    Family {
        name: "suggestions",
        n: 200,
        growth: Growth::Linear,
        about: "suggestions from n observed endpoints of one route, 3 exchanges each",
        env: &[],
        run: suggestions,
    },
    // 4n stays under 256 values, where Rhai's string interner starts to evict (`throttle_cache`): a step at a threshold,
    // bounded per value, that the ratio would read as growth. Rhai refuses maps of more than 500 keys anyway.
    Family {
        name: "script",
        n: 60,
        growth: Growth::Linear,
        about: "one request whose rule script returns n values, all of them used by the response template",
        env: &[],
        run: script,
    },
    // The bound on followed endpoints stays above 4n, so that every exchange opens or moves an endpoint and none is
    // evicted: the family measures the eviction order kept up to date, not the eviction itself.
    Family {
        name: "observations",
        n: 200,
        growth: Growth::Linear,
        about: "n observed exchanges on n new endpoints, then one more on each of them, the last created first",
        env: &[("TRAFFIC_OBSERVATION_MAX_KEYS", "1000")],
        run: observations,
    },
    Family {
        name: "evictions",
        n: 200,
        growth: Growth::Linear,
        about: "2n observed exchanges on 2n new endpoints, with room for n: each of the last n evicts one",
        env: &[],
        run: evictions,
    },
];

/// Runs one family at size `n`: `cargo test --features scale-probe --test scale -- probe <family> <n>`.
pub fn run(family: &str, n: usize) -> Result<(), String> {
    let family = FAMILIES
        .iter()
        .find(|f| f.name == family)
        .ok_or_else(|| format!("unknown family {family}"))?;
    (family.run)(n);
    Ok(())
}

/// The only function whose instructions are counted. Never inlined, so that Callgrind sees it by name.
#[inline(never)]
fn measured(work: &mut dyn FnMut()) {
    work();
}

/// Runs `work` once uncounted, then once counted: the first run pays what a process does once (lazy statics, the heap
/// growing to its working size), which is not the growth of the work and made the counts jump from one size to the next.
fn warm_then_measure(work: &mut dyn FnMut()) {
    work();
    measured(work);
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// A fresh directory for one run, removed when the value is dropped.
struct DataDir(PathBuf);

impl DataDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "mimicway-scale-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir_all(&dir).expect("data directory");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for DataDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rule(name: &str, method: &str, sub_path: Option<&str>, conditions: Value, body: Value) -> Value {
    json!({
        "name": name, "method": method, "sub_path": sub_path, "action": "mock", "script": null,
        "conditions": conditions,
        "response": {"status": 200, "headers": [], "body": body}
    })
}

/// The rule `i` of a service: its own sub-path and a header condition. The rules before the one a request matches fail
/// on their sub-path; the matching rule reads its condition too.
fn numbered_rule(i: usize) -> Value {
    rule(
        &format!("rule-{i}"),
        "GET",
        Some(&format!("/items/{i}")),
        json!({"all_of": [{"source": {"type": "Header", "key": "x-tenant"},
                           "operator": {"type": "Eq", "value": format!("tenant-{i}")}}]}),
        json!([{"type": "Literal", "value": format!("item {i}")}]),
    )
}

fn service(name: &str, group: Option<&str>, rules: Vec<Value>) -> Value {
    json!({
        "name": name, "listen_path": "", "real_target_url": "", "is_mocked": true,
        "rewrite_directory_urls": false, "group_name": group, "wsdl_mode": "auto",
        "rules": rules
    })
}

fn config(services: Vec<Value>, groups: Vec<Value>) -> MockConfig {
    serde_json::from_value(json!({"services": services, "groups": groups})).expect("configuration")
}

/// `n` services of 4 rules each, ungrouped.
fn config_of_services(n: usize) -> MockConfig {
    let services = (0..n)
        .map(|s| {
            service(
                &format!("svc-{s}"),
                None,
                (0..4).map(numbered_rule).collect(),
            )
        })
        .collect();
    config(services, Vec::new())
}

/// The production router over `config`, with the default limits and browser guard.
fn router(rt: &tokio::runtime::Runtime, dir: &Path, config: MockConfig) -> axum::Router {
    rt.block_on(async {
        let store = MockStore::new(dir.join("mock-config.yaml")).expect("store");
        store.replace(config).await.expect("configuration saved");
        store.flush().await;
        let state = AppState {
            store,
            proxy: crate::engine::ProxyClient::new(),
            seq_counters: Default::default(),
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
            messaging: crate::messaging::MessagingState {
                message_log: crate::messaging::message_log::MessageLog::new(),
                reply_topic: None,
                publisher: crate::messaging::consumer::Publisher::None,
            },
            #[cfg(feature = "tcp-mock")]
            tcp_runtime: crate::tcp::TcpRuntime::load_and_spawn(
                dir,
                std::net::Ipv4Addr::LOCALHOST.into(),
            )
            .await,
        };
        crate::server::build_router_with(
            state,
            ui_files::UiSource::Directory(dir.join("ui")),
            browser_guard::BrowserGuard::new(""),
            limits::Limits::default(),
        )
    })
}

fn get(path: &str, headers: &[(&str, &str)]) -> axum::http::Request<Body> {
    let mut request = axum::http::Request::get(path).header("host", "127.0.0.1:7342");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(Body::empty()).expect("request")
}

/// Sends `request` and reads the whole response; panics unless it is a 200, so that a family never measures an error
/// page.
async fn send(app: &axum::Router, request: axum::http::Request<Body>) -> Vec<u8> {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(
        status,
        200,
        "the probe expects a mocked answer: {}",
        String::from_utf8_lossy(&body)
    );
    body.to_vec()
}

fn rules(n: usize) {
    let dir = DataDir::new();
    let rt = runtime();
    let app = router(
        &rt,
        dir.path(),
        config(
            vec![service("svc", None, (0..n).map(numbered_rule).collect())],
            Vec::new(),
        ),
    );
    let last = n - 1;
    let tenant = format!("tenant-{last}");
    let path = format!("/svc/items/{last}");
    warm_then_measure(&mut || {
        let body = rt.block_on(send(&app, get(&path, &[("x-tenant", &tenant)])));
        black_box(body);
    });
}

fn services(n: usize) {
    let dir = DataDir::new();
    let rt = runtime();
    let groups = (0..n)
        .map(|g| json!({"name": format!("group-{g}"), "code": format!("g{g}")}))
        .collect();
    let services = (0..n)
        .map(|s| {
            service(
                &format!("svc-{s}"),
                Some(&format!("group-{s}")),
                vec![numbered_rule(0)],
            )
        })
        .collect();
    let app = router(&rt, dir.path(), config(services, groups));
    let last = n - 1;
    let path = format!("/g{last}/svc-{last}/items/0");
    warm_then_measure(&mut || {
        let body = rt.block_on(send(&app, get(&path, &[("x-tenant", "tenant-0")])));
        black_box(body);
    });
}

fn requests(n: usize) {
    let dir = DataDir::new();
    let rt = runtime();
    let app = router(
        &rt,
        dir.path(),
        config(
            vec![service("svc", None, (0..20).map(numbered_rule).collect())],
            Vec::new(),
        ),
    );
    let calls: Vec<(String, String)> = (0..n)
        .map(|i| {
            (
                format!("/svc/items/{}", i % 20),
                format!("tenant-{}", i % 20),
            )
        })
        .collect();
    warm_then_measure(&mut || {
        rt.block_on(async {
            for (path, tenant) in &calls {
                black_box(send(&app, get(path, &[("x-tenant", tenant)])).await);
            }
        });
    });
}

fn conflicts(n: usize) {
    let rules: Vec<crate::models::Rule> = (0..n)
        .map(|i| serde_json::from_value(numbered_rule(i)).expect("rule"))
        .collect();
    let others: Vec<OtherRuleConflictInput> = rules
        .iter()
        .map(|r| OtherRuleConflictInput {
            name: &r.name,
            method: &r.method,
            sub_path: &r.sub_path,
            conditions: &r.conditions,
        })
        .collect();
    // A draft with a parameter in its sub-path, that every rule's path could overlap: each pair is really compared.
    let sub_path = Some("/items/:id".to_string());
    let conditions = ConditionGroup::default();
    let draft = RuleConflictDraft {
        method: "GET",
        sub_path: &sub_path,
        conditions: &conditions,
    };
    warm_then_measure(&mut || {
        black_box(MatchEngine::find_rule_conflicts(
            &draft,
            &others,
            others.len() / 2,
        ));
    });
}

fn load(n: usize) {
    let dir = DataDir::new();
    let yaml = serde_yaml::to_string(&config_of_services(n)).expect("yaml");
    std::fs::write(MockStore::config_file(dir.path()), yaml).expect("configuration file");
    let rt = runtime();
    warm_then_measure(&mut || {
        let store = rt
            .block_on(MockStore::load_or_init(dir.path()))
            .expect("load");
        black_box(store);
    });
}

fn save(n: usize) {
    let dir = DataDir::new();
    let rt = runtime();
    let store = rt.block_on(async {
        std::fs::write(
            MockStore::config_file(dir.path()),
            serde_yaml::to_string(&config_of_services(n)).expect("yaml"),
        )
        .expect("configuration file");
        MockStore::load_or_init(dir.path()).await.expect("load")
    });
    // Counted cold: a second change in the same burst would skip the backup, which saving pays.
    measured(&mut || {
        rt.block_on(async {
            let saved = store
                .update(|c| c.services[0].rules[0].name = "renamed".into())
                .await
                .expect("save");
            black_box(saved);
            store.flush().await;
        });
    });
}

fn suggestions(n: usize) {
    let key = |i: usize| ObservationKey {
        group_name: None,
        service_name: "svc".into(),
        method: "GET".into(),
        sub_path: format!("/orders/{i}"),
    };
    let exchange = |i: usize| {
        ObservedExchange::new(
            HashMap::new(),
            HashMap::from([("accept".to_string(), "application/json".to_string())]),
            b"",
            None,
            200,
            HashMap::from([("content-type".to_string(), "application/json".to_string())]),
            json!({"id": i, "status": "open"}).to_string().as_bytes(),
            Some("application/json".into()),
        )
    };
    let endpoints: Vec<(ObservationKey, Vec<ObservedExchange>)> = (0..n)
        .map(|i| (key(i), (0..3).map(|_| exchange(i)).collect()))
        .collect();
    warm_then_measure(&mut || {
        black_box(crate::server::suggestion::suggest_for_service(&endpoints));
    });
}

fn observations(n: usize) {
    assert!(
        crate::server::observation::max_keys() >= n,
        "TRAFFIC_OBSERVATION_MAX_KEYS must stay above the family's sizes"
    );
    // The second pass goes backwards: each endpoint it updates is then as far from the front of the eviction order as
    // from its back, so that a scan from either end costs its length.
    let order: Vec<usize> = (0..n).chain((0..n).rev()).collect();
    let mut batches = vec![observed_calls(&order), observed_calls(&order)];
    warm_then_measure(&mut || {
        let store = crate::server::observation::ObservationStore::new();
        for (key, exchange) in batches.pop().expect("a batch per run") {
            store.record(key, exchange);
        }
        assert_eq!(store.key_count(), n, "no endpoint evicted");
        black_box(store);
    });
}

/// The bound is given to the store directly rather than through the environment: it has to follow n, at half the
/// endpoints, so that a step that searches the endpoints to evict one grows with n.
fn evictions(n: usize) {
    let order: Vec<usize> = (0..2 * n).collect();
    let mut batches = vec![observed_calls(&order), observed_calls(&order)];
    warm_then_measure(&mut || {
        let store = crate::server::observation::ObservationStore::new();
        for (key, exchange) in batches.pop().expect("a batch per run") {
            store.record_within(key, exchange, n, 8);
        }
        assert_eq!(store.key_count(), n, "the first n endpoints evicted");
        black_box(store);
    });
}

/// One small exchange per endpoint number of `order`, built before the count. One batch serves the warm-up run and
/// another the counted run, each into a store of its own.
fn observed_calls(order: &[usize]) -> Vec<(ObservationKey, ObservedExchange)> {
    let exchange = ObservedExchange::new(
        HashMap::new(),
        HashMap::new(),
        b"",
        None,
        200,
        HashMap::new(),
        b"{}",
        Some("application/json".into()),
    );
    order
        .iter()
        .map(|i| {
            let key = ObservationKey {
                group_name: None,
                service_name: "svc".into(),
                method: "GET".into(),
                sub_path: format!("/orders/{i}"),
            };
            (key, exchange.clone())
        })
        .collect()
}

fn script(n: usize) {
    let dir = DataDir::new();
    let rt = runtime();
    let source = format!("let m = #{{}}; for i in 0..{n} {{ m[`v${{i}}`] = i * 2; }} m");
    let template: String = (0..n).map(|i| format!("{{{{script.v{i}}}}},")).collect();
    let mut scripted = rule(
        "scripted",
        "GET",
        None,
        json!({}),
        json!([{"type": "Template", "template": template}]),
    );
    scripted["script"] = json!(source);
    let app = router(
        &rt,
        dir.path(),
        config(vec![service("svc", None, vec![scripted])], Vec::new()),
    );
    warm_then_measure(&mut || {
        let body = rt.block_on(send(&app, get("/svc/values", &[])));
        let last = format!(",{},", 2 * (n - 1));
        assert!(
            String::from_utf8_lossy(&body).contains(&last),
            "every value rendered"
        );
        black_box(body);
    });
}
