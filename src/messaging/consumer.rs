// Kafka consumer and publisher ("messaging-kafka" feature).
//
// The loop waits on `StreamConsumer::recv().await` for the next message: rdkafka waits on the socket, and there is
// no polling or sleeping here.
//
// `process_message()` holds the whole handling (match, render, log, publish), whatever the message's origin: the
// Kafka loop (`run()`) and `POST /api/messaging/simulate` both call it, so a simulated message goes exactly where a
// real one would, and end-to-end tests need no broker.
use crate::engine::TemplateRenderer;
use crate::engine::template::TemplateContext;
use crate::messaging::KafkaConfig;
use crate::messaging::matcher::match_message;
use crate::messaging::message_log::MessageLog;
use crate::store::MockStore;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::{Headers, Message};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::util::Timeout;
use std::collections::HashMap;
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
use std::time::Duration;

/// (topic, payload) of each message a fake publisher received.
#[cfg(test)]
pub type PublishedMessages = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// Publishing to a Kafka topic. `Kafka` is the real one; `Fake`, for tests, records the calls in memory so that
/// publishing to the reply topic can be checked without a broker.
#[derive(Clone)]
pub enum Publisher {
    Kafka(Arc<FutureProducer>),
    #[cfg(test)]
    Fake(PublishedMessages),
    None,
}

impl Publisher {
    pub async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), String> {
        match self {
            Publisher::Kafka(producer) => {
                let record = FutureRecord::to(topic).payload(payload).key(topic);
                producer
                    .send(record, Timeout::After(Duration::from_secs(5)))
                    .await
                    .map(|_| ())
                    .map_err(|(e, _)| e.to_string())
            }
            #[cfg(test)]
            Publisher::Fake(log) => {
                log.lock()
                    .unwrap()
                    .push((topic.to_string(), payload.to_vec()));
                Ok(())
            }
            Publisher::None => Err("no reply publisher configured".into()),
        }
    }
}

fn build_publisher(config: &KafkaConfig) -> Publisher {
    if config.reply_topic.is_none() {
        return Publisher::None;
    }
    let created: Result<FutureProducer, _> = ClientConfig::new()
        .set("bootstrap.servers", config.brokers.join(","))
        .create();
    match created {
        Ok(producer) => Publisher::Kafka(Arc::new(producer)),
        Err(e) => {
            tracing::error!(error = %e, "messaging: failed to create Kafka producer, replies disabled");
            Publisher::None
        }
    }
}

/// Handles one message, whatever its origin (Kafka or the simulation route): match, render, log, publish.
pub async fn process_message(
    store: &MockStore,
    message_log: &MessageLog,
    reply_topic: Option<&str>,
    publisher: &Publisher,
    topic: &str,
    payload: &[u8],
    headers: HashMap<String, String>,
) {
    let config = store.snapshot().await;
    let matched = match_message(&config.services, payload, &headers);

    let (service_name, rule_name) = match &matched {
        Some(m) => (Some(m.service_name.to_string()), Some(m.rule.name.clone())),
        None => (None, None),
    };

    message_log.record_in(
        topic,
        service_name.as_deref(),
        rule_name.as_deref(),
        matched.is_some(),
        payload,
    );

    let Some(m) = matched else {
        tracing::debug!(topic = %topic, "messaging: no matching rule for message");
        return;
    };

    let empty_params: HashMap<String, String> = HashMap::new();
    let ctx = TemplateContext {
        path_params: &empty_params,
        query_params: &empty_params,
        headers: &headers,
        request_body: payload,
        // Messaging has no per-rule counter: {{seq}} is always 0 for a message.
        seq_counter: 0,
        // Scripts do not run for Kafka messages: {{script}}, {{pre_script}} and {{post_script}} render empty.
        script_result: None,
        pre_script_result: None,
        post_script_result: None,
    };
    let rendered = TemplateRenderer::render_body(&m.rule.response.body, &[], &ctx);

    let Some(reply) = reply_topic else { return };

    match publisher.publish(reply, rendered.as_bytes()).await {
        Ok(()) => {
            message_log.record_out(
                reply,
                service_name.as_deref(),
                rule_name.as_deref(),
                rendered.as_bytes(),
            );
        }
        Err(e) => {
            tracing::error!(error = %e, topic = %reply, "messaging: failed to publish reply");
        }
    }
}

fn extract_headers(msg: &rdkafka::message::BorrowedMessage<'_>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(headers) = msg.headers() {
        for header in headers.iter() {
            if let Some(value) = header.value
                && let Ok(s) = std::str::from_utf8(value)
            {
                map.insert(header.key.to_string(), s.to_string());
            }
        }
    }
    map
}

/// Starts the Kafka consumer in a background task and returns its `Publisher`, so that the simulation route
/// publishes through the same producer: a simulated message behaves like a real one.
pub fn spawn(config: KafkaConfig, store: MockStore, message_log: MessageLog) -> Publisher {
    let publisher = build_publisher(&config);
    let publisher_for_task = publisher.clone();
    tokio::spawn(run(config, store, message_log, publisher_for_task));
    publisher
}

