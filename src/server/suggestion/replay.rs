//! The suggested rules replayed on the calls they come from, matched and rendered as the interception does: what the
//! property tests and the fuzz target check on generated traffic.
use super::{SuggestedRule, Suggestion, suggest_for_service};
use crate::engine::script::{ScriptContext, ScriptEngine};
use crate::engine::{MatchEngine, RequestData, TemplateContext, TemplateRenderer};
use crate::models::{Rule, RuleAction};
use crate::server::observation::{ObservationKey, ObservedExchange};

pub(crate) fn rules_of(suggestion: &Suggestion) -> Vec<&SuggestedRule> {
    match suggestion {
        Suggestion::Unconditional { rule } | Suggestion::Echo { rule } => vec![rule.as_ref()],
        Suggestion::Conditional { rules } => rules.iter().collect(),
        Suggestion::VarianceUnexplained { .. } => vec![],
    }
}

/// The rule the form would save from a suggested one: the root's empty sub-path becomes no sub-path, as the form does.
fn as_rule(i: usize, suggested: &SuggestedRule) -> Rule {
    Rule {
        name: format!("suggested-{i}"),
        method: suggested.method.clone(),
        sub_path: (!suggested.sub_path.is_empty()).then(|| suggested.sub_path.clone()),
        action: RuleAction::Mock,
        script: suggested.script.clone(),
        response_mode: None,
        conditions: suggested.conditions.clone(),
        response: suggested.response.clone(),
    }
}

/// What `rules` answer to an observed call whose request body the mock receives as `received`, matched and rendered
/// as the interception does: the rule's script run first, a failing one giving an empty result.
fn answer(
    rules: &[Rule],
    key: &ObservationKey,
    exchange: &ObservedExchange,
    received: &[u8],
) -> Option<(u16, String)> {
    let req = RequestData {
        query_params: exchange.request_query_params.clone(),
        headers: exchange.request_headers.clone(),
        body: received.to_vec(),
        content_type: None,
        path_params: exchange.request_path_params.clone(),
        method: key.method.clone(),
        remaining_path: key.sub_path.clone(),
    };
    let (rule, sub_params) = MatchEngine::first_match(rules, &req)?;
    let mut params = req.path_params.clone();
    params.extend(sub_params);
    let script_result = rule.script.as_ref().map(|code| {
        let ctx = ScriptContext {
            body: String::from_utf8_lossy(&req.body).into_owned(),
            headers: req.headers.clone(),
            query_params: req.query_params.clone(),
            path_params: params.clone(),
        };
        ScriptEngine::new().execute(code, &ctx).unwrap_or_default()
    });
    let ctx = TemplateContext {
        path_params: &params,
        query_params: &req.query_params,
        headers: &req.headers,
        request_body: &req.body,
        seq_counter: 0,
        script_result: script_result.as_ref(),
    };
    let body = TemplateRenderer::render_body(&rule.response.body, &[], &ctx);
    Some((rule.response.status, body))
}

/// Suggests rules for `endpoints`, saves all of them in the order given, and replays every observed call of a route
/// that got rules: each must get back the status and the body the backend sent, and each route's pattern must match
/// the paths it lists. A route with more paths than are listed is skipped (its members are not all known). A suggestion
/// for a SOAP operation stands for the calls in which the reader of XPath conditions finds that operation; the calls it
/// left out must all be server errors, as many as it says, and are not replayed.
pub(crate) fn check(endpoints: &[(ObservationKey, Vec<ObservedExchange>)]) -> Result<(), String> {
    let received: Vec<Vec<Vec<u8>>> = endpoints
        .iter()
        .map(|(_, exchanges)| {
            exchanges
                .iter()
                .map(|e| e.request_body.as_bytes().to_vec())
                .collect()
        })
        .collect();
    check_received(endpoints, &received)
}

/// `check`, the request bodies given as the mock receives them (`received`, in the shape of `endpoints`), where
/// observation kept them as text: a body that is not UTF-8 is not the text kept of it.
pub(crate) fn check_received(
    endpoints: &[(ObservationKey, Vec<ObservedExchange>)],
    received: &[Vec<Vec<u8>>],
) -> Result<(), String> {
    let suggestions = suggest_for_service(endpoints);
    let rules: Vec<Rule> = suggestions
        .iter()
        .flat_map(|s| rules_of(&s.outcome))
        .enumerate()
        .map(|(i, r)| as_rule(i, r))
        .collect();
    for suggestion in &suggestions {
        for path in &suggestion.paths {
            if !suggestion.sub_path.is_empty()
                && crate::engine::matcher::match_path(&suggestion.sub_path, path).is_none()
            {
                return Err(format!("{} does not match {path}", suggestion.sub_path));
            }
        }
        if rules_of(&suggestion.outcome).is_empty()
            || suggestion.paths.len() != suggestion.path_count
        {
            continue;
        }
        let operation = suggestion
            .operation
            .as_ref()
            .map(|name| format!("Envelope/Body/{name}"));
        let calls: Vec<(&ObservationKey, &ObservedExchange, &[u8])> = endpoints
            .iter()
            .zip(received)
            .filter(|((key, _), _)| {
                key.method == suggestion.method && suggestion.paths.contains(&key.sub_path)
            })
            .flat_map(|((key, exchanges), bodies)| {
                exchanges
                    .iter()
                    .zip(bodies)
                    .map(move |(e, b)| (key, e, b.as_slice()))
            })
            .filter(|(_, _, body)| {
                operation
                    .as_ref()
                    .is_none_or(|path| MatchEngine::extract_xpath(body, path).is_some())
            })
            .collect();
        for left in &suggestion.left_out {
            let count = calls
                .iter()
                .filter(|(_, e, _)| e.response_status == left.status)
                .count();
            if left.status < 500 || count != left.count {
                return Err(format!(
                    "{} {}: left out {left:?}, {count} such calls",
                    suggestion.method, suggestion.sub_path
                ));
            }
        }
        let replayed = calls.into_iter().filter(|(_, e, _)| {
            !suggestion
                .left_out
                .iter()
                .any(|left| left.status == e.response_status)
        });
        for (key, exchange, body) in replayed {
            let expected = Some((exchange.response_status, exchange.response_body.clone()));
            let got = answer(&rules, key, exchange, body);
            if got != expected {
                return Err(format!(
                    "{} {:?} under {}: expected {expected:?}, got {got:?}; suggestions {suggestions:?}",
                    key.method, key.sub_path, suggestion.sub_path
                ));
            }
        }
    }
    Ok(())
}
