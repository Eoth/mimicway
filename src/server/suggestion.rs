// Mock rules suggested from the exchanges `server::observation` captured while a user observed a service. Always
// computed from `ObservationStore` when asked: the input is already bounded, so recomputing costs less than a cache
// to invalidate, and suggestions can never lag behind what was observed.
//
// First the literal paths of each method are grouped into route patterns (`routes`): `/orders/1` and `/orders/2`
// pool their samples under `/orders/{id}`, whose parameter becomes a condition source like any request field. A route
// whose calls all name one SOAP operation each (`body_fields`) is split further, one suggestion per operation, each of
// its rules carrying the condition `Envelope/Body/<operation>` exists.
//
// Then, per route (or operation), the trap: two calls to the same route can legitimately get different answers, and a
// rule built from the first answer seen would silently break the others. Hence three steps: (1) group the observations
// by equivalent response; (2) one group gives an unconditional rule; (3) several groups need a request field whose
// every value leads to one group (`find_discriminator`, and the table above it of what is chosen and refused) before
// anything is suggested: one rule per group, accepting the values that lead to it. When each group has one value and
// that value is all that differs between the responses (the backend repeats it), one rule that repeats it
// (`{{path.name}}`) replaces one rule per value. When no field tells the groups apart, the calls answered with a server
// error are set aside and the others tried alone; failing that, nothing is suggested: no rule beats a fragile one.
use crate::models::{
    BodyFragment, Condition, ConditionGroup, ConditionSource, HeaderEntry, MockResponse, Operator,
};
use crate::server::observation::{ObservationKey, ObservedExchange};
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap};

mod body_fields;
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
/// correlation and tracing ids, credentials, the length of the body), not because of a decision of the backend. A
/// condition on them could fit the sample perfectly and still be useless or harmful (a rule conditioned on a token
/// never matches again, one on a length breaks as soon as a value in the body changes).
const NOISY_HEADERS: &[&str] = &[
    "content-length",
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
/// of the conditional rules covering each distinct response.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SuggestedRule {
    pub method: String,
    pub sub_path: String,
    /// Every condition of the rule: the SOAP operation (`all_of`), the value that leads to its response (`all_of`), or
    /// the values (`any_of`, one `Eq` each) when several lead to it.
    pub conditions: ConditionGroup,
    /// The one condition of the rule when it has exactly one, in `all_of`; `None` otherwise, `conditions` then says
    /// everything.
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
    /// Variance explained by a request field: one rule per distinct response, accepting the values of the field that
    /// lead to it (`Eq`, or `any_of` several `Eq`).
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
    /// The SOAP operation the suggestion stands for, when the route's calls name one each (`body_fields`).
    pub operation: Option<String>,
    /// Calls answered with a server error that no field explains, left out of the suggestion, by status.
    pub left_out: Vec<LeftOut>,
    /// Calls whose request body was cut at `observation::max_body_size()`: no field of their body was read.
    pub truncated_request_bodies: usize,
    #[serde(flatten)]
    pub outcome: Suggestion,
}

/// How many calls answered with `status` were set aside.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LeftOut {
    pub status: u16,
    pub count: usize,
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
    // The operations of one path are endpoints of their own (`ObservationKey::operation`): their exchanges are pooled
    // again here, routes are inferred from paths. Paths keep the order of their first endpoint.
    let mut by_method: BTreeMap<&str, (Vec<routes::ObservedPath>, HashMap<&str, usize>)> =
        BTreeMap::new();
    for (key, exchanges) in endpoints {
        let (paths, index) = by_method.entry(key.method.as_str()).or_default();
        let at = *index.entry(key.sub_path.as_str()).or_insert_with(|| {
            paths.push(routes::ObservedPath {
                path: &key.sub_path,
                exchanges: Vec::new(),
            });
            paths.len() - 1
        });
        paths[at].exchanges.extend(exchanges);
    }
    let by_method = by_method
        .into_iter()
        .map(|(method, (paths, _))| (method, paths));

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
                for &exchange in &path.exchanges {
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

/// The SOAP operation an observed request names, as suggestions read it (`body_fields`): what splits the observation of
/// one address into one endpoint per operation (`ObservationKey::operation`).
pub fn request_operation(exchange: &ObservedExchange) -> Option<String> {
    body_fields::read(&exchange.request_body, exchange.request_body_truncated).operation
}

/// The suggestions for one service, one per route (`observed_routes`), or one per operation of a route whose calls
/// all name one SOAP operation; a route or an operation observed fewer than `min_samples()` times is left out.
pub fn suggest_for_service(
    endpoints: &[(ObservationKey, Vec<ObservedExchange>)],
) -> Vec<EndpointSuggestion> {
    let mut suggestions = Vec::new();
    for route in observed_routes(endpoints) {
        let calls: Vec<Call> = route
            .samples
            .iter()
            .map(|sample| Call {
                sample,
                body: body_fields::read(
                    &sample.exchange.request_body,
                    sample.exchange.request_body_truncated,
                ),
            })
            .collect();
        let mut by_operation: BTreeMap<Option<&str>, Vec<&Call>> = BTreeMap::new();
        let split = calls.iter().all(|c| c.body.operation.is_some());
        for call in &calls {
            let operation = split.then_some(call.body.operation.as_deref()).flatten();
            by_operation.entry(operation).or_default().push(call);
        }
        for (operation, group) in by_operation {
            let base: Vec<Condition> = operation
                .map(|name| Condition {
                    source: ConditionSource::XPath(format!("Envelope/Body/{name}")),
                    operator: Operator::Exists,
                })
                .into_iter()
                .collect();
            let Some((outcome, left_out)) =
                suggest_calls(route.method, &route.pattern, &group, &base)
            else {
                continue;
            };
            suggestions.push(EndpointSuggestion {
                method: route.method.to_string(),
                path_count: route.literal_paths.len(),
                paths: route
                    .literal_paths
                    .iter()
                    .take(MAX_LISTED_PATHS)
                    .map(|p| p.to_string())
                    .collect(),
                sub_path: route.pattern.clone(),
                sample_count: group.len(),
                operation: operation.map(str::to_string),
                left_out,
                truncated_request_bodies: group
                    .iter()
                    .filter(|c| c.sample.exchange.request_body_truncated)
                    .count(),
                outcome,
            });
        }
    }
    suggestions
}

/// One observed call of a route, with what its request body offers.
struct Call<'a> {
    sample: &'a Sample<'a>,
    body: body_fields::BodyReading,
}

