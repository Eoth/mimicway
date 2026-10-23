// The configuration schema, stored as YAML and exchanged as JSON by the API.
// MockConfig -> Service[] and Group[]
// Service -> Rule[] (first match wins)
// Rule -> method, sub_path, conditions, response, scripts
// A change here changes the stored files, the API and the UI at once.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MockConfig {
    pub services: Vec<Service>,
    pub groups: Vec<Group>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Group {
    pub name: String,
    pub code: String,
    #[serde(default)]
    pub admins: Vec<String>,
    #[serde(default)]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum WsdlMode {
    #[default]
    Auto,
    Proxy,
    Mock,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Service {
    pub name: String,
    pub listen_path: String,
    pub real_target_url: String,
    pub is_mocked: bool,
    // Read by no code: no response is ever rewritten from it (rules suggested from observed traffic are, see
    // `server::suggestion::links`, and relayed traffic never is). The service form stores its "SOAP" type in it.
    // Optional when read, so a configuration without it loads; still written, so a file saved now loads in a version
    // that requires it.
    #[serde(default)]
    pub rewrite_directory_urls: bool,
    pub group_name: Option<String>,
    pub wsdl_mode: WsdlMode,
    pub rules: Vec<Rule>,
}

// pre_script and post_script run at the same point as `script` (intercept.rs, just before the template is rendered)
// and are not chained: each gets the same context (the request only) and none sees another's result, which avoids
// ordering and shared-state questions between scripts. "Pre" and "post" are a naming convention for whoever writes
// the rule (preparing, then finishing data), not two phases. Their results are {{pre_script}}/{{pre_script.field}}
// and {{post_script}}/{{post_script.field}}, next to {{script}}/{{script.field}}. As options, a missing key reads as
// None.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub name: String,
    pub method: String,
    pub sub_path: Option<String>,
    pub action: RuleAction,
    pub pre_script: Option<String>,
    pub script: Option<String>,
    pub post_script: Option<String>,
    // For the editor only (matching and rendering never read it): which view of the rule form produced
    // `response.body` (json-paste, json-guided, xml-paste, xml-guided, text, advanced, empty), so that
    // RuleResponseSection.svelte reopens the same view instead of the advanced template. A missing key falls back to
    // guessing from the body's shape, never to a loading error.
    #[serde(default)]
    pub response_mode: Option<ResponseEditorMode>,
    pub conditions: ConditionGroup,
    pub response: MockResponse,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum ResponseEditorMode {
    JsonPaste,
    JsonGuided,
    XmlPaste,
    XmlGuided,
    Text,
    Advanced,
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RuleAction {
    #[default]
    Mock,
    Proxy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ConditionGroup {
    #[serde(default)]
    pub all_of: Vec<Condition>,
    #[serde(default)]
    pub any_of: Vec<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Condition {
    pub source: ConditionSource,
    pub operator: Operator,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "key")]
pub enum ConditionSource {
    QueryParam(String),
    Header(String),
    JsonPointer(String),
    XPath(String),
    FormField(String),
    PathParam(String),
    BodyRaw,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum Operator {
    Eq(String),
    Contains(String),
    Regex(String),
    Exists,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MockResponse {
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    pub body: Vec<BodyFragment>,
    #[serde(default)]
    pub chaos: Option<ChaosConfig>,
}

fn default_status() -> u16 {
    200
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum BodyFragment {
    Literal { value: String },
    PickFrom { values: Vec<String> },
    FakeData { kind: FakeKind },
    Uuid,
    PathSegment { index: usize },
    Template { template: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum FakeKind {
    FirstName,
    LastName,
    Email,
    PhoneNumberFR,
    Integer { min: i64, max: i64 },
    CompanyName,
    StreetName,
    CityFR,
    PostcodeFR,
    Siren,
    Siret,
    FullAddressFR,
    DatePast,
    DateFuture,
    TimestampMs,
    BoolRandom,
    LoremSentence,
    CountryFR,
    IbanFR,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChaosConfig {
    #[serde(default)]
    pub delay_ms: Option<u64>,
    #[serde(default)]
    pub delay_min_ms: Option<u64>,
    #[serde(default)]
    pub delay_max_ms: Option<u64>,
    #[serde(default)]
    pub error_rate: Option<f64>,
    #[serde(default = "default_chaos_status")]
    pub error_status: u16,
}

fn default_chaos_status() -> u16 {
    500
}

impl MockConfig {
    pub fn empty() -> Self {
        Self {
            services: vec![],
            groups: vec![],
        }
    }

    /// `(service, group)` for every service whose `group_name` names no group of the configuration. Such a
    /// service has no group code, so its URL and its permissions silently fall back to the ungrouped ones.
    pub fn unknown_group_references(&self) -> Vec<(&str, &str)> {
        self.services
            .iter()
            .filter_map(|s| {
                let group = s.group_name.as_deref()?;
                (!self.groups.iter().any(|g| g.name == group)).then_some((s.name.as_str(), group))
            })
            .collect()
    }

    /// Gives every service after the first of its (group, name) slot a free name, `name-2`, `name-3`..., and returns
    /// `(group, old name, new name)` for each. Versions before 0.2.0 could save two services in one slot: routing only
    /// ever reached the first (so no served URL changes), and the interface, which lists services by name within their
    /// group, failed to render. The suffix keeps the name valid: same characters, and a dash no group code has.
    pub fn rename_duplicate_services(&mut self) -> Vec<(Option<String>, String, String)> {
        let mut taken: std::collections::HashSet<(Option<String>, String)> = self
            .services
            .iter()
            .map(|s| (s.group_name.clone(), s.name.clone()))
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut renamed = Vec::new();
        for service in &mut self.services {
            if seen.insert((service.group_name.clone(), service.name.clone())) {
                continue;
            }
            let free = (2u64..)
                .map(|n| format!("{}-{n}", service.name))
                .find(|candidate| !taken.contains(&(service.group_name.clone(), candidate.clone())))
                .unwrap_or_default();
            taken.insert((service.group_name.clone(), free.clone()));
            seen.insert((service.group_name.clone(), free.clone()));
            let old = std::mem::replace(&mut service.name, free.clone());
            renamed.push((service.group_name.clone(), old, free));
        }
        renamed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> MockConfig {
        MockConfig {
            services: vec![Service {
                name: "service-a".into(),

                listen_path: "/service-a/*".into(),
                real_target_url: "http://service-a.default.svc:8080".into(),
                is_mocked: true,
                rewrite_directory_urls: false,
                group_name: None,
                wsdl_mode: WsdlMode::default(),
                rules: vec![Rule {
                    name: "rule-login-ok".into(),
                    method: "GET".into(),
                    sub_path: None,
                    action: RuleAction::default(),
                    pre_script: None,
                    script: None,
                    post_script: None,
                    response_mode: None,
                    conditions: ConditionGroup {
                        all_of: vec![
                            Condition {
                                source: ConditionSource::Header("x-env".into()),
                                operator: Operator::Eq("staging".into()),
                            },
                            Condition {
                                source: ConditionSource::JsonPointer("/user/role".into()),
                                operator: Operator::Contains("admin".into()),
                            },
                        ],
                        any_of: vec![Condition {
                            source: ConditionSource::QueryParam("debug".into()),
                            operator: Operator::Exists,
                        }],
                    },
                    response: MockResponse {
                        status: 200,
                        headers: vec![HeaderEntry {
                            name: "Content-Type".into(),
                            value: "application/json".into(),
                        }],
                        body: vec![
                            BodyFragment::Literal {
                                value: r#"{"id":""#.into(),
                            },
                            BodyFragment::Uuid,
                            BodyFragment::Literal {
                                value: r#"","name":""#.into(),
                            },
                            BodyFragment::FakeData {
                                kind: FakeKind::FirstName,
                            },
                            BodyFragment::Literal {
                                value: r#"","status":""#.into(),
                            },
                            BodyFragment::PickFrom {
                                values: vec!["active".into(), "pending".into()],
                            },
                            BodyFragment::Literal {
                                value: r#""}"#.into(),
                            },
                        ],
                        chaos: Some(ChaosConfig {
                            delay_ms: Some(150),
                            delay_min_ms: None,
                            delay_max_ms: None,
                            error_rate: Some(0.1),
                            error_status: 503,
                        }),
                    },
                }],
            }],
            groups: vec![],
        }
    }

    fn named(name: &str, group: Option<&str>) -> Service {
        let mut service = sample_config().services.remove(0);
        service.name = name.into();
        service.group_name = group.map(str::to_string);
        service
    }

    fn slots(config: &MockConfig) -> Vec<(Option<&str>, &str)> {
        config
            .services
            .iter()
            .map(|s| (s.group_name.as_deref(), s.name.as_str()))
            .collect()
    }

    #[test]
    fn duplicate_services_get_a_free_name_and_the_first_keeps_its_own() {
        let mut config = MockConfig {
            services: vec![
                named("users", None),
                named("users", None),
                named("users-2", None),
                named("users", Some("team-a")),
                named("users", None),
            ],
            groups: vec![],
        };
        let renamed = config.rename_duplicate_services();
        assert_eq!(
            slots(&config),
            vec![
                (None, "users"),
                (None, "users-3"),
                (None, "users-2"),
                (Some("team-a"), "users"),
                (None, "users-4"),
            ],
            "a suffix never takes the name of a later service, and namesakes of other groups stay as they are"
        );
        assert_eq!(
            renamed,
            vec![
                (None, "users".to_string(), "users-3".to_string()),
                (None, "users".to_string(), "users-4".to_string())
            ]
        );
        for service in &config.services {
            assert!(crate::server::validation::validate_service(service).is_ok());
        }
    }

    #[test]
    fn a_configuration_without_duplicates_is_left_alone() {
        let mut config = MockConfig {
            services: vec![named("users", None), named("users", Some("team-a"))],
            groups: vec![],
        };
        let before = config.clone();
        assert!(config.rename_duplicate_services().is_empty());
        assert_eq!(config, before);
    }

    #[test]
    fn roundtrip_yaml_serialization() {
        let config = sample_config();
        let yaml = serde_yaml::to_string(&config).expect("serialize");
        let parsed: MockConfig = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(config, parsed);
    }

    #[test]
    fn deserialize_minimal_service() {
        let yaml = r#"
services:
  - name: svc
    listen_path: /svc/*
    real_target_url: http://svc:80
    is_mocked: false
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: auto
    rules: []
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(config.services.len(), 1);
        assert!(config.services[0].rules.is_empty());
        assert!(!config.services[0].is_mocked);
    }

    #[test]
    fn deserialize_rule_without_response_mode_defaults_to_none() {
        // response_mode only drives the editor: a configuration written without it must still load. This YAML leaves the
        // key out on purpose.
        let yaml = r#"
services:
  - name: svc
    listen_path: /svc/*
    real_target_url: http://svc:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: auto
    rules:
      - name: legacy-rule
        method: GET
        sub_path: ~
        action: mock
        pre_script: ~
        script: ~
        post_script: ~
        conditions:
          all_of: []
          any_of: []
        response:
          status: 200
          body:
            - type: Literal
              value: ok
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(config.services[0].rules[0].response_mode, None);
    }

    #[test]
    fn response_mode_roundtrip_through_yaml() {
        let mut config = sample_config();
        config.services[0].rules[0].response_mode = Some(ResponseEditorMode::JsonPaste);
        let yaml = serde_yaml::to_string(&config).expect("serialize");
        assert!(yaml.contains("response_mode: json-paste"));
        let parsed: MockConfig = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(
            parsed.services[0].rules[0].response_mode,
            Some(ResponseEditorMode::JsonPaste)
        );
    }

    #[test]
    fn deserialize_all_condition_sources() {
        let yaml = r#"
services:
  - name: test
    listen_path: /t/*
    real_target_url: http://t:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: auto
    rules:
      - name: all-sources
        method: ANY
        sub_path: ~
        action: mock
        pre_script: ~
        script: ~
        post_script: ~
        conditions:
          all_of:
            - source: { type: QueryParam, key: q }
              operator: { type: Eq, value: "1" }
            - source: { type: Header, key: x-test }
              operator: { type: Contains, value: foo }
            - source: { type: JsonPointer, key: "/a/b" }
              operator: { type: Regex, value: "^ok$" }
            - source: { type: XPath, key: "Envelope/Body/id" }
              operator: { type: Exists }
            - source: { type: FormField, key: username }
              operator: { type: Eq, value: admin }
            - source: { type: BodyRaw }
              operator: { type: Contains, value: hello }
          any_of: []
        response:
          status: 201
          body:
            - type: Literal
              value: ok
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        let rule = &config.services[0].rules[0];
        assert_eq!(rule.conditions.all_of.len(), 6);
        assert_eq!(rule.response.status, 201);
    }

    #[test]
    fn deserialize_all_body_fragments() {
        let yaml = r#"
services:
  - name: frag
    listen_path: /f/*
    real_target_url: http://f:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: auto
    rules:
      - name: fragments
        method: ANY
        sub_path: ~
        action: mock
        pre_script: ~
        script: ~
        post_script: ~
        conditions: {}
        response:
          body:
            - type: Literal
              value: "start"
            - type: Uuid
            - type: PickFrom
              values: [a, b, c]
            - type: FakeData
              kind: { type: FirstName }
            - type: FakeData
              kind: { type: LastName }
            - type: FakeData
              kind: { type: Email }
            - type: FakeData
              kind: { type: PhoneNumberFR }
            - type: FakeData
              kind: { type: Integer, min: 1, max: 100 }
            - type: PathSegment
              index: 2
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        let body = &config.services[0].rules[0].response.body;
        assert_eq!(body.len(), 9);
        assert_eq!(body[8], BodyFragment::PathSegment { index: 2 });
    }

    #[test]
    fn deserialize_chaos_config() {
        let yaml = r#"
services:
  - name: chaos
    listen_path: /c/*
    real_target_url: http://c:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: auto
    rules:
      - name: chaos-rule
        method: ANY
        sub_path: ~
        action: mock
        pre_script: ~
        script: ~
        post_script: ~
        conditions: {}
        response:
          body:
            - type: Literal
              value: err
          chaos:
            delay_ms: 500
            error_rate: 0.25
            error_status: 502
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        let chaos = config.services[0].rules[0]
            .response
            .chaos
            .as_ref()
            .expect("chaos present");
        assert_eq!(chaos.delay_ms, Some(500));
        assert_eq!(chaos.error_rate, Some(0.25));
        assert_eq!(chaos.error_status, 502);
    }

    #[test]
    fn chaos_defaults() {
        let yaml = r#"
services:
  - name: def
    listen_path: /d/*
    real_target_url: http://d:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: auto
    rules:
      - name: no-chaos
        method: ANY
        sub_path: ~
        action: mock
        pre_script: ~
        script: ~
        post_script: ~
        conditions: {}
        response:
          body:
            - type: Literal
              value: ok
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        assert!(config.services[0].rules[0].response.chaos.is_none());
    }

    #[test]
    fn empty_config() {
        let config = MockConfig::empty();
        assert!(config.services.is_empty());
        let yaml = serde_yaml::to_string(&config).expect("serialize");
        let parsed: MockConfig = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(config, parsed);
    }

    #[test]
    fn status_defaults_to_200() {
        let yaml = r#"
services:
  - name: s
    listen_path: /s/*
    real_target_url: http://s:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: auto
    rules:
      - name: r
        method: GET
        sub_path: ~
        action: mock
        pre_script: ~
        script: ~
        post_script: ~
        conditions: {}
        response:
          body:
            - type: Literal
              value: hi
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(config.services[0].rules[0].response.status, 200);
    }

    #[test]
    fn rule_action_proxy_roundtrip() {
        let yaml = r#"
services:
  - name: s
    listen_path: /s/*
    real_target_url: http://s:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: auto
    rules:
      - name: proxy-rule
        method: ANY
        sub_path: ~
        action: proxy
        pre_script: ~
        script: ~
        post_script: ~
        conditions: {}
        response:
          body:
            - type: Literal
              value: unused
      - name: mock-rule
        method: ANY
        sub_path: ~
        action: mock
        pre_script: ~
        script: ~
        post_script: ~
        conditions: {}
        response:
          body:
            - type: Literal
              value: mocked
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(config.services[0].rules[0].action, RuleAction::Proxy);
        assert_eq!(config.services[0].rules[1].action, RuleAction::Mock);

        let re_yaml = serde_yaml::to_string(&config).expect("serialize");
        let re_parsed: MockConfig = serde_yaml::from_str(&re_yaml).expect("re-deserialize");
        assert_eq!(config, re_parsed);
    }

    #[test]
    fn deserialize_config_with_groups() {
        let yaml = r#"
services:
  - name: svc
    listen_path: /svc/*
    real_target_url: http://svc:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: team-a
    wsdl_mode: auto
    rules: []
groups:
  - name: team-a
    code: tma01
    admins:
      - lead
    members:
      - dev1
      - dev2
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(config.groups.len(), 1);
        assert_eq!(config.groups[0].name, "team-a");
        assert_eq!(config.groups[0].admins, vec!["lead"]);
        assert_eq!(config.groups[0].members, vec!["dev1", "dev2"]);
        assert_eq!(config.services[0].group_name.as_deref(), Some("team-a"));
    }

    #[test]
    fn groups_roundtrip_serialization() {
        let config = MockConfig {
            services: vec![],
            groups: vec![Group {
                name: "team-x".into(),
                code: "tmx01".into(),
                admins: vec!["admin1".into()],
                members: vec!["user1".into(), "user2".into()],
            }],
        };
        let yaml = serde_yaml::to_string(&config).expect("serialize");
        let parsed: MockConfig = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(config, parsed);
    }

    #[test]
    fn wsdl_mode_mock_roundtrip() {
        let yaml = r#"
services:
  - name: svc
    listen_path: /svc/*
    real_target_url: http://svc:80
    is_mocked: true
    rewrite_directory_urls: false
    group_name: ~
    wsdl_mode: mock
    rules: []
groups: []
"#;
        let config: MockConfig = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(config.services[0].wsdl_mode, WsdlMode::Mock);
        let re_yaml = serde_yaml::to_string(&config).expect("serialize");
        let re_parsed: MockConfig = serde_yaml::from_str(&re_yaml).expect("re-deserialize");
        assert_eq!(re_parsed.services[0].wsdl_mode, WsdlMode::Mock);
    }

    #[test]
    fn the_directory_rewriting_flag_may_be_missing_or_set_and_changes_nothing() {
        let yaml = |flag: &str| {
            format!(
                "services:\n  - name: svc\n    listen_path: /svc/*\n    real_target_url: http://svc:80\n    \
                 is_mocked: false\n{flag}    group_name: ~\n    wsdl_mode: auto\n    rules: []\ngroups: []\n"
            )
        };
        let missing: MockConfig = serde_yaml::from_str(&yaml("")).expect("without the flag");
        assert!(!missing.services[0].rewrite_directory_urls);
        let set: MockConfig = serde_yaml::from_str(&yaml("    rewrite_directory_urls: true\n"))
            .expect("with the flag");
        assert!(set.services[0].rewrite_directory_urls);
    }
}
