// Observation of service-level proxy traffic (is_mocked=false), the input of rule suggestions
// (`server::suggestion`; see `intercept.rs::do_proxy` for where it plugs in). Two states, both in memory only,
// never in the configuration:
//
// - `ObservationToggle`: which services a user is observing right now, turned on and off through the API, never
//   automatically. Until someone turns it on for a service, its proxy stays pure streaming, at no extra cost.
// - `ObservationStore`: the exchanges (request and backend response) captured meanwhile, grouped by service, method,
//   literal path and SOAP operation, bounded twice (number of endpoints, exchanges per endpoint) so memory never grows unbounded.
//   Beside them, the relays that failed with their request (`FailedCall`), in a bounded list of their own: a failure never
//   pushes an exchange out, and suggestions never read one (a 502 written by Mimicway is not an answer of the backend).
//   Both are read back call by call through `calls` (the panel's list and the details of one call).
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, RwLock};

pub mod calls;

const DEFAULT_MAX_BODY_SIZE: usize = 16 * 1024;
const DEFAULT_MAX_BUFFER_SIZE: usize = 10 * 1024 * 1024;
const DEFAULT_SAMPLES_PER_KEY: usize = 8;
const DEFAULT_MAX_KEYS: usize = 200;
const DEFAULT_MAX_FAILED_CALLS: usize = 100;
/// Failed relays kept for one service, within `max_failed_calls()`: a caller sending calls that fail through one
/// observed service pushes out that service's oldest failures, never another service's.
pub(crate) const MAX_FAILED_CALLS_PER_SERVICE: usize = 20;
/// Distinct SOAP operations followed for one path of a service (one method): a service names a few dozen at most.
/// Past them, a new operation takes the place of the operation of that path updated least recently, as endpoints do in
/// the whole store: a caller inventing an operation per call turns one fixed path into this many endpoints at most,
/// and a real operation comes back as soon as it is called again.
pub(crate) const MAX_OPERATIONS_PER_PATH: usize = 32;

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

/// Failed relays kept at once, across all observed services, each with its request (bodies cut at `max_body_size()`);
/// beyond that, the oldest goes first. Apart from the exchanges, so that a backend that stays down never pushes a good
/// sample out.
pub fn max_failed_calls() -> usize {
    max_failed_calls_in(crate::settings::env)
}

fn max_failed_calls_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
    crate::settings::number(
        lookup,
        "TRAFFIC_OBSERVATION_MAX_FAILED_CALLS",
        DEFAULT_MAX_FAILED_CALLS,
    )
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
    /// The literal rest of the path after the service's listen path: what a rule's sub-path is matched against, so a
    /// rule suggested from it matches the calls it comes from. `/orders/1` and `/orders/2` are two endpoints here;
    /// suggestions group them under a route pattern (`server::suggestion`).
    pub sub_path: String,
    /// The SOAP operation the request names (`suggestion::request_operation`), `None` for any other request: the
    /// operations of one address are endpoints of their own, as paths are, so that each keeps its own samples instead
    /// of the busiest one pushing the others out.
    pub operation: Option<String>,
}

/// An exchange whose two sides were both buffered within `max_buffer_size()`. A partial one (a response too large
/// or of unknown size) is never stored (see `ProxyClient::forward_with_capture`).
#[derive(Debug, Clone, Serialize)]
pub struct ObservedExchange {
    /// Given by the store when it records the exchange (0 before): the order of every call kept, exchanges and failed
    /// relays together, and the identifier the API reads one call by.
    pub id: u64,
    pub timestamp: u64,
    /// The whole path the caller requested, service prefix included, as Mimicway received it; empty when not known.
    pub request_path: String,
    /// The query as the caller wrote it, `None` without `?`.
    pub request_query: Option<String>,
    pub request_query_params: HashMap<String, String>,
    pub request_headers: HashMap<String, String>,
    /// The request body as text, a byte that is not UTF-8 replaced (`from_utf8_lossy`): read it only when
    /// `request_body_utf8` holds, the rules that will match the request read its bytes.
    pub request_body: String,
    pub request_body_truncated: bool,
    /// The size of the whole request body, in bytes, before it was cut at `max_body_size()`.
    pub request_body_size: usize,
    /// Whether the whole request body was UTF-8 text. A body in another encoding (a SOAP envelope in ISO-8859-1) is not
    /// the text kept above: conditions read the bytes the mock receives, and no XPath condition reads a body that is
    /// not UTF-8 (`MatchEngine::extract_xpath`), so suggestions read no field and no operation in it.
    pub request_body_utf8: bool,
    pub request_content_type: Option<String>,
    /// The parameters the service's listen path captured (`/v1/users/{id}`), empty when it names none.
    pub request_path_params: HashMap<String, String>,
    /// Mimicway's own address as the caller reached it (the request's `Host`), when it sent one: an address of Mimicway
    /// in a response is not another backend (`suggestion::links`).
    pub request_host: Option<String>,
    pub response_status: u16,
    pub response_headers: HashMap<String, String>,
    pub response_body: String,
    pub response_body_truncated: bool,
    pub response_body_size: usize,
    /// Whether the whole response body was UTF-8 text, as for the request.
    pub response_body_utf8: bool,
    pub response_content_type: Option<String>,
}

