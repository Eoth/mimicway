// Mock rules suggested from the exchanges `server::observation` captured while a user observed a service. Always
// computed from `ObservationStore` when asked: the input is already bounded, so recomputing costs less than a cache
// to invalidate, and suggestions can never lag behind what was observed.
//
// First the literal paths of each method are grouped into route patterns (`routes`): `/orders/1` and `/orders/2`
// pool their samples under `/orders/{id}`, whose parameter becomes a condition source like any request field.
//
// Then, per route, the trap: two calls to the same route can legitimately get different answers, and a rule built
// from the first answer seen would silently break the others. Hence three steps: (1) group the observations by
// equivalent response; (2) one group gives an unconditional rule; (3) several groups need a request field that tells
// them apart perfectly (same value, same group; different groups, different values) before anything is suggested.
// When that value is all that differs between the responses (the backend repeats it), one rule that repeats it
// (`{{path.name}}`) replaces one rule per value. When no field tells the groups apart, nothing is suggested: no rule
// beats a fragile one.
use crate::models::{
    BodyFragment, Condition, ConditionSource, HeaderEntry, MockResponse, Operator,
};
use crate::server::observation::{ObservationKey, ObservedExchange};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub mod links;
mod routes;

const DEFAULT_MIN_SAMPLES: usize = 3;
/// Literal paths listed with a suggestion, as examples of what its pattern covers.
const MAX_LISTED_PATHS: usize = 10;

/// Observations needed for one route (service, method, route pattern) before anything is suggested: one response, or
/// two identical ones, prove nothing about how stable the route is.
pub fn min_samples() -> usize {
    std::env::var("TRAFFIC_OBSERVATION_MIN_SAMPLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MIN_SAMPLES)
}

/// Request headers never used to tell responses apart: they change from call to call by nature (timestamps,
/// correlation and tracing ids, credentials), not because of a decision of the backend. A condition on them could
/// fit the sample perfectly and still be useless or harmful (a rule conditioned on a token never matches again).
const NOISY_HEADERS: &[&str] = &[
    "date",
    "x-request-id",
    "x-correlation-id",
    "traceparent",
    "tracestate",
    "authorization",
    "cookie",
    "set-cookie",
    "user-agent",
];

/// A suggested rule: the unconditional one (no variance observed, or a response that repeats a request value), or one
/// of the conditional rules covering each distinct response (`condition` is then `Some`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SuggestedRule {
    pub method: String,
    pub sub_path: String,
    pub condition: Option<Condition>,
    /// The request field whose value the response repeats, through a template fragment of `response.body`.
    pub echo: Option<ConditionSource>,
    pub sample_count: usize,
    pub response: MockResponse,
    /// The services of Mimicway this rule's response now sends the application to, instead of the backends they stand
    /// in front of (`links::redirect_suggestions`).
    pub redirected_to: Vec<links::ServiceRef>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "outcome")]
pub enum Suggestion {
    /// No variance in the sample: one rule, without condition.
    Unconditional { rule: Box<SuggestedRule> },
    /// Responses that differ only by a request value they repeat: one rule, without condition, whose response repeats
    /// that value.
    Echo { rule: Box<SuggestedRule> },
    /// Variance explained by a request field: one rule per value seen, each with an `Eq` condition.
    Conditional { rules: Vec<SuggestedRule> },
    /// Variance that no field explains reliably in the sample: nothing to suggest, only a diagnosis.
    VarianceUnexplained { response_class_count: usize },
}

/// The suggestion for one route of a service.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EndpointSuggestion {
    pub method: String,
    /// The route pattern, in the syntax of a rule's sub-path (`/orders/{id}`).
    pub sub_path: String,
    /// Literal paths observed under the pattern, sorted, at most `MAX_LISTED_PATHS` of them.
    pub paths: Vec<String>,
    pub path_count: usize,
    pub sample_count: usize,
    #[serde(flatten)]
    pub outcome: Suggestion,
}

/// One observed exchange of a route, with the path parameters it carries: those the route's pattern takes from its
/// literal path, and those the service's listen path captured.
struct Sample<'a> {
    exchange: &'a ObservedExchange,
    path_params: BTreeMap<String, String>,
}

/// One route of a service as observed: its method, its pattern, the literal paths it groups and their samples, oldest
/// first, so that the last sample of a group is the latest response seen.
pub(super) struct ObservedRoute<'a> {
    pub method: &'a str,
    pub pattern: String,
    pub literal_paths: Vec<&'a str>,
    samples: Vec<Sample<'a>>,
}

