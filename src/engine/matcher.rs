use crate::models::{Condition, ConditionGroup, ConditionSource, Operator, Rule};
use serde::Serialize;
use std::collections::HashMap;

pub struct MatchEngine;

#[derive(Debug, Clone)]
pub struct RequestData {
    pub query_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub path_params: HashMap<String, String>,
    pub method: String,
    pub remaining_path: String,
}

/// How one condition was judged, for the rule tester only; the production path (`matches_group`) stays a plain
/// boolean so as not to weigh on every request. Holds the value found (or its absence) and, when the condition
/// fails, a hint when the same key exists in another source of the request (a path parameter typed as a query
/// parameter, for instance).
#[derive(Debug, Clone, Serialize)]
pub struct ConditionEvaluation {
    pub condition: Condition,
    pub matched: bool,
    pub found_value: Option<String>,
    pub hint: Option<String>,
}

/// How a `ConditionGroup` (all_of and any_of) was judged: the detailed twin of `MatchEngine::matches_group` (see
/// `evaluate_group` for why the all/any glue is written twice).
#[derive(Debug, Clone, Serialize)]
pub struct GroupEvaluation {
    pub all_of: Vec<ConditionEvaluation>,
    pub any_of: Vec<ConditionEvaluation>,
    pub matched: bool,
}

/// Input of the rule tester: the rule being edited, saved or not, tested against a captured request.
pub struct RuleTestInput<'a> {
    pub method: &'a str,
    pub sub_path: &'a Option<String>,
    pub conditions: &'a ConditionGroup,
}

/// Result of the rule tester: method, sub-path and conditions, each with its own verdict, and the detail of each
/// condition.
#[derive(Debug, Clone, Serialize)]
pub struct RuleTestOutcome {
    pub method_matches: bool,
    pub sub_path_matches: bool,
    pub path_params: HashMap<String, String>,
    pub group: GroupEvaluation,
    pub overall_matched: bool,
}

/// The rule about to be saved (created or edited), for conflict detection (`find_rule_conflicts`). Same shape as
/// `RuleTestInput`, a separate type because it serves another purpose.
pub struct RuleConflictDraft<'a> {
    pub method: &'a str,
    pub sub_path: &'a Option<String>,
    pub conditions: &'a ConditionGroup,
}

/// Another rule of the same service, as stored; its position is its index in the slice given to
/// `find_rule_conflicts`.
pub struct OtherRuleConflictInput<'a> {
    pub name: &'a str,
    pub method: &'a str,
    pub sub_path: &'a Option<String>,
    pub conditions: &'a ConditionGroup,
}

/// Which of two overlapping rules would really apply, given the current order (first match wins). Informative
/// only: it explains, it changes nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ConflictWinner {
    /// The rule being saved comes first: the other rule would be hidden for the requests concerned.
    Draft,
    /// The other rule, already stored before it, comes first: the rule being saved would never trigger for these
    /// requests.
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleConflict {
    pub other_rule_name: String,
    pub winner: ConflictWinner,
}

