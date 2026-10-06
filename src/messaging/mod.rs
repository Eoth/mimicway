// Kafka messaging, compiled only with the "messaging-kafka" feature: without it, this module and its dependency do
// not exist in the binary. The configuration (`KafkaConfig`, read from the environment) lives here; the consumer
// and publisher, and the message log, have their own modules. docs/en/kafka-messaging.md describes the behavior.
pub mod consumer;
pub mod matcher;
pub mod message_log;

use message_log::MessageLog;

/// The messaging state the HTTP layer needs: the message log (present whenever the feature is compiled, empty when
/// Kafka is off at run time) and the reply topic and publisher, so that `POST /api/messaging/simulate` publishes
/// exactly as the consumer does (see consumer::process_message).
#[derive(Clone)]
pub struct MessagingState {
    pub message_log: MessageLog,
    pub reply_topic: Option<String>,
    pub publisher: consumer::Publisher,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KafkaConfig {
    pub enabled: bool,
    pub brokers: Vec<String>,
    pub consumer_group: String,
    pub listen_topic: String,
    pub reply_topic: Option<String>,
}

impl KafkaConfig {
    pub fn from_env() -> Self {
        Self::from_lookup(crate::settings::env)
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let enabled = lookup("KAFKA_ENABLED")
            .unwrap_or_else(|| "false".into())
            .eq_ignore_ascii_case("true");

        let brokers: Vec<String> = lookup("KAFKA_BROKERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let consumer_group = lookup("KAFKA_CONSUMER_GROUP").unwrap_or_else(|| "mimicway".into());
        let listen_topic = lookup("KAFKA_LISTEN_TOPIC").unwrap_or_default();
        let reply_topic = lookup("KAFKA_REPLY_TOPIC").filter(|s| !s.is_empty());

        Self {
            enabled,
            brokers,
            consumer_group,
            listen_topic,
            reply_topic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::vars;

    #[test]
    fn defaults_disabled_with_empty_brokers() {
        let cfg = KafkaConfig::from_lookup(vars(&[]));
        assert!(!cfg.enabled);
        assert!(cfg.brokers.is_empty());
        assert_eq!(cfg.consumer_group, "mimicway");
        assert_eq!(cfg.listen_topic, "");
        assert!(cfg.reply_topic.is_none());
    }

    #[test]
    fn parses_broker_list_from_env() {
        let cfg =
            KafkaConfig::from_lookup(vars(&[("KAFKA_BROKERS", "broker1:9092, broker2:9092")]));
        assert_eq!(cfg.brokers, vec!["broker1:9092", "broker2:9092"]);
    }

    #[test]
    fn enabled_true_from_env() {
        let cfg = KafkaConfig::from_lookup(vars(&[("KAFKA_ENABLED", "true")]));
        assert!(cfg.enabled);
    }

    #[test]
    fn reply_topic_from_env() {
        let cfg = KafkaConfig::from_lookup(vars(&[("KAFKA_REPLY_TOPIC", "mimicway.replies")]));
        assert_eq!(cfg.reply_topic, Some("mimicway.replies".to_string()));
    }

    #[test]
    fn empty_reply_topic_env_var_is_none() {
        let cfg = KafkaConfig::from_lookup(vars(&[("KAFKA_REPLY_TOPIC", "")]));
        assert!(cfg.reply_topic.is_none());
    }

    #[test]
    fn custom_consumer_group() {
        let cfg = KafkaConfig::from_lookup(vars(&[("KAFKA_CONSUMER_GROUP", "my-group")]));
        assert_eq!(cfg.consumer_group, "my-group");
    }

    #[test]
    fn listen_topic_from_env() {
        let cfg = KafkaConfig::from_lookup(vars(&[("KAFKA_LISTEN_TOPIC", "orders.in")]));
        assert_eq!(cfg.listen_topic, "orders.in");
    }
}
