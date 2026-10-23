use crate::i18n::tr;
use crate::models::Service;

const RESERVED_NAMES: &[&str] = &[
    "api",
    "auth",
    "index.html",
    "assets",
    "favicon.ico",
    "runtime-config.json",
];
const VALID_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "HEAD"];

const RESERVED_PATH_PREFIXES: &[&str] = &[
    "/api/",
    "/api",
    "/index.html",
    "/assets/",
    "/favicon.ico",
    "/runtime-config.json",
];

static NAME_CHARSET_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^[A-Za-z0-9_-]+$").unwrap());

// The two names MockStore gives backups: "mock-config-{ts}-{seq}.yaml" (backups/) and "pre-reset-{ts}.yaml"
// (backups/protected/). The character set leaves out path separators and "..", so a name decoded from a URL
// segment cannot leave the backup directory.
static BACKUP_FILENAME_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^[A-Za-z0-9_-]+\.yaml$").unwrap());

pub fn is_reserved_name(name: &str) -> bool {
    let normalized = name.trim().to_lowercase();
    RESERVED_NAMES.contains(&normalized.as_str())
}

pub fn is_internal_route(path: &str) -> bool {
    if path == "/" || path.is_empty() {
        return true;
    }
    let lower = path.to_lowercase();
    RESERVED_PATH_PREFIXES
        .iter()
        .any(|prefix| lower == *prefix || lower.starts_with(prefix))
}

/// True for the management API (`/api` and everything under it, whatever the case), the only surface that
/// requires a token when authentication is enabled. Matched by path segment, so a service named `apis` or
/// `api-gateway` is not mistaken for it.
pub fn is_management_api_route(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower == "/api" || lower.starts_with("/api/")
}

/// True when a segment of `path` is `.` or `..`, literally or percent-encoded (`%2e`, any case). Such a request
/// is refused before any proxying: URL parsing resolves those segments, so `/svc/../admin` would reach a path of
/// the real backend outside the base path configured for the service.
pub fn has_dot_segment(path: &str) -> bool {
    path.split('/').any(|segment| {
        let decoded = segment.to_ascii_lowercase().replace("%2e", ".");
        decoded == "." || decoded == ".."
    })
}