impl<'a> ObservedRoute<'a> {
    pub fn exchanges(&self) -> impl Iterator<Item = &'a ObservedExchange> + '_ {
        self.samples.iter().map(|s| s.exchange)
    }
}

/// The routes of a service, from the exchanges observed on each of its endpoints (method, literal path): grouped by
/// method and route pattern, most specific pattern first.
pub(super) fn observed_routes(
    endpoints: &[(ObservationKey, Vec<ObservedExchange>)],
) -> Vec<ObservedRoute<'_>> {
    let mut by_method: BTreeMap<&str, Vec<routes::ObservedPath>> = BTreeMap::new();
    for (key, exchanges) in endpoints {
        by_method
            .entry(key.method.as_str())
            .or_default()
            .push(routes::ObservedPath {
                path: &key.sub_path,
                exchanges,
            });
    }

    let mut observed = Vec::new();
    for (method, paths) in by_method {
        let reserved: BTreeSet<String> = paths
            .iter()
            .flat_map(|p| p.exchanges.iter())
            .flat_map(|e| e.request_path_params.keys().cloned())
            .collect();
        for route in routes::infer(&paths, &reserved) {
            let mut samples = Vec::new();
            let mut literal_paths = Vec::new();
            for &member in &route.members {
                let path = &paths[member];
                literal_paths.push(path.path);
                let params = route.params_of(path.path);
                for exchange in path.exchanges {
                    let mut path_params: BTreeMap<String, String> = exchange
                        .request_path_params
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    path_params.extend(params.iter().cloned());
                    samples.push(Sample {
                        exchange,
                        path_params,
                    });
                }
            }
            samples.sort_by_key(|s| s.exchange.timestamp);
            literal_paths.sort_unstable();
            observed.push(ObservedRoute {
                method,
                pattern: route.pattern(),
                literal_paths,
                samples,
            });
        }
    }
    observed
}

/// The suggestions for one service, one per route (`observed_routes`); a route observed fewer than `min_samples()`
/// times is left out.
pub fn suggest_for_service(
    endpoints: &[(ObservationKey, Vec<ObservedExchange>)],
) -> Vec<EndpointSuggestion> {
    observed_routes(endpoints)
        .into_iter()
        .filter_map(|route| {
            let outcome = suggest_route(route.method, &route.pattern, &route.samples)?;
            Some(EndpointSuggestion {
                method: route.method.to_string(),
                path_count: route.literal_paths.len(),
                paths: route
                    .literal_paths
                    .iter()
                    .take(MAX_LISTED_PATHS)
                    .map(|p| p.to_string())
                    .collect(),
                sub_path: route.pattern,
                sample_count: route.samples.len(),
                outcome,
            })
        })
        .collect()
}

/// What makes two responses equal: status and body, byte for byte. Bodies are already truncated to
/// `observation::max_body_size()`, so two bodies that differ only beyond that point count as equal. Response
/// headers are left out: the same body with another correlation header is not a different answer to explain.
fn response_signature(exchange: &ObservedExchange) -> (u16, &str) {
    (exchange.response_status, exchange.response_body.as_str())
}

/// The suggestion for one route from its samples (oldest first). `None` while the sample is smaller than
/// `min_samples()`: no decision yet, either way.
fn suggest_route(method: &str, sub_path: &str, samples: &[Sample]) -> Option<Suggestion> {
    if samples.len() < min_samples() {
        return None;
    }

    // Groups of equivalent responses, as indices into `samples`, in order of first appearance.
    let mut classes: Vec<Vec<usize>> = Vec::new();
    for (i, sample) in samples.iter().enumerate() {
        let signature = response_signature(sample.exchange);
        match classes
            .iter_mut()
            .find(|class| response_signature(samples[class[0]].exchange) == signature)
        {
            Some(class) => class.push(i),
            None => classes.push(vec![i]),
        }
    }
    let latest = |class: &[usize]| samples[class[class.len() - 1]].exchange;
    let rule = |condition, echo, sample_count, response| SuggestedRule {
        method: method.to_string(),
        sub_path: sub_path.to_string(),
        condition,
        echo,
        sample_count,
        response,
        redirected_to: Vec::new(),
    };

    if classes.len() == 1 {
        let response = build_response(latest(&classes[0]));
        return Some(Suggestion::Unconditional {
            rule: Box::new(rule(None, None, samples.len(), response)),
        });
    }

    if let Some((candidate, values)) = find_discriminator(samples, &classes) {
        let representatives: Vec<&ObservedExchange> = classes.iter().map(|c| latest(c)).collect();
        if let Some(response) = echo_response(&candidate, &representatives, &values) {
            return Some(Suggestion::Echo {
                rule: Box::new(rule(
                    None,
                    Some(candidate.source()),
                    samples.len(),
                    response,
                )),
            });
        }
        let rules = classes
            .iter()
            .zip(values)
            .map(|(class, value)| {
                let condition = Condition {
                    source: candidate.source(),
                    operator: Operator::Eq(value),
                };
                rule(
                    Some(condition),
                    None,
                    class.len(),
                    build_response(latest(class)),
                )
            })
            .collect();
        return Some(Suggestion::Conditional { rules });
    }

    Some(Suggestion::VarianceUnexplained {
        response_class_count: classes.len(),
    })
}

