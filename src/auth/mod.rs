pub mod keycloak;
pub mod middleware;
#[cfg(test)]
pub(crate) mod test_realm;

use crate::models::{Group, MockConfig};

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub enabled: bool,
    pub keycloak_url: String,
    pub realm: String,
    pub client_id: String,
    /// Expected `iss` of the tokens; empty means the realm URL derived from `keycloak_url` (`KEYCLOAK_ISSUER`).
    pub issuer: String,
    pub super_admins: Vec<String>,
    /// Whether the UI shows the full reset button when authentication is off (`SHOW_RESET_BUTTON`, hidden by
    /// default); with authentication on, the user's super-admin role decides. Display only: the server checks the
    /// permission itself, and without authentication every caller is an anonymous super-admin anyway.
    pub show_reset_button: bool,
}

impl AuthConfig {
    /// The authentication settings, or a message naming what is missing when authentication is enabled without
    /// the Keycloak settings it needs (Mimicway then refuses to start rather than run half-protected).
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(crate::settings::env)
    }

    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let enabled = lookup("AUTH_ENABLED")
            .unwrap_or_else(|| "false".into())
            .eq_ignore_ascii_case("true");

        let keycloak_url = lookup("KEYCLOAK_URL").unwrap_or_default();
        let realm = lookup("KEYCLOAK_REALM").unwrap_or_default();
        let client_id = lookup("KEYCLOAK_CLIENT_ID").unwrap_or_default();
        let issuer = lookup("KEYCLOAK_ISSUER")
            .unwrap_or_default()
            .trim()
            .trim_end_matches('/')
            .to_string();
        let super_admins: Vec<String> = lookup("SUPER_ADMINS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let show_reset_button = lookup("SHOW_RESET_BUTTON")
            .unwrap_or_else(|| "false".into())
            .eq_ignore_ascii_case("true");

        if enabled && (keycloak_url.is_empty() || realm.is_empty() || client_id.is_empty()) {
            return Err(
                "AUTH_ENABLED=true requires KEYCLOAK_URL, KEYCLOAK_REALM and KEYCLOAK_CLIENT_ID"
                    .into(),
            );
        }

        Ok(Self {
            enabled,
            keycloak_url,
            realm,
            client_id,
            issuer,
            super_admins,
            show_reset_button,
        })
    }

    pub fn is_super_admin(&self, username: &str) -> bool {
        self.super_admins.iter().any(|sa| sa == username)
    }
}

pub fn can_access_service(
    username: &str,
    is_super_admin: bool,
    service: &crate::models::Service,
    groups: &[Group],
) -> bool {
    if is_super_admin {
        return true;
    }
    match &service.group_name {
        None => false,
        Some(gn) => groups.iter().any(|g| {
            g.name == *gn
                && (g.admins.contains(&username.to_string())
                    || g.members.contains(&username.to_string()))
        }),
    }
}

pub fn can_manage_group(username: &str, is_super_admin: bool, group: &Group) -> bool {
    is_super_admin || group.admins.contains(&username.to_string())
}