async fn run(config: KafkaConfig, store: MockStore, message_log: MessageLog, publisher: Publisher) {
    let consumer: StreamConsumer = match ClientConfig::new()
        .set("group.id", &config.consumer_group)
        .set("bootstrap.servers", config.brokers.join(","))
        .set("enable.auto.commit", "true")
        .create()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "messaging: failed to create Kafka consumer, messaging disabled");
            return;
        }
    };

    if let Err(e) = consumer.subscribe(&[config.listen_topic.as_str()]) {
        tracing::error!(error = %e, topic = %config.listen_topic, "messaging: failed to subscribe to topic");
        return;
    }

    tracing::info!(
        topic = %config.listen_topic,
        group = %config.consumer_group,
        reply_topic = ?config.reply_topic,
        "messaging: Kafka consumer started"
    );

    let reply_topic = config.reply_topic.clone();

    // recv().await waits for the next message; nothing polls.
    loop {
        match consumer.recv().await {
            Ok(borrowed) => {
                let payload = borrowed.payload().unwrap_or(&[]).to_vec();
                let headers = extract_headers(&borrowed);
                let topic = borrowed.topic().to_string();
                process_message(
                    &store,
                    &message_log,
                    reply_topic.as_deref(),
                    &publisher,
                    &topic,
                    &payload,
                    headers,
                )
                .await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "messaging: error receiving Kafka message");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    async fn store_with_rule(conditions: ConditionGroup, response_literal: &str) -> MockStore {
        let dir = crate::server::test_support::temp_data_dir("msgtest");
        std::fs::create_dir_all(&dir).unwrap();
        let store = MockStore::new(MockStore::config_file(&dir)).unwrap();
        let config = MockConfig {
            services: vec![Service {
                name: "svc-a".into(),
                listen_path: String::new(),
                real_target_url: "http://unused".into(),
                is_mocked: true,
                rewrite_directory_urls: false,
                group_name: None,
                wsdl_mode: WsdlMode::default(),
                rules: vec![Rule {
                    name: "rule-a".into(),
                    method: "ANY".into(),
                    sub_path: None,
                    action: RuleAction::default(),
                    pre_script: None,
                    script: None,
                    post_script: None,
                    response_mode: None,
                    conditions,
                    response: MockResponse {
                        status: 200,
                        headers: vec![],
                        body: vec![BodyFragment::Literal {
                            value: response_literal.into(),
                        }],
                        chaos: None,
                    },
                }],
            }],
            groups: vec![],
        };
        // replace() applies the change in memory at once (only the disk write is deferred): snapshot() sees it right away,
        // no flush or sleep needed.
        store.replace(config).await.unwrap();
        store
    }

    fn always_match_conditions() -> ConditionGroup {
        ConditionGroup::default()
    }

    #[tokio::test]
    async fn process_message_logs_match_and_no_reply_topic() {
        let store = store_with_rule(always_match_conditions(), "pong").await;

        let message_log = MessageLog::new();
        let publisher = Publisher::Fake(Arc::new(Mutex::new(Vec::new())));

        process_message(
            &store,
            &message_log,
            None,
            &publisher,
            "orders.in",
            b"{}",
            HashMap::new(),
        )
        .await;

        let entries = message_log.recent(10);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].matched);
        assert_eq!(entries[0].service_name.as_deref(), Some("svc-a"));
        assert_eq!(entries[0].rule_matched.as_deref(), Some("rule-a"));
        assert_eq!(entries[0].direction, "in");
    }

    #[tokio::test]
    async fn process_message_publishes_rendered_body_on_reply_topic() {
        let store = store_with_rule(always_match_conditions(), "pong-body").await;

        let message_log = MessageLog::new();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let publisher = Publisher::Fake(calls.clone());

        process_message(
            &store,
            &message_log,
            Some("orders.reply"),
            &publisher,
            "orders.in",
            b"{}",
            HashMap::new(),
        )
        .await;

        let published = calls.lock().unwrap();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].0, "orders.reply");
        assert_eq!(published[0].1, b"pong-body");

        let entries = message_log.recent(10);
        assert_eq!(
            entries.len(),
            2,
            "expected one 'in' entry and one 'out' entry"
        );
        assert!(
            entries
                .iter()
                .any(|e| e.direction == "out" && e.topic == "orders.reply")
        );
    }

    #[tokio::test]
    async fn process_message_no_match_does_not_publish() {
        let store = store_with_rule(
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::JsonPointer("/type".into()),
                    operator: Operator::Eq("never".into()),
                }],
                any_of: vec![],
            },
            "unused",
        )
        .await;

        let message_log = MessageLog::new();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let publisher = Publisher::Fake(calls.clone());

        process_message(
            &store,
            &message_log,
            Some("orders.reply"),
            &publisher,
            "orders.in",
            b"{}",
            HashMap::new(),
        )
        .await;

        assert!(
            calls.lock().unwrap().is_empty(),
            "unmatched message must not trigger a reply publish"
        );
        let entries = message_log.recent(10);
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].matched);
    }

    #[test]
    fn build_publisher_is_none_without_reply_topic() {
        let config = KafkaConfig {
            enabled: true,
            brokers: vec!["localhost:9092".into()],
            consumer_group: "g".into(),
            listen_topic: "t".into(),
            reply_topic: None,
        };
        assert!(matches!(build_publisher(&config), Publisher::None));
    }

    #[tokio::test]
    async fn publisher_none_publish_errors() {
        let publisher = Publisher::None;
        let result = publisher.publish("t", b"x").await;
        assert!(result.is_err());
    }
}
