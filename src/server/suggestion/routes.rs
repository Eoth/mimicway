// Route patterns inferred from the literal paths observed for one method of a service, so that `/orders/1` and
// `/orders/2` pool their samples under `/orders/{id}` and one suggestion covers them.
//
// A segment becomes a parameter when its values look like identifiers (numbers, dates, UUIDs, hashes, long tokens),
// even with a single value seen: a rule bound to one identifier rarely outlives the session it was observed in. A
// segment of plain words becomes one when at least two words answer with responses of the same structure (status,
// media type, keys of a JSON body or elements of an XML one): `/directory/restOrders` and `/directory/restStock`
// answering the same document are one route with a `{name}`, while `/orders` and `/customers`, answering different
// documents, stay two. Plain text bodies have no structure to compare, so their words stay literal.
//
// Guessing too much is safe: the correlation that follows (`super::suggest_route`) still tells the responses apart by
// the parameter's value when they differ, one rule per value, so a pattern never answers an observed path otherwise
// than it was observed. A wrong guess only costs readability.
use crate::engine::matcher::{MatchEngine, url_decode};
use crate::server::observation::ObservedExchange;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Paths with more segments are left out of suggestions: real routes are far shorter, and the inference below
/// recurses once per segment.
const MAX_SEGMENTS: usize = 32;
/// Bounds on the work spent describing one response body: bodies are already truncated
/// (`observation::max_body_size`), these only cap a body built to be expensive.
const MAX_SKELETON: usize = 4096;
const MAX_XML_EVENTS: usize = 4096;
const MAX_XML_DEPTH: usize = 64;

/// One literal path of a method, with the exchanges observed on it.
pub(super) struct ObservedPath<'a> {
    pub path: &'a str,
    pub exchanges: &'a [ObservedExchange],
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Param(String),
}

/// A route pattern and the observed paths it stands for (indices into the input of `infer`).
#[derive(Debug)]
pub(super) struct Route {
    segments: Vec<Segment>,
    pub members: Vec<usize>,
}

impl Route {
    /// The pattern in the syntax of a rule's sub-path: `/orders/{id}`, or the empty string for the service's root.
    pub fn pattern(&self) -> String {
        self.segments
            .iter()
            .map(|segment| match segment {
                Segment::Literal(value) => format!("/{value}"),
                Segment::Param(name) => format!("/{{{name}}}"),
            })
            .collect()
    }

    /// The parameters a member path gives under the pattern, decoded as the matcher decodes them.
    pub fn params_of(&self, path: &str) -> Vec<(String, String)> {
        self.segments
            .iter()
            .zip(segments(path))
            .filter_map(|(segment, raw)| match segment {
                Segment::Param(name) => Some((name.clone(), url_decode(raw))),
                Segment::Literal(_) => None,
            })
            .collect()
    }
}

/// Path segments as the matcher splits them (`match_path`): empty ones are ignored.
fn segments(path: &str) -> impl Iterator<Item = &str> {
    path.split('/').filter(|s| !s.is_empty())
}

/// Groups the paths of one method into route patterns, the most specific first: where two patterns differ, a literal
/// segment comes before a parameter, as its rule must (first match wins). `reserved` holds the parameter names of the
/// service's listen path, which a pattern must not reuse: at run time the rule's own parameter would hide them.
pub(super) fn infer(paths: &[ObservedPath], reserved: &BTreeSet<String>) -> Vec<Route> {
    let split: Vec<Vec<&str>> = paths.iter().map(|p| segments(p.path).collect()).collect();
    let mut by_length: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, segs) in split.iter().enumerate() {
        if segs.len() <= MAX_SEGMENTS {
            by_length.entry(segs.len()).or_default().push(i);
        }
    }
    let inference = Inference {
        paths,
        split: &split,
        reserved,
    };
    let mut routes = Vec::new();
    for members in by_length.into_values() {
        inference.partition(members, Vec::new(), &mut routes);
    }
    routes.sort_by(|a, b| specificity(&a.segments, &b.segments));
    routes
}

struct Inference<'a, 'p> {
    paths: &'a [ObservedPath<'p>],
    split: &'a [Vec<&'p str>],
    reserved: &'a BTreeSet<String>,
}

