use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};

const MAX_ENTRIES: usize = 200;
const DEFAULT_MAX_BODY_SIZE: usize = 16 * 1024;

/// Largest body kept in `CapturedRequest::body`, in bytes (`REQUEST_LOG_MAX_BODY_SIZE`, 16 KiB by default, like
/// `messaging::message_log::max_body_size`). Bodies up to 10 MiB are buffered for matching; keeping them whole in
/// 200 entries would use memory for nothing the rule tester needs.
pub fn max_body_size() -> usize {
    max_body_size_in(crate::settings::env)
}

fn max_body_size_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
    crate::settings::number(lookup, "REQUEST_LOG_MAX_BODY_SIZE", DEFAULT_MAX_BODY_SIZE)
}

/// A request Mimicway really received, kept so that the rule tester can replay a rule being edited against real
/// traffic, read-only. `path_params` holds the service-level parameters only, before the matching rule's sub-path
/// added its own: the tester recomputes the sub-path parameters for the draft rule, which may differ from the rule
/// that matched then.
///
/// Only requests handled by rules (mocked, unmatched, proxied by a rule) have one: their body is already buffered
/// for matching, so keeping it is a copy, not a new capture. A service-level proxy streams without buffering, and
/// building one there would add a full read of the body that does not happen today.
#[derive(Debug, Clone, Serialize)]
pub struct CapturedRequest {
    pub remaining_path: String,
    pub path_params: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub body_truncated: bool,
    pub content_type: Option<String>,
}

impl CapturedRequest {
    pub fn from_request_data(req: &crate::engine::matcher::RequestData) -> Self {
        let max = max_body_size();
        let truncated = req.body.len() > max;
        let slice = &req.body[..req.body.len().min(max)];
        Self {
            remaining_path: req.remaining_path.clone(),
            path_params: req.path_params.clone(),
            query_params: req.query_params.clone(),
            headers: crate::server::redaction::redact_headers(&req.headers),
            body: String::from_utf8_lossy(slice).into_owned(),
            body_truncated: truncated,
            content_type: req.content_type.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub timestamp: u64,
    pub service_name: String,
    /// The service's group, `None` when ungrouped: with the name, what identifies the service.
    pub group_name: Option<String>,
    pub method: String,
    pub path: String,
    pub mode: String,
    pub rule_matched: Option<String>,
    pub target_url: Option<String>,
    pub status: u16,
    pub captured: Option<CapturedRequest>,
    /// For a response sent as server-sent events: what the stream has sent so far, updated as it goes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamRecord>,
}

/// The part of a log entry that a stream of events fills while it runs: the entry is written once, when the stream
/// opens, and reading the log shows how far it got.
#[derive(Debug, Clone, Default)]
pub struct StreamRecord(Arc<std::sync::Mutex<StreamProgress>>);

#[derive(Debug, Default, Serialize)]
pub struct StreamProgress {
    /// The events of the rule, the cut included when there is one.
    pub planned: usize,
    pub sent_count: usize,
    /// The events sent, in order, while their data fits in `max_body_size()` bytes; `sent_count` goes on counting.
    pub sent: Vec<SentEvent>,
    pub sent_truncated: bool,
    pub outcome: StreamOutcome,
}

#[derive(Debug, Serialize)]
pub struct SentEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamOutcome {
    /// Events remain to be sent.
    #[default]
    Open,
    /// Every event was sent and the response ended.
    Ended,
    /// The connection was dropped where the rule cuts the stream.
    Cut,
    /// The client went away before the end.
    ClientGone,
}

impl StreamRecord {
    pub fn new(planned: usize) -> Self {
        Self(Arc::new(std::sync::Mutex::new(StreamProgress {
            planned,
            ..StreamProgress::default()
        })))
    }

    pub fn sent(&self, event: Option<&str>, data: &str) {
        let mut progress = self.0.lock().unwrap_or_else(|e| e.into_inner());
        progress.sent_count += 1;
        let kept: usize = progress.sent.iter().map(|e| e.data.len()).sum();
        if progress.sent_truncated || kept + data.len() > max_body_size() {
            progress.sent_truncated = true;
            return;
        }
        progress.sent.push(SentEvent {
            event: event.map(str::to_string),
            data: data.to_string(),
        });
    }

    /// Records how the stream finished; the first outcome stays, so that the end seen by the stream wins over the
    /// drop that follows it.
    pub fn finish(&self, outcome: StreamOutcome) {
        let mut progress = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if progress.outcome == StreamOutcome::Open {
            progress.outcome = outcome;
        }
    }

    pub fn outcome(&self) -> StreamOutcome {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).outcome
    }
}

impl Serialize for StreamRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .serialize(serializer)
    }
}