impl MatchEngine {
    pub fn first_match<'a>(
        rules: &'a [Rule],
        req: &RequestData,
    ) -> Option<(&'a Rule, HashMap<String, String>)> {
        rules.iter().find_map(|rule| {
            if !Self::matches_method(&rule.method, &req.method) {
                return None;
            }
            let sub_params = Self::matches_sub_path(&rule.sub_path, &req.remaining_path)?;
            if !Self::matches_group(&rule.conditions, req, Some(&sub_params)) {
                return None;
            }
            Some((rule, sub_params))
        })
    }

    /// Shared by the production path (`first_match`) and the rule tester (`evaluate_rule_test`).
    pub(crate) fn matches_method(rule_method: &str, request_method: &str) -> bool {
        rule_method.eq_ignore_ascii_case(request_method)
    }

    /// Shared with the rule tester as well.
    pub(crate) fn matches_sub_path(
        sub_path: &Option<String>,
        remaining: &str,
    ) -> Option<HashMap<String, String>> {
        match sub_path {
            None => Some(HashMap::new()),
            Some(pattern) => match_path(pattern, remaining).map(|(params, _)| params),
        }
    }

    /// Also used by `messaging::matcher` to match a Kafka message against a rule's conditions, without the HTTP-only
    /// method and sub-path (`sub_params` is then `None`).
    ///
    /// `sub_params` holds what the rule's own sub-path pattern captured (`/orders/{id}`): a path parameter condition
    /// reads it first, then the service's listen path parameters, the precedence the rendered response and the rule
    /// tester (`evaluate_rule_test`) follow. Passed by reference so that the request is never copied per rule.
    pub(crate) fn matches_group(
        group: &ConditionGroup,
        req: &RequestData,
        sub_params: Option<&HashMap<String, String>>,
    ) -> bool {
        let eval = |c: &Condition| Self::eval(c, req, sub_params);
        let all_ok = group.all_of.is_empty() || group.all_of.iter().all(eval);
        let any_ok = group.any_of.is_empty() || group.any_of.iter().any(eval);
        all_ok && any_ok
    }

    fn eval(
        condition: &Condition,
        req: &RequestData,
        sub_params: Option<&HashMap<String, String>>,
    ) -> bool {
        let extracted = Self::extract(&condition.source, req, sub_params);
        Self::apply_op(&condition.operator, extracted.as_deref())
    }

    /// The detailed version of `matches_group`, for the rule tester only. It uses the same primitives (`extract`,
    /// `apply_op`), so no matching logic is duplicated; only the all/any glue below is.
    ///
    /// `matches_group` does not delegate to it because production calls `matches_group` on every request, and building
    /// the detail (cloned conditions, hints, strings) only to drop it would slow that path for nothing. A test
    /// (`evaluate_group_matches_agree_with_matches_group`) keeps the two in agreement instead.
    pub fn evaluate_group(group: &ConditionGroup, req: &RequestData) -> GroupEvaluation {
        let all_of: Vec<ConditionEvaluation> = group
            .all_of
            .iter()
            .map(|c| Self::eval_detailed(c, req))
            .collect();
        let any_of: Vec<ConditionEvaluation> = group
            .any_of
            .iter()
            .map(|c| Self::eval_detailed(c, req))
            .collect();
        let all_ok = all_of.is_empty() || all_of.iter().all(|e| e.matched);
        let any_ok = any_of.is_empty() || any_of.iter().any(|e| e.matched);
        GroupEvaluation {
            all_of,
            any_of,
            matched: all_ok && any_ok,
        }
    }

    fn eval_detailed(condition: &Condition, req: &RequestData) -> ConditionEvaluation {
        let found_value = Self::extract(&condition.source, req, None);
        let matched = Self::apply_op(&condition.operator, found_value.as_deref());
        let hint = if matched {
            None
        } else {
            Self::cross_source_hint(&condition.source, req)
        };
        ConditionEvaluation {
            condition: condition.clone(),
            matched,
            found_value,
            hint,
        }
    }

    /// When a condition on a named key (query parameter, header, path parameter, form field) fails, looks for the same
    /// key in another source of the captured request, to point at a probably wrong source choice. JSON pointers, XPath
    /// and the raw body are paths or whole bodies, not names that compare across sources: no hint for them.
    fn cross_source_hint(source: &ConditionSource, req: &RequestData) -> Option<String> {
        let query = || crate::i18n::tr("query parameter", &[]);
        let header = || crate::i18n::tr("header", &[]);
        let path = || crate::i18n::tr("path parameter", &[]);
        let form = || crate::i18n::tr("form field", &[]);
        let (key, current_label) = match source {
            ConditionSource::QueryParam(k) => (k, query()),
            ConditionSource::Header(k) => (k, header()),
            ConditionSource::PathParam(k) => (k, path()),
            ConditionSource::FormField(k) => (k, form()),
            _ => return None,
        };

        let mut found_in = Vec::new();
        if !matches!(source, ConditionSource::PathParam(_)) && req.path_params.contains_key(key) {
            found_in.push(path());
        }
        if !matches!(source, ConditionSource::QueryParam(_)) && req.query_params.contains_key(key) {
            found_in.push(query());
        }
        if !matches!(source, ConditionSource::Header(_))
            && req.headers.keys().any(|h| h.eq_ignore_ascii_case(key))
        {
            found_in.push(header());
        }

        if found_in.is_empty() {
            None
        } else {
            Some(crate::i18n::tr(
                "'{0}' was not found as a {1}, but it is present as a {2} in this request",
                &[key, &current_label, &found_in.join(", ")],
            ))
        }
    }

    /// The rule tester (`POST /api/rule-test`): evaluates method, sub-path and conditions of the rule being edited
    /// against a captured request, with no change and no network call, through the same `matches_method`,
    /// `matches_sub_path` and `evaluate_group` as `first_match`.
    pub fn evaluate_rule_test(input: RuleTestInput, req: &RequestData) -> RuleTestOutcome {
        let method_matches = Self::matches_method(input.method, &req.method);
        let sub_params = Self::matches_sub_path(input.sub_path, &req.remaining_path);
        let sub_path_matches = sub_params.is_some();

        let mut path_params = req.path_params.clone();
        if let Some(p) = sub_params {
            path_params.extend(p);
        }

        let merged_req = RequestData {
            path_params: path_params.clone(),
            ..req.clone()
        };
        let group = Self::evaluate_group(input.conditions, &merged_req);

        RuleTestOutcome {
            method_matches,
            sub_path_matches,
            overall_matched: method_matches && sub_path_matches && group.matched,
            path_params,
            group,
        }
    }

    /// Conflict detection when a rule is saved (`POST /api/rule-conflicts`, stateless); never called on the
    /// production path. Two rules conflict when one request could satisfy both (same method, compatible sub-paths,
    /// overlapping conditions): only the first in `rules` order would then apply.
    ///
    /// `other_rules` are the service's other rules in their current order (the one being edited left out by the
    /// caller). `draft_position` is where the saved rule will sit: its index for an edit in place, or
    /// `other_rules.len()` for a new rule (the UI appends it). The winner follows from that position alone: a rule with
    /// a lower index is evaluated first and wins, and the reverse.
    ///
    /// **Detection is pragmatic, not exhaustive**, on purpose:
    /// - `method`: equal, ignoring case; rules on different methods can never match the same request.
    /// - `sub_path`: compatible when both are absent, when one is (`None` matches any remaining path), or when both
    ///   patterns have the same shape once parameters are reduced to a placeholder (`/a/{id}` and `/a/{orderId}` are
    ///   compatible, `/a/{id}` and `/b` are not). Patterns that overlap without having the same shape (`/a/*` and
    ///   `/a/{id}/b`) are missed: a false negative rather than a full pattern overlap engine.
    /// - `conditions`: only the clear cases: identical `all_of` and `any_of`, or one `all_of` strictly included in the
    ///   other with both `any_of` empty (a general rule next to a more specific one). As soon as an `any_of` is
    ///   non-empty and the two differ, nothing is reported: OR semantics would need real modeling, and a missed
    ///   conflict is better than a false alarm.
    ///
    /// It compares conditions with their derived `PartialEq` and patterns with `normalize_colon_syntax`, but not with
    /// `matches_group`, which judges conditions against a concrete request; here two sets of conditions are compared
    /// with no request at all.
    pub fn find_rule_conflicts(
        draft: &RuleConflictDraft,
        other_rules: &[OtherRuleConflictInput],
        draft_position: usize,
    ) -> Vec<RuleConflict> {
        other_rules
            .iter()
            .enumerate()
            .filter(|(_, other)| Self::rules_could_overlap(draft, other))
            .map(|(i, other)| RuleConflict {
                other_rule_name: other.name.to_string(),
                winner: if i < draft_position {
                    ConflictWinner::Other
                } else {
                    ConflictWinner::Draft
                },
            })
            .collect()
    }

    fn rules_could_overlap(draft: &RuleConflictDraft, other: &OtherRuleConflictInput) -> bool {
        draft.method.eq_ignore_ascii_case(other.method)
            && Self::sub_paths_could_overlap(draft.sub_path, other.sub_path)
            && Self::conditions_could_overlap(draft.conditions, other.conditions)
    }

    fn sub_paths_could_overlap(a: &Option<String>, b: &Option<String>) -> bool {
        match (a, b) {
            (None, _) | (_, None) => true,
            (Some(pa), Some(pb)) => {
                Self::canonicalize_sub_path(pa) == Self::canonicalize_sub_path(pb)
            }
        }
    }

    fn canonicalize_sub_path(pattern: &str) -> String {
        normalize_colon_syntax(pattern)
            .split('/')
            .map(|seg| {
                if seg.starts_with('{') && seg.ends_with('}') && seg.len() >= 2 {
                    "{}"
                } else {
                    seg
                }
            })
            .collect::<Vec<_>>()
            .join("/")
    }

    fn conditions_could_overlap(a: &ConditionGroup, b: &ConditionGroup) -> bool {
        if Self::condition_sets_equal(&a.all_of, &b.all_of)
            && Self::condition_sets_equal(&a.any_of, &b.any_of)
        {
            return true;
        }
        a.any_of.is_empty()
            && b.any_of.is_empty()
            && (Self::is_strict_subset(&a.all_of, &b.all_of)
                || Self::is_strict_subset(&b.all_of, &a.all_of))
    }

    fn condition_sets_equal(a: &[Condition], b: &[Condition]) -> bool {
        a.len() == b.len() && a.iter().all(|c| b.contains(c))
    }

    fn is_strict_subset(smaller: &[Condition], larger: &[Condition]) -> bool {
        smaller.len() < larger.len() && smaller.iter().all(|c| larger.contains(c))
    }

    fn extract(
        source: &ConditionSource,
        req: &RequestData,
        sub_params: Option<&HashMap<String, String>>,
    ) -> Option<String> {
        match source {
            ConditionSource::QueryParam(key) => req.query_params.get(key).cloned(),
            ConditionSource::Header(key) => {
                let lower = key.to_ascii_lowercase();
                req.headers
                    .iter()
                    .find(|(k, _)| k.to_ascii_lowercase() == lower)
                    .map(|(_, v)| v.clone())
            }
            ConditionSource::JsonPointer(pointer) => Self::extract_json_pointer(&req.body, pointer),
            ConditionSource::XPath(path) => Self::extract_xpath(&req.body, path),
            ConditionSource::FormField(field) => Self::extract_form_field(&req.body, field),
            ConditionSource::PathParam(key) => sub_params
                .and_then(|p| p.get(key))
                .or_else(|| req.path_params.get(key))
                .cloned(),
            ConditionSource::BodyRaw => String::from_utf8(req.body.clone()).ok(),
        }
    }

    fn extract_json_pointer(body: &[u8], pointer: &str) -> Option<String> {
        let value: serde_json::Value = serde_json::from_slice(body).ok()?;
        let found = value.pointer(pointer)?;
        match found {
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        }
    }

    // Also used by `engine::template::resolve_variable` for {{xpath.path}}, so XML is parsed in one place.
    pub(crate) fn extract_xpath(body: &[u8], path: &str) -> Option<String> {
        let text = std::str::from_utf8(body).ok()?;
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        Self::walk_xml(text, &segments)
    }

    // Tracks the real stack of ancestors (local names, in nesting order) and compares the whole stack with the expected
    // segments. A flat depth counter, moved on any end event, broke as soon as an unrelated sibling element (an empty
    // <Header></Header> written in full before <Body>) closed between two matched segments.
    fn walk_xml(xml: &str, segments: &[&str]) -> Option<String> {
        use quick_xml::events::Event;
        use quick_xml::reader::Reader;

        let mut reader = Reader::from_str(xml);
        let mut stack: Vec<String> = Vec::new();
        let mut capture = false;
        let mut result = String::new();

        let path_matches = |stack: &[String]| {
            stack.len() == segments.len()
                && stack
                    .iter()
                    .map(String::as_str)
                    .eq(segments.iter().copied())
        };

        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    stack.push(Self::local_name(&e));
                    if !capture && path_matches(&stack) {
                        capture = true;
                    }
                }
                Ok(Event::Empty(e)) => {
                    // A self-closing element (`<tag/>`): no separate text or end event follows.
                    stack.push(Self::local_name(&e));
                    if path_matches(&stack) {
                        return Some(result);
                    }
                    stack.pop();
                }
                Ok(event @ (Event::Text(_) | Event::GeneralRef(_))) => {
                    if capture && let Some(text) = Self::xml_text(&event) {
                        result.push_str(&text);
                    }
                }
                Ok(Event::End(_)) => {
                    if capture {
                        return Some(result);
                    }
                    stack.pop();
                }
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
        }
        None
    }

    // Also used by the parse_xml_items script function, so namespace prefixes (`soap:Body` -> `Body`) are dropped in
    // one place.
    pub(crate) fn local_name(e: &quick_xml::events::BytesStart<'_>) -> String {
        let name = e.name();
        let full: &str = name.as_ref();
        full.split(':').next_back().unwrap_or(full).to_string()
    }

    /// The text a text or reference event stands for. The reader reports `&amp;`, `&#233;`... as reference
    /// events of their own: the five predefined entities and character references are resolved, any other
    /// reference is dropped (no DTD is ever read, so it cannot be defined).
    pub(crate) fn xml_text(event: &quick_xml::events::Event<'_>) -> Option<String> {
        use quick_xml::events::Event;
        match event {
            Event::Text(text) => Some(text.xml10_content().into_owned()),
            Event::GeneralRef(reference) => match reference.resolve_char_ref() {
                Ok(Some(c)) => Some(c.to_string()),
                Ok(None) => quick_xml::escape::resolve_xml_entity(&reference.xml10_content())
                    .map(str::to_string),
                Err(_) => None,
            },
            _ => None,
        }
    }

    fn extract_form_field(body: &[u8], field: &str) -> Option<String> {
        let text = std::str::from_utf8(body).ok()?;
        url::form_urlencoded::parse(text.as_bytes())
            .find(|(k, _)| k == field)
            .map(|(_, v)| v.into_owned())
    }

    fn apply_op(op: &Operator, value: Option<&str>) -> bool {
        match op {
            Operator::Exists => value.is_some(),
            Operator::Eq(expected) => value == Some(expected.as_str()),
            Operator::Contains(sub) => value.is_some_and(|v| v.contains(sub.as_str())),
            Operator::Regex(pattern) => {
                let Some(v) = value else { return false };
                crate::engine::regex_cache::text(pattern).is_some_and(|re| re.is_match(v))
            }
        }
    }
}

