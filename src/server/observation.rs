// Observation of service-level proxy traffic (is_mocked=false), the input of rule suggestions
// (`server::suggestion`; see `intercept.rs::do_proxy` for where it plugs in). Two states, both in memory only,
// never in the configuration:
//
// - `ObservationToggle`: which services a user is observing right now, turned on and off through the API, never
//   automatically. Until someone turns it on for a service, its proxy stays pure streaming, at no extra cost.
// - `ObservationStore`: the exchanges (request and backend response) captured meanwhile, grouped by service, method
//   and literal path, bounded twice (number of endpoints, exchanges per endpoint) so memory never grows unbounded.
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};

const DEFAULT_MAX_BODY_SIZE: usize = 16 * 1024;
const DEFAULT_MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024;
const DEFAULT_SAMPLES_PER_KEY: usize = 8;
const DEFAULT_MAX_KEYS: usize = 200;

/// Largest body kept in an `ObservedExchange`, in bytes (request and response truncated separately), like
/// `request_log::max_body_size` and `message_log::max_body_size`.
pub fn max_body_size() -> usize {
    max_body_size_in(crate::settings::env)
}

fn max_body_size_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
    crate::settings::number(
        lookup,
        "TRAFFIC_OBSERVATION_MAX_BODY_SIZE",
        DEFAULT_MAX_BODY_SIZE,
    )
}

/// Largest body, in bytes, that an exchange may have to be captured, judged on its Content-Length: a larger body, or
/// one of unknown size, is streamed as usual and the exchange is not observed. The same cap as the request body that
/// matching buffers (`intercept.rs`, 10 MiB).
pub fn max_buffer_size() -> usize {
    max_buffer_size_in(crate::settings::env)
}

fn max_buffer_size_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
    crate::settings::number(
        lookup,
        "TRAFFIC_OBSERVATION_MAX_BUFFER_SIZE",
        DEFAULT_MAX_BUFFER_SIZE,
    )
}

/// Exchanges kept per endpoint (service, method, path); beyond that, the oldest goes first, as in `RequestLog`.
pub fn samples_per_key() -> usize {
    samples_per_key_in(crate::settings::env)
}

fn samples_per_key_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
    crate::settings::number(
        lookup,
        "TRAFFIC_OBSERVATION_SAMPLES_PER_KEY",
        DEFAULT_SAMPLES_PER_KEY,
    )
}

/// Endpoints followed at once, across all observed services; beyond that, the endpoint updated least recently makes
/// room for the new one.
pub fn max_keys() -> usize {
    max_keys_in(crate::settings::env)
}

fn max_keys_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
    crate::settings::number(lookup, "TRAFFIC_OBSERVATION_MAX_KEYS", DEFAULT_MAX_KEYS)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn truncate_body(bytes: &[u8]) -> (String, bool) {
    let max = max_body_size();
    let truncated = bytes.len() > max;
    let slice = &bytes[..bytes.len().min(max)];
    (String::from_utf8_lossy(slice).into_owned(), truncated)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ObservationKey {
    pub group_name: Option<String>,
    pub service_name: String,
    pub method: String,
    /// The literal path after the service prefix. A service-level proxy has no rule patterns, so `/orders/1` and
    /// `/orders/2` are two endpoints for now (path parameters are not inferred).
    pub sub_path: String,
}

/// An exchange whose two sides were both buffered within `max_buffer_size()`. A partial one (a response too large
/// or of unknown size) is never stored (see `ProxyClient::forward_with_capture`).
#[derive(Debug, Clone, Serialize)]
pub struct ObservedExchange {
    pub timestamp: u64,
    pub request_query_params: HashMap<String, String>,
    pub request_headers: HashMap<String, String>,
    pub request_body: String,
    pub request_body_truncated: bool,
    pub request_content_type: Option<String>,
    pub response_status: u16,
    pub response_headers: HashMap<String, String>,
    pub response_body: String,
    pub response_body_truncated: bool,
    pub response_content_type: Option<String>,
}

impl ObservedExchange {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_query_params: HashMap<String, String>,
        request_headers: HashMap<String, String>,
        request_body: &[u8],
        request_content_type: Option<String>,
        response_status: u16,
        response_headers: HashMap<String, String>,
        response_body: &[u8],
        response_content_type: Option<String>,
    ) -> Self {
        let (req_body, req_truncated) = truncate_body(request_body);
        let (resp_body, resp_truncated) = truncate_body(response_body);
        Self {
            timestamp: now_ms(),
            request_query_params,
            request_headers: crate::server::redaction::redact_headers(&request_headers),
            request_body: req_body,
            request_body_truncated: req_truncated,
            request_content_type,
            response_status,
            response_headers: crate::server::redaction::redact_headers(&response_headers),
            response_body: resp_body,
            response_body_truncated: resp_truncated,
            response_content_type,
        }
    }
}