/// Response headers never copied into a suggested rule (they stay in `ObservedExchange` for inspection):
/// `content-length` is computed from the rule's actual body when it is rendered (a fixed value would be wrong as
/// soon as the body is edited), and `date` would freeze a timestamp that means nothing in a static configuration.
const NEVER_SUGGESTED_RESPONSE_HEADERS: &[&str] = &["content-length", "date"];

/// Values are written as literal text (`literal_template`): header values are rendered when a rule answers, and what a
/// backend sent never becomes a template.
fn suggested_headers(exchange: &ObservedExchange) -> Vec<HeaderEntry> {
    exchange
        .response_headers
        .iter()
        .filter(|(name, _)| {
            !NEVER_SUGGESTED_RESPONSE_HEADERS.contains(&name.to_lowercase().as_str())
                && !crate::server::redaction::is_sensitive_header(name)
        })
        .map(|(name, value)| HeaderEntry {
            name: name.clone(),
            value: crate::engine::template::literal_template(value),
        })
        .collect()
}

/// The captured body becomes a `Literal` fragment, which rendering copies as it is: whatever a backend sent, it never
/// becomes a template.
fn build_response(exchange: &ObservedExchange) -> MockResponse {
    MockResponse {
        status: exchange.response_status,
        headers: suggested_headers(exchange),
        body: vec![BodyFragment::Literal {
            value: exchange.response_body.clone(),
        }],
        chaos: None,
        events: None,
    }
}

/// The response of an echo rule, when the groups' responses (their latest one each) differ only by the value that
/// tells them apart: same status, each body holding its group's value, and the same text around every occurrence. The
/// text around stays in `Literal` fragments; only the expression that reads the value (`{{path.name}}`, written here,
/// never taken from the traffic) is a template. Only the body is generalized: a response that repeats the value in a
/// header is not.
fn echo_response(
    candidate: &Candidate,
    representatives: &[&ObservedExchange],
    values: &[String],
) -> Option<MockResponse> {
    let expression = candidate.template_expression()?;
    let latest = representatives.iter().max_by_key(|e| e.timestamp)?;
    let headers = suggested_headers(latest);
    let mut pieces: Option<Vec<&str>> = None;
    for (exchange, value) in representatives.iter().zip(values) {
        if value.is_empty()
            || exchange.response_status != latest.response_status
            || !exchange.response_body.contains(value.as_str())
            || headers.iter().any(|h| h.value.contains(value.as_str()))
            || suggested_headers(exchange)
                .iter()
                .any(|h| h.value.contains(value.as_str()))
        {
            return None;
        }
        let split: Vec<&str> = exchange.response_body.split(value.as_str()).collect();
        match &pieces {
            None => pieces = Some(split),
            Some(previous) if *previous != split => return None,
            Some(_) => {}
        }
    }
    let mut body = Vec::new();
    for (i, piece) in pieces?.into_iter().enumerate() {
        if i > 0 {
            body.push(BodyFragment::Template {
                template: expression.clone(),
            });
        }
        if !piece.is_empty() {
            body.push(BodyFragment::Literal {
                value: piece.to_string(),
            });
        }
    }
    Some(MockResponse {
        status: latest.response_status,
        headers,
        body,
        chaos: None,
        events: None,
    })
}

/// A candidate condition source, without its value (known once the field is chosen).
#[derive(Clone)]
enum Candidate {
    PathParam(String),
    QueryParam(String),
    JsonPointer(String),
    Header(String),
}

impl Candidate {
    fn source(&self) -> ConditionSource {
        match self {
            Candidate::PathParam(k) => ConditionSource::PathParam(k.clone()),
            Candidate::QueryParam(k) => ConditionSource::QueryParam(k.clone()),
            // RFC 6901: `~` and `/` in a name are written `~0` and `~1`, or the pointer names another field.
            Candidate::JsonPointer(k) => ConditionSource::JsonPointer(format!(
                "/{}",
                k.replace('~', "~0").replace('/', "~1")
            )),
            Candidate::Header(k) => ConditionSource::Header(k.clone()),
        }
    }

