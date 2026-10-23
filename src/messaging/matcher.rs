// Matches a Kafka message (payload and headers) against the HTTP rules.
//
// `MatchEngine::matches_group()` evaluates the rule's all_of/any_of conditions with the same extractors as HTTP, so
// no extraction or evaluation logic is duplicated here.
//
// A message has no HTTP method and no path, so `Rule.method` and `Rule.sub_path` (required by the shared model) are
// ignored: only `Rule.conditions` count. Query and path parameters are always empty in the request built from a
// message, so a condition on them never matches a message, as expected.
//
// One listening topic serves all services (there is no per-service scope like HTTP paths): every service is tried in
// configuration order, and the first matching rule wins, as for HTTP.
use crate::engine::{MatchEngine, RequestData};
use crate::models::{Rule, Service};
use std::collections::HashMap;

pub struct MatchedMessage<'a> {
    pub service_name: &'a str,
    pub rule: &'a Rule,
}

pub fn match_message<'a>(
    services: &'a [Service],
    payload: &[u8],
    headers: &HashMap<String, String>,
) -> Option<MatchedMessage<'a>> {
    let req = RequestData {
        query_params: HashMap::new(),
        headers: headers.clone(),
        body: payload.to_vec(),
        content_type: headers.get("content-type").cloned(),
        path_params: HashMap::new(),
        method: String::new(),
        remaining_path: String::new(),
    };

    for service in services {
        if let Some(rule) = service
            .rules
            .iter()
            .find(|r| MatchEngine::matches_group(&r.conditions, &req, None))
        {
            return Some(MatchedMessage {
                service_name: &service.name,
                rule,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn service_with_rule(name: &str, conditions: ConditionGroup, body_literal: &str) -> Service {
        Service {
            name: name.into(),
            listen_path: String::new(),
            real_target_url: "http://unused".into(),
            is_mocked: true,
            rewrite_directory_urls: false,
            group_name: None,
            wsdl_mode: WsdlMode::default(),
            rules: vec![Rule {
                name: format!("{name}-rule"),
                method: "ANY".into(),
                sub_path: None,
                action: RuleAction::default(),
                script: None,
                response_mode: None,
                conditions,
                response: MockResponse {
                    status: 200,
                    headers: vec![],
                    body: vec![BodyFragment::Literal {
                        value: body_literal.into(),
                    }],
                    chaos: None,
                },
            }],
        }
    }

    #[test]
    fn matches_on_json_pointer_condition() {
        let services = vec![service_with_rule(
            "svc-a",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::JsonPointer("/type".into()),
                    operator: Operator::Eq("order.created".into()),
                }],
                any_of: vec![],
            },
            "ok",
        )];
        let payload = br#"{"type":"order.created"}"#;
        let m = match_message(&services, payload, &HashMap::new()).unwrap();
        assert_eq!(m.service_name, "svc-a");
        assert_eq!(m.rule.name, "svc-a-rule");
    }

    #[test]
    fn matches_on_header_condition() {
        let services = vec![service_with_rule(
            "svc-a",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::Header("event-type".into()),
                    operator: Operator::Eq("created".into()),
                }],
                any_of: vec![],
            },
            "ok",
        )];
        let mut headers = HashMap::new();
        headers.insert("event-type".to_string(), "created".to_string());
        let m = match_message(&services, b"{}", &headers).unwrap();
        assert_eq!(m.service_name, "svc-a");
    }

    #[test]
    fn no_match_returns_none() {
        let services = vec![service_with_rule(
            "svc-a",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::JsonPointer("/type".into()),
                    operator: Operator::Eq("order.created".into()),
                }],
                any_of: vec![],
            },
            "ok",
        )];
        let payload = br#"{"type":"order.cancelled"}"#;
        assert!(match_message(&services, payload, &HashMap::new()).is_none());
    }

    #[test]
    fn empty_conditions_always_match() {
        let services = vec![service_with_rule(
            "catch-all",
            ConditionGroup::default(),
            "ok",
        )];
        let m = match_message(&services, b"anything", &HashMap::new()).unwrap();
        assert_eq!(m.service_name, "catch-all");
    }

    #[test]
    fn first_matching_service_wins_across_multiple_services() {
        let services = vec![
            service_with_rule(
                "svc-a",
                ConditionGroup {
                    all_of: vec![Condition {
                        source: ConditionSource::JsonPointer("/type".into()),
                        operator: Operator::Eq("nope".into()),
                    }],
                    any_of: vec![],
                },
                "a",
            ),
            service_with_rule("svc-b", ConditionGroup::default(), "b"),
        ];
        let m = match_message(&services, b"{}", &HashMap::new()).unwrap();
        assert_eq!(m.service_name, "svc-b");
    }

    #[test]
    fn method_and_sub_path_are_ignored_for_messages() {
        let mut svc = service_with_rule("svc-a", ConditionGroup::default(), "ok");
        svc.rules[0].method = "DELETE".into();
        svc.rules[0].sub_path = Some("/never/matches/a/message".into());
        let services = [svc];
        let m = match_message(&services, b"{}", &HashMap::new());
        assert!(
            m.is_some(),
            "method/sub_path must not gate message matching"
        );
    }

    #[test]
    fn no_services_returns_none() {
        assert!(match_message(&[], b"{}", &HashMap::new()).is_none());
    }
}
