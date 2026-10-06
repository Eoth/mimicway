// In-memory log of the Kafka messages handled (incoming ones, and the replies published to the reply topic), a FIFO
// like RequestLog.
//
// Bounded twice: by count (MAX_ENTRIES, like RequestLog's 200) and by age (MESSAGE_LOG_TTL_MS, 24 hours by default).
// Age alone would not bound memory when messages arrive faster than they expire; count alone would keep an old
// message for days on a quiet topic, beyond the short retention wanted. `push()` drops expired entries first, then
// applies the count limit. The purge happens on writes, never in a background task.
//
// Bodies longer than MESSAGE_LOG_MAX_BODY_SIZE (16 KiB by default) are truncated; the metadata (topic, time, real
// size, match result) is always complete.
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, RwLock};

const MAX_ENTRIES: usize = 500;
const DEFAULT_TTL_MS: u64 = 24 * 60 * 60 * 1000;
const DEFAULT_MAX_BODY_SIZE: usize = 16 * 1024;

pub fn ttl_ms() -> u64 {
    ttl_ms_in(crate::settings::env)
}

fn ttl_ms_in(lookup: impl Fn(&str) -> Option<String>) -> u64 {
    crate::settings::number(lookup, "MESSAGE_LOG_TTL_MS", DEFAULT_TTL_MS)
}

pub fn max_body_size() -> usize {
    max_body_size_in(crate::settings::env)
}

fn max_body_size_in(lookup: impl Fn(&str) -> Option<String>) -> usize {
    crate::settings::number(lookup, "MESSAGE_LOG_MAX_BODY_SIZE", DEFAULT_MAX_BODY_SIZE)
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageLogEntry {
    pub timestamp: u64,
    /// "in" (received on the listening topic) or "out" (published on the reply topic).
    pub direction: String,
    pub topic: String,
    pub service_name: Option<String>,
    pub rule_matched: Option<String>,
    pub matched: bool,
    pub body_preview: String,
    pub body_truncated: bool,
    pub body_size_bytes: usize,
}

#[derive(Clone)]
pub struct MessageLog {
    entries: Arc<RwLock<VecDeque<MessageLogEntry>>>,
}

impl MessageLog {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_ENTRIES))),
        }
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    fn truncate_body(body: &[u8]) -> (String, bool) {
        let max = max_body_size();
        let truncated = body.len() > max;
        let slice = &body[..body.len().min(max)];
        (String::from_utf8_lossy(slice).into_owned(), truncated)
    }

    pub fn push(&self, entry: MessageLogEntry) {
        let mut entries = self.entries.write().unwrap();

        let ttl = ttl_ms();
        let now = Self::now_ms();
        while let Some(front) = entries.front() {
            if now.saturating_sub(front.timestamp) >= ttl {
                entries.pop_front();
            } else {
                break;
            }
        }

        while entries.len() >= MAX_ENTRIES {
            entries.pop_front();
        }

        entries.push_back(entry);
    }

    fn record(
        &self,
        direction: &str,
        topic: &str,
        service_name: Option<&str>,
        rule_matched: Option<&str>,
        matched: bool,
        body: &[u8],
    ) {
        let (body_preview, body_truncated) = Self::truncate_body(body);
        self.push(MessageLogEntry {
            timestamp: Self::now_ms(),
            direction: direction.into(),
            topic: topic.into(),
            service_name: service_name.map(String::from),
            rule_matched: rule_matched.map(String::from),
            matched,
            body_preview,
            body_truncated,
            body_size_bytes: body.len(),
        });
    }

    pub fn record_in(
        &self,
        topic: &str,
        service_name: Option<&str>,
        rule_matched: Option<&str>,
        matched: bool,
        body: &[u8],
    ) {
        self.record("in", topic, service_name, rule_matched, matched, body);
    }

    pub fn record_out(
        &self,
        topic: &str,
        service_name: Option<&str>,
        rule_matched: Option<&str>,
        body: &[u8],
    ) {
        self.record("out", topic, service_name, rule_matched, true, body);
    }

    pub fn recent(&self, limit: usize) -> Vec<MessageLogEntry> {
        let entries = self.entries.read().unwrap();
        entries.iter().rev().take(limit).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().unwrap().is_empty()
    }
}