impl<'p> Inference<'_, 'p> {
    /// Splits `members` (paths of the same length that agree with `prefix`) on their next segment, one group per
    /// literal value plus at most one parameter group, and goes on with each group.
    fn partition(&self, members: Vec<usize>, prefix: Vec<Segment>, out: &mut Vec<Route>) {
        let position = prefix.len();
        let Some(&first) = members.first() else {
            return;
        };
        if position == self.split[first].len() {
            out.push(Route {
                segments: prefix,
                members,
            });
            return;
        }

        let mut by_value: BTreeMap<&'p str, Vec<usize>> = BTreeMap::new();
        for &m in &members {
            by_value.entry(self.split[m][position]).or_default().push(m);
        }
        let mut identifiers: Vec<usize> = Vec::new();
        let mut kinds = BTreeSet::new();
        let mut words: Vec<(&'p str, Vec<usize>)> = Vec::new();
        for (value, paths) in by_value {
            match identifier(value) {
                Some(kind) => {
                    kinds.insert(kind);
                    identifiers.extend(paths);
                }
                None if !literal_safe(value) => identifiers.extend(paths),
                None => words.push((value, paths)),
            }
        }

        // One parameter group at most: two patterns with a parameter at the same place would be the same pattern.
        // Words next to identifiers stay literal (`/users/me` beside `/users/{id}`).
        let mut groups: Vec<(Segment, Vec<usize>)> = Vec::new();
        if !identifiers.is_empty() {
            let base = if kinds.len() == 1 && kinds.contains(&Identifier::Hash) {
                "hash"
            } else {
                "id"
            };
            groups.push((Segment::Param(self.fresh_name(base, &prefix)), identifiers));
        } else if let Some(cluster) = self.same_shape_words(&words) {
            let mut merged = Vec::new();
            words.retain(|(value, paths)| {
                let keep = !cluster.contains(value);
                if !keep {
                    merged.extend(paths.iter().copied());
                }
                keep
            });
            groups.push((Segment::Param(self.fresh_name("name", &prefix)), merged));
        }
        groups.extend(
            words
                .into_iter()
                .map(|(value, paths)| (Segment::Literal(value.to_string()), paths)),
        );

        for (segment, mut paths) in groups {
            paths.sort_unstable();
            let mut next = prefix.clone();
            next.push(segment);
            self.partition(paths, next, out);
        }
    }

    /// `base`, or `base2`, `base3`... when the pattern or the listen path already has a parameter of that name.
    fn fresh_name(&self, base: &str, prefix: &[Segment]) -> String {
        let taken = |name: &str| {
            self.reserved.contains(name)
                || prefix
                    .iter()
                    .any(|s| matches!(s, Segment::Param(p) if p == name))
        };
        let mut name = base.to_string();
        let mut n = 2;
        while taken(&name) {
            name = format!("{base}{n}");
            n += 1;
        }
        name
    }

    /// The largest set of at least two words whose paths mostly answer with the same structured shape, if any.
    fn same_shape_words(&self, words: &[(&'p str, Vec<usize>)]) -> Option<BTreeSet<&'p str>> {
        if words.len() < 2 {
            return None;
        }
        let mut by_shape: BTreeMap<Shape, (BTreeSet<&'p str>, usize)> = BTreeMap::new();
        for (value, paths) in words {
            let exchanges = || paths.iter().flat_map(|&p| self.paths[p].exchanges.iter());
            let Some(shape) = dominant_shape(exchanges()) else {
                continue;
            };
            if shape.skeleton.is_empty() {
                continue;
            }
            let entry = by_shape.entry(shape).or_default();
            entry.0.insert(*value);
            entry.1 += exchanges().count();
        }
        by_shape
            .into_values()
            .filter(|(words, _)| words.len() >= 2)
            .max_by_key(|(words, exchanges)| (words.len(), *exchanges))
            .map(|(words, _)| words)
    }
}

/// Where two patterns of the same length differ first: a literal before a parameter, then by text. The service's root
/// goes last: a rule cannot match the root alone, so its suggested rule has no sub-path and matches any path.
fn specificity(a: &[Segment], b: &[Segment]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    for (x, y) in a.iter().zip(b) {
        let order = match (x, y) {
            (Segment::Literal(x), Segment::Literal(y)) | (Segment::Param(x), Segment::Param(y)) => {
                x.cmp(y)
            }
            (Segment::Literal(_), Segment::Param(_)) => Ordering::Less,
            (Segment::Param(_), Segment::Literal(_)) => Ordering::Greater,
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Number,
    Uuid,
    Hash,
    Token,
}

/// What a segment looks like when it names one item rather than a kind of resource.
fn identifier(value: &str) -> Option<Identifier> {
    let bytes = value.as_bytes();
    let digit = bytes.iter().any(u8::is_ascii_digit);
    let alpha = bytes.iter().any(u8::is_ascii_alphabetic);
    // Digits, alone or with separators: 42, 2026-10-12, 1.5.
    if digit
        && !alpha
        && bytes.first().is_some_and(u8::is_ascii_digit)
        && bytes.last().is_some_and(u8::is_ascii_digit)
        && bytes
            .iter()
            .all(|b| b.is_ascii_digit() || b"-._:".contains(b))
    {
        return Some(Identifier::Number);
    }
    if bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
    {
        return Some(Identifier::Uuid);
    }
    // Letters and digits mixed: a word rarely has digits, an identifier usually does (`v1` and `oauth2` are too short).
    if digit && alpha && bytes.len() >= 8 && bytes.iter().all(u8::is_ascii_hexdigit) {
        return Some(Identifier::Hash);
    }
    if digit
        && alpha
        && bytes.len() >= 10
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
    {
        return Some(Identifier::Token);
    }
    None
}

/// Whether a value can stand as a literal segment of a pattern: the matcher reads a segment written `:name`, or
/// `{name}` once decoded, as a parameter and `*` as a wildcard, so such a value can only be matched by a parameter.
fn literal_safe(raw: &str) -> bool {
    if raw.starts_with(':') {
        return false;
    }
    let decoded = url_decode(raw);
    !(decoded == "*" || decoded.starts_with('{') && decoded.ends_with('}'))
}

/// What a response looks like whatever its values: status, media type, and the keys of a JSON body or the elements
/// of an XML one. Two responses of one shape are the same kind of answer about different items.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Shape {
    status: u16,
    media: String,
    skeleton: String,
}

fn shape(exchange: &ObservedExchange) -> Shape {
    let media = exchange
        .response_content_type
        .as_deref()
        .and_then(|ct| ct.split(';').next())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let body = exchange.response_body.trim_start();
    let skeleton = if media.contains("json") || body.starts_with('{') || body.starts_with('[') {
        serde_json::from_str::<serde_json::Value>(body)
            .map(|value| {
                let mut out = String::new();
                json_skeleton(&value, &mut out);
                out
            })
            .unwrap_or_default()
    } else if media.contains("xml") || body.starts_with('<') {
        xml_skeleton(body)
    } else {
        String::new()
    };
    Shape {
        status: exchange.response_status,
        media,
        skeleton,
    }
}

/// Keys (sorted) and value types of a JSON document; an array is described by its first item. The parser's own depth
/// limit bounds the recursion.
fn json_skeleton(value: &serde_json::Value, out: &mut String) {
    use serde_json::Value;
    if out.len() >= MAX_SKELETON {
        return;
    }
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_unstable_by_key(|(key, _)| *key);
            out.push('{');
            for (key, value) in entries {
                out.push_str(key);
                out.push(':');
                json_skeleton(value, out);
                out.push(',');
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            if let Some(first) = items.first() {
                json_skeleton(first, out);
            }
            out.push(']');
        }
        Value::String(_) => out.push('s'),
        Value::Number(_) => out.push('n'),
        Value::Bool(_) => out.push('b'),
        Value::Null => out.push('z'),
    }
}

/// The distinct paths of elements (local names, `Envelope/Body/getUrlResponse`) of an XML document, up to the first
/// error.
fn xml_skeleton(body: &str) -> String {
    use quick_xml::events::Event;
    let mut reader = quick_xml::reader::Reader::from_str(body);
    let mut stack: Vec<String> = Vec::new();
    let mut paths = BTreeSet::new();
    for _ in 0..MAX_XML_EVENTS {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                stack.push(MatchEngine::local_name(&e));
                if stack.len() <= MAX_XML_DEPTH {
                    paths.insert(stack.join("/"));
                }
            }
            Ok(Event::Empty(e)) => {
                stack.push(MatchEngine::local_name(&e));
                if stack.len() <= MAX_XML_DEPTH {
                    paths.insert(stack.join("/"));
                }
                stack.pop();
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => {}
        }
    }
    let mut out = String::new();
    for path in paths {
        if out.len() + path.len() >= MAX_SKELETON {
            break;
        }
        out.push_str(&path);
        out.push('\n');
    }
    out
}

/// The most frequent shape among `exchanges`; on a tie, the one seen last.
fn dominant_shape<'a>(exchanges: impl Iterator<Item = &'a ObservedExchange>) -> Option<Shape> {
    let mut counts: BTreeMap<Shape, (usize, u64)> = BTreeMap::new();
    for exchange in exchanges {
        let entry = counts.entry(shape(exchange)).or_default();
        entry.0 += 1;
        entry.1 = entry.1.max(exchange.timestamp);
    }
    counts
        .into_iter()
        .max_by_key(|(_, rank)| *rank)
        .map(|(shape, _)| shape)
}

#[cfg(test)]
mod tests;
