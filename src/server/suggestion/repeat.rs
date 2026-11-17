//! Responses that repeat what their own request sent: one rule that reads each value from the request, where one rule
//! per value seen would answer a request never seen with the values of another.
//!
//! Two shapes, found on the calls of one route or operation whose responses differ:
//! - values: each response is the same text around values of its own request (path, query, header, JSON or XML field
//!   of the body). The response becomes that text, in `Literal` fragments, and one template expression per value
//!   (`{{xpath.Envelope/Body/GetStatus/ref | replace("&", "&amp;") | …}}`), written here from a field name checked
//!   first, never from the traffic; its pipes escape the value as the format of the response does (templates substitute
//!   values as they are), so that a value never seen with `&` or a quote keeps the response valid.
//! - a list: the request holds a repeated element (`<slot>` items, a JSON array of objects) and the response a block
//!   per item, each block the same text around values of the item of the same rank. Templates cannot loop: a Rhai
//!   script is written that reads the items (`parse_xml_items`, `parse_json`) and builds the blocks, each value escaped
//!   for the format of the response, and the response reads its result (`{{script.items}}`). Every text observed that
//!   enters the script is a Rhai string literal written by `rhai_string`, so that no traffic can write code.
//!
//! How the shape is found: the responses are read side by side from their start (`align`). Where they stop being equal,
//! a field whose value starts at the same offset in every response, and covers the point where they differ, is taken
//! (the longest such values first: a short value found there by chance loses to the field the backend repeats). For a
//! list, tried before the values alone: the calls with items read side by side, the list taken where they differ; its
//! blocks read the same way (the first blocks, then the second ones with them), then the text between two blocks; then
//! every call again, its list text one more value, so that the calls without an item bring their evidence. A value seen
//! with a single content in every response stays in the literal text: it proves nothing.
//!
//! The guarantee: whatever this reading finds, the rule is suggested only if the engine itself (templates and script,
//! `renders_back`) gives back every observed response byte for byte from its own request; otherwise the caller
//! suggests what it suggested before. The reading is bounded (values of `MAX_VALUE` bytes at most, `MAX_REPEATS`
//! values, a few lists and starts tried, a gap between blocks of `MAX_GAP` bytes at most), so that its cost stays
//! proportional to the length of the responses times their number.
use super::{Call, Candidate};
use crate::engine::script::{ScriptContext, ScriptEngine};
use crate::engine::{TemplateContext, TemplateRenderer};
use crate::models::{BodyFragment, ConditionSource, MockResponse};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::OnceLock;

/// Longest value followed in a response, in bytes: a value repeated is a reference, a date, a code.
const MAX_VALUE: usize = 256;
/// Values read in one response (fragments of the template, or of a block).
const MAX_REPEATS: usize = 64;
/// Repeated elements of the request tried as a list, per suggestion.
const MAX_LISTS: usize = 4;
/// Places tried for the first block of a list, at the point where the responses differ.
const MAX_LIST_STARTS: usize = 8;
/// Longest text between two blocks, in bytes, and places tried for the second block.
const MAX_GAP: usize = 4096;
const MAX_GAP_TRIES: usize = 16;
/// Distinct fields of the items of one list.
const MAX_ITEM_FIELDS: usize = 32;
/// The field of the script's result that holds the blocks.
const LIST_FIELD: &str = "items";

/// A list the response repeats: one block per item of the request.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RepeatedList {
    /// The repeated element of the request: the XPath of the element (`Envelope/Body/Book/slot`), or the JSON pointer
    /// of the array (`/slots`).
    pub items: ConditionSource,
    /// The fields of an item that each block repeats, in the order of a block.
    pub fields: Vec<String>,
}

/// The response of a rule that repeats its request, checked against every call it comes from.
pub(super) struct Repetition {
    pub body: Vec<BodyFragment>,
    /// The request fields the response repeats, in the order of their first appearance.
    pub fields: Vec<ConditionSource>,
    pub list: Option<RepeatedList>,
    pub script: Option<String>,
}

/// One response read side by side with the others: its text and, per candidate, the value it should repeat.
struct Stream<'a> {
    text: &'a [u8],
    values: Vec<&'a [u8]>,
}

#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Literal(Vec<u8>),
    /// The value of a candidate (a request field, or a field of an item inside a block).
    Value(usize),
    List(Block),
}

/// One block per item: `inner` from the first value of an item to its last value, `gap` between two blocks.
#[derive(Debug, Clone, PartialEq)]
struct Block {
    inner: Vec<Piece>,
    gap: Vec<u8>,
}

fn push_literal(pieces: &mut Vec<Piece>, text: &[u8]) {
    if text.is_empty() {
        return;
    }
    match pieces.last_mut() {
        Some(Piece::Literal(value)) => value.extend_from_slice(text),
        _ => pieces.push(Piece::Literal(text.to_vec())),
    }
}