/// Percent-decodes `s` into UTF-8 (an invalid sequence becomes U+FFFD): `%C3%A9` is `é`, in any script. Decoding
/// byte by byte into chars read every escaped byte as Latin-1, so non-ASCII path parameters came out garbled.
pub(crate) fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex_digit(bytes[i + 1]), hex_digit(bytes[i + 2]))
        {
            decoded.push(high << 4 | low);
            i += 3;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_digit(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|d| d as u8)
}

pub(crate) fn match_path(
    listen_path: &str,
    request_path: &str,
) -> Option<(HashMap<String, String>, String)> {
    let pattern_str = normalize_colon_syntax(listen_path);

    let decoded_pat: Vec<String> = pattern_str
        .split('/')
        .filter(|s| !s.is_empty())
        .map(url_decode)
        .collect();
    let pattern_segs: Vec<&str> = decoded_pat.iter().map(|s| s.as_str()).collect();
    let decoded_req: Vec<String> = request_path
        .split('/')
        .filter(|s| !s.is_empty())
        .map(url_decode)
        .collect();
    let request_segs: Vec<&str> = decoded_req.iter().map(|s| s.as_str()).collect();

    if pattern_segs.is_empty() {
        return None;
    }

    let mut params = HashMap::new();
    let mut has_wildcard = false;
    let mut matched_count = 0;

    for (i, pat) in pattern_segs.iter().enumerate() {
        if *pat == "*" {
            has_wildcard = true;
            matched_count = i;
            break;
        }
        if i >= request_segs.len() {
            return None;
        }
        if pat.starts_with('{') && pat.ends_with('}') {
            let name = &pat[1..pat.len() - 1];
            params.insert(name.to_string(), request_segs[i].to_string());
        } else if *pat != request_segs[i] {
            return None;
        }
        matched_count = i + 1;
    }

    if !has_wildcard && request_segs.len() != pattern_segs.len() {
        return None;
    }

    let remaining = if matched_count < request_segs.len() {
        format!("/{}", request_segs[matched_count..].join("/"))
    } else {
        String::new()
    };

    Some((params, remaining))
}