#[derive(Default)]
struct ObservationInner {
    buckets: HashMap<ObservationKey, VecDeque<ObservedExchange>>,
    // Endpoints by last update (front = least recent), to evict when `max_keys()` is reached; recording an exchange
    // moves its endpoint to the back.
    key_order: VecDeque<ObservationKey>,
}

/// The bounded store of observed exchanges, by service, method and path (see the top of this file for both bounds).
#[derive(Clone)]
pub struct ObservationStore {
    inner: Arc<RwLock<ObservationInner>>,
}

impl ObservationStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ObservationInner::default())),
        }
    }

    pub fn record(&self, key: ObservationKey, exchange: ObservedExchange) {
        let mut inner = self.inner.write().unwrap();
        let is_new_key = !inner.buckets.contains_key(&key);

        if is_new_key
            && inner.buckets.len() >= max_keys()
            && let Some(evicted) = inner.key_order.pop_front()
        {
            inner.buckets.remove(&evicted);
        }

        if let Some(pos) = inner.key_order.iter().position(|k| k == &key) {
            inner.key_order.remove(pos);
        }
        inner.key_order.push_back(key.clone());

        let bucket = inner.buckets.entry(key).or_default();
        let cap = samples_per_key();
        if bucket.len() >= cap {
            bucket.pop_front();
        }
        bucket.push_back(exchange);
    }

    /// The exchanges kept for an endpoint, oldest first; empty for an unknown endpoint.
    pub fn observations(&self, key: &ObservationKey) -> Vec<ObservedExchange> {
        let inner = self.inner.read().unwrap();
        inner
            .buckets
            .get(key)
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// How many endpoints are followed (diagnosis, tests).
    pub fn key_count(&self) -> usize {
        self.inner.read().unwrap().buckets.len()
    }

    /// Every endpoint followed for a service, so that suggestions (`server::suggestion`) know which method and path to
    /// examine.
    pub fn keys_for_service(&self, group: Option<&str>, service_name: &str) -> Vec<ObservationKey> {
        let inner = self.inner.read().unwrap();
        inner
            .buckets
            .keys()
            .filter(|k| k.group_name.as_deref() == group && k.service_name == service_name)
            .cloned()
            .collect()
    }
}

impl Default for ObservationStore {
    fn default() -> Self {
        Self::new()
    }
}

/// A service's identity (group name, name), as `service_matches` in api.rs defines it.
type ServiceKey = (Option<String>, String);

/// The services under observation, turned on and off by a user, never automatically.
#[derive(Clone)]
pub struct ObservationToggle {
    active: Arc<RwLock<HashSet<ServiceKey>>>,
}