    fn extract(&self, sample: &Sample, json_fields: &HashMap<String, String>) -> Option<String> {
        let exchange = sample.exchange;
        match self {
            Candidate::PathParam(k) => sample.path_params.get(k).cloned(),
            Candidate::QueryParam(k) => exchange.request_query_params.get(k).cloned(),
            Candidate::JsonPointer(k) => json_fields.get(k).cloned(),
            Candidate::Header(k) => exchange.request_headers.get(k).cloned(),
        }
    }

    /// The template expression that reads this source's value in a response. Only for a name made of letters, digits,
    /// `_`, `-` and `.`: a query parameter's name comes from the observed traffic, and an expression ends at the first
    /// `}}`, so any other name could write more template than the one variable meant.
    fn template_expression(&self) -> Option<String> {
        let (variable, key) = match self {
            Candidate::PathParam(k) => ("path", k),
            Candidate::QueryParam(k) => ("query", k),
            Candidate::Header(k) => ("header", k),
            Candidate::JsonPointer(_) => return None,
        };
        let plain = !key.is_empty()
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
        plain.then(|| format!("{{{{{variable}.{key}}}}}"))
    }
}

/// Reads the request body as a flat JSON object (no arrays or nested objects for now): each scalar field becomes a
/// `JsonPointer("/key")` candidate.
fn json_top_level_fields(body: &str) -> HashMap<String, String> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(body) else {
        return HashMap::new();
    };
    map.into_iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::String(s) => Some((k, s)),
            serde_json::Value::Number(n) => Some((k, n.to_string())),
            serde_json::Value::Bool(b) => Some((k, b.to_string())),
            _ => None,
        })
        .collect()
}

/// Looks, in order of preference (path parameter, query parameter, top-level JSON field, header), for a field whose
/// value splits the samples exactly like the response groups: same value, same group; different groups, different
/// values. Returns the chosen field and the value that stands for each group (in the order of `classes`).
fn find_discriminator(
    samples: &[Sample],
    classes: &[Vec<usize>],
) -> Option<(Candidate, Vec<String>)> {
    let json_fields: Vec<HashMap<String, String>> = samples
        .iter()
        .map(|s| json_top_level_fields(&s.exchange.request_body))
        .collect();

    let mut path_keys = BTreeSet::new();
    let mut query_keys = BTreeSet::new();
    let mut json_keys = BTreeSet::new();
    let mut header_keys = BTreeSet::new();
    for (sample, fields) in samples.iter().zip(&json_fields) {
        path_keys.extend(sample.path_params.keys().cloned());
        query_keys.extend(sample.exchange.request_query_params.keys().cloned());
        json_keys.extend(fields.keys().cloned());
        header_keys.extend(
            sample
                .exchange
                .request_headers
                .keys()
                .filter(|k| !NOISY_HEADERS.contains(&k.to_lowercase().as_str()))
                .cloned(),
        );
    }

    path_keys
        .into_iter()
        .map(Candidate::PathParam)
        .chain(query_keys.into_iter().map(Candidate::QueryParam))
        .chain(json_keys.into_iter().map(Candidate::JsonPointer))
        .chain(header_keys.into_iter().map(Candidate::Header))
        .find_map(|candidate| {
            discriminator_values(&candidate, samples, classes, &json_fields)
                .map(|values| (candidate, values))
        })
}

/// Checks one candidate: every sample of a group carries the same value for this field (a missing value disqualifies
/// it), and no other group shares that value. Returns the value of each group when it holds.
fn discriminator_values(
    candidate: &Candidate,
    samples: &[Sample],
    classes: &[Vec<usize>],
    json_fields: &[HashMap<String, String>],
) -> Option<Vec<String>> {
    let mut values = Vec::with_capacity(classes.len());
    for class in classes {
        let mut class_value: Option<String> = None;
        for &i in class {
            let v = candidate.extract(&samples[i], &json_fields[i])?;
            match &class_value {
                None => class_value = Some(v),
                Some(existing) if *existing != v => return None,
                Some(_) => {}
            }
        }
        values.push(class_value?);
    }

    let distinct: BTreeSet<&String> = values.iter().collect();
    (distinct.len() == values.len()).then_some(values)
}

#[cfg(any(test, fuzzing))]
pub(crate) mod replay;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proptests;