impl Default for MessageLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::vars;

    #[test]
    fn ttl_default_is_24h() {
        assert_eq!(ttl_ms_in(vars(&[])), 24 * 60 * 60 * 1000);
    }

    #[test]
    fn ttl_from_env() {
        assert_eq!(ttl_ms_in(vars(&[("MESSAGE_LOG_TTL_MS", "1000")])), 1000);
    }

    #[test]
    fn max_body_size_default_is_16kb() {
        assert_eq!(max_body_size_in(vars(&[])), 16 * 1024);
    }

    #[test]
    fn max_body_size_from_env() {
        let lookup = vars(&[("MESSAGE_LOG_MAX_BODY_SIZE", "10")]);
        assert_eq!(max_body_size_in(lookup), 10);
    }

    // The tests below run with the default limits: tests never change the process environment (see crate::settings).

    #[test]
    fn record_in_matched_entry() {
        let log = MessageLog::new();
        log.record_in("orders.in", Some("svc-a"), Some("rule-1"), true, b"hello");
        let entries = log.recent(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].direction, "in");
        assert_eq!(entries[0].topic, "orders.in");
        assert_eq!(entries[0].service_name.as_deref(), Some("svc-a"));
        assert_eq!(entries[0].rule_matched.as_deref(), Some("rule-1"));
        assert!(entries[0].matched);
        assert_eq!(entries[0].body_preview, "hello");
        assert!(!entries[0].body_truncated);
        assert_eq!(entries[0].body_size_bytes, 5);
    }

    #[test]
    fn record_in_no_match_entry() {
        let log = MessageLog::new();
        log.record_in("orders.in", None, None, false, b"unmatched");
        let entries = log.recent(10);
        assert!(!entries[0].matched);
        assert!(entries[0].service_name.is_none());
        assert!(entries[0].rule_matched.is_none());
    }

    #[test]
    fn record_out_entry() {
        let log = MessageLog::new();
        log.record_out(
            "orders.reply",
            Some("svc-a"),
            Some("rule-1"),
            b"response body",
        );
        let entries = log.recent(10);
        assert_eq!(entries[0].direction, "out");
        assert_eq!(entries[0].topic, "orders.reply");
    }

    #[test]
    fn body_truncated_beyond_max_size() {
        let mut body = vec![b'0'; DEFAULT_MAX_BODY_SIZE];
        body.extend_from_slice(b"beyond");
        let log = MessageLog::new();
        log.record_in("t", None, None, false, &body);
        let entries = log.recent(10);
        assert!(entries[0].body_truncated);
        assert_eq!(entries[0].body_preview.len(), DEFAULT_MAX_BODY_SIZE);
        assert_eq!(
            entries[0].body_size_bytes,
            DEFAULT_MAX_BODY_SIZE + 6,
            "real size must be preserved even when body is truncated"
        );
    }

    #[test]
    fn body_not_truncated_under_max_size() {
        let log = MessageLog::new();
        log.record_in("t", None, None, false, b"short");
        let entries = log.recent(10);
        assert!(!entries[0].body_truncated);
    }

    #[test]
    fn entry_count_bounded_by_max_entries() {
        let log = MessageLog::new();
        for i in 0..(MAX_ENTRIES + 50) {
            log.record_in("t", None, None, false, format!("msg-{i}").as_bytes());
        }
        assert_eq!(
            log.len(),
            MAX_ENTRIES,
            "entry count must never exceed MAX_ENTRIES"
        );
        let entries = log.recent(1);
        assert_eq!(entries[0].body_preview, format!("msg-{}", MAX_ENTRIES + 49));
    }

    #[test]
    fn expired_entries_purged_on_next_write() {
        let log = MessageLog::new();
        log.push(MessageLogEntry {
            timestamp: MessageLog::now_ms() - DEFAULT_TTL_MS - 1000,
            direction: "in".into(),
            topic: "t".into(),
            service_name: None,
            rule_matched: None,
            matched: false,
            body_preview: "old".into(),
            body_truncated: false,
            body_size_bytes: 3,
        });
        assert_eq!(log.len(), 1);

        // Any subsequent write is the purge trigger (event-driven, no timer).
        log.record_in("t", None, None, false, b"new");
        assert_eq!(
            log.len(),
            1,
            "expired entry must be purged, only the fresh one remains"
        );
        assert_eq!(log.recent(1)[0].body_preview, "new");
    }

    #[test]
    fn fresh_entries_survive_purge() {
        let log = MessageLog::new();
        log.record_in("t", None, None, false, b"a");
        log.record_in("t", None, None, false, b"b");
        assert_eq!(log.len(), 2);
    }

    #[test]
    fn recent_returns_most_recent_first() {
        let log = MessageLog::new();
        log.record_in("t", None, None, false, b"first");
        log.record_in("t", None, None, false, b"second");
        let entries = log.recent(10);
        assert_eq!(entries[0].body_preview, "second");
        assert_eq!(entries[1].body_preview, "first");
    }
}