pub fn visible_services(
    username: &str,
    is_super_admin: bool,
    config: &MockConfig,
) -> Vec<crate::models::Service> {
    if is_super_admin {
        return config.services.clone();
    }
    config
        .services
        .iter()
        .filter(|s| can_access_service(username, false, s, &config.groups))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Group, MockConfig, Service, WsdlMode};
    use crate::settings::vars;

    fn test_config() -> AuthConfig {
        AuthConfig {
            enabled: true,
            keycloak_url: "https://kc.example.com".into(),
            realm: "test".into(),
            client_id: "mimicway".into(),
            super_admins: vec!["admin1".into()],
            issuer: String::new(),
            show_reset_button: false,
        }
    }

    fn test_groups() -> Vec<Group> {
        vec![Group {
            name: "team-a".into(),
            code: "tma01".into(),
            admins: vec!["lead-a".into()],
            members: vec!["dev-a1".into(), "dev-a2".into()],
        }]
    }

    fn test_service(group: Option<&str>) -> Service {
        Service {
            name: "svc".into(),
            listen_path: "/v1/*".into(),
            real_target_url: "http://svc:80".into(),
            is_mocked: true,
            rewrite_directory_urls: false,
            group_name: group.map(String::from),
            wsdl_mode: WsdlMode::default(),
            rules: vec![],
        }
    }

    #[test]
    fn super_admin_from_config() {
        let cfg = test_config();
        assert!(cfg.is_super_admin("admin1"));
        assert!(!cfg.is_super_admin("nobody"));
    }

    #[test]
    fn super_admin_can_access_any_service() {
        let groups = test_groups();
        assert!(can_access_service(
            "admin1",
            true,
            &test_service(Some("team-a")),
            &groups
        ));
        assert!(can_access_service(
            "admin1",
            true,
            &test_service(None),
            &groups
        ));
    }

    #[test]
    fn group_member_can_access_own_service() {
        let groups = test_groups();
        assert!(can_access_service(
            "dev-a1",
            false,
            &test_service(Some("team-a")),
            &groups
        ));
        assert!(can_access_service(
            "lead-a",
            false,
            &test_service(Some("team-a")),
            &groups
        ));
    }

    #[test]
    fn outsider_cannot_access_service() {
        let groups = test_groups();
        assert!(!can_access_service(
            "outsider",
            false,
            &test_service(Some("team-a")),
            &groups
        ));
    }

    #[test]
    fn no_group_service_only_super_admin() {
        let groups = test_groups();
        assert!(!can_access_service(
            "dev-a1",
            false,
            &test_service(None),
            &groups
        ));
    }

    #[test]
    fn can_manage_group_admin_or_super() {
        let group = &test_groups()[0];
        assert!(can_manage_group("lead-a", false, group));
        assert!(can_manage_group("anyone", true, group));
        assert!(!can_manage_group("dev-a1", false, group));
    }

    #[test]
    fn visible_services_filters_by_group() {
        let config = MockConfig {
            services: vec![
                test_service(Some("team-a")),
                Service {
                    name: "svc-b".into(),
                    group_name: Some("team-b".into()),
                    ..test_service(None)
                },
            ],
            groups: test_groups(),
        };
        let visible = visible_services("dev-a1", false, &config);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].name, "svc");
    }

    #[test]
    fn member_cannot_manage_group() {
        let group = &test_groups()[0];
        assert!(!can_manage_group("dev-a1", false, group));
        assert!(!can_manage_group("dev-a2", false, group));
    }

    #[test]
    fn group_admin_can_manage() {
        let group = &test_groups()[0];
        assert!(can_manage_group("lead-a", false, group));
    }

    #[test]
    fn super_admin_can_manage_any_group() {
        let group = &test_groups()[0];
        assert!(can_manage_group("random-user", true, group));
    }

    #[test]
    fn service_with_group_visible_to_members() {
        let config = MockConfig {
            services: vec![test_service(Some("team-a"))],
            groups: test_groups(),
        };
        assert_eq!(visible_services("dev-a1", false, &config).len(), 1);
        assert_eq!(visible_services("dev-a2", false, &config).len(), 1);
        assert_eq!(visible_services("outsider", false, &config).len(), 0);
        assert_eq!(visible_services("admin1", true, &config).len(), 1);
    }

    #[test]
    fn show_reset_button_defaults_to_false() {
        let cfg = AuthConfig::from_lookup(vars(&[])).unwrap();
        assert!(!cfg.show_reset_button);
    }

    #[test]
    fn show_reset_button_true_from_env() {
        let cfg = AuthConfig::from_lookup(vars(&[("SHOW_RESET_BUTTON", "true")])).unwrap();
        assert!(cfg.show_reset_button);
    }

    #[test]
    fn authentication_without_its_keycloak_settings_is_refused() {
        let error = AuthConfig::from_lookup(vars(&[("AUTH_ENABLED", "true")])).unwrap_err();
        assert!(error.contains("KEYCLOAK_URL"), "{error}");
    }

    #[test]
    fn authentication_settings_are_read_and_trimmed() {
        let cfg = AuthConfig::from_lookup(vars(&[
            ("AUTH_ENABLED", "TRUE"),
            ("KEYCLOAK_URL", "https://kc.example.com"),
            ("KEYCLOAK_REALM", "realm"),
            ("KEYCLOAK_CLIENT_ID", "mimicway"),
            (
                "KEYCLOAK_ISSUER",
                " https://login.example.com/realms/realm/ ",
            ),
            ("SUPER_ADMINS", " alice, ,bob "),
        ]))
        .unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.issuer, "https://login.example.com/realms/realm");
        assert_eq!(cfg.super_admins, vec!["alice", "bob"]);
    }
}