/// How many bytes every stream shares from its position on.
fn common_prefix(streams: &[Stream], pos: &[usize]) -> usize {
    let first = &streams[0].text[pos[0]..];
    let mut length = first.len();
    for (stream, &p) in streams.iter().zip(pos).skip(1) {
        let other = &stream.text[p..];
        length = length.min(
            first[..length.min(other.len())]
                .iter()
                .zip(other)
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    length
}

/// The candidate whose value starts at the same offset `t <= d` in every stream and covers the offset `d` in one of
/// them at least, the longest values first, then the smallest offset, then the first candidate; with the bytes it
/// covers in all.
fn value_at(streams: &[Stream], pos: &[usize], d: usize) -> Option<(usize, usize, usize)> {
    let mut best: Option<(usize, usize, usize)> = None;
    for c in 0..streams[0].values.len() {
        let longest = streams.iter().map(|s| s.values[c].len()).max().unwrap_or(0);
        if longest == 0 {
            continue;
        }
        // A list, longer than any value, starts at most `MAX_GAP` bytes before the point where the responses differ.
        for t in (d + 1)
            .saturating_sub(longest)
            .max(d.saturating_sub(MAX_GAP))..=d
        {
            let fits = streams.iter().zip(pos).all(|(s, &p)| {
                s.text
                    .get(p + t..)
                    .is_some_and(|rest| rest.starts_with(s.values[c]))
            });
            let covers = streams.iter().any(|s| t + s.values[c].len() > d);
            if fits && covers {
                let score: usize = streams.iter().map(|s| t + s.values[c].len()).sum();
                if best.is_none_or(|(_, _, b)| score > b) {
                    best = Some((c, t, score));
                }
                break;
            }
        }
    }
    best
}

/// Reads the streams side by side from `starts`, as the module comment says. `None` at the first point where they
/// differ and nothing explains it, or past the bounds; with `stop`, what was read up to that point instead. The list,
/// when given, is taken at most once. Returns the pieces and the positions reached.
fn align(
    streams: &[Stream],
    starts: &[usize],
    stop: bool,
    mut list: Option<&ListSearch>,
) -> Option<(Vec<Piece>, Vec<usize>)> {
    let mut pos = starts.to_vec();
    let mut pieces = Vec::new();
    let mut values = 0;
    loop {
        let d = common_prefix(streams, &pos);
        if streams
            .iter()
            .zip(&pos)
            .all(|(s, &p)| p + d == s.text.len())
        {
            push_literal(&mut pieces, &streams[0].text[pos[0]..pos[0] + d]);
            for (p, s) in pos.iter_mut().zip(streams) {
                *p = s.text.len();
            }
            return Some((pieces, pos));
        }
        let value = (values < MAX_REPEATS)
            .then(|| value_at(streams, &pos, d))
            .flatten();
        let block = list.and_then(|l| l.find(streams, &pos, d));
        match (value, block) {
            (_, Some((t, block, lengths, score))) if value.is_none_or(|(_, _, v)| score >= v) => {
                push_literal(&mut pieces, &streams[0].text[pos[0]..pos[0] + t]);
                pieces.push(Piece::List(block));
                for (p, length) in pos.iter_mut().zip(lengths) {
                    *p += t + length;
                }
                list = None;
            }
            (Some((c, t, _)), _) => {
                push_literal(&mut pieces, &streams[0].text[pos[0]..pos[0] + t]);
                pieces.push(Piece::Value(c));
                for (p, s) in pos.iter_mut().zip(streams) {
                    *p += t + s.values[c].len();
                }
                values += 1;
            }
            _ if stop => return Some((pieces, pos)),
            _ => return None,
        }
    }
}

/// How the values of a list are written in the response, and so escaped by the script.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Escape {
    /// Text of an XML element: `&`, `<` and `>`.
    XmlText,
    /// Every markup character of XML, quotes too, as `xml_element` writes them.
    XmlAll,
    /// The inside of a JSON string; numbers and booleans as JSON writes them.
    Json,
    /// As they are, for a response that is neither XML nor JSON.
    Raw,
}

impl Escape {
    fn apply(self, value: &str) -> String {
        match self {
            Escape::XmlText => value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;"),
            Escape::XmlAll => value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;"),
            Escape::Json | Escape::Raw => value.to_string(),
        }
    }

    /// A request value as the response writes it, read by a template: the same escaping as `value_pipes`.
    fn value(self, value: &str) -> String {
        match self {
            Escape::XmlText | Escape::XmlAll => self.apply(value),
            Escape::Json => value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t"),
            Escape::Raw => value.to_string(),
        }
    }

    /// The pipes that escape a value read by a template, as `value` does: templates substitute values as they are.
    fn value_pipes(self) -> &'static str {
        match self {
            Escape::XmlText => {
                r#" | replace("&", "&amp;") | replace("<", "&lt;") | replace(">", "&gt;")"#
            }
            Escape::XmlAll => {
                r#" | replace("&", "&amp;") | replace("<", "&lt;") | replace(">", "&gt;") | replace("\"", "&quot;") | replace("'", "&apos;")"#
            }
            Escape::Json => {
                " | replace(\"\\\\\", \"\\\\\\\\\") | replace(\"\\\"\", \"\\\\\\\"\") \
                 | replace(\"\n\", \"\\\\n\") | replace(\"\r\", \"\\\\r\") | replace(\"\t\", \"\\\\t\")"
            }
            Escape::Raw => "",
        }
    }

    /// The Rhai function `esc(v)` of the script: `v` as this escape writes it, an absent value as an empty text.
    fn rhai_function(self) -> &'static str {
        // `"" + v` is the text of `v`, and an empty text for an absent value (`()`).
        match self {
            Escape::XmlText => {
                "fn esc(v) {\n    let s = \"\" + v;\n    s.replace(\"&\", \"&amp;\");\n    s.replace(\"<\", \"&lt;\");\n    \
                 s.replace(\">\", \"&gt;\");\n    s\n}\n"
            }
            Escape::XmlAll => {
                "fn esc(v) {\n    let s = \"\" + v;\n    s.replace(\"&\", \"&amp;\");\n    s.replace(\"<\", \"&lt;\");\n    \
                 s.replace(\">\", \"&gt;\");\n    s.replace(\"\\\"\", \"&quot;\");\n    s.replace(\"'\", \"&apos;\");\n    s\n}\n"
            }
            Escape::Json => {
                "fn esc(v) {\n    if type_of(v) == \"()\" { return \"\"; }\n    let s = to_json(v);\n    \
                 if type_of(v) == \"string\" { s.sub_string(1, s.len() - 2) } else { s }\n}\n"
            }
            Escape::Raw => "fn esc(v) {\n    \"\" + v\n}\n",
        }
    }
}