impl ObservationToggle {
    pub fn new() -> Self {
        Self {
            active: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn key(group: Option<&str>, name: &str) -> ServiceKey {
        (group.map(|g| g.to_string()), name.to_string())
    }

    pub fn enable(&self, group: Option<&str>, name: &str) {
        self.active.write().unwrap().insert(Self::key(group, name));
    }

    pub fn disable(&self, group: Option<&str>, name: &str) {
        self.active.write().unwrap().remove(&Self::key(group, name));
    }

    /// Whether a service is observed, without allocating: called on every request of a service-level proxy, observed or
    /// not. A linear scan rather than `HashSet::contains`, which would need a `(Option<String>, String)` key built per
    /// call; users observe a handful of services at most.
    pub fn is_enabled(&self, group: Option<&str>, name: &str) -> bool {
        self.active
            .read()
            .unwrap()
            .iter()
            .any(|(g, n)| g.as_deref() == group && n == name)
    }

    /// The services under observation (status API, diagnosis).
    pub fn active_services(&self) -> Vec<ServiceKey> {
        self.active.read().unwrap().iter().cloned().collect()
    }

    /// Only for `reset_config`, which deletes every service: otherwise a service created again afterwards would start
    /// out observed.
    pub fn clear_all(&self) {
        self.active.write().unwrap().clear();
    }
}

impl Default for ObservationToggle {
    fn default() -> Self {
        Self::new()
    }
}

/// Both observation states in one `AppState` field, like `messaging::MessagingState`.
#[derive(Clone)]
pub struct ObservationState {
    pub toggle: ObservationToggle,
    pub store: ObservationStore,
}

impl ObservationState {
    pub fn new() -> Self {
        Self {
            toggle: ObservationToggle::new(),
            store: ObservationStore::new(),
        }
    }
}

impl Default for ObservationState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::vars;

    fn sample_key(path: &str) -> ObservationKey {
        ObservationKey {
            group_name: None,
            service_name: "svc".into(),
            method: "GET".into(),
            sub_path: path.into(),
        }
    }

    fn sample_exchange(body: &[u8]) -> ObservedExchange {
        ObservedExchange::new(
            HashMap::new(),
            HashMap::new(),
            body,
            Some("application/json".into()),
            200,
            HashMap::new(),
            body,
            Some("application/json".into()),
        )
    }

    #[test]
    fn toggle_starts_disabled() {
        let toggle = ObservationToggle::new();
        assert!(!toggle.is_enabled(None, "svc"));
    }

    #[test]
    fn toggle_enable_disable_round_trip() {
        let toggle = ObservationToggle::new();
        toggle.enable(None, "svc");
        assert!(toggle.is_enabled(None, "svc"));
        toggle.disable(None, "svc");
        assert!(!toggle.is_enabled(None, "svc"));
    }

    #[test]
    fn toggle_distinguishes_group_scope() {
        let toggle = ObservationToggle::new();
        toggle.enable(Some("teamA"), "svc");
        assert!(toggle.is_enabled(Some("teamA"), "svc"));
        assert!(!toggle.is_enabled(None, "svc"));
        assert!(!toggle.is_enabled(Some("teamB"), "svc"));
    }

    #[test]
    fn toggle_active_services_lists_enabled_only() {
        let toggle = ObservationToggle::new();
        toggle.enable(None, "svc-a");
        toggle.enable(Some("g"), "svc-b");
        let mut active = toggle.active_services();
        active.sort();
        assert_eq!(
            active,
            vec![
                (None, "svc-a".to_string()),
                (Some("g".to_string()), "svc-b".to_string())
            ]
        );
    }

    #[test]
    fn toggle_clear_all_disables_every_service() {
        let toggle = ObservationToggle::new();
        toggle.enable(None, "svc-a");
        toggle.enable(Some("g"), "svc-b");
        toggle.clear_all();
        assert!(toggle.active_services().is_empty());
        assert!(!toggle.is_enabled(None, "svc-a"));
        assert!(!toggle.is_enabled(Some("g"), "svc-b"));
    }

    #[test]
    fn store_records_and_returns_observations_in_order() {
        let store = ObservationStore::new();
        let key = sample_key("/orders/1");
        store.record(key.clone(), sample_exchange(b"{\"a\":1}"));
        store.record(key.clone(), sample_exchange(b"{\"a\":2}"));
        let observed = store.observations(&key);
        assert_eq!(observed.len(), 2);
        assert_eq!(observed[0].response_body, "{\"a\":1}");
        assert_eq!(observed[1].response_body, "{\"a\":2}");
    }

    #[test]
    fn store_unknown_key_returns_empty() {
        let store = ObservationStore::new();
        assert!(store.observations(&sample_key("/unknown")).is_empty());
    }

    // The bounds below are the defaults: tests never change the process environment (see crate::settings).

    #[test]
    fn store_bounds_samples_per_key() {
        let store = ObservationStore::new();
        let key = sample_key("/orders/1");
        for i in 0..=DEFAULT_SAMPLES_PER_KEY {
            store.record(key.clone(), sample_exchange(i.to_string().as_bytes()));
        }
        let observed = store.observations(&key);
        assert_eq!(
            observed.len(),
            DEFAULT_SAMPLES_PER_KEY,
            "capped at the sample limit, the oldest evicted"
        );
        assert_eq!(observed[0].response_body, "1");
        assert_eq!(
            observed[DEFAULT_SAMPLES_PER_KEY - 1].response_body,
            DEFAULT_SAMPLES_PER_KEY.to_string()
        );
    }

    #[test]
    fn store_bounds_distinct_keys() {
        let store = ObservationStore::new();
        for i in 0..=DEFAULT_MAX_KEYS {
            store.record(sample_key(&format!("/{i}")), sample_exchange(b"x"));
        }
        assert_eq!(
            store.key_count(),
            DEFAULT_MAX_KEYS,
            "capped at the endpoint limit"
        );
        assert!(
            store.observations(&sample_key("/0")).is_empty(),
            "the endpoint updated least recently must be evicted"
        );
        assert!(
            !store
                .observations(&sample_key(&format!("/{DEFAULT_MAX_KEYS}")))
                .is_empty()
        );
    }

    #[test]
    fn store_touching_existing_key_protects_it_from_eviction() {
        let store = ObservationStore::new();
        for i in 0..DEFAULT_MAX_KEYS {
            store.record(sample_key(&format!("/{i}")), sample_exchange(b"x"));
        }
        // Touch /0 again: it becomes the most recent, so /1 is the oldest and goes instead of /0.
        store.record(sample_key("/0"), sample_exchange(b"again"));
        store.record(sample_key("/new"), sample_exchange(b"x"));
        assert!(!store.observations(&sample_key("/0")).is_empty());
        assert!(store.observations(&sample_key("/1")).is_empty());
    }

    #[test]
    fn exchange_truncates_bodies_beyond_max_size() {
        let mut body = vec![b'0'; DEFAULT_MAX_BODY_SIZE];
        body.extend_from_slice(b"beyond");
        let exchange = sample_exchange(&body);
        assert!(exchange.request_body_truncated);
        assert_eq!(exchange.request_body.len(), DEFAULT_MAX_BODY_SIZE);
        assert!(exchange.response_body_truncated);
        assert_eq!(exchange.response_body.len(), DEFAULT_MAX_BODY_SIZE);
    }

    #[test]
    fn limits_default_without_their_variables() {
        assert_eq!(max_body_size_in(vars(&[])), 16 * 1024);
        assert_eq!(max_buffer_size_in(vars(&[])), 10 * 1024 * 1024);
        assert_eq!(samples_per_key_in(vars(&[])), 8);
        assert_eq!(max_keys_in(vars(&[])), 200);
    }

    #[test]
    fn limits_follow_their_variables() {
        let lookup = vars(&[
            ("TRAFFIC_OBSERVATION_MAX_BODY_SIZE", "3"),
            ("TRAFFIC_OBSERVATION_MAX_BUFFER_SIZE", "4"),
            ("TRAFFIC_OBSERVATION_SAMPLES_PER_KEY", "5"),
            ("TRAFFIC_OBSERVATION_MAX_KEYS", "6"),
        ]);
        assert_eq!(max_body_size_in(&lookup), 3);
        assert_eq!(max_buffer_size_in(&lookup), 4);
        assert_eq!(samples_per_key_in(&lookup), 5);
        assert_eq!(max_keys_in(&lookup), 6);
    }
}