impl Call<'_> {
    fn exchange(&self) -> &ObservedExchange {
        self.sample.exchange
    }

    /// Every field of the call a condition may read, with its value: path parameters, query parameters, body fields
    /// and headers, noisy headers left out.
    fn fields(&self) -> impl Iterator<Item = (Candidate<'_>, &str)> {
        let exchange = self.exchange();
        let path = self
            .sample
            .path_params
            .iter()
            .map(|(k, v)| (Candidate::PathParam(k), v.as_str()));
        let query = exchange
            .request_query_params
            .iter()
            .map(|(k, v)| (Candidate::QueryParam(k), v.as_str()));
        // `body_fields` reads JSON pointers and XPaths only.
        let body = self
            .body
            .fields
            .iter()
            .filter_map(|(source, v)| match source {
                ConditionSource::XPath(p) => Some((Candidate::XPath(p), v.as_str())),
                ConditionSource::JsonPointer(p) => Some((Candidate::JsonPointer(p), v.as_str())),
                _ => None,
            });
        let headers = exchange
            .request_headers
            .iter()
            .filter(|(k, _)| !NOISY_HEADERS.contains(&k.to_lowercase().as_str()))
            .map(|(k, v)| (Candidate::Header(k), v.as_str()));
        path.chain(query).chain(body).chain(headers)
    }
}

/// What makes two responses equal: status and body, byte for byte. Bodies are already truncated to
/// `observation::max_body_size()`, so two bodies that differ only beyond that point count as equal. Response
/// headers are left out: the same body with another correlation header is not a different answer to explain.
fn response_signature(exchange: &ObservedExchange) -> (u16, &str) {
    (exchange.response_status, exchange.response_body.as_str())
}

/// The suggestion for the calls of one route or operation (oldest first), each rule carrying the conditions `base`
/// first, and the calls set aside. `None` while there are fewer calls than `min_samples()`: no decision yet, either way.
/// When no field explains the responses, the calls answered with a `5xx` status are set aside and the others tried
/// alone, if there are still `min_samples()` of them: a server error that nothing in the request explains says nothing
/// about the answers the others got.
fn suggest_calls(
    method: &str,
    sub_path: &str,
    calls: &[&Call],
    base: &[Condition],
) -> Option<(Suggestion, Vec<LeftOut>)> {
    if calls.len() < min_samples() {
        return None;
    }
    let unexplained = match explain(method, sub_path, calls, base) {
        Ok(suggestion) => return Some((suggestion, Vec::new())),
        Err(unexplained) => unexplained,
    };
    let (errors, kept): (Vec<&Call>, Vec<&Call>) = calls
        .iter()
        .partition(|c| c.exchange().response_status >= 500);
    if !errors.is_empty()
        && kept.len() >= min_samples()
        && let Ok(suggestion) = explain(method, sub_path, &kept, base)
    {
        let mut by_status: BTreeMap<u16, usize> = BTreeMap::new();
        for call in errors {
            *by_status
                .entry(call.exchange().response_status)
                .or_default() += 1;
        }
        let left_out = by_status
            .into_iter()
            .map(|(status, count)| LeftOut { status, count })
            .collect();
        return Some((suggestion, left_out));
    }
    Some((unexplained, Vec::new()))
}