/// Where the items of a list are in a request body.
#[derive(Clone, Debug, PartialEq)]
enum ListSource {
    /// The element repeated at this path, read by `parse_xml_items`.
    Xml(String),
    /// The array at this JSON pointer, read by `parse_json`.
    Json(String),
}

/// The items of `source` in `body`, each a map of its fields to their text as the script reads them; `None` when the
/// body does not hold them as the script would read them (an item that is no object, a limit of the script reached).
fn read_items(source: &ListSource, body: &str) -> Option<Vec<BTreeMap<String, String>>> {
    match source {
        ListSource::Xml(path) => {
            let items = crate::engine::script::parse_xml_items_impl(body, path).ok()?;
            items
                .into_iter()
                .map(|item| {
                    let map = item.try_cast::<rhai::Map>()?;
                    map.into_iter()
                        .map(|(k, v)| Some((k.to_string(), v.into_string().ok()?)))
                        .collect()
                })
                .collect()
        }
        ListSource::Json(pointer) => {
            let value = crate::engine::json_pointer::value_at(body.as_bytes(), pointer)?;
            let serde_json::Value::Array(items) = value else {
                return None;
            };
            items
                .into_iter()
                .map(|item| {
                    let serde_json::Value::Object(map) = item else {
                        return None;
                    };
                    map.into_iter()
                        .filter_map(|(k, v)| {
                            let text = match v {
                                // Escaped for JSON here, as `to_json` writes a string, without its quotes.
                                serde_json::Value::String(s) => {
                                    let quoted = serde_json::to_string(&s).ok()?;
                                    quoted[1..quoted.len() - 1].to_string()
                                }
                                serde_json::Value::Number(n) => n.to_string(),
                                serde_json::Value::Bool(b) => b.to_string(),
                                _ => return None,
                            };
                            Some(Some((k, text)))
                        })
                        .collect()
                })
                .collect()
        }
    }
}

/// The lists a request may hold, from the body fields of the calls: the parent of an XML field (`…/slot/date` gives
/// `…/slot`), the array before an index (`/slots/0/date` gives `/slots`). The first ones in the order of the fields,
/// leaving out a parent of more than `MAX_ITEM_FIELDS` field names: items of one kind have a few fields, and reading
/// the items of a large record would cost for nothing.
fn list_sources(calls: &[&Call]) -> Vec<ListSource> {
    // Per list, in the order found: its rank and the names of its fields.
    let mut found: Vec<(ListSource, BTreeSet<&str>)> = Vec::new();
    for call in calls {
        for (source, _) in &call.body.fields {
            let Some((list, name)) = list_of(source) else {
                continue;
            };
            match found.iter_mut().find(|(known, _)| *known == list) {
                Some((_, names)) => {
                    names.insert(name);
                }
                None => found.push((list, BTreeSet::from([name]))),
            }
        }
    }
    found
        .into_iter()
        .filter(|(_, names)| names.len() <= MAX_ITEM_FIELDS)
        .map(|(list, _)| list)
        .take(MAX_LISTS)
        .collect()
}

/// The list a body field would be the field of an item of, and the name of that field.
fn list_of(source: &ConditionSource) -> Option<(ListSource, &str)> {
    match source {
        ConditionSource::XPath(path) => path
            .rsplit_once('/')
            .filter(|(parent, _)| parent.contains('/'))
            .map(|(parent, name)| (ListSource::Xml(parent.to_string()), name)),
        ConditionSource::JsonPointer(pointer) => {
            let segments: Vec<&str> = pointer.split('/').collect();
            let n = segments.len();
            (n >= 4 && segments[n - 2].bytes().all(|b| b.is_ascii_digit())).then(|| {
                (
                    ListSource::Json(segments[..n - 2].join("/")),
                    segments[n - 1],
                )
            })
        }
        _ => None,
    }
}