#[derive(Clone)]
pub struct RequestLog {
    entries: Arc<RwLock<VecDeque<LogEntry>>>,
}

impl RequestLog {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_ENTRIES))),
        }
    }

    pub fn push(&self, entry: LogEntry) {
        let mut entries = self.entries.write().unwrap();
        if entries.len() >= MAX_ENTRIES {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        let entries = self.entries.read().unwrap();
        entries.iter().rev().take(limit).cloned().collect()
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub fn log_mock(
        &self,
        service: &crate::models::Service,
        method: &str,
        path: &str,
        rule: &str,
        status: u16,
        captured: Option<CapturedRequest>,
    ) {
        self.push(LogEntry {
            timestamp: Self::now_ms(),
            service_name: service.name.clone(),
            group_name: service.group_name.clone(),
            method: method.into(),
            path: path.into(),
            mode: "mock".into(),
            rule_matched: Some(rule.into()),
            target_url: None,
            status,
            captured,
            stream: None,
        });
    }

    /// Logs a mocked request answered with a stream of events, once, and returns the record the stream fills.
    #[allow(clippy::too_many_arguments)]
    pub fn log_stream(
        &self,
        service: &crate::models::Service,
        method: &str,
        path: &str,
        rule: &str,
        status: u16,
        captured: Option<CapturedRequest>,
        planned: usize,
    ) -> StreamRecord {
        let record = StreamRecord::new(planned);
        self.push(LogEntry {
            timestamp: Self::now_ms(),
            service_name: service.name.clone(),
            group_name: service.group_name.clone(),
            method: method.into(),
            path: path.into(),
            mode: "mock".into(),
            rule_matched: Some(rule.into()),
            target_url: None,
            status,
            captured,
            stream: Some(record.clone()),
        });
        record
    }

    pub fn log_proxy(
        &self,
        service: &crate::models::Service,
        method: &str,
        path: &str,
        target: &str,
        status: u16,
        captured: Option<CapturedRequest>,
    ) {
        self.push(LogEntry {
            timestamp: Self::now_ms(),
            service_name: service.name.clone(),
            group_name: service.group_name.clone(),
            method: method.into(),
            path: path.into(),
            mode: "proxy".into(),
            rule_matched: None,
            target_url: Some(crate::server::redaction::redact_url_credentials(target)),
            status,
            captured,
            stream: None,
        });
    }

    pub fn log_no_rule(
        &self,
        service: &crate::models::Service,
        method: &str,
        path: &str,
        captured: Option<CapturedRequest>,
    ) {
        self.push(LogEntry {
            timestamp: Self::now_ms(),
            service_name: service.name.clone(),
            group_name: service.group_name.clone(),
            method: method.into(),
            path: path.into(),
            mode: "no-rule".into(),
            rule_matched: None,
            target_url: None,
            status: 404,
            captured,
            stream: None,
        });
    }
}

impl Default for RequestLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> crate::models::Service {
        crate::server::test_support::mock_service("svc", "")
    }
    use crate::engine::matcher::RequestData;
    use crate::settings::vars;

    fn req_data(body: &[u8]) -> RequestData {
        RequestData {
            query_params: HashMap::from([("id".to_string(), "42".to_string())]),
            headers: HashMap::from([("x-env".to_string(), "prod".to_string())]),
            body: body.to_vec(),
            content_type: Some("application/json".into()),
            path_params: HashMap::from([("svcParam".to_string(), "abc".to_string())]),
            method: "GET".into(),
            remaining_path: "/orders/1".into(),
        }
    }

    #[test]
    fn max_body_size_default_is_16kb() {
        assert_eq!(max_body_size_in(vars(&[])), 16 * 1024);
    }

    #[test]
    fn max_body_size_from_env() {
        let lookup = vars(&[("REQUEST_LOG_MAX_BODY_SIZE", "10")]);
        assert_eq!(max_body_size_in(lookup), 10);
    }

    #[test]
    fn captured_request_preserves_full_detail_under_limit() {
        let data = req_data(b"{\"a\":1}");
        let captured = CapturedRequest::from_request_data(&data);
        assert_eq!(captured.body, "{\"a\":1}");
        assert!(!captured.body_truncated);
        assert_eq!(captured.query_params.get("id").unwrap(), "42");
        assert_eq!(captured.path_params.get("svcParam").unwrap(), "abc");
        assert_eq!(captured.remaining_path, "/orders/1");
        assert_eq!(captured.content_type.as_deref(), Some("application/json"));
    }

    #[test]
    fn captured_request_truncates_body_beyond_max_size() {
        let mut body = vec![b'0'; DEFAULT_MAX_BODY_SIZE];
        body.extend_from_slice(b"beyond");
        let captured = CapturedRequest::from_request_data(&req_data(&body));
        assert!(captured.body_truncated);
        assert_eq!(captured.body.len(), DEFAULT_MAX_BODY_SIZE);
        assert!(!captured.body.contains("beyond"));
    }

    #[test]
    fn log_mock_stores_captured_detail() {
        let log = RequestLog::new();
        let captured = CapturedRequest::from_request_data(&req_data(b"body"));
        log.log_mock(
            &svc(),
            "GET",
            "/svc/orders/1",
            "rule-1",
            200,
            Some(captured),
        );
        let entries = log.recent(1);
        assert!(entries[0].captured.is_some());
        assert_eq!(
            entries[0].captured.as_ref().unwrap().remaining_path,
            "/orders/1"
        );
    }

    #[test]
    fn log_proxy_service_level_has_no_captured_detail() {
        let log = RequestLog::new();
        log.log_proxy(
            &svc(),
            "GET",
            "/svc/orders/1",
            "http://backend/orders/1",
            200,
            None,
        );
        let entries = log.recent(1);
        assert!(entries[0].captured.is_none());
    }

    #[test]
    fn log_no_rule_stores_captured_detail() {
        let log = RequestLog::new();
        let captured = CapturedRequest::from_request_data(&req_data(b""));
        log.log_no_rule(&svc(), "GET", "/svc/unknown", Some(captured));
        let entries = log.recent(1);
        assert!(entries[0].captured.is_some());
    }

    #[test]
    fn entry_count_bounded_by_max_entries() {
        let log = RequestLog::new();
        for i in 0..(MAX_ENTRIES + 20) {
            log.log_no_rule(&svc(), "GET", &format!("/svc/{i}"), None);
        }
        assert_eq!(log.recent(usize::MAX).len(), MAX_ENTRIES);
        let entries = log.recent(1);
        assert_eq!(entries[0].path, format!("/svc/{}", MAX_ENTRIES + 19));
    }

    #[test]
    fn recent_returns_most_recent_first() {
        let log = RequestLog::new();
        log.log_no_rule(&svc(), "GET", "/svc/first", None);
        log.log_no_rule(&svc(), "GET", "/svc/second", None);
        let entries = log.recent(10);
        assert_eq!(entries[0].path, "/svc/second");
        assert_eq!(entries[1].path, "/svc/first");
    }
}