/// The suggestion for `calls` when one response, or a field, explains them all; otherwise the diagnosis.
fn explain(
    method: &str,
    sub_path: &str,
    calls: &[&Call],
    base: &[Condition],
) -> Result<Suggestion, Suggestion> {
    // Groups of equivalent responses, as indices into `calls`, in order of first appearance. Found by signature in a
    // map: comparing each call with every group found so far costs calls × groups, quadratic when every response
    // differs.
    let mut classes: Vec<Vec<usize>> = Vec::new();
    let mut class_index: HashMap<(u16, &str), usize> = HashMap::new();
    let mut class_of = Vec::with_capacity(calls.len());
    for (i, call) in calls.iter().enumerate() {
        let class = match class_index.entry(response_signature(call.exchange())) {
            Entry::Occupied(class) => *class.get(),
            Entry::Vacant(slot) => {
                slot.insert(classes.len());
                classes.push(Vec::new());
                classes.len() - 1
            }
        };
        classes[class].push(i);
        class_of.push(class);
    }
    let latest = |class: &[usize]| calls[class[class.len() - 1]].exchange();
    let rule = |conditions: ConditionGroup, echo, sample_count, response| SuggestedRule {
        method: method.to_string(),
        sub_path: sub_path.to_string(),
        condition: match (conditions.all_of.as_slice(), conditions.any_of.is_empty()) {
            ([only], true) => Some(only.clone()),
            _ => None,
        },
        conditions,
        echo,
        sample_count,
        response,
        redirected_to: Vec::new(),
    };
    let base_group = || ConditionGroup {
        all_of: base.to_vec(),
        any_of: Vec::new(),
    };

    if classes.len() == 1 {
        let response = build_response(latest(&classes[0]));
        return Ok(Suggestion::Unconditional {
            rule: Box::new(rule(base_group(), None, calls.len(), response)),
        });
    }

    let Some(choice) = find_discriminator(calls, &class_of, classes.len()) else {
        return Err(Suggestion::VarianceUnexplained {
            response_class_count: classes.len(),
        });
    };
    if choice.values.iter().all(|values| values.len() == 1) {
        let representatives: Vec<&ObservedExchange> = classes.iter().map(|c| latest(c)).collect();
        let values: Vec<&str> = choice.values.iter().map(|v| v[0]).collect();
        if let Some(response) = echo_response(&choice.candidate, &representatives, &values) {
            return Ok(Suggestion::Echo {
                rule: Box::new(rule(
                    base_group(),
                    Some(choice.candidate.source()),
                    calls.len(),
                    response,
                )),
            });
        }
    }
    let rules = classes
        .iter()
        .zip(&choice.values)
        .map(|(class, values)| {
            let mut conditions = base_group();
            let equal = |value: &str| Condition {
                source: choice.candidate.source(),
                operator: Operator::Eq(value.to_string()),
            };
            match values.as_slice() {
                [one] => conditions.all_of.push(equal(one)),
                several => conditions.any_of = several.iter().map(|v| equal(v)).collect(),
            }
            rule(conditions, None, class.len(), build_response(latest(class)))
        })
        .collect();
    Ok(Suggestion::Conditional { rules })
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
    values: &[&str],
) -> Option<MockResponse> {
    let expression = candidate.template_expression()?;
    let latest = representatives.iter().max_by_key(|e| e.timestamp)?;
    let headers = suggested_headers(latest);
    let mut pieces: Option<Vec<&str>> = None;
    for (exchange, &value) in representatives.iter().zip(values) {
        if value.is_empty()
            || exchange.response_status != latest.response_status
            || !exchange.response_body.contains(value)
            || headers.iter().any(|h| h.value.contains(value))
            || suggested_headers(exchange)
                .iter()
                .any(|h| h.value.contains(value))
        {
            return None;
        }
        let split: Vec<&str> = exchange.response_body.split(value).collect();
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

/// A candidate condition source, without its value (known once the field is chosen), borrowed from the calls. The
/// order of the variants is the order of preference between fields with as many values.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Candidate<'a> {
    PathParam(&'a str),
    QueryParam(&'a str),
    /// A scalar of a JSON body, by its pointer, already escaped (RFC 6901).
    JsonPointer(&'a str),
    /// The text of an element of an XML body, by its path of local names.
    XPath(&'a str),
    Header(&'a str),
}

impl Candidate<'_> {
    fn source(&self) -> ConditionSource {
        match *self {
            Candidate::PathParam(k) => ConditionSource::PathParam(k.to_string()),
            Candidate::QueryParam(k) => ConditionSource::QueryParam(k.to_string()),
            Candidate::JsonPointer(k) => ConditionSource::JsonPointer(k.to_string()),
            Candidate::XPath(k) => ConditionSource::XPath(k.to_string()),
            Candidate::Header(k) => ConditionSource::Header(k.to_string()),
        }
    }

    /// The template expression that reads this source's value in a response. Only for a name made of letters, digits,
    /// `_`, `-` and `.`: a query parameter's name comes from the observed traffic, and an expression ends at the first
    /// `}}`, so any other name could write more template than the one variable meant. Body fields are not repeated yet.
    fn template_expression(&self) -> Option<String> {
        let (variable, key) = match *self {
            Candidate::PathParam(k) => ("path", k),
            Candidate::QueryParam(k) => ("query", k),
            Candidate::Header(k) => ("header", k),
            Candidate::JsonPointer(_) | Candidate::XPath(_) => return None,
        };
        let plain = !key.is_empty()
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
        plain.then(|| format!("{{{{{variable}.{key}}}}}"))
    }
}

/// The field chosen to tell the responses apart, and for each response group (in the order of `classes`) the values
/// that lead to it, sorted.
struct Choice<'a> {
    candidate: Candidate<'a>,
    values: Vec<Vec<&'a str>>,
}

/// What the calls say of one field: in how many it appears, and for each value, the response group it led to and how
/// many times it was seen; `conflict` once a value led to two groups.
#[derive(Default)]
struct FieldStats<'a> {
    present: usize,
    conflict: bool,
    values: HashMap<&'a str, (usize, usize)>,
}

// Which field tells the response groups apart. A field of the request (path parameter, query parameter, body field,
// header) is judged on the calls of one route or operation, after the calls answered with a server error are set aside
// when that is needed (`suggest_calls`); `n` calls, `k > 1` response groups.
//
// | The field                                                          | Verdict                                         |
// |--------------------------------------------------------------------|-------------------------------------------------|
// | missing from at least one call (a body cut at the size limit too)  | refused: an `Eq` condition never matches a call |
// |                                                                    | without it                                      |
// | a noisy header (`NOISY_HEADERS`)                                   | never a candidate                               |
// | one of its values seen with two responses                          | refused: it does not explain them               |
// | each value leads to one response, each response reached by one     | explains: one `Eq` rule per response, or one    |
// | value (k values)                                                   | rule that repeats it (`echo_response`)          |
// | each value leads to one response, a response reached by several    | explains: that response gets one rule, `any_of` |
// | values, at least one value seen twice                              | an `Eq` per value                               |
// | each value leads to one response, a response reached by several    | refused for lack of evidence: a value that      |
// | values, every value seen once (n values)                           | changes at every call would explain any sample  |
//
// Among the fields that explain, the one with the fewest distinct values wins: it cannot have fewer than `k`, and an
// identifier that changes at every call never wins over the name that decides. On a tie, the order of `Candidate`
// (path parameter, query parameter, JSON field, XML field, header), then the name of the field, in byte order. No field
// explains: `None`.
//
// One pass over the fields of every call, whatever their number: each (field, value) of a call updates its field's
// statistics once.
fn find_discriminator<'a>(
    calls: &[&'a Call],
    class_of: &[usize],
    class_count: usize,
) -> Option<Choice<'a>> {
    let mut stats: HashMap<Candidate<'a>, FieldStats<'a>> = HashMap::new();
    for (call, &class) in calls.iter().zip(class_of) {
        for (candidate, value) in call.fields() {
            let field = stats.entry(candidate).or_default();
            if field.conflict {
                continue;
            }
            field.present += 1;
            match field.values.entry(value) {
                Entry::Occupied(mut seen) => {
                    let (seen_class, count) = seen.get_mut();
                    if *seen_class == class {
                        *count += 1;
                    } else {
                        field.conflict = true;
                        field.values = HashMap::new();
                    }
                }
                Entry::Vacant(slot) => {
                    slot.insert((class, 1));
                }
            }
        }
    }

    let (candidate, field) = stats
        .into_iter()
        .filter(|(_, field)| {
            let explains = !field.conflict && field.present == calls.len();
            let several_per_response = field.values.len() > class_count;
            let seen_once = field.values.values().all(|&(_, count)| count == 1);
            explains && !(several_per_response && seen_once)
        })
        .min_by(|(a, fa), (b, fb)| (fa.values.len(), a).cmp(&(fb.values.len(), b)))?;
    let mut values: Vec<Vec<&str>> = vec![Vec::new(); class_count];
    for (value, (class, _)) in field.values {
        values[class].push(value);
    }
    for list in &mut values {
        list.sort_unstable();
    }
    Some(Choice { candidate, values })
}

#[cfg(any(test, fuzzing))]
pub(crate) mod replay;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod proptests;