/// A list tried in the responses: per stream (a call with one item at least), the escaped values of each item by field
/// (`""` for a field an item lacks, as the script reads it).
struct ListSearch {
    fields: Vec<String>,
    items: Vec<Vec<Vec<Vec<u8>>>>,
}

impl ListSearch {
    fn rendered(&self, inner: &[Piece], stream: usize, item: usize, out: &mut Vec<u8>) {
        for piece in inner {
            match piece {
                Piece::Literal(text) => out.extend_from_slice(text),
                Piece::Value(f) => out.extend_from_slice(&self.items[stream][item][*f]),
                Piece::List(_) => {}
            }
        }
    }

    /// The text of the blocks of a stream, as the script writes them, without the parts that move around the list.
    fn core(&self, block: &Block, stream: usize) -> Vec<u8> {
        let mut out = Vec::new();
        for item in 0..self.items[stream].len() {
            if item > 0 {
                out.extend_from_slice(&block.gap);
            }
            self.rendered(&block.inner, stream, item, &mut out);
        }
        out
    }

    /// The list, when it starts at an offset `t <= d` of every stream with the value of a field of its first item that
    /// covers `d`: the offset, the blocks, the length of the blocks in each stream and the bytes covered in all.
    fn find(
        &self,
        streams: &[Stream],
        pos: &[usize],
        d: usize,
    ) -> Option<(usize, Block, Vec<usize>, usize)> {
        let mut starts = Vec::new();
        for f in 0..self.fields.len() {
            let value = |i: usize| self.items[i][0][f].as_slice();
            let longest = (0..streams.len()).map(|i| value(i).len()).max()?;
            for t in (d + 1).saturating_sub(longest)..=d {
                let fits = streams.iter().zip(pos).enumerate().all(|(i, (s, &p))| {
                    s.text
                        .get(p + t..)
                        .is_some_and(|rest| rest.starts_with(value(i)))
                });
                if fits && (0..streams.len()).any(|i| t + value(i).len() > d) {
                    starts.push(t);
                }
            }
        }
        starts.sort_unstable();
        starts.dedup();
        for &t in starts.iter().take(MAX_LIST_STARTS) {
            let at: Vec<usize> = pos.iter().map(|p| p + t).collect();
            if let Some((block, lengths)) = self.blocks(streams, &at) {
                let score = lengths.iter().map(|l| t + l).sum();
                return Some((t, block, lengths, score));
            }
        }
        None
    }

    /// The pieces of a block read side by side in `starts` (one stream per block, with the values of its item), cut
    /// after their last value.
    fn inner(&self, streams: &[Stream], blocks: &[(usize, usize, usize)]) -> Option<Vec<Piece>> {
        let side: Vec<Stream> = blocks
            .iter()
            .map(|&(i, item, _)| Stream {
                text: streams[i].text,
                values: self.items[i][item].iter().map(Vec::as_slice).collect(),
            })
            .collect();
        let starts: Vec<usize> = blocks.iter().map(|&(_, _, start)| start).collect();
        let (mut inner, _) = align(&side, &starts, true, None)?;
        let last = inner.iter().rposition(|p| matches!(p, Piece::Value(_)))?;
        inner.truncate(last + 1);
        Some(inner)
    }

    /// Where each stream's first block, read as `inner`, ends.
    fn ends(&self, inner: &[Piece], at: &[usize]) -> Vec<usize> {
        (0..at.len())
            .map(|i| {
                let mut first = Vec::new();
                self.rendered(inner, i, 0, &mut first);
                at[i] + first.len()
            })
            .collect()
    }

    /// The gaps that may follow a first block read as `inner`: the same text in every stream with several items,
    /// followed by `next` (the second block, or its start); at most `MAX_GAP_TRIES` of them, the shortest first.
    fn gaps(
        &self,
        streams: &[Stream],
        ends: &[usize],
        several: &[usize],
        next: impl Fn(usize) -> Vec<u8>,
    ) -> Vec<usize> {
        let Some(&r) = several.first() else {
            return vec![0];
        };
        let reference = next(r);
        let text = &streams[r].text[ends[r]..];
        (0..=MAX_GAP.min(text.len()))
            .filter(|&g| {
                text[g..].starts_with(&reference)
                    && several.iter().all(|&i| {
                        let other = &streams[i].text[ends[i]..];
                        other.len() >= g
                            && other[..g] == text[..g]
                            && other[g..].starts_with(&next(i))
                    })
            })
            .take(MAX_GAP_TRIES)
            .collect()
    }