fn normalize_colon_syntax(s: &str) -> String {
    s.split('/')
        .map(|seg| {
            if let Some(name) = seg.strip_prefix(':') {
                format!("{{{name}}}")
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;

    fn make_req(
        query: &[(&str, &str)],
        headers: &[(&str, &str)],
        body: &[u8],
        ct: Option<&str>,
    ) -> RequestData {
        RequestData {
            query_params: query
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.to_vec(),
            content_type: ct.map(String::from),
            path_params: HashMap::new(),
            method: "GET".into(),
            remaining_path: String::new(),
        }
    }

    fn simple_rule(name: &str, conditions: ConditionGroup) -> Rule {
        Rule {
            name: name.into(),
            method: "GET".into(),
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
                body: vec![BodyFragment::Literal { value: name.into() }],
                chaos: None,
            },
        }
    }

    #[test]
    fn first_match_returns_first_matching_rule() {
        let rules = vec![
            simple_rule(
                "r1",
                ConditionGroup {
                    all_of: vec![Condition {
                        source: ConditionSource::Header("x-env".into()),
                        operator: Operator::Eq("prod".into()),
                    }],
                    any_of: vec![],
                },
            ),
            simple_rule(
                "r2",
                ConditionGroup {
                    all_of: vec![Condition {
                        source: ConditionSource::Header("x-env".into()),
                        operator: Operator::Eq("staging".into()),
                    }],
                    any_of: vec![],
                },
            ),
        ];
        let req = make_req(&[], &[("x-env", "staging")], b"", None);
        let (matched, _) = MatchEngine::first_match(&rules, &req).unwrap();
        assert_eq!(matched.name, "r2");
    }

    #[test]
    fn no_match_returns_none() {
        let rules = vec![simple_rule(
            "r1",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::Header("x-env".into()),
                    operator: Operator::Eq("prod".into()),
                }],
                any_of: vec![],
            },
        )];
        let req = make_req(&[], &[("x-env", "dev")], b"", None);
        assert!(MatchEngine::first_match(&rules, &req).is_none());
    }

    #[test]
    fn empty_conditions_always_match() {
        let rules = vec![simple_rule("catch-all", ConditionGroup::default())];
        let req = make_req(&[], &[], b"", None);
        let (matched, _) = MatchEngine::first_match(&rules, &req).unwrap();
        assert_eq!(matched.name, "catch-all");
    }

    #[test]
    fn query_param_eq() {
        let rules = vec![simple_rule(
            "qp",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::QueryParam("id".into()),
                    operator: Operator::Eq("42".into()),
                }],
                any_of: vec![],
            },
        )];
        let req = make_req(&[("id", "42")], &[], b"", None);
        assert!(MatchEngine::first_match(&rules, &req).is_some());
    }

    #[test]
    fn header_case_insensitive() {
        let rules = vec![simple_rule(
            "hdr",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::Header("Content-Type".into()),
                    operator: Operator::Contains("json".into()),
                }],
                any_of: vec![],
            },
        )];
        let req = make_req(&[], &[("content-type", "application/json")], b"", None);
        assert!(MatchEngine::first_match(&rules, &req).is_some());
    }

    #[test]
    fn json_pointer_extraction() {
        let body = br#"{"user":{"role":"admin","id":5}}"#;
        let rules = vec![simple_rule(
            "jp",
            ConditionGroup {
                all_of: vec![
                    Condition {
                        source: ConditionSource::JsonPointer("/user/role".into()),
                        operator: Operator::Eq("admin".into()),
                    },
                    Condition {
                        source: ConditionSource::JsonPointer("/user/id".into()),
                        operator: Operator::Eq("5".into()),
                    },
                ],
                any_of: vec![],
            },
        )];
        let req = make_req(&[], &[], body, Some("application/json"));
        assert!(MatchEngine::first_match(&rules, &req).is_some());
    }

    #[test]
    fn xpath_extraction() {
        let body = br#"<Envelope><Body><id>123</id></Body></Envelope>"#;
        let rules = vec![simple_rule(
            "xp",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::XPath("Envelope/Body/id".into()),
                    operator: Operator::Eq("123".into()),
                }],
                any_of: vec![],
            },
        )];
        let req = make_req(&[], &[], body, Some("text/xml"));
        assert!(MatchEngine::first_match(&rules, &req).is_some());
    }

    #[test]
    fn xpath_with_namespace() {
        let body = br#"<soap:Envelope><soap:Body><ns:id>abc</ns:id></soap:Body></soap:Envelope>"#;
        let rules = vec![simple_rule(
            "xpns",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::XPath("Envelope/Body/id".into()),
                    operator: Operator::Eq("abc".into()),
                }],
                any_of: vec![],
            },
        )];
        let req = make_req(&[], &[], body, Some("text/xml"));
        assert!(MatchEngine::first_match(&rules, &req).is_some());
    }

    #[test]
    fn xpath_soap_with_header_sibling_before_body() {
        // An empty <Header></Header> written in full next to <Body> must not break the match of an ancestor already
        // matched (Envelope).
        let body = br#"<SOAP:Envelope><SOAP-ENV:Header></SOAP-ENV:Header><SOAP-ENV:Body><ns3:recherche><ns3:Siret>12345678901234</ns3:Siret></ns3:recherche></SOAP-ENV:Body></SOAP:Envelope>"#;
        let rules = vec![simple_rule(
            "soap-header-sibling",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::XPath("Envelope/Body/recherche".into()),
                    operator: Operator::Exists,
                }],
                any_of: vec![],
            },
        )];
        let req = make_req(&[], &[], body, Some("text/xml"));
        assert!(MatchEngine::first_match(&rules, &req).is_some());
    }

    #[test]
    fn xpath_soap_distinguishes_sibling_operations() {
        // Two operations of one service (recherche, mode), same envelope with a Header: only the rule whose XPath names the
        // operation present in the body matches.
        let body_recherche = br#"<SOAP:Envelope><SOAP-ENV:Header></SOAP-ENV:Header><SOAP-ENV:Body><ns3:recherche><ns3:Siret>12345678901234</ns3:Siret></ns3:recherche></SOAP-ENV:Body></SOAP:Envelope>"#;
        let rules = vec![
            simple_rule(
                "op-mode",
                ConditionGroup {
                    all_of: vec![Condition {
                        source: ConditionSource::XPath("Envelope/Body/mode".into()),
                        operator: Operator::Exists,
                    }],
                    any_of: vec![],
                },
            ),
            simple_rule(
                "op-recherche",
                ConditionGroup {
                    all_of: vec![Condition {
                        source: ConditionSource::XPath("Envelope/Body/recherche".into()),
                        operator: Operator::Exists,
                    }],
                    any_of: vec![],
                },
            ),
        ];
        let req = make_req(&[], &[], body_recherche, Some("text/xml"));
        let (matched, _) = MatchEngine::first_match(&rules, &req).unwrap();
        assert_eq!(matched.name, "op-recherche");
    }

    #[test]
    fn xpath_matches_self_closing_target_element() {
        // A self-closing operation element (<ns3:mode/>, with no children) must match too.
        let body = br#"<SOAP:Envelope><SOAP-ENV:Header/><SOAP-ENV:Body><ns3:mode/></SOAP-ENV:Body></SOAP:Envelope>"#;
        let result = MatchEngine::extract_xpath(body, "Envelope/Body/mode");
        assert_eq!(result, Some(String::new()));
    }

    #[test]
    fn form_field_extraction() {
        let body = b"username=admin&password=secret";
        let rules = vec![simple_rule(
            "form",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::FormField("username".into()),
                    operator: Operator::Eq("admin".into()),
                }],
                any_of: vec![],
            },
        )];
        let req = make_req(&[], &[], body, Some("application/x-www-form-urlencoded"));
        assert!(MatchEngine::first_match(&rules, &req).is_some());
    }

    #[test]
    fn body_raw_contains() {
        let body = b"Hello World test payload";
        let rules = vec![simple_rule(
            "raw",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::BodyRaw,
                    operator: Operator::Contains("World".into()),
                }],
                any_of: vec![],
            },
        )];
        let req = make_req(&[], &[], body, None);
        assert!(MatchEngine::first_match(&rules, &req).is_some());
    }

    #[test]
    fn regex_operator() {
        let rules = vec![simple_rule(
            "rgx",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::QueryParam("code".into()),
                    operator: Operator::Regex(r"^\d{3}$".into()),
                }],
                any_of: vec![],
            },
        )];
        let yes = make_req(&[("code", "200")], &[], b"", None);
        let no = make_req(&[("code", "abcd")], &[], b"", None);
        assert!(MatchEngine::first_match(&rules, &yes).is_some());
        assert!(MatchEngine::first_match(&rules, &no).is_none());
    }

    #[test]
    fn exists_operator() {
        let rules = vec![simple_rule(
            "ex",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::Header("x-debug".into()),
                    operator: Operator::Exists,
                }],
                any_of: vec![],
            },
        )];
        let yes = make_req(&[], &[("x-debug", "")], b"", None);
        let no = make_req(&[], &[], b"", None);
        assert!(MatchEngine::first_match(&rules, &yes).is_some());
        assert!(MatchEngine::first_match(&rules, &no).is_none());
    }

    #[test]
    fn all_of_and_any_of_combined() {
        let rules = vec![simple_rule(
            "combo",
            ConditionGroup {
                all_of: vec![Condition {
                    source: ConditionSource::Header("x-env".into()),
                    operator: Operator::Eq("staging".into()),
                }],
                any_of: vec![
                    Condition {
                        source: ConditionSource::QueryParam("debug".into()),
                        operator: Operator::Exists,
                    },
                    Condition {
                        source: ConditionSource::QueryParam("trace".into()),
                        operator: Operator::Exists,
                    },
                ],
            },
        )];

        let ok1 = make_req(&[("debug", "1")], &[("x-env", "staging")], b"", None);
        let ok2 = make_req(&[("trace", "1")], &[("x-env", "staging")], b"", None);
        let fail_header = make_req(&[("debug", "1")], &[("x-env", "prod")], b"", None);
        let fail_any = make_req(&[], &[("x-env", "staging")], b"", None);

        assert!(MatchEngine::first_match(&rules, &ok1).is_some());
        assert!(MatchEngine::first_match(&rules, &ok2).is_some());
        assert!(MatchEngine::first_match(&rules, &fail_header).is_none());
        assert!(MatchEngine::first_match(&rules, &fail_any).is_none());
    }

    #[test]
    fn path_param_condition_reads_the_rule_s_own_sub_path_parameters() {
        // `/orders/{id}` with a condition on `id`: the rule tester merged the sub-path parameters before judging the
        // conditions, production did not, so the rule passed in the tester and never matched a real request.
        let rule = |name: &str, id: &str| Rule {
            sub_path: Some("/orders/{id}".into()),
            ..simple_rule(
                name,
                cg(
                    vec![Condition {
                        source: ConditionSource::PathParam("id".into()),
                        operator: Operator::Eq(id.into()),
                    }],
                    vec![],
                ),
            )
        };
        let rules = vec![rule("first", "1"), rule("second", "2")];
        let mut req = make_req(&[], &[], b"", None);
        req.remaining_path = "/orders/2".into();

        let (matched, params) = MatchEngine::first_match(&rules, &req).unwrap();
        assert_eq!(matched.name, "second");
        assert_eq!(params.get("id").map(String::as_str), Some("2"));

        // A parameter of the service's listen path keeps its own value unless the sub-path names the same parameter,
        // which then wins, as when the response is rendered.
        req.path_params.insert("id".into(), "1".into());
        let (matched, _) = MatchEngine::first_match(&rules, &req).unwrap();
        assert_eq!(matched.name, "second");
    }

    // --- Method matching tests ---

    #[test]
    fn rule_method_get_matches_get_only() {
        let mut rule = simple_rule("get-only", ConditionGroup::default());
        rule.method = "GET".into();
        let rules = vec![rule];

        let mut req_get = make_req(&[], &[], b"", None);
        req_get.method = "GET".into();
        assert!(MatchEngine::first_match(&rules, &req_get).is_some());

        let mut req_post = make_req(&[], &[], b"", None);
        req_post.method = "POST".into();
        assert!(MatchEngine::first_match(&rules, &req_post).is_none());
    }

    #[test]
    fn rule_method_must_match_exactly() {
        let mut rule = simple_rule("get-only", ConditionGroup::default());
        rule.method = "GET".into();
        let rules = vec![rule];

        let mut req_post = make_req(&[], &[], b"", None);
        req_post.method = "POST".into();
        assert!(MatchEngine::first_match(&rules, &req_post).is_none());
    }

    #[test]
    fn sub_path_matches_remaining() {
        let mut rule = simple_rule("sub", ConditionGroup::default());
        rule.sub_path = Some("/users/{id}".into());
        let rules = vec![rule];

        let mut req = make_req(&[], &[], b"", None);
        req.remaining_path = "/users/42".into();
        let (matched, sub_params) = MatchEngine::first_match(&rules, &req).unwrap();
        assert_eq!(matched.name, "sub");
        assert_eq!(sub_params.get("id").unwrap(), "42");
    }

    #[test]
    fn sub_path_no_match() {
        let mut rule = simple_rule("sub", ConditionGroup::default());
        rule.sub_path = Some("/users/{id}".into());
        let rules = vec![rule];

        let mut req = make_req(&[], &[], b"", None);
        req.remaining_path = "/orders/1".into();
        assert!(MatchEngine::first_match(&rules, &req).is_none());
    }

    #[test]
    fn method_and_sub_path_combined() {
        let mut get_rule = simple_rule("get-users", ConditionGroup::default());
        get_rule.method = "GET".into();
        get_rule.sub_path = Some("/users/{id}".into());

        let mut post_rule = simple_rule("post-users", ConditionGroup::default());
        post_rule.method = "POST".into();
        post_rule.sub_path = Some("/users".into());

        let rules = vec![get_rule, post_rule];

        let mut req_get = make_req(&[], &[], b"", None);
        req_get.method = "GET".into();
        req_get.remaining_path = "/users/42".into();
        let (matched, params) = MatchEngine::first_match(&rules, &req_get).unwrap();
        assert_eq!(matched.name, "get-users");
        assert_eq!(params.get("id").unwrap(), "42");

        let mut req_post = make_req(&[], &[], b"", None);
        req_post.method = "POST".into();
        req_post.remaining_path = "/users".into();
        let (matched, _) = MatchEngine::first_match(&rules, &req_post).unwrap();
        assert_eq!(matched.name, "post-users");
    }

    // --- match_path tests ---

    #[test]
    fn match_path_wildcard() {
        let r = match_path("/svc-a/*", "/svc-a/foo/bar");
        assert!(r.is_some());
        let (_, remaining) = r.unwrap();
        assert_eq!(remaining, "/foo/bar");
    }

    #[test]
    fn match_path_named_param() {
        let r = match_path("/v4/insee/{siret}", "/v4/insee/44306184100047");
        assert!(r.is_some());
        let (params, _) = r.unwrap();
        assert_eq!(params.get("siret").unwrap(), "44306184100047");
    }

    #[test]
    fn match_path_url_encoded() {
        let r = match_path("/api/v1/*", "/api/v1/hello%20world");
        assert!(r.is_some());
        let (_, remaining) = r.unwrap();
        assert_eq!(remaining, "/hello world");
    }

    #[test]
    fn match_path_encoded_param() {
        let r = match_path("/users/{name}", "/users/John%20Doe");
        assert!(r.is_some());
        let (params, _) = r.unwrap();
        assert_eq!(params.get("name").unwrap(), "John Doe");
    }

    #[test]
    fn match_path_special_chars() {
        let r = match_path("/api/*", "/api/path%3Awith%3Acolons");
        assert!(r.is_some());
        let (_, remaining) = r.unwrap();
        assert_eq!(remaining, "/path:with:colons");
    }

    #[test]
    fn match_path_jenkins_spaces_in_pattern() {
        let pattern = "/job/Zone - Services/job/{branch}/build";
        let url = "/job/Zone%20-%20Services/job/develop/build";
        let r = match_path(pattern, url);
        assert!(r.is_some(), "spaces in pattern must match %20 in URL");
        let (params, _) = r.unwrap();
        assert_eq!(params.get("branch").unwrap(), "develop");
    }

    #[test]
    fn match_path_pattern_encoded() {
        let pattern = "/job/Zone%20-%20Services/job/{branch}/build";
        let url = "/job/Zone%20-%20Services/job/main/build";
        let r = match_path(pattern, url);
        assert!(r.is_some(), "%20 in pattern must match %20 in URL");
        let (params, _) = r.unwrap();
        assert_eq!(params.get("branch").unwrap(), "main");
    }

    #[test]
    fn match_path_jenkins_deep_nested() {
        let pattern = "/job/Zone - Services aux usagers/job/QUARTIER-Gestion/job/ILOT-Cnt/job/{space}/job/{project}/build";
        let url = "/job/Zone%20-%20Services%20aux%20usagers/job/QUARTIER-Gestion/job/ILOT-Cnt/job/my-space/job/my-project/build";
        let r = match_path(pattern, url);
        assert!(r.is_some());
        let (params, _) = r.unwrap();
        assert_eq!(params.get("space").unwrap(), "my-space");
        assert_eq!(params.get("project").unwrap(), "my-project");
    }

    #[test]
    fn match_path_empty_never_matches() {
        assert!(match_path("", "/").is_none());
        assert!(match_path("/", "/").is_none());
    }

    // --- evaluate_group / ConditionEvaluation (rule tester) ---

    fn cg(all_of: Vec<Condition>, any_of: Vec<Condition>) -> ConditionGroup {
        ConditionGroup { all_of, any_of }
    }

    #[test]
    fn evaluate_group_reports_found_value_on_match() {
        let group = cg(
            vec![Condition {
                source: ConditionSource::QueryParam("id".into()),
                operator: Operator::Eq("42".into()),
            }],
            vec![],
        );
        let req = make_req(&[("id", "42")], &[], b"", None);
        let result = MatchEngine::evaluate_group(&group, &req);
        assert!(result.matched);
        assert!(result.all_of[0].matched);
        assert_eq!(result.all_of[0].found_value.as_deref(), Some("42"));
        assert!(result.all_of[0].hint.is_none());
    }

    #[test]
    fn evaluate_group_hint_query_param_found_as_path_param() {
        // The typical mistake: a path parameter chosen as a query parameter.
        let group = cg(
            vec![Condition {
                source: ConditionSource::QueryParam("foo".into()),
                operator: Operator::Eq("bar".into()),
            }],
            vec![],
        );
        let mut req = make_req(&[], &[], b"", None);
        req.path_params.insert("foo".into(), "bar".into());
        let result = MatchEngine::evaluate_group(&group, &req);
        assert!(!result.matched);
        assert!(!result.all_of[0].matched);
        assert!(result.all_of[0].found_value.is_none());
        let hint = result.all_of[0].hint.as_deref().unwrap();
        assert!(hint.contains("foo"));
        assert!(hint.contains("path parameter"));
    }

    #[test]
    fn evaluate_group_hint_path_param_found_as_header() {
        let group = cg(
            vec![Condition {
                source: ConditionSource::PathParam("token".into()),
                operator: Operator::Exists,
            }],
            vec![],
        );
        let req = make_req(&[], &[("token", "abc")], b"", None);
        let result = MatchEngine::evaluate_group(&group, &req);
        assert!(!result.all_of[0].matched);
        assert_eq!(
            result.all_of[0].hint.as_deref(),
            Some(
                "'token' was not found as a path parameter, but it is present as a header in this request"
            )
        );
    }

    #[test]
    fn evaluate_group_no_hint_when_key_nowhere_else() {
        let group = cg(
            vec![Condition {
                source: ConditionSource::QueryParam("missing".into()),
                operator: Operator::Exists,
            }],
            vec![],
        );
        let req = make_req(&[], &[], b"", None);
        let result = MatchEngine::evaluate_group(&group, &req);
        assert!(!result.all_of[0].matched);
        assert!(result.all_of[0].hint.is_none());
    }

    #[test]
    fn evaluate_group_no_hint_for_structured_sources() {
        // JSON pointers, XPath and the raw body are not named keys: never a cross-source hint, even on failure.
        let group = cg(
            vec![Condition {
                source: ConditionSource::JsonPointer("/missing".into()),
                operator: Operator::Exists,
            }],
            vec![],
        );
        let req = make_req(&[], &[], b"{}", Some("application/json"));
        let result = MatchEngine::evaluate_group(&group, &req);
        assert!(!result.all_of[0].matched);
        assert!(result.all_of[0].hint.is_none());
    }

    #[test]
    fn evaluate_group_matches_agree_with_matches_group() {
        // evaluate_group must never disagree with the production boolean, over a matrix of cases (match or not, all_of
        // and any_of, several sources).
        let cases: Vec<(ConditionGroup, RequestData)> = vec![
            (
                cg(
                    vec![Condition {
                        source: ConditionSource::Header("x-env".into()),
                        operator: Operator::Eq("prod".into()),
                    }],
                    vec![],
                ),
                make_req(&[], &[("x-env", "prod")], b"", None),
            ),
            (
                cg(
                    vec![Condition {
                        source: ConditionSource::Header("x-env".into()),
                        operator: Operator::Eq("prod".into()),
                    }],
                    vec![],
                ),
                make_req(&[], &[("x-env", "dev")], b"", None),
            ),
            (
                cg(
                    vec![],
                    vec![
                        Condition {
                            source: ConditionSource::QueryParam("debug".into()),
                            operator: Operator::Exists,
                        },
                        Condition {
                            source: ConditionSource::QueryParam("trace".into()),
                            operator: Operator::Exists,
                        },
                    ],
                ),
                make_req(&[("trace", "1")], &[], b"", None),
            ),
            (ConditionGroup::default(), make_req(&[], &[], b"", None)),
        ];

        for (group, req) in cases {
            let boolean_result = MatchEngine::matches_group(&group, &req, None);
            let detailed_result = MatchEngine::evaluate_group(&group, &req).matched;
            assert_eq!(
                boolean_result, detailed_result,
                "matches_group et evaluate_group doivent toujours s'accorder"
            );
        }
    }

    // --- evaluate_rule_test tests ---

    #[test]
    fn evaluate_rule_test_nominal_match() {
        let conditions = cg(
            vec![Condition {
                source: ConditionSource::PathParam("id".into()),
                operator: Operator::Eq("42".into()),
            }],
            vec![],
        );
        let sub_path = Some("/orders/{id}".into());
        let mut req = make_req(&[], &[], b"", None);
        req.method = "GET".into();
        req.remaining_path = "/orders/42".into();

        let outcome = MatchEngine::evaluate_rule_test(
            RuleTestInput {
                method: "GET",
                sub_path: &sub_path,
                conditions: &conditions,
            },
            &req,
        );

        assert!(outcome.method_matches);
        assert!(outcome.sub_path_matches);
        assert!(outcome.overall_matched);
        assert_eq!(outcome.path_params.get("id").unwrap(), "42");
        assert!(outcome.group.matched);
    }

    #[test]
    fn evaluate_rule_test_method_mismatch() {
        let conditions = ConditionGroup::default();
        let sub_path = None;
        let mut req = make_req(&[], &[], b"", None);
        req.method = "POST".into();

        let outcome = MatchEngine::evaluate_rule_test(
            RuleTestInput {
                method: "GET",
                sub_path: &sub_path,
                conditions: &conditions,
            },
            &req,
        );

        assert!(!outcome.method_matches);
        assert!(!outcome.overall_matched);
    }

    #[test]
    fn evaluate_rule_test_sub_path_mismatch_still_reports_conditions() {
        let conditions = cg(
            vec![Condition {
                source: ConditionSource::Header("x-env".into()),
                operator: Operator::Eq("prod".into()),
            }],
            vec![],
        );
        let sub_path = Some("/orders/{id}".into());
        let mut req = make_req(&[], &[("x-env", "prod")], b"", None);
        req.remaining_path = "/other/path".into();

        let outcome = MatchEngine::evaluate_rule_test(
            RuleTestInput {
                method: "GET",
                sub_path: &sub_path,
                conditions: &conditions,
            },
            &req,
        );

        assert!(!outcome.sub_path_matches);
        assert!(!outcome.overall_matched);
        // The conditions are still detailed when the sub-path itself does not match: the UI needs them to explain.
        assert!(outcome.group.matched);
    }

    #[test]
    fn evaluate_rule_test_no_sub_path_matches_any_remaining() {
        let conditions = ConditionGroup::default();
        let sub_path = None;
        let mut req = make_req(&[], &[], b"", None);
        req.remaining_path = "/anything/at/all".into();

        let outcome = MatchEngine::evaluate_rule_test(
            RuleTestInput {
                method: "GET",
                sub_path: &sub_path,
                conditions: &conditions,
            },
            &req,
        );

        assert!(outcome.sub_path_matches);
        assert!(outcome.path_params.is_empty());
    }

    // --- find_rule_conflicts tests ---

    fn header_eq(key: &str, val: &str) -> Condition {
        Condition {
            source: ConditionSource::Header(key.into()),
            operator: Operator::Eq(val.into()),
        }
    }

    #[test]
    fn find_rule_conflicts_identical_conditions_detected() {
        let draft_conditions = cg(vec![header_eq("x-env", "prod")], vec![]);
        let other_conditions = cg(vec![header_eq("x-env", "prod")], vec![]);
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &None,
            conditions: &draft_conditions,
        };
        let other = OtherRuleConflictInput {
            name: "existing",
            method: "GET",
            sub_path: &None,
            conditions: &other_conditions,
        };

        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].other_rule_name, "existing");
    }

    #[test]
    fn find_rule_conflicts_subset_conditions_detected() {
        // The draft has no condition (matches everything) and the other rule one more: a general rule added after a more
        // specific one, a legitimate fallback that is still reported.
        let draft_conditions = ConditionGroup::default();
        let other_conditions = cg(vec![header_eq("x-env", "prod")], vec![]);
        let draft = RuleConflictDraft {
            method: "POST",
            sub_path: &None,
            conditions: &draft_conditions,
        };
        let other = OtherRuleConflictInput {
            name: "specific-rule",
            method: "POST",
            sub_path: &None,
            conditions: &other_conditions,
        };

        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].other_rule_name, "specific-rule");
    }

    #[test]
    fn find_rule_conflicts_no_false_positive_on_disjoint_conditions() {
        let draft_conditions = cg(vec![header_eq("x-env", "prod")], vec![]);
        let other_conditions = cg(vec![header_eq("x-env", "staging")], vec![]);
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &None,
            conditions: &draft_conditions,
        };
        let other = OtherRuleConflictInput {
            name: "staging-rule",
            method: "GET",
            sub_path: &None,
            conditions: &other_conditions,
        };

        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn find_rule_conflicts_no_conflict_on_different_method() {
        let conditions = ConditionGroup::default();
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &None,
            conditions: &conditions,
        };
        let other = OtherRuleConflictInput {
            name: "post-rule",
            method: "POST",
            sub_path: &None,
            conditions: &conditions,
        };

        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn find_rule_conflicts_no_conflict_on_incompatible_sub_path_shape() {
        let conditions = ConditionGroup::default();
        let draft_sub = Some("/a/{id}".to_string());
        let other_sub = Some("/b".to_string());
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &draft_sub,
            conditions: &conditions,
        };
        let other = OtherRuleConflictInput {
            name: "other-shape",
            method: "GET",
            sub_path: &other_sub,
            conditions: &conditions,
        };

        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn find_rule_conflicts_sub_path_same_shape_different_param_name_compatible() {
        let conditions = ConditionGroup::default();
        let draft_sub = Some("/orders/{id}".to_string());
        let other_sub = Some("/orders/:orderId".to_string());
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &draft_sub,
            conditions: &conditions,
        };
        let other = OtherRuleConflictInput {
            name: "colon-syntax-rule",
            method: "GET",
            sub_path: &other_sub,
            conditions: &conditions,
        };

        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert_eq!(conflicts.len(), 1);
    }

    #[test]
    fn find_rule_conflicts_missing_sub_path_matches_any_pattern() {
        let conditions = ConditionGroup::default();
        let other_sub = Some("/x/{id}".to_string());
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &None,
            conditions: &conditions,
        };
        let other = OtherRuleConflictInput {
            name: "specific-path-rule",
            method: "GET",
            sub_path: &other_sub,
            conditions: &conditions,
        };

        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert_eq!(conflicts.len(), 1);
    }

    #[test]
    fn find_rule_conflicts_winner_other_when_positioned_before_draft() {
        let conditions = ConditionGroup::default();
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &None,
            conditions: &conditions,
        };
        let other = OtherRuleConflictInput {
            name: "earlier-rule",
            method: "GET",
            sub_path: &None,
            conditions: &conditions,
        };

        // The other rule is at index 0 and the draft is appended (position 1): the other rule is evaluated first and wins.
        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert_eq!(conflicts[0].winner, ConflictWinner::Other);
    }

    #[test]
    fn find_rule_conflicts_winner_draft_when_positioned_after_draft() {
        let conditions = ConditionGroup::default();
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &None,
            conditions: &conditions,
        };
        let other = OtherRuleConflictInput {
            name: "later-rule",
            method: "GET",
            sub_path: &None,
            conditions: &conditions,
        };

        // The draft is edited in place at the head (position 0): the other rule comes after it, so the draft wins.
        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 0);
        assert_eq!(conflicts[0].winner, ConflictWinner::Draft);
    }

    #[test]
    fn find_rule_conflicts_non_empty_any_of_skips_subset_detection() {
        // As documented: once an any_of is non-empty and the two differ, inclusion is not analyzed, rather than risk a
        // false alarm on OR semantics.
        let draft_conditions = ConditionGroup::default();
        let other_conditions = cg(
            vec![],
            vec![
                header_eq("x-env", "prod"),
                Condition {
                    source: ConditionSource::QueryParam("debug".into()),
                    operator: Operator::Exists,
                },
            ],
        );
        let draft = RuleConflictDraft {
            method: "GET",
            sub_path: &None,
            conditions: &draft_conditions,
        };
        let other = OtherRuleConflictInput {
            name: "any-of-rule",
            method: "GET",
            sub_path: &None,
            conditions: &other_conditions,
        };

        let conflicts = MatchEngine::find_rule_conflicts(&draft, &[other], 1);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn path_params_are_decoded_as_utf8_in_any_script() {
        let (params, _) = match_path("/users/{name}", "/users/%C3%A9t%C3%A9").unwrap();
        assert_eq!(params["name"], "été");
        let (params, _) = match_path("/商品/{id}", "/%E5%95%86%E5%93%81/7").unwrap();
        assert_eq!(params["id"], "7");
        let (params, _) = match_path("/q/{text}", "/q/%D9%85%D8%B1%D8%AD%D8%A8%D8%A7").unwrap();
        assert_eq!(params["text"], "مرحبا");
        assert!(match_path("/café/{id}", "/caf%C3%A9/42").is_some());
    }

    #[test]
    fn only_two_hex_digits_form_an_escape() {
        assert_eq!(url_decode("%+1"), "%+1");
        assert_eq!(url_decode("100%"), "100%");
        assert_eq!(url_decode("%2"), "%2");
        assert_eq!(url_decode("%41%zz"), "A%zz");
    }

    #[test]
    fn xpath_text_resolves_entities_and_character_references() {
        let body =
            b"<order><customer>Tom &amp; Jerry &#233;t&#xE9; &lt;3 &unknown;</customer></order>";
        assert_eq!(
            MatchEngine::extract_xpath(body, "order/customer").as_deref(),
            Some("Tom & Jerry été <3 ")
        );
    }
}