impl ObservedExchange {
    // One argument per part of the exchange, request then response, as the proxy captured them.
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
            id: 0,
            timestamp: now_ms(),
            request_path: String::new(),
            request_query: None,
            request_query_params,
            request_headers: crate::server::redaction::redact_headers(&request_headers),
            request_body: req_body,
            request_body_truncated: req_truncated,
            request_body_size: request_body.len(),
            request_body_utf8: std::str::from_utf8(request_body).is_ok(),
            request_content_type,
            request_path_params: HashMap::new(),
            request_host: None,
            response_status,
            response_headers: crate::server::redaction::redact_headers(&response_headers),
            response_body: resp_body,
            response_body_truncated: resp_truncated,
            response_body_size: response_body.len(),
            response_body_utf8: std::str::from_utf8(response_body).is_ok(),
            response_content_type,
        }
    }
}

/// A relay of an observed service that got no response to hand back, kept with the request as it was buffered.
#[derive(Debug, Clone)]
pub struct FailedCall {
    pub group_name: Option<String>,
    pub service_name: String,
    pub method: String,
    /// The SOAP operation the request names, as for an exchange's endpoint.
    pub operation: Option<String>,
    /// The request, in an exchange whose response side is empty (status 0, no header, no body): the relay got none.
    /// It never enters an endpoint, so suggestions never read it.
    pub request: ObservedExchange,
    pub cause: crate::engine::FailureCause,
    /// The HTTP client's own words for the failure, without the URL.
    pub detail: Option<String>,
    /// What Mimicway answered the caller instead (502, or 504 after a timeout).
    pub status: u16,
    /// The URL the relay aimed at, its credentials removed, without the query.
    pub target: String,
}

#[derive(Default)]
struct Bucket {
    /// The endpoint's place in `key_order`: the sequence number of its last update.
    seq: u64,
    exchanges: VecDeque<ObservedExchange>,
}

impl ObservationInner {
    fn next_id(&mut self, group: Option<&str>, name: &str) -> u64 {
        let next = self
            .next_ids
            .entry((group.map(str::to_string), name.to_string()))
            .or_insert(0);
        *next += 1;
        *next - 1
    }
}