    /// The blocks of a list whose first block starts at `at` in every stream: the first blocks read side by side (with
    /// the values of each first item), cut after their last value. A field whose value is the same in every first
    /// item stays literal there; so the second blocks, found after the first ones, are read with them, which sees it
    /// change. Then the gap, the same text in every stream between two blocks. `None` unless every stream holds its
    /// blocks exactly there.
    fn blocks(&self, streams: &[Stream], at: &[usize]) -> Option<(Block, Vec<usize>)> {
        let firsts: Vec<(usize, usize, usize)> =
            (0..streams.len()).map(|i| (i, 0, at[i])).collect();
        let first = self.inner(streams, &firsts)?;
        let several: Vec<usize> = (0..streams.len())
            .filter(|&i| self.items[i].len() > 1)
            .collect();

        let mut tried = vec![first.clone()];
        if !several.is_empty() {
            let lead = first
                .iter()
                .position(|p| matches!(p, Piece::Value(_)))
                .map_or(0, |v| v + 1);
            let ends = self.ends(&first, at);
            let start_of_second = |i: usize| {
                let mut out = Vec::new();
                self.rendered(&first[..lead], i, 1, &mut out);
                out
            };
            for g in self.gaps(streams, &ends, &several, start_of_second) {
                let mut blocks = firsts.clone();
                blocks.extend(several.iter().map(|&i| (i, 1, ends[i] + g)));
                if let Some(inner) = self.inner(streams, &blocks)
                    && !tried.contains(&inner)
                {
                    tried.insert(0, inner);
                }
            }
        }

        for inner in tried {
            let ends = self.ends(&inner, at);
            let mut block = Block {
                inner,
                gap: Vec::new(),
            };
            let second = |i: usize| {
                let mut out = Vec::new();
                self.rendered(&block.inner, i, 1, &mut out);
                out
            };
            for g in self.gaps(streams, &ends, &several, second) {
                if let Some(&r) = several.first() {
                    block.gap = streams[r].text[ends[r]..ends[r] + g].to_vec();
                }
                let lengths: Option<Vec<usize>> = (0..streams.len())
                    .map(|i| {
                        let core = self.core(&block, i);
                        streams[i].text[at[i]..]
                            .starts_with(&core)
                            .then_some(core.len())
                    })
                    .collect();
                if let Some(lengths) = lengths {
                    return Some((block, lengths));
                }
            }
        }
        None
    }
}

/// The candidates a response may repeat: fields present in every call with a value of `MAX_VALUE` bytes at most, and
/// a template expression; with their value in each call, in the order of `Candidate`.
fn candidates<'a>(calls: &[&'a Call]) -> Vec<(Candidate<'a>, String, Vec<&'a str>)> {
    let mut found: BTreeMap<Candidate<'a>, Vec<&'a str>> = BTreeMap::new();
    for (i, call) in calls.iter().enumerate() {
        for (candidate, value) in call.fields() {
            let values = found.entry(candidate).or_default();
            if values.len() == i {
                values.push(value);
            }
        }
    }
    found
        .into_iter()
        .filter(|(_, values)| {
            values.len() == calls.len() && values.iter().all(|v| v.len() <= MAX_VALUE)
        })
        .filter_map(|(candidate, values)| {
            Some((candidate, candidate.template_expression()?, values))
        })
        .collect()
}

/// The script engine that checks scripts as the interception runs them, built once.
fn engine() -> &'static ScriptEngine {
    static ENGINE: OnceLock<ScriptEngine> = OnceLock::new();
    ENGINE.get_or_init(ScriptEngine::new)
}

/// Whether `response`, with `script`, rendered by the engine for each call's request as the interception renders it,
/// gives back the call's observed status and body byte for byte.
pub(super) fn renders_back(response: &MockResponse, script: Option<&str>, calls: &[&Call]) -> bool {
    calls.iter().all(|call| {
        let exchange = call.exchange();
        let path_params: HashMap<String, String> = call
            .sample
            .path_params
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let script_result = match script {
            Some(code) => {
                let ctx = ScriptContext {
                    body: exchange.request_body.clone(),
                    headers: exchange.request_headers.clone(),
                    query_params: exchange.request_query_params.clone(),
                    path_params: path_params.clone(),
                };
                match engine().execute(code, &ctx) {
                    Ok(result) => Some(result),
                    Err(_) => return false,
                }
            }
            None => None,
        };
        let ctx = TemplateContext {
            path_params: &path_params,
            query_params: &exchange.request_query_params,
            headers: &exchange.request_headers,
            request_body: exchange.request_body.as_bytes(),
            seq_counter: 0,
            script_result: script_result.as_ref(),
        };
        response.status == exchange.response_status
            && TemplateRenderer::render_body(&response.body, &[], &ctx) == exchange.response_body
    })
}

/// `text` as a Rhai string literal: letters, digits, spaces and the punctuation of markup as they are, every other
/// character (quotes, backslashes, line breaks, `$`, backquotes, anything outside ASCII) as a `\U` escape. Whatever the
/// text, the literal is one string token whose value is the text: observed traffic never becomes code.
pub(super) fn rhai_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || " <>/=:;,.-_#!?()[]{}@*+%&'|~^".contains(c) {
            out.push(c);
        } else {
            let _ = write!(out, "\\U{:08x}", u32::from(c));
        }
    }
    out.push('"');
    out
}