#[derive(Debug)]
pub struct ValidationError {
    pub field: &'static str,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

pub fn validate_service(service: &Service) -> Result<(), ValidationError> {
    let name = service.name.trim();
    let error = |field, message| Err(ValidationError { field, message });

    if name.is_empty() {
        return error("name", tr("The service name is required.", &[]));
    }
    if is_reserved_name(name) {
        return error(
            "name",
            tr(
                "The name \"{0}\" is reserved by Mimicway (forbidden names: {1}).",
                &[&name, &RESERVED_NAMES.join(", ")],
            ),
        );
    }
    if name.contains('/') || name.contains('\\') {
        return error(
            "name",
            tr(
                "A service name cannot contain a path separator (/ or \\).",
                &[],
            ),
        );
    }
    if !NAME_CHARSET_RE.is_match(name) {
        return error(
            "name",
            tr(
                "A service name can only contain letters, digits, dashes (-) and underscores (_).",
                &[],
            ),
        );
    }

    let effective = format!(
        "/{}/{}",
        name,
        service.listen_path.trim().trim_start_matches('/')
    );
    if is_internal_route(&effective) {
        return error(
            "listen_path",
            tr(
                "The resulting pattern \"{0}\" would conflict with an internal route of Mimicway.",
                &[&effective],
            ),
        );
    }

    // An empty real_target_url is how a purely mocked service is written, not an omission. A direct proxy
    // (is_mocked=false) without a target would forward every request to an empty URL: refused at the source.
    if !service.is_mocked && service.real_target_url.trim().is_empty() {
        return error(
            "real_target_url",
            tr(
                "A service without a target cannot be a direct proxy (is_mocked=false): switch the mock mode on or set a target.",
                &[],
            ),
        );
    }

    let mut seen_rules = std::collections::HashSet::new();
    for rule in &service.rules {
        let rn = rule.name.trim();
        if rn.is_empty() {
            return error("rules", tr("The rule name is required.", &[]));
        }
        let method_upper = rule.method.trim().to_uppercase();
        if !VALID_METHODS.contains(&method_upper.as_str()) {
            return error(
                "rules",
                tr(
                    "Invalid HTTP method \"{0}\" for the rule \"{1}\". Accepted values: {2}.",
                    &[&rule.method, &rn, &VALID_METHODS.join(", ")],
                ),
            );
        }
        // A pattern that does not compile would never match, silently: refuse it when it is saved.
        for condition in rule.conditions.all_of.iter().chain(&rule.conditions.any_of) {
            if let crate::models::Operator::Regex(pattern) = &condition.operator
                && let Err(reason) = crate::engine::regex_cache::check_text(pattern)
            {
                return error(
                    "rules",
                    tr(
                        "Rule \"{0}\": invalid regular expression \"{1}\": {2}",
                        &[&rn, pattern, &reason],
                    ),
                );
            }
        }
        if !seen_rules.insert(rn.to_lowercase()) {
            return error(
                "rules",
                tr(
                    "The rule name \"{0}\" is used several times in this service. Each rule needs a unique name.",
                    &[&rn],
                ),
            );
        }
    }

    Ok(())
}

/// Validates a backup file name taken from a URL segment (`POST /api/config/restore/:filename`) before any disk
/// access. `..` and path separators are refused before the character set is checked, so that a traversal attempt
/// gets its own message rather than a mere "invalid format".
pub fn validate_backup_filename(filename: &str) -> Result<(), ValidationError> {
    let name = filename.trim();
    let error = |message| {
        Err(ValidationError {
            field: "filename",
            message,
        })
    };

    if name.is_empty() {
        return error(tr("The backup file name is required.", &[]));
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return error(tr(
            "A file name cannot contain a path separator (/ or \\) nor \"..\".",
            &[],
        ));
    }
    if !BACKUP_FILENAME_RE.is_match(name) {
        return error(tr("Invalid backup file name.", &[]));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Service;

    use crate::models::{BodyFragment, MockResponse, Rule, RuleAction, WsdlMode};

    fn svc(name: &str, listen_path: &str) -> Service {
        Service {
            name: name.into(),
            listen_path: listen_path.into(),
            real_target_url: "http://example.com".into(),
            is_mocked: false,
            rewrite_directory_urls: false,
            group_name: None,
            wsdl_mode: WsdlMode::default(),
            rules: vec![],
        }
    }

    fn svc_with_rules(name: &str, rule_names: &[&str]) -> Service {
        let mut s = svc(name, "/v1/*");
        s.rules = rule_names
            .iter()
            .map(|rn| Rule {
                name: rn.to_string(),
                method: "GET".into(),
                sub_path: None,
                action: RuleAction::default(),
                script: None,
                response_mode: None,
                conditions: Default::default(),
                response: MockResponse {
                    status: 200,
                    headers: vec![],
                    body: vec![BodyFragment::Literal { value: "ok".into() }],
                    chaos: None,
                },
            })
            .collect();
        s
    }

    #[test]
    fn reject_empty_name() {
        assert!(validate_service(&svc("", "/foo")).is_err());
        assert!(validate_service(&svc("  ", "/foo")).is_err());
    }

    #[test]
    fn reject_reserved_names() {
        assert!(validate_service(&svc("api", "/foo")).is_err());
        assert!(validate_service(&svc("API", "/foo")).is_err());
        assert!(validate_service(&svc("index.html", "/foo")).is_err());
        assert!(validate_service(&svc("assets", "/foo")).is_err());
        assert!(validate_service(&svc("favicon.ico", "/foo")).is_err());
        // The character set alone would refuse it; the reserved list states it anyway, as for index.html.
        assert!(validate_service(&svc("runtime-config.json", "/foo")).is_err());
    }

    #[test]
    fn reject_name_with_slashes() {
        assert!(validate_service(&svc("my/svc", "/foo")).is_err());
        assert!(validate_service(&svc("my\\svc", "/foo")).is_err());
    }

    #[test]
    fn reject_name_with_spaces() {
        assert!(validate_service(&svc("my svc", "/foo")).is_err());
    }

    #[test]
    fn reject_name_with_special_chars() {
        assert!(validate_service(&svc("svc@name", "/foo")).is_err());
        assert!(validate_service(&svc("svc.name", "/foo")).is_err());
        assert!(validate_service(&svc("100%svc", "/foo")).is_err());
        assert!(validate_service(&svc("caf\u{e9}-svc", "/foo")).is_err());
        assert!(validate_service(&svc("svc#1", "/foo")).is_err());
    }

    #[test]
    fn accept_name_with_underscore_and_digits() {
        assert!(validate_service(&svc("svc_v2-42", "/foo")).is_ok());
        assert!(validate_service(&svc("SVC_NAME", "/foo")).is_ok());
    }

    #[test]
    fn accept_empty_listen_path() {
        assert!(validate_service(&svc("my-svc", "")).is_ok());
        assert!(validate_service(&svc("my-svc", "  ")).is_ok());
        assert!(validate_service(&svc("my-svc", "/")).is_ok());
    }

    #[test]
    fn accept_root_wildcard() {
        assert!(validate_service(&svc("my-svc", "/*")).is_ok());
        assert!(validate_service(&svc("my-svc", "*")).is_ok());
    }

    #[test]
    fn accept_scoped_wildcard() {
        assert!(validate_service(&svc("my-svc", "/v1/*")).is_ok());
        assert!(validate_service(&svc("my-svc", "/users/{id}")).is_ok());
    }

    #[test]
    fn reject_effective_pattern_hitting_internal() {
        assert!(validate_service(&svc("api", "/config")).is_err());
    }

    #[test]
    fn accept_valid_service() {
        assert!(validate_service(&svc("insee", "/v4/sirene/{siret}")).is_ok());
        assert!(validate_service(&svc("users-api", "/v1/users/{id}")).is_ok());
    }

    #[test]
    fn accept_empty_target_when_purely_mocked() {
        // A purely mocked service: no target is fine while the rules answer (is_mocked stays true).
        let mut s = svc("purely-mocked", "/v1/*");
        s.real_target_url = "".into();
        s.is_mocked = true;
        assert!(validate_service(&s).is_ok());
    }

    #[test]
    fn reject_empty_target_without_is_mocked() {
        // A pure proxy without a target would forward every request to an empty URL.
        let mut s = svc("broken", "/v1/*");
        s.real_target_url = "".into();
        s.is_mocked = false;
        let err = validate_service(&s).unwrap_err();
        assert_eq!(err.field, "real_target_url");
    }

    #[test]
    fn reject_whitespace_only_target_without_is_mocked() {
        let mut s = svc("broken2", "/v1/*");
        s.real_target_url = "   ".into();
        s.is_mocked = false;
        assert!(validate_service(&s).is_err());
    }

    #[test]
    fn is_internal_route_works() {
        assert!(is_internal_route("/"));
        assert!(is_internal_route(""));
        assert!(is_internal_route("/api/services"));
        assert!(is_internal_route("/api"));
        assert!(is_internal_route("/index.html"));
        assert!(is_internal_route("/assets/main.js"));
        assert!(is_internal_route("/runtime-config.json"));
        assert!(!is_internal_route("/my-svc/foo"));
        assert!(!is_internal_route("/insee/v4/sirene/123"));
    }

    #[test]
    fn dot_segments_are_detected_literal_or_encoded() {
        for path in [
            "/svc/../admin",
            "/svc/./x",
            "/svc/%2e%2e/admin",
            "/svc/%2E./admin",
            "/svc/.%2e",
            "/svc/..",
        ] {
            assert!(has_dot_segment(path), "{path}");
        }
        for path in [
            "/svc/...",
            "/svc/a..b",
            "/svc/.hidden",
            "/svc/v1.2/x",
            "/svc/%2e%2e%2e",
            "/",
        ] {
            assert!(!has_dot_segment(path), "{path}");
        }
    }

    #[test]
    fn management_api_route_covers_the_api_whatever_the_case() {
        assert!(is_management_api_route("/api"));
        assert!(is_management_api_route("/api/services"));
        assert!(is_management_api_route("/api/auth/me"));
        assert!(is_management_api_route("/API/services"));
        assert!(is_management_api_route("/api/../api/config"));
    }

    #[test]
    fn management_api_route_excludes_the_spa_shell_and_service_traffic() {
        for path in [
            "/",
            "",
            "/index.html",
            "/assets/main.js",
            "/favicon.ico",
            "/runtime-config.json",
            "/my-svc/foo",
            "/abcde/my-svc/foo",
            "/apis/x",
            "/api-gateway/x",
        ] {
            assert!(!is_management_api_route(path), "{path}");
        }
    }

    #[test]
    fn reject_a_rule_whose_regex_does_not_compile() {
        let mut service = svc_with_rules("svc", &["r1"]);
        service.rules[0]
            .conditions
            .any_of
            .push(crate::models::Condition {
                source: crate::models::ConditionSource::QueryParam("id".into()),
                operator: crate::models::Operator::Regex("([0-9]+".into()),
            });
        let error = validate_service(&service).unwrap_err();
        assert!(error.message.contains("([0-9]+"), "{}", error.message);
        service.rules[0].conditions.any_of[0].operator =
            crate::models::Operator::Regex("^[0-9]+$".into());
        assert!(validate_service(&service).is_ok());
    }

    #[test]
    fn reject_duplicate_rule_names() {
        assert!(validate_service(&svc_with_rules("svc", &["r1", "r1"])).is_err());
    }

    #[test]
    fn reject_duplicate_rule_names_case_insensitive() {
        assert!(validate_service(&svc_with_rules("svc", &["Rule-A", "rule-a"])).is_err());
    }

    #[test]
    fn accept_unique_rule_names() {
        assert!(validate_service(&svc_with_rules("svc", &["r1", "r2", "r3"])).is_ok());
    }

    #[test]
    fn reject_empty_rule_name() {
        assert!(validate_service(&svc_with_rules("svc", &[""])).is_err());
    }

    #[test]
    fn accept_same_rule_name_across_services() {
        assert!(validate_service(&svc_with_rules("svc-a", &["shared-rule"])).is_ok());
        assert!(validate_service(&svc_with_rules("svc-b", &["shared-rule"])).is_ok());
    }

    #[test]
    fn accept_valid_backup_filenames() {
        assert!(validate_backup_filename("mock-config-1690000000000-000001.yaml").is_ok());
        assert!(validate_backup_filename("pre-reset-1690000000000.yaml").is_ok());
    }

    #[test]
    fn reject_empty_backup_filename() {
        assert!(validate_backup_filename("").is_err());
        assert!(validate_backup_filename("   ").is_err());
    }

    #[test]
    fn reject_backup_filename_path_traversal() {
        assert!(validate_backup_filename("../mock-config.yaml").is_err());
        assert!(validate_backup_filename("../../etc/passwd").is_err());
        assert!(validate_backup_filename("protected/../../mock-config.yaml").is_err());
        assert!(validate_backup_filename("..\\mock-config.yaml").is_err());
    }

    #[test]
    fn reject_backup_filename_with_path_separators() {
        assert!(validate_backup_filename("protected/pre-reset-1.yaml").is_err());
        assert!(validate_backup_filename("subdir\\mock-config.yaml").is_err());
        assert!(validate_backup_filename("/etc/passwd").is_err());
    }

    #[test]
    fn reject_backup_filename_wrong_extension_or_charset() {
        assert!(validate_backup_filename("mock-config.txt").is_err());
        assert!(validate_backup_filename("mock config.yaml").is_err());
        assert!(validate_backup_filename("mock-config.yaml.bak").is_err());
        assert!(validate_backup_filename("caf\u{e9}.yaml").is_err());
    }
}