#[derive(Default)]
struct ObservationInner {
    buckets: HashMap<ObservationKey, Bucket>,
    // Endpoints by last update (first = least recent), to evict when `max_keys()` is reached. Recording an exchange
    // moves its endpoint to a new, larger sequence number, found through its bucket: no scan of the order.
    key_order: BTreeMap<u64, ObservationKey>,
    next_seq: u64,
    /// The next identifier of a call of each service, exchange or failure: in the order its calls came, and telling
    /// nothing of the other services (how many calls they had). Forgotten with the service's calls.
    next_ids: HashMap<ServiceKey, u64>,
    /// Oldest first, `max_failed_calls()` at most, `MAX_FAILED_CALLS_PER_SERVICE` of one service at most.
    failures: VecDeque<FailedCall>,
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
        self.record_within(key, exchange, max_keys(), samples_per_key());
    }

    /// `record` with its two bounds given, so that tests and the scale test (`scale_probe`) can set them without changing
    /// the environment.
    pub(crate) fn record_within(
        &self,
        key: ObservationKey,
        exchange: ObservedExchange,
        max_keys: usize,
        samples_per_key: usize,
    ) {
        self.record_within_if(key, exchange, max_keys, samples_per_key, || true);
    }

    /// `record_within`, only if `accept` holds once the store is locked: nothing that forgets a service can come
    /// between the check and the recording (`ObservationState::record`).
    fn record_within_if(
        &self,
        key: ObservationKey,
        mut exchange: ObservedExchange,
        max_keys: usize,
        samples_per_key: usize,
        accept: impl FnOnce() -> bool,
    ) {
        let mut guard = self.inner.write().unwrap();
        if !accept() {
            return;
        }
        let inner = &mut *guard;
        let seq = inner.next_seq;
        inner.next_seq += 1;
        exchange.id = inner.next_id(key.group_name.as_deref(), &key.service_name);

        // A new operation of a path that already has `MAX_OPERATIONS_PER_PATH`: the operation of that path updated least
        // recently leaves its place. Counted over the endpoints followed, `max_keys` at most, and only when the endpoint
        // is new.
        if key.operation.is_some() && !inner.buckets.contains_key(&key) {
            let operations: Vec<(u64, &ObservationKey)> = inner
                .buckets
                .iter()
                .filter(|(k, _)| {
                    k.operation.is_some()
                        && k.sub_path == key.sub_path
                        && k.method == key.method
                        && k.service_name == key.service_name
                        && k.group_name == key.group_name
                })
                .map(|(k, bucket)| (bucket.seq, k))
                .collect();
            if operations.len() >= MAX_OPERATIONS_PER_PATH
                && let Some(&(oldest, _)) = operations.iter().min_by_key(|(seq, _)| *seq)
                && let Some(evicted) = inner.key_order.remove(&oldest)
            {
                inner.buckets.remove(&evicted);
            }
        }

        let bucket = match inner.buckets.get_mut(&key) {
            Some(bucket) => {
                inner.key_order.remove(&bucket.seq);
                inner.key_order.insert(seq, key);
                bucket.seq = seq;
                bucket
            }
            None => {
                if inner.buckets.len() >= max_keys
                    && let Some((_, evicted)) = inner.key_order.pop_first()
                {
                    inner.buckets.remove(&evicted);
                }
                inner.key_order.insert(seq, key.clone());
                inner.buckets.entry(key).or_insert(Bucket {
                    seq,
                    exchanges: VecDeque::new(),
                })
            }
        };
        if bucket.exchanges.len() >= samples_per_key {
            bucket.exchanges.pop_front();
        }
        bucket.exchanges.push_back(exchange);
    }

    pub fn record_failure(&self, call: FailedCall) {
        self.record_failure_within(call, max_failed_calls());
    }

    /// `record_failure` with its bound given, like `record_within`. The exchanges are never touched.
    pub(crate) fn record_failure_within(&self, call: FailedCall, max_failed_calls: usize) {
        self.record_failure_within_if(call, max_failed_calls, || true);
    }

    fn record_failure_within_if(
        &self,
        mut call: FailedCall,
        max_failed_calls: usize,
        accept: impl FnOnce() -> bool,
    ) {
        if max_failed_calls == 0 {
            return;
        }
        let mut guard = self.inner.write().unwrap();
        if !accept() {
            return;
        }
        let inner = &mut *guard;
        call.request.id = inner.next_id(call.group_name.as_deref(), &call.service_name);
        let same_service =
            |f: &FailedCall| f.group_name == call.group_name && f.service_name == call.service_name;
        let of_service = inner.failures.iter().filter(|f| same_service(f)).count();
        if of_service >= MAX_FAILED_CALLS_PER_SERVICE.min(max_failed_calls) {
            if let Some(oldest) = inner.failures.iter().position(same_service) {
                inner.failures.remove(oldest);
            }
        } else if inner.failures.len() >= max_failed_calls {
            inner.failures.pop_front();
        }
        inner.failures.push_back(call);
    }

    /// The calls kept for a service, exchanges and failed relays together, the most recent first.
    pub fn calls(&self, group: Option<&str>, service_name: &str) -> Vec<calls::CallSummary> {
        let inner = self.inner.read().unwrap();
        let exchanges = inner
            .buckets
            .iter()
            .filter(|(k, _)| k.group_name.as_deref() == group && k.service_name == service_name)
            .flat_map(|(key, bucket)| {
                bucket
                    .exchanges
                    .iter()
                    .map(move |e| calls::CallSummary::of_exchange(key, e))
            });
        let failures = inner
            .failures
            .iter()
            .filter(|f| f.group_name.as_deref() == group && f.service_name == service_name)
            .map(calls::CallSummary::of_failure);
        let mut list: Vec<calls::CallSummary> = exchanges.chain(failures).collect();
        list.sort_unstable_by_key(|call| std::cmp::Reverse(call.id));
        list
    }

    /// One call kept for a service, whole as it was kept; `None` when no call of that service has this identifier (any
    /// more).
    pub fn call(
        &self,
        group: Option<&str>,
        service_name: &str,
        id: u64,
    ) -> Option<calls::CallDetail> {
        let inner = self.inner.read().unwrap();
        let exchange = inner
            .buckets
            .iter()
            .filter(|(k, _)| k.group_name.as_deref() == group && k.service_name == service_name)
            .find_map(|(key, bucket)| {
                let exchange = bucket.exchanges.iter().find(|e| e.id == id)?;
                Some(calls::CallDetail::of_exchange(key, exchange))
            });
        exchange.or_else(|| {
            inner
                .failures
                .iter()
                .filter(|f| f.group_name.as_deref() == group && f.service_name == service_name)
                .find(|f| f.request.id == id)
                .map(calls::CallDetail::of_failure)
        })
    }

    /// The exchanges kept for an endpoint, oldest first; empty for an unknown endpoint.
    pub fn observations(&self, key: &ObservationKey) -> Vec<ObservedExchange> {
        let inner = self.inner.read().unwrap();
        inner
            .buckets
            .get(key)
            .map(|b| b.exchanges.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// How many endpoints are followed (diagnosis, tests).
    pub fn key_count(&self) -> usize {
        self.inner.read().unwrap().buckets.len()
    }

    /// Forgets every call kept for a service, exchanges and failed relays, and only those.
    pub fn forget_service(&self, group: Option<&str>, service_name: &str) {
        let mut guard = self.inner.write().unwrap();
        let inner = &mut *guard;
        let belongs =
            |k: &ObservationKey| k.group_name.as_deref() == group && k.service_name == service_name;
        inner.buckets.retain(|k, _| !belongs(k));
        inner.key_order.retain(|_, k| !belongs(k));
        inner
            .failures
            .retain(|f| !(f.group_name.as_deref() == group && f.service_name == service_name));
        inner
            .next_ids
            .retain(|(g, n), _| !(g.as_deref() == group && n == service_name));
    }

    pub fn clear(&self) {
        *self.inner.write().unwrap() = ObservationInner::default();
    }

    /// Keeps the calls of the services `kept` names (group, name), and forgets every other one.
    pub fn retain_services(&self, kept: impl Fn(Option<&str>, &str) -> bool) {
        let mut guard = self.inner.write().unwrap();
        let inner = &mut *guard;
        inner
            .buckets
            .retain(|k, _| kept(k.group_name.as_deref(), &k.service_name));
        inner
            .key_order
            .retain(|_, k| kept(k.group_name.as_deref(), &k.service_name));
        inner
            .failures
            .retain(|f| kept(f.group_name.as_deref(), &f.service_name));
        inner.next_ids.retain(|(g, n), _| kept(g.as_deref(), n));
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

/// The services under observation, turned on and off by a user, never automatically. Each time observation is turned
/// on for a service it gets a generation of its own, drawn from one counter: a relay reads it when it starts, and what
/// it captured is kept only if the generation is still the same when it ends (`ObservationState::record`), so that a
/// relay still running when observation stopped, or started again, keeps nothing.
#[derive(Clone)]
pub struct ObservationToggle {
    active: Arc<RwLock<HashMap<ServiceKey, u64>>>,
    next_generation: Arc<std::sync::atomic::AtomicU64>,
}

impl ObservationToggle {
    pub fn new() -> Self {
        Self {
            active: Arc::new(RwLock::new(HashMap::new())),
            next_generation: Arc::default(),
        }
    }

    fn key(group: Option<&str>, name: &str) -> ServiceKey {
        (group.map(|g| g.to_string()), name.to_string())
    }

    /// Turns observation on; a service already observed keeps its generation.
    pub fn enable(&self, group: Option<&str>, name: &str) {
        let generation = self
            .next_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.active
            .write()
            .unwrap()
            .entry(Self::key(group, name))
            .or_insert(generation);
    }

    pub fn disable(&self, group: Option<&str>, name: &str) {
        self.active.write().unwrap().remove(&Self::key(group, name));
    }

    /// The generation of a service's observation, `None` when it is not observed, without allocating: called on every
    /// request of a service-level proxy, observed or not. A linear scan rather than a lookup, which would need a
    /// `(Option<String>, String)` key built per call; users observe a handful of services at most.
    pub fn generation(&self, group: Option<&str>, name: &str) -> Option<u64> {
        self.active
            .read()
            .unwrap()
            .iter()
            .find(|((g, n), _)| g.as_deref() == group && n == name)
            .map(|(_, generation)| *generation)
    }

    pub fn is_enabled(&self, group: Option<&str>, name: &str) -> bool {
        self.generation(group, name).is_some()
    }

    /// The services under observation (status API, diagnosis).
    pub fn active_services(&self) -> Vec<ServiceKey> {
        self.active.read().unwrap().keys().cloned().collect()
    }

    /// When the whole configuration is replaced: otherwise a service created again afterwards would start out observed.
    pub fn clear_all(&self) {
        self.active.write().unwrap().clear();
    }

    /// Keeps observing the services `kept` names (group, name), and stops every other one.
    pub fn retain(&self, kept: impl Fn(Option<&str>, &str) -> bool) {
        self.active
            .write()
            .unwrap()
            .retain(|(g, n), _| kept(g.as_deref(), n));
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

    /// Stops observing a service and forgets what was observed, when a user turns observation off (a change of the
    /// configuration goes through `follow`): suggestions only ever read the traffic observed since observation was
    /// last turned on, and a service created later under the same name never inherits another one's traffic.
    pub fn stop(&self, group: Option<&str>, name: &str) {
        self.toggle.disable(group, name);
        self.store.forget_service(group, name);
    }

    /// `stop` for every service, when the whole configuration is reset.
    pub fn stop_all(&self) {
        self.toggle.clear_all();
        self.store.clear();
    }

    /// Keeps an exchange of a relay that started while observation had `generation`, only if it still has: the store is
    /// locked while the switch is read, and turning observation off forgets the store after the switch, so a relay
    /// that ends after observation stopped, or started again, keeps nothing. Lock order: store, then switch; nothing
    /// holds the switch while it waits for the store.
    pub fn record(&self, key: ObservationKey, exchange: ObservedExchange, generation: u64) {
        let (group, name) = (key.group_name.clone(), key.service_name.clone());
        self.store
            .record_within_if(key, exchange, max_keys(), samples_per_key(), || {
                self.toggle.generation(group.as_deref(), &name) == Some(generation)
            });
    }

    /// `record` for a relay that failed.
    pub fn record_failure(&self, call: FailedCall, generation: u64) {
        let (group, name) = (call.group_name.clone(), call.service_name.clone());
        self.store
            .record_failure_within_if(call, max_failed_calls(), || {
                self.toggle.generation(group.as_deref(), &name) == Some(generation)
            });
    }

    /// Follows a change of the configuration, whatever made it (`MockStore::on_change`): a service whose identity
    /// (group, name) is gone, renamed, moved, deleted, or left out of its group when the group is renamed or deleted,
    /// stops being observed and its calls are forgotten, under that identity and under any new one; a replacement of
    /// the whole configuration (import, restore, reset) forgets everything. A service created later under an identity
    /// that was forgotten starts out unobserved, with nothing of the traffic of the one before.
    pub fn follow(&self, change: crate::store::ConfigChange<'_>) {
        match change {
            crate::store::ConfigChange::Replaced => self.stop_all(),
            crate::store::ConfigChange::Edited { after, .. } => {
                let exists = |group: Option<&str>, name: &str| {
                    after
                        .services
                        .iter()
                        .any(|s| s.group_name.as_deref() == group && s.name == name)
                };
                self.toggle.retain(exists);
                self.store.retain_services(exists);
            }
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
            operation: None,
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
    fn forgetting_a_service_keeps_the_others_and_their_eviction_order() {
        let store = ObservationStore::new();
        let other = |path: &str| ObservationKey {
            service_name: "other".into(),
            ..sample_key(path)
        };
        store.record(sample_key("/a"), sample_exchange(b"x"));
        store.record(other("/b"), sample_exchange(b"x"));
        store.record(
            ObservationKey {
                group_name: Some("g".into()),
                ..sample_key("/c")
            },
            sample_exchange(b"x"),
        );

        store.forget_service(None, "svc");

        assert!(store.observations(&sample_key("/a")).is_empty());
        assert_eq!(
            store.key_count(),
            2,
            "same name in a group, other service kept"
        );
        // The forgotten endpoint left the eviction order too: filling the store evicts the others, never a ghost.
        for i in 0..DEFAULT_MAX_KEYS - 2 {
            store.record(other(&format!("/fill{i}")), sample_exchange(b"x"));
        }
        assert_eq!(store.key_count(), DEFAULT_MAX_KEYS);
        store.record(other("/one-more"), sample_exchange(b"x"));
        assert_eq!(store.key_count(), DEFAULT_MAX_KEYS);
        assert!(
            store.observations(&other("/b")).is_empty(),
            "the oldest left"
        );
    }

    /// The eviction order as it was first written, a scan of a list by last update: the reference the store must agree
    /// with, whatever it uses to find an endpoint's place.
    #[derive(Default)]
    struct ReferenceStore {
        order: VecDeque<ObservationKey>,
        buckets: HashMap<ObservationKey, VecDeque<String>>,
    }

    impl ReferenceStore {
        fn record(&mut self, key: ObservationKey, body: String, max_keys: usize, cap: usize) {
            if !self.buckets.contains_key(&key)
                && self.buckets.len() >= max_keys
                && let Some(evicted) = self.order.pop_front()
            {
                self.buckets.remove(&evicted);
            }
            if let Some(pos) = self.order.iter().position(|k| k == &key) {
                self.order.remove(pos);
            }
            self.order.push_back(key.clone());
            let bucket = self.buckets.entry(key).or_default();
            if bucket.len() >= cap {
                bucket.pop_front();
            }
            bucket.push_back(body);
        }

        fn forget_service(&mut self, (group, name): (Option<&str>, &str)) {
            let belongs =
                |k: &ObservationKey| k.group_name.as_deref() == group && k.service_name == name;
            self.buckets.retain(|k, _| !belongs(k));
            self.order.retain(|k| !belongs(k));
        }
    }

    #[derive(Debug, Clone)]
    enum Op {
        Record { service: usize, path: usize },
        Forget { service: usize },
    }

    /// A service is its group and its name: the same name in a group is another service.
    const SERVICES: [(Option<&str>, &str); 3] =
        [(None, "svc"), (Some("g"), "svc"), (None, "other")];
    const PATHS: usize = 5;

    fn key_of(service: usize, path: usize) -> ObservationKey {
        let (group, name) = SERVICES[service];
        ObservationKey {
            group_name: group.map(str::to_string),
            service_name: name.into(),
            ..sample_key(&format!("/{path}"))
        }
    }

    fn arb_op() -> impl proptest::strategy::Strategy<Value = Op> {
        use proptest::prelude::*;
        prop_oneof![
            8 => (0..SERVICES.len(), 0..PATHS).prop_map(|(service, path)| Op::Record { service, path }),
            1 => (0..SERVICES.len()).prop_map(|service| Op::Forget { service }),
        ]
    }

    #[test]
    fn eviction_agrees_with_a_scan_of_the_endpoints_by_last_update() {
        use proptest::prelude::*;
        use proptest::test_runner::TestRunner;
        let config = crate::test_seed::config(512, 0x6f62_7365_7276_6564);
        let strategy = (0..5usize, 0..4usize, prop::collection::vec(arb_op(), 0..40));
        let result = TestRunner::new(config).run(&strategy, |(max_keys, cap, ops)| {
            let store = ObservationStore::new();
            let mut reference = ReferenceStore::default();
            for (i, op) in ops.iter().enumerate() {
                match *op {
                    Op::Record { service, path } => {
                        let body = i.to_string();
                        store.record_within(
                            key_of(service, path),
                            sample_exchange(body.as_bytes()),
                            max_keys,
                            cap,
                        );
                        reference.record(key_of(service, path), body, max_keys, cap);
                    }
                    Op::Forget { service } => {
                        let (group, name) = SERVICES[service];
                        store.forget_service(group, name);
                        reference.forget_service(SERVICES[service]);
                    }
                }
                prop_assert_eq!(store.key_count(), reference.buckets.len());
                for service in 0..SERVICES.len() {
                    for path in 0..PATHS {
                        let key = key_of(service, path);
                        let kept: Vec<String> = store
                            .observations(&key)
                            .into_iter()
                            .map(|e| e.response_body)
                            .collect();
                        let expected: Vec<String> = reference
                            .buckets
                            .get(&key)
                            .map(|b| b.iter().cloned().collect())
                            .unwrap_or_default();
                        prop_assert_eq!(kept, expected, "{:?} after {:?}", key, &ops[..=i]);
                    }
                }
            }
            Ok(())
        });
        if let Err(e) = result {
            panic!("{e}");
        }
    }

    #[test]
    fn stopping_turns_observation_off_and_forgets_the_service() {
        let state = ObservationState::new();
        state.toggle.enable(None, "svc");
        state.store.record(sample_key("/a"), sample_exchange(b"x"));
        state.stop(None, "svc");
        assert!(!state.toggle.is_enabled(None, "svc"));
        assert_eq!(state.store.key_count(), 0);

        state.toggle.enable(Some("g"), "svc");
        state.store.record(sample_key("/a"), sample_exchange(b"x"));
        state.stop_all();
        assert!(state.toggle.active_services().is_empty());
        assert_eq!(state.store.key_count(), 0);
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
        assert_eq!(max_failed_calls_in(vars(&[])), 100);
    }

    #[test]
    fn limits_follow_their_variables() {
        let lookup = vars(&[
            ("TRAFFIC_OBSERVATION_MAX_BODY_SIZE", "3"),
            ("TRAFFIC_OBSERVATION_MAX_BUFFER_SIZE", "4"),
            ("TRAFFIC_OBSERVATION_SAMPLES_PER_KEY", "5"),
            ("TRAFFIC_OBSERVATION_MAX_KEYS", "6"),
            ("TRAFFIC_OBSERVATION_MAX_FAILED_CALLS", "7"),
        ]);
        assert_eq!(max_body_size_in(&lookup), 3);
        assert_eq!(max_buffer_size_in(&lookup), 4);
        assert_eq!(samples_per_key_in(&lookup), 5);
        assert_eq!(max_keys_in(&lookup), 6);
        assert_eq!(max_failed_calls_in(&lookup), 7);
    }

    #[test]
    fn a_path_follows_so_many_operations_and_the_least_recent_leaves_its_place() {
        // A caller invents operations first; the real ones, called after, are each followed again as they come.
        let store = ObservationStore::new();
        let key = |operation: &str| ObservationKey {
            operation: Some(operation.to_string()),
            ..sample_key("/ws")
        };
        for invented in 0..MAX_OPERATIONS_PER_PATH {
            store.record_within(
                key(&format!("Op{invented}")),
                sample_exchange(b"<x/>"),
                1000,
                8,
            );
        }
        for real in ["GetStatus", "Book"] {
            store.record_within(key(real), sample_exchange(b"<x/>"), 1000, 8);
            store.record_within(key(real), sample_exchange(b"<x/>"), 1000, 8);
        }
        assert_eq!(store.key_count(), MAX_OPERATIONS_PER_PATH);
        assert_eq!(store.observations(&key("GetStatus")).len(), 2);
        assert_eq!(store.observations(&key("Book")).len(), 2);
        assert!(store.observations(&key("Op0")).is_empty());
        assert!(store.observations(&key("Op1")).is_empty());
        assert_eq!(store.observations(&key("Op2")).len(), 1);
        assert!(store.observations(&sample_key("/ws")).is_empty());
        // Another path has operations of its own.
        let other = ObservationKey {
            operation: Some("Op99".into()),
            ..sample_key("/ws/other")
        };
        store.record_within(other.clone(), sample_exchange(b"<x/>"), 1000, 8);
        assert_eq!(store.observations(&other).len(), 1);
        assert_eq!(store.observations(&key("Op2")).len(), 1);
    }

    fn failed_call(group: Option<&str>, name: &str, body: &[u8]) -> FailedCall {
        let mut request = ObservedExchange::new(
            HashMap::new(),
            HashMap::new(),
            body,
            None,
            0,
            HashMap::new(),
            b"",
            None,
        );
        request.request_path = format!("/{name}/ws");
        FailedCall {
            group_name: group.map(str::to_string),
            service_name: name.into(),
            method: "POST".into(),
            operation: None,
            request,
            cause: crate::engine::FailureCause::Unreachable,
            detail: Some("Connection refused".into()),
            status: 502,
            target: "http://backend.test/ws".into(),
        }
    }

    // The two lists never push each other out: a backend that stays down keeps every good sample, and a busy service
    // keeps every failure.
    #[test]
    fn failed_relays_are_bounded_apart_from_the_exchanges() {
        let store = ObservationStore::new();
        for i in 0..3 {
            store.record_within(sample_key(&format!("/{i}")), sample_exchange(b"ok"), 3, 1);
        }
        for i in 0..10 {
            store.record_failure_within(failed_call(None, "svc", i.to_string().as_bytes()), 4);
        }
        assert_eq!(store.key_count(), 3, "no exchange pushed out by a failure");
        let failures: Vec<String> = store
            .calls(None, "svc")
            .into_iter()
            .filter(|c| c.failure.is_some())
            .map(|c| store.call(None, "svc", c.id).unwrap().request.body.text)
            .collect();
        assert_eq!(
            failures,
            ["9", "8", "7", "6"],
            "the four latest, most recent first"
        );
        for i in 3..6 {
            store.record_within(sample_key(&format!("/{i}")), sample_exchange(b"ok"), 3, 1);
        }
        assert_eq!(
            store.calls(None, "svc").len(),
            3 + 4,
            "no failure pushed out by an exchange"
        );
        store.record_failure_within(failed_call(None, "svc", b"never"), 0);
        assert_eq!(
            store.calls(None, "svc").len(),
            7,
            "a bound of 0 keeps no failure"
        );
    }

    // One list per service, a service being its group and its name, exchanges and failures in the order they came.
    #[test]
    fn the_calls_of_a_service_come_most_recent_first_and_only_theirs() {
        let store = ObservationStore::new();
        let grouped = |path: &str| ObservationKey {
            group_name: Some("g".into()),
            ..sample_key(path)
        };
        store.record(sample_key("/a"), sample_exchange(b"first"));
        store.record_failure(failed_call(None, "svc", b"second"));
        store.record(grouped("/a"), sample_exchange(b"other service"));
        store.record_failure(failed_call(Some("g"), "svc", b"other service"));
        store.record(sample_key("/b"), sample_exchange(b"third"));
        store.record(sample_key("/a"), sample_exchange(b"fourth"));

        let calls = store.calls(None, "svc");
        let bodies: Vec<String> = calls
            .iter()
            .map(|c| store.call(None, "svc", c.id).unwrap().request.body.text)
            .collect();
        assert_eq!(bodies, ["fourth", "third", "second", "first"]);
        assert_eq!(calls[2].status, None);
        assert_eq!(
            calls[2].failure,
            Some(crate::engine::FailureCause::Unreachable)
        );
        assert_eq!(calls[0].status, Some(200));
        assert_eq!(calls[0].failure, None);
        assert_eq!(store.calls(Some("g"), "svc").len(), 2);
        // An identifier reads a call of the service it is asked for, never one of another service.
        for other in store.calls(Some("g"), "svc") {
            let own = store.call(Some("g"), "svc", other.id).unwrap();
            assert_eq!(own.request.body.text, "other service");
            if let Some(mine) = store.call(None, "svc", other.id) {
                assert_ne!(mine.request.body.text, "other service");
            }
        }
        assert!(store.call(None, "svc", u64::MAX).is_none());
    }

    #[test]
    fn a_failed_relay_reads_back_with_its_request_and_why_no_response_came() {
        let store = ObservationStore::new();
        store.record_failure(failed_call(None, "svc", b"<Envelope/>"));
        let id = store.calls(None, "svc")[0].id;
        let call = store.call(None, "svc", id).unwrap();
        assert!(call.response.is_none());
        let failure = call.failure.unwrap();
        assert_eq!(failure.cause, crate::engine::FailureCause::Unreachable);
        assert_eq!(failure.status, 502);
        assert_eq!(failure.detail.as_deref(), Some("Connection refused"));
        assert_eq!(failure.target, "http://backend.test/ws");
        assert_eq!(call.method, "POST");
        assert_eq!(call.request.path, "/svc/ws");
        assert_eq!(call.request.body.text, "<Envelope/>");
        assert_eq!(call.request.body.size, 11);
    }

    #[test]
    fn stopping_forgets_the_failed_relays_of_the_service_only() {
        let state = ObservationState::new();
        state.store.record_failure(failed_call(None, "svc", b"x"));
        state
            .store
            .record_failure(failed_call(Some("g"), "svc", b"x"));
        state.stop(None, "svc");
        assert!(state.store.calls(None, "svc").is_empty());
        assert_eq!(state.store.calls(Some("g"), "svc").len(), 1);
        state.stop_all();
        assert!(state.store.calls(Some("g"), "svc").is_empty());
    }

    // What the panel says of a body: its real size beside the part kept, and whether the text kept is the original.
    #[test]
    fn a_kept_body_says_its_whole_size_and_whether_it_was_utf8() {
        let mut body = vec![b'a'; DEFAULT_MAX_BODY_SIZE];
        body.extend_from_slice(b"\xE9t\xE9");
        let exchange = sample_exchange(&body);
        let store = ObservationStore::new();
        store.record(sample_key("/a"), exchange);
        let id = store.calls(None, "svc")[0].id;
        let call = store.call(None, "svc", id).unwrap();
        for kept in [&call.request.body, &call.response.as_ref().unwrap().body] {
            assert!(kept.truncated);
            assert_eq!(kept.size, DEFAULT_MAX_BODY_SIZE + 3);
            assert_eq!(kept.text.len(), DEFAULT_MAX_BODY_SIZE);
            assert!(!kept.utf8);
        }
        let plain = sample_exchange("été".as_bytes());
        assert!(plain.request_body_utf8 && plain.response_body_utf8);
        assert_eq!(plain.request_body_size, 5);
        assert_eq!(plain.response_body_size, 5);
    }

    #[test]
    fn a_relay_keeps_its_capture_only_while_its_generation_of_observation_lasts() {
        let state = ObservationState::new();
        state.toggle.enable(None, "svc");
        let first = state.toggle.generation(None, "svc").unwrap();
        state.toggle.enable(None, "svc");
        assert_eq!(
            state.toggle.generation(None, "svc"),
            Some(first),
            "turned on again: the same"
        );
        state.record(sample_key("/a"), sample_exchange(b"kept"), first);
        state.record_failure(failed_call(None, "svc", b"kept"), first);
        assert_eq!(state.store.calls(None, "svc").len(), 2);

        state.stop(None, "svc");
        state.record(sample_key("/a"), sample_exchange(b"late"), first);
        state.record_failure(failed_call(None, "svc", b"late"), first);
        assert!(
            state.store.calls(None, "svc").is_empty(),
            "stopped: nothing kept"
        );

        state.toggle.enable(None, "svc");
        let second = state.toggle.generation(None, "svc").unwrap();
        assert_ne!(second, first);
        state.record(sample_key("/a"), sample_exchange(b"late"), first);
        state.record_failure(failed_call(None, "svc", b"late"), first);
        assert!(
            state.store.calls(None, "svc").is_empty(),
            "started again: the old relay keeps nothing"
        );
        state.record(sample_key("/a"), sample_exchange(b"new"), second);
        assert_eq!(state.store.calls(None, "svc").len(), 1);
        assert_eq!(state.toggle.generation(Some("g"), "svc"), None);
    }

    // A caller flooding one observed service with calls that fail pushes out that service's oldest failures only.
    #[test]
    fn one_service_keeps_so_many_failures_and_never_pushes_out_another_ones() {
        let store = ObservationStore::new();
        for i in 0..3 {
            store.record_failure_within(failed_call(None, "quiet", i.to_string().as_bytes()), 100);
        }
        for i in 0..150 {
            store.record_failure_within(failed_call(None, "busy", i.to_string().as_bytes()), 100);
        }
        assert_eq!(store.calls(None, "quiet").len(), 3);
        let busy: Vec<String> = store
            .calls(None, "busy")
            .into_iter()
            .map(|c| store.call(None, "busy", c.id).unwrap().request.body.text)
            .collect();
        assert_eq!(busy.len(), MAX_FAILED_CALLS_PER_SERVICE);
        assert_eq!(busy[0], "149");
        assert_eq!(busy[MAX_FAILED_CALLS_PER_SERVICE - 1], "130");
        // The global bound still holds, below the share of a service too.
        let small = ObservationStore::new();
        for i in 0..10 {
            small.record_failure_within(failed_call(None, "busy", i.to_string().as_bytes()), 4);
        }
        assert_eq!(small.calls(None, "busy").len(), 4);
    }

    // Reading the calls of one service tells nothing of how many calls another one had.
    #[test]
    fn the_identifiers_of_a_service_tell_nothing_of_the_others() {
        let store = ObservationStore::new();
        let other = |path: &str| ObservationKey {
            service_name: "other".into(),
            ..sample_key(path)
        };
        for round in 0..3 {
            for i in 0..10 {
                store.record(other(&format!("/{i}")), sample_exchange(b"x"));
                store.record_failure(failed_call(None, "other", b"x"));
            }
            store.record(sample_key("/a"), sample_exchange(b"x"));
            if round == 1 {
                store.record_failure(failed_call(None, "svc", b"x"));
            }
        }
        let mut ids: Vec<u64> = store.calls(None, "svc").iter().map(|c| c.id).collect();
        ids.reverse();
        assert_eq!(
            ids,
            [0, 1, 2, 3],
            "one count per service, in the order its calls came"
        );
        // Forgotten with the service: observed again, it starts over.
        store.forget_service(None, "svc");
        store.record(sample_key("/a"), sample_exchange(b"x"));
        assert_eq!(store.calls(None, "svc")[0].id, 0);
    }
}