/// The script that writes the blocks of `list`: `head` before the first block, `gap` between two, `tail` after the
/// last, nothing at all for no item; each value of an item escaped as `escape` says.
fn list_script(
    source: &ListSource,
    fields: &[String],
    block: &Block,
    head: &str,
    tail: &str,
    escape: Escape,
) -> String {
    let mut script = String::from(
        "// One block per item of the request's list, written from the observed calls.\n",
    );
    script.push_str(escape.rhai_function());
    match source {
        ListSource::Xml(path) => {
            let _ = writeln!(
                script,
                "let items = parse_xml_items(request.body, {});",
                rhai_string(path)
            );
        }
        ListSource::Json(pointer) => {
            script.push_str(
                "fn at(v, key, index) {\n    if type_of(v) == \"map\" { return v[key]; }\n    \
                 if type_of(v) == \"array\" && index >= 0 && index < v.len() { return v[index]; }\n    ()\n}\n",
            );
            script.push_str("let items = parse_json(request.body);\n");
            for segment in pointer.split('/').skip(1) {
                let key = segment.replace("~1", "/").replace("~0", "~");
                let index: i64 = if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) {
                    key.parse().unwrap_or(-1)
                } else {
                    -1
                };
                let _ = writeln!(script, "items = at(items, {}, {index});", rhai_string(&key));
            }
            script.push_str("if type_of(items) != \"array\" { items = []; }\n");
        }
    }
    // One statement per item: a script may run 10,000 operations, and each statement costs some.
    let _ = writeln!(
        script,
        "let out = \"\";\nlet sep = {};\nfor item in items {{",
        rhai_string(head)
    );
    if matches!(source, ListSource::Json(_)) {
        script.push_str("    if type_of(item) != \"map\" { item = #{}; }\n");
    }
    let mut line = String::from("    out += sep");
    for piece in &block.inner {
        match piece {
            Piece::Literal(text) => {
                let _ = write!(line, " + {}", rhai_string(&String::from_utf8_lossy(text)));
            }
            Piece::Value(f) => {
                let _ = write!(line, " + esc(item[{}])", rhai_string(&fields[*f]));
            }
            Piece::List(_) => {}
        }
    }
    let _ = write!(
        script,
        "{line};\n    sep = {};\n}}\nif items.len() > 0 {{ out += {}; }}\n#{{ {LIST_FIELD}: out }}\n",
        rhai_string(&String::from_utf8_lossy(&block.gap)),
        rhai_string(tail)
    );
    script
}

