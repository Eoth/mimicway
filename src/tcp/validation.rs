// Checks a TcpService before TcpRuntime stores it, with the same error shape as HTTP's `validate_service`
// (ValidationError { field, message }) on another schema: a port and raw byte rules instead of paths and methods.
use crate::i18n::tr;
use crate::tcp::config::{TcpMatcher, TcpService};
use crate::tcp::hex;

static NAME_CHARSET_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"^[A-Za-z0-9_-]+$").unwrap());

#[derive(Debug)]
pub struct ValidationError {
    pub field: &'static str,
    pub message: String,
}

/// `existing`: the other services, the one being validated already left out by the caller (server/api.rs), or a PUT
/// that changes nothing would conflict with itself.
pub fn validate_tcp_service(
    service: &TcpService,
    existing: &[TcpService],
) -> Result<(), ValidationError> {
    let name = service.name.trim();
    let error = |field, message| Err(ValidationError { field, message });

    if name.is_empty() {
        return error("name", tr("The service name is required.", &[]));
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
    // Trimmed on both sides: a name stored with spaces before names were trimmed still takes its slot.
    if existing.iter().any(|s| s.name.trim() == name) {
        return error(
            "name",
            tr("A TCP service named \"{0}\" already exists.", &[&name]),
        );
    }
    if existing
        .iter()
        .any(|s| s.listen_port == service.listen_port)
    {
        return error(
            "listen_port",
            tr(
                "The port {0} is already used by another TCP service.",
                &[&service.listen_port],
            ),
        );
    }

    for (i, rule) in service.rules.iter().enumerate() {
        if rule.name.trim().is_empty() {
            return error("rules", tr("The rule #{0} has no name.", &[&i]));
        }
        match &rule.matcher {
            TcpMatcher::Prefix(hex_str) if hex::decode(hex_str).is_err() => {
                return error(
                    "rules",
                    tr(
                        "Rule \"{0}\": the prefix is not valid hexadecimal.",
                        &[&rule.name],
                    ),
                );
            }
            TcpMatcher::Regex(pattern)
                if crate::engine::regex_cache::check_bytes(pattern).is_err() =>
            {
                return error(
                    "rules",
                    tr("Rule \"{0}\": invalid regex pattern.", &[&rule.name]),
                );
            }
            _ => {}
        }
        if hex::decode(&rule.response_hex).is_err() {
            return error(
                "rules",
                tr(
                    "Rule \"{0}\": the response is not valid hexadecimal.",
                    &[&rule.name],
                ),
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tcp::config::TcpRule;

    fn valid_service() -> TcpService {
        TcpService {
            name: "svc".into(),
            listen_port: 9000,
            rules: vec![TcpRule {
                name: "r".into(),
                matcher: TcpMatcher::Any,
                response_hex: "706f6e67".into(),
            }],
        }
    }

    #[test]
    fn accepts_valid_service() {
        assert!(validate_tcp_service(&valid_service(), &[]).is_ok());
    }

    #[test]
    fn rejects_empty_name() {
        let mut s = valid_service();
        s.name = "  ".into();
        let e = validate_tcp_service(&s, &[]).unwrap_err();
        assert_eq!(e.field, "name");
    }

    #[test]
    fn rejects_name_with_invalid_chars() {
        let mut s = valid_service();
        s.name = "not a name!".into();
        let e = validate_tcp_service(&s, &[]).unwrap_err();
        assert_eq!(e.field, "name");
    }

    #[test]
    fn rejects_duplicate_name() {
        let existing = vec![valid_service()];
        let mut s = valid_service();
        s.listen_port = 9001;
        let e = validate_tcp_service(&s, &existing).unwrap_err();
        assert_eq!(e.field, "name");
    }

    #[test]
    fn a_name_stored_with_spaces_still_takes_its_slot() {
        let mut stored = valid_service();
        stored.name = "svc ".into();
        let mut s = valid_service();
        s.listen_port = 9001;
        let e = validate_tcp_service(&s, &[stored]).unwrap_err();
        assert_eq!(e.field, "name");
    }

    #[test]
    fn rejects_duplicate_port() {
        let existing = vec![valid_service()];
        let mut s = valid_service();
        s.name = "other".into();
        let e = validate_tcp_service(&s, &existing).unwrap_err();
        assert_eq!(e.field, "listen_port");
    }

    #[test]
    fn rejects_rule_without_name() {
        let mut s = valid_service();
        s.rules[0].name = "".into();
        let e = validate_tcp_service(&s, &[]).unwrap_err();
        assert_eq!(e.field, "rules");
    }

    #[test]
    fn rejects_invalid_prefix_hex() {
        let mut s = valid_service();
        s.rules[0].matcher = TcpMatcher::Prefix("zz".into());
        let e = validate_tcp_service(&s, &[]).unwrap_err();
        assert_eq!(e.field, "rules");
    }

    #[test]
    fn rejects_invalid_regex_pattern() {
        let mut s = valid_service();
        s.rules[0].matcher = TcpMatcher::Regex("(unclosed".into());
        let e = validate_tcp_service(&s, &[]).unwrap_err();
        assert_eq!(e.field, "rules");
    }

    #[test]
    fn rejects_invalid_response_hex() {
        let mut s = valid_service();
        s.rules[0].response_hex = "zz".into();
        let e = validate_tcp_service(&s, &[]).unwrap_err();
        assert_eq!(e.field, "rules");
    }

    #[test]
    fn empty_response_hex_is_valid() {
        // A rule that matches and answers nothing (closing at once) is legitimate.
        let mut s = valid_service();
        s.rules[0].response_hex = String::new();
        assert!(validate_tcp_service(&s, &[]).is_ok());
    }
}