/// The longest common suffix of `a` and `b`, in bytes.
fn common_suffix(a: &[u8], b: &[u8]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

fn text(bytes: Vec<u8>) -> Option<String> {
    String::from_utf8(bytes).ok()
}

/// The response that repeats its request for `calls` (oldest first), read on `representatives` (one call per distinct
/// response), and checked on every call; `None` when there is none, and the caller suggests what it did before.
/// Request values are first looked for as the format of the response escapes them, read by an expression that escapes
/// them the same way; then as they are (an XML value in a CDATA section), read as they are.
pub(super) fn infer(
    representatives: &[&Call],
    calls: &[&Call],
    headers: &[crate::models::HeaderEntry],
) -> Option<Repetition> {
    let status = representatives[0].exchange().response_status;
    if calls.iter().any(|c| c.exchange().response_status != status) {
        return None;
    }
    let format = match representatives[0]
        .exchange()
        .response_body
        .trim_start()
        .as_bytes()
        .first()
    {
        Some(b'<') => Escape::XmlText,
        Some(b'{' | b'[') => Escape::Json,
        _ => Escape::Raw,
    };
    // Quotes escaped too only when a value holds one: otherwise both escapes write the same responses.
    let quoted = candidates(representatives)
        .iter()
        .any(|(_, _, values)| values.iter().any(|v| v.contains(['"', '\''])));
    let attempts: &[Escape] = match format {
        Escape::Raw => &[Escape::Raw],
        Escape::XmlText if quoted => &[Escape::XmlText, Escape::XmlAll, Escape::Raw],
        _ => &[format, Escape::Raw],
    };
    attempts
        .iter()
        .find_map(|&values| infer_with(representatives, calls, headers, format, values))
}

/// `infer` with the request values written as `values` escapes them, and the lists as `format` does. The lists are
/// tried first: when every call holds as many items, the values alone would read the blocks as fields of the items at
/// fixed ranks (`/slots/2/type`), which answers no request with another number of items.
fn infer_with(
    representatives: &[&Call],
    calls: &[&Call],
    headers: &[crate::models::HeaderEntry],
    format: Escape,
    values: Escape,
) -> Option<Repetition> {
    let status = representatives[0].exchange().response_status;
    let found: Vec<(Candidate, String, Vec<String>)> = candidates(representatives)
        .into_iter()
        .map(|(candidate, expression, raw)| {
            let expression = format!(
                "{}{}}}}}",
                expression.trim_end_matches('}'),
                values.value_pipes()
            );
            let escaped = raw.iter().map(|v| values.value(v)).collect();
            (candidate, expression, escaped)
        })
        .collect();
    let streams: Vec<Stream> = representatives
        .iter()
        .enumerate()
        .map(|(i, call)| Stream {
            text: call.exchange().response_body.as_bytes(),
            values: found.iter().map(|(_, _, v)| v[i].as_bytes()).collect(),
        })
        .collect();
    let value_of = |piece: &Piece| match piece {
        Piece::Value(c) => found.get(*c),
        _ => None,
    };
    let response = |body| MockResponse {
        status,
        headers: headers.to_vec(),
        body,
        chaos: None,
        events: None,
    };
    let repeated = |pieces: &[Piece]| -> Vec<ConditionSource> {
        let mut fields: Vec<ConditionSource> = Vec::new();
        for piece in pieces {
            if let Some((candidate, _, _)) = value_of(piece) {
                let source = candidate.source();
                if !fields.contains(&source) {
                    fields.push(source);
                }
            }
        }
        fields
    };
    // A value repeated in a header is not generalized: headers are copied as they are.
    let in_headers = |pieces: &[Piece]| {
        pieces.iter().any(|piece| match value_of(piece) {
            Some((repeated, _, _)) => calls.iter().any(|call| {
                call.fields().any(|(candidate, value)| {
                    candidate == *repeated
                        && !value.is_empty()
                        && super::suggested_headers(call.exchange())
                            .iter()
                            .chain(headers)
                            .any(|h| h.value.contains(value))
                })
            }),
            _ => false,
        })
    };
    let fragments = |pieces: &[Piece]| -> Option<Vec<BodyFragment>> {
        pieces
            .iter()
            .map(|piece| {
                Some(match (piece, value_of(piece)) {
                    (Piece::Literal(bytes), _) => BodyFragment::Literal {
                        value: text(bytes.clone())?,
                    },
                    (_, Some((_, expression, _))) => BodyFragment::Template {
                        template: expression.clone(),
                    },
                    // The list: a column after the candidates, or the blocks themselves.
                    _ => BodyFragment::Template {
                        template: format!("{{{{script.{LIST_FIELD}}}}}"),
                    },
                })
            })
            .collect()
    };

    for source in list_sources(representatives) {
        // The fields of the first item are body fields already read: a response holding a block per item holds one of
        // them. Checked before the items are read, which costs a reading of the body.
        let first_shown = representatives.iter().all(|call| {
            let mut fields = call
                .body
                .fields
                .iter()
                .filter(|(field, _)| list_of(field).is_some_and(|(list, _)| list == source))
                .peekable();
            let response = &call.exchange().response_body;
            fields.peek().is_none()
                || fields.any(|(_, v)| {
                    // A body field is the value itself; the response writes it escaped.
                    let written = match source {
                        ListSource::Json(_) => [Escape::Json.value(v), String::new()],
                        ListSource::Xml(_) => [format.apply(v), Escape::XmlAll.apply(v)],
                    };
                    !v.is_empty()
                        && written
                            .iter()
                            .any(|w| !w.is_empty() && response.contains(w))
                })
        });
        if !first_shown {
            continue;
        }
        let read: Option<Vec<Vec<BTreeMap<String, String>>>> = representatives
            .iter()
            .map(|call| read_items(&source, &call.exchange().request_body))
            .collect();
        let Some(read) = read else { continue };
        if read.iter().map(Vec::len).max().unwrap_or(0) < 2 {
            continue;
        }
        let fields: Vec<String> = read
            .iter()
            .flatten()
            .flat_map(|item| item.keys().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        // Items of one kind have a few fields; and a response holding a block per item holds the values of its first
        // and last items. Both checked before any reading side by side, which costs the items times their fields.
        let shown = |i: usize| {
            let (Some(first), Some(last)) = (read[i].first(), read[i].last()) else {
                return true;
            };
            let response = &representatives[i].exchange().response_body;
            fields.iter().any(|f| {
                [first, last].iter().all(|item| {
                    item.get(f).is_some_and(|v| {
                        !v.is_empty()
                            && (response.contains(&format.apply(v))
                                || response.contains(&Escape::XmlAll.apply(v)))
                    })
                })
            })
        };
        if fields.len() > MAX_ITEM_FIELDS || !(0..read.len()).all(shown) {
            continue;
        }
        let quoted = read
            .iter()
            .flatten()
            .flat_map(|item| item.values())
            .any(|v| v.contains(['"', '\'']));
        let escapes: &[Escape] = match (format, &source) {
            (Escape::XmlText, _) if quoted => &[Escape::XmlText, Escape::XmlAll],
            (Escape::Json, ListSource::Xml(_)) | (Escape::XmlText, ListSource::Json(_)) => {
                continue;
            }
            _ => std::slice::from_ref(&format),
        };
        for &escape in escapes {
            if let Some(repetition) = infer_list(
                &source,
                &fields,
                &read,
                escape,
                representatives,
                &found,
                calls,
                &response,
                &fragments,
                &in_headers,
                &repeated,
            ) {
                return Some(repetition);
            }
        }
    }
    let starts = vec![0; streams.len()];
    if let Some((pieces, _)) = align(&streams, &starts, false, None)
        && pieces.iter().any(|p| matches!(p, Piece::Value(_)))
        && !in_headers(&pieces)
    {
        let body = fragments(&pieces)?;
        let response = response(body);
        if renders_back(&response, None, calls) {
            return Some(Repetition {
                body: response.body,
                fields: repeated(&pieces),
                list: None,
                script: None,
            });
        }
    }
    None
}

/// `infer` for one list and one escape: the responses of the calls with one item at least read side by side, the list
/// taken where they differ; the text that the list holds before its first block and after its last moved into the
/// script, so that a request with no item gets the text around the list alone.
#[allow(clippy::too_many_arguments)]
fn infer_list(
    source: &ListSource,
    fields: &[String],
    read: &[Vec<BTreeMap<String, String>>],
    escape: Escape,
    representatives: &[&Call],
    found: &[(Candidate, String, Vec<String>)],
    calls: &[&Call],
    response: &dyn Fn(Vec<BodyFragment>) -> MockResponse,
    fragments: &dyn Fn(&[Piece]) -> Option<Vec<BodyFragment>>,
    in_headers: &dyn Fn(&[Piece]) -> bool,
    repeated: &dyn Fn(&[Piece]) -> Vec<ConditionSource>,
) -> Option<Repetition> {
    // A field kept by the script is read with `item[name]`: an absent one reads as an empty text, as below.
    let all = ListSearch {
        fields: fields.to_vec(),
        items: read
            .iter()
            .map(|items| {
                items
                    .iter()
                    .map(|item| {
                        fields
                            .iter()
                            .map(|f| {
                                item.get(f)
                                    .map(|v| escape.apply(v).into_bytes())
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect(),
    };
    // First the calls with one item at least, side by side: where the blocks are, and what they are made of.
    let with_items: Vec<usize> = (0..representatives.len())
        .filter(|&i| !read[i].is_empty())
        .collect();
    let search = ListSearch {
        fields: fields.to_vec(),
        items: with_items.iter().map(|&i| all.items[i].clone()).collect(),
    };
    let streams: Vec<Stream> = with_items
        .iter()
        .map(|&i| Stream {
            text: representatives[i].exchange().response_body.as_bytes(),
            values: found.iter().map(|(_, _, v)| v[i].as_bytes()).collect(),
        })
        .collect();
    let starts = vec![0; streams.len()];
    let (pieces, _) = align(&streams, &starts, false, Some(&search))?;
    let at = pieces.iter().position(|p| matches!(p, Piece::List(_)))?;
    let Piece::List(block) = pieces[at].clone() else {
        return None;
    };

    // The gap between two blocks is the text after a block, then the text before the next: the part of it that ends
    // the text before the list starts each block, the part that starts the text after the list ends each block.
    let mut head: &[u8] = &[];
    if at > 0
        && let Piece::Literal(before) = &pieces[at - 1]
    {
        let mut length = common_suffix(before, &block.gap);
        if matches!(escape, Escape::XmlText | Escape::XmlAll) {
            let suffix = &block.gap[block.gap.len() - length..];
            if let Some(tag) = suffix.iter().position(|&b| b == b'<') {
                length -= tag;
            }
        }
        head = &before[before.len() - length..];
    }
    let mut tail: &[u8] = &[];
    if let Some(Piece::Literal(after)) = pieces.get(at + 1) {
        let rest = &block.gap[..block.gap.len() - head.len()];
        let length = after.iter().zip(rest).take_while(|(a, b)| a == b).count();
        tail = &after[..length];
    }

    // Then every call, the text of its list (nothing for no item) one more value to look for: the calls without an
    // item bring their evidence on the rest of the response too.
    let lists: Vec<Vec<u8>> = (0..representatives.len())
        .map(|i| {
            if read[i].is_empty() {
                return Vec::new();
            }
            let mut text = head.to_vec();
            text.extend(all.core(&block, i));
            text.extend_from_slice(tail);
            text
        })
        .collect();
    let streams: Vec<Stream> = representatives
        .iter()
        .enumerate()
        .map(|(i, call)| Stream {
            text: call.exchange().response_body.as_bytes(),
            values: found
                .iter()
                .map(|(_, _, v)| v[i].as_bytes())
                .chain([lists[i].as_slice()])
                .collect(),
        })
        .collect();
    let starts = vec![0; streams.len()];
    let (pieces, _) = align(&streams, &starts, false, None)?;
    if in_headers(&pieces) || !pieces.contains(&Piece::Value(found.len())) {
        return None;
    }

    let used: Vec<String> = block
        .inner
        .iter()
        .filter_map(|p| match p {
            Piece::Value(f) => Some(fields[*f].clone()),
            _ => None,
        })
        .fold(Vec::new(), |mut out, f| {
            if !out.contains(&f) {
                out.push(f);
            }
            out
        });
    let script = list_script(
        source,
        fields,
        &block,
        &text(head.to_vec())?,
        &text(tail.to_vec())?,
        escape,
    );
    let response = response(fragments(&pieces)?);
    if !renders_back(&response, Some(&script), calls) {
        return None;
    }
    Some(Repetition {
        body: response.body,
        fields: repeated(&pieces),
        list: Some(RepeatedList {
            items: match source {
                ListSource::Xml(path) => ConditionSource::XPath(path.clone()),
                ListSource::Json(pointer) => ConditionSource::JsonPointer(pointer.clone()),
            },
            fields: used,
        }),
        script: Some(script),
    })
}

#[cfg(test)]
mod tests;
