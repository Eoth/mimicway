//! Responses that repeat what their own request sent: one rule that reads each value from the request, where one rule
//! per value seen would answer a request never seen with the values of another.
//!
//! Two shapes, found on the calls of one route or operation whose responses differ:
//! - values: each response is the same text around values of its own request (path, query, header, JSON or XML field
//!   of the body). The response becomes that text, in `Literal` fragments, and one template expression per value
//!   (`{{xpath.Envelope/Body/GetStatus/ref | xml_text}}`), written here from a field name checked first, never from
//!   the traffic. Its pipe escapes the value for where the hole stands in the document (`Context`: the text of an XML
//!   element, an attribute, a CDATA section, a JSON string), never for the values observed alone, so that any value to
//!   come keeps the document well formed; a hole where no escaping can promise it (out of any JSON string, inside a tag)
//!   is not generalized.
//! - a list: the request holds a repeated element (`<slot>` items, a JSON array of objects) and the response a block
//!   per item, each block the same text around values of the item of the same rank. Templates cannot loop: a Rhai
//!   script is written that reads the items (`parse_xml_items`, `parse_json`) and builds the blocks, each value escaped
//!   for where it stands (out of a JSON string, written as the JSON value it is, `to_json`), and the response reads its
//!   result (`{{script.items}}`). Every text observed that
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
//! suggests what it suggested before. The reading is bounded: values of `MAX_VALUE` bytes at most (items included),
//! `MAX_REPEATS` values, a few lists, starts and gaps tried, a gap between blocks of `MAX_GAP` bytes at most. Reading
//! side by side then costs the length of the responses, times their number, times the candidates, times `MAX_VALUE`
//! (the offsets a value may start at); the checks cost one rendering per call and per shape tried, within
//! `MAX_RENDERS` renderings for all the suggestions of a service.
use super::{Call, Candidate, Renders};
use crate::engine::escape::{Escaping, is_json_number};
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
    /// The most items the script writes within the limits of a script (`script_budget.rs`), measured on requests
    /// built like the observed ones, their values as long as the longest seen: past it, the script stops and the
    /// response holds the text around the list without its blocks.
    pub max_items: usize,
}

/// The response of a rule that repeats its request, checked against every call it comes from.
pub(super) struct Repetition {
    pub body: Vec<BodyFragment>,
    /// The request fields the response repeats, in the order of their first appearance.
    pub fields: Vec<ConditionSource>,
    pub list: Option<RepeatedList>,
    pub script: Option<String>,
    /// The texts observed that the script writes as they are.
    pub script_texts: Vec<String>,
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

/// The format of the responses: what a hole of the template is escaped for depends on it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Format {
    Xml,
    Json,
    /// Neither: a value is written as it is.
    Text,
}

/// The format of the responses of `calls`, by their content type, then by reading them: the format declared when its
/// reader reads them all whole, else the one whose reader does, failing that text (a body declared JSON that no JSON
/// reader accepts is no JSON document to keep well formed).
fn format_of(calls: &[&Call]) -> Option<Format> {
    let declared = calls[0]
        .exchange()
        .response_content_type
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let all = |reads: fn(&str) -> bool| calls.iter().all(|c| reads(&c.exchange().response_body));
    if declared.contains("json") && all(reads_json) {
        return Some(Format::Json);
    }
    if declared.contains("xml") && all(reads_xml) {
        return Some(Format::Xml);
    }
    Some(if all(reads_json) {
        Format::Json
    } else if all(reads_xml) {
        Format::Xml
    } else {
        Format::Text
    })
}

fn reads_json(body: &str) -> bool {
    serde_json::from_str::<serde::de::IgnoredAny>(body).is_ok()
}

/// Whether quick-xml reads `body` whole as one element, its attributes and references included.
fn reads_xml(body: &str) -> bool {
    use quick_xml::events::Event;
    let mut reader = quick_xml::reader::Reader::from_str(body);
    let (mut depth, mut roots) = (0usize, 0usize);
    loop {
        match reader.read_event() {
            Ok(event @ (Event::Start(_) | Event::Empty(_))) => {
                let (Event::Start(e) | Event::Empty(e)) = &event else {
                    return false;
                };
                let attributes_read = e.attributes().all(|a| {
                    a.is_ok_and(|a| {
                        a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .is_ok()
                    })
                });
                if !attributes_read {
                    return false;
                }
                roots += usize::from(depth == 0);
                depth += usize::from(matches!(event, Event::Start(_)));
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(event @ (Event::Text(_) | Event::GeneralRef(_) | Event::CData(_))) => {
                if crate::engine::MatchEngine::xml_text(&event).is_none() {
                    return false;
                }
            }
            Ok(Event::Eof) => return depth == 0 && roots == 1,
            Err(_) => return false,
            Ok(_) => {}
        }
    }
}

/// How request values are looked for in the responses, read side by side: as the format writes them in one of its
/// contexts. The template or the script then escapes each value for the context its hole is found in (`Context`),
/// whatever the encoding that found it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Encoding {
    XmlText,
    XmlAttr,
    JsonString,
    /// As it is: an XML value in a CDATA section, or any value of a text response.
    Raw,
}

impl Encoding {
    fn apply(self, value: &str) -> String {
        match self {
            Encoding::XmlText => Escaping::XmlText.apply(value),
            Encoding::XmlAttr => Escaping::XmlAttr.apply(value),
            Encoding::JsonString => Escaping::JsonString.apply(value),
            Encoding::Raw => value.to_string(),
        }
    }

    fn of(format: Format) -> &'static [Encoding] {
        match format {
            Format::Xml => &[Encoding::XmlText, Encoding::XmlAttr, Encoding::Raw],
            Format::Json => &[Encoding::JsonString],
            Format::Text => &[Encoding::Raw],
        }
    }
}

/// Where a hole of the template stands in the document, which says how a value is escaped there.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Context {
    /// The text of an XML element.
    XmlText,
    /// The value of an XML attribute.
    XmlAttr,
    /// The inside of a CDATA section.
    Cdata,
    /// The inside of a JSON string.
    JsonString,
    /// A place of a JSON document out of any string: only a JSON value fits, which a script writes (`to_json`), and a
    /// template (`json_value`) where every value observed there was a JSON number.
    JsonBare,
    /// A text response.
    Text,
}

impl Context {
    /// The pipes that escape a value read by a template here; `None` where no pipe keeps the document well formed
    /// whatever the values observed (`Attempt::fragments` writes `json_value` out of any JSON string, given numbers).
    fn pipe(self) -> Option<&'static str> {
        match self {
            Context::XmlText => Some(" | xml_text"),
            Context::XmlAttr => Some(" | xml_attr"),
            Context::Cdata => Some(" | cdata"),
            Context::JsonString => Some(" | json_string"),
            Context::JsonBare => None,
            Context::Text => Some(""),
        }
    }

    /// A field of an item written here by the script, as a Rhai expression.
    fn script(self, field: &str) -> String {
        let read = format!("item[{}]", rhai_string(field));
        match self {
            Context::XmlText => format!("xml_text({read})"),
            Context::XmlAttr => format!("xml_attr({read})"),
            Context::Cdata => format!("cdata({read})"),
            Context::JsonString => format!("json_string({read})"),
            Context::JsonBare => format!("to_json({read})"),
            Context::Text => format!("(\"\" + {read})"),
        }
    }
}

/// The context of each hole of `skeleton`, a document whose holes are `\0` bytes (no document of these formats holds
/// one), in order; `None` when a hole stands where no escaping keeps the document well formed: inside a tag, a
/// comment, a processing instruction or a declaration, right after a backslash in a JSON string, in text right before
/// `>` (a value ending in `]]` would write `]]>`), in a CDATA section next to `]`, `>` or another hole (a value would
/// end the section early).
fn contexts(format: Format, skeleton: &[u8]) -> Option<Vec<Context>> {
    match format {
        Format::Text => Some(vec![
            Context::Text;
            skeleton.iter().filter(|&&b| b == 0).count()
        ]),
        Format::Json => json_contexts(skeleton),
        Format::Xml => xml_contexts(skeleton),
    }
}

fn json_contexts(skeleton: &[u8]) -> Option<Vec<Context>> {
    let (mut in_string, mut escaped) = (false, false);
    let mut found = Vec::new();
    for &b in skeleton {
        match (b, in_string, escaped) {
            (0, true, true) => return None,
            (0, true, false) => found.push(Context::JsonString),
            (0, false, _) => found.push(Context::JsonBare),
            (_, true, true) => escaped = false,
            (b'\\', true, false) => escaped = true,
            (b'"', true, false) => in_string = false,
            (b'"', false, _) => in_string = true,
            _ => {}
        }
    }
    Some(found)
}

fn xml_contexts(skeleton: &[u8]) -> Option<Vec<Context>> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        Text,
        Tag,
        Attr(u8),
        Cdata,
        /// A comment, a processing instruction or a declaration, until its end marker.
        Other(&'static [u8]),
    }
    let mut state = State::Text;
    let mut found = Vec::new();
    let mut i = 0;
    while i < skeleton.len() {
        let rest = &skeleton[i..];
        if rest[0] == 0 {
            let (before, after) = (&skeleton[..i], &skeleton[i + 1..]);
            found.push(match state {
                State::Text if !after.starts_with(b">") => Context::XmlText,
                State::Attr(_) => Context::XmlAttr,
                State::Cdata
                    if !before.ends_with(b"]")
                        && !before.ends_with(b"\0")
                        && !after.starts_with(b">")
                        && !after.starts_with(b"]>")
                        && !after.starts_with(b"\0") =>
                {
                    Context::Cdata
                }
                _ => return None,
            });
            i += 1;
            continue;
        }
        let (next, step) = match state {
            State::Text if rest.starts_with(b"<![CDATA[") => (State::Cdata, 9),
            State::Text if rest.starts_with(b"<!--") => (State::Other(b"-->"), 4),
            State::Text if rest.starts_with(b"<?") => (State::Other(b"?>"), 2),
            State::Text if rest.starts_with(b"<!") => (State::Other(b">"), 2),
            State::Text if rest[0] == b'<' => (State::Tag, 1),
            State::Tag if rest[0] == b'"' || rest[0] == b'\'' => (State::Attr(rest[0]), 1),
            State::Tag if rest[0] == b'>' => (State::Text, 1),
            State::Attr(quote) if rest[0] == quote => (State::Tag, 1),
            State::Cdata if rest.starts_with(b"]]>") => (State::Text, 3),
            State::Other(end) if rest.starts_with(end) => (State::Text, end.len()),
            _ => (state, 1),
        };
        state = next;
        i += step;
    }
    Some(found)
}

/// Where the items of a list are in a request body.
#[derive(Clone, Debug, PartialEq)]
enum ListSource {
    /// The element repeated at this path, read by `parse_xml_items`.
    Xml(String),
    /// The array at this JSON pointer, read by `parse_json`.
    Json(String),
}

/// The items of `source` in `body`, each a map of its scalar fields to their text (a JSON number or boolean as JSON
/// writes it); `None` when the body does not hold them as the script would read them (an item that is no object, a
/// limit of the script reached, a value longer than `MAX_VALUE`). A field that is no scalar is left out: it is never
/// repeated as a text.
fn read_items(source: &ListSource, body: &str) -> Option<Vec<BTreeMap<String, String>>> {
    let items = read_all_items(source, body)?;
    // Values bounded as every value followed (`MAX_VALUE`): reading the blocks side by side costs their lengths.
    items
        .iter()
        .flat_map(|item| item.values())
        .all(|v| v.len() <= MAX_VALUE)
        .then_some(items)
}

fn read_all_items(source: &ListSource, body: &str) -> Option<Vec<BTreeMap<String, String>>> {
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
                                serde_json::Value::String(s) => s,
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
/// gives back the call's observed status and body byte for byte; `false` when `renders` has not one rendering per call
/// left.
pub(super) fn renders_back(
    response: &MockResponse,
    script: Option<&str>,
    calls: &[&Call],
    renders: &Renders,
) -> bool {
    renders.take(calls.len())
        && calls.iter().all(|call| {
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
                && TemplateRenderer::render_body(&response.body, &[], &ctx)
                    == exchange.response_body
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
/// last, nothing at all for no item; each value of an item escaped for the context of its hole, `contexts` in the
/// order of the values of `block.inner`.
fn list_script(
    source: &ListSource,
    fields: &[String],
    block: &Block,
    contexts: &[Context],
    head: &str,
    tail: &str,
) -> String {
    let mut script = String::from(
        "// One block per item of the request's list, written from the observed calls.\n",
    );
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
    let mut holes = contexts.iter();
    for piece in &block.inner {
        match piece {
            Piece::Literal(text) => {
                let _ = write!(line, " + {}", rhai_string(&String::from_utf8_lossy(text)));
            }
            Piece::Value(f) => {
                let context = holes.next().copied().unwrap_or(Context::Text);
                let _ = write!(line, " + {}", context.script(&fields[*f]));
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
/// Request values are looked for as the format of the responses writes them in each of its contexts (`Encoding`); each
/// hole found is then escaped for its own context (`Context`), so that the rule stays right for any value to come.
pub(super) fn infer(
    representatives: &[&Call],
    calls: &[&Call],
    headers: &[crate::models::HeaderEntry],
    renders: &Renders,
    lists: bool,
) -> Option<Repetition> {
    let status = representatives[0].exchange().response_status;
    if calls.iter().any(|c| c.exchange().response_status != status) {
        return None;
    }
    let format = format_of(representatives)?;
    Encoding::of(format).iter().find_map(|&encoding| {
        infer_with(
            representatives,
            calls,
            headers,
            format,
            encoding,
            renders,
            lists,
        )
    })
}

/// What the attempts of one format and one encoding share.
struct Attempt<'a, 'c> {
    representatives: &'a [&'a Call<'c>],
    calls: &'a [&'a Call<'c>],
    headers: &'a [crate::models::HeaderEntry],
    format: Format,
    encoding: Encoding,
    renders: &'a Renders,
    /// The candidates, their template expression without pipe, and their value in each representative, encoded.
    found: Vec<(Candidate<'a>, String, Vec<String>)>,
}

impl Attempt<'_, '_> {
    fn status(&self) -> u16 {
        self.representatives[0].exchange().response_status
    }

    fn response(&self, body: Vec<BodyFragment>) -> MockResponse {
        MockResponse {
            status: self.status(),
            headers: self.headers.to_vec(),
            body,
            chaos: None,
            events: None,
        }
    }

    fn candidate(&self, piece: &Piece) -> Option<&Candidate<'_>> {
        match piece {
            Piece::Value(c) => self.found.get(*c).map(|(candidate, _, _)| candidate),
            _ => None,
        }
    }

    fn repeated(&self, pieces: &[Piece]) -> Vec<ConditionSource> {
        let mut fields: Vec<ConditionSource> = Vec::new();
        for candidate in pieces.iter().filter_map(|p| self.candidate(p)) {
            let source = candidate.source();
            if !fields.contains(&source) {
                fields.push(source);
            }
        }
        fields
    }

    /// Whether a value the pieces repeat shows in a header: headers are copied as they are, never generalized.
    fn in_headers(&self, pieces: &[Piece]) -> bool {
        pieces
            .iter()
            .filter_map(|p| self.candidate(p))
            .any(|repeated| {
                self.calls.iter().any(|call| {
                    call.fields().any(|(candidate, value)| {
                        candidate == *repeated
                            && !value.is_empty()
                            && super::suggested_headers(call.exchange())
                                .iter()
                                .chain(self.headers)
                                .any(|h| h.value.contains(value))
                    })
                })
            })
    }

    /// The body of the rule: literal pieces as they are, each value read by its expression and escaped for the context
    /// of its hole (`contexts`, in the order of the holes), the list (any other hole) read from the script.
    fn fragments(&self, pieces: &[Piece], contexts: &[Context]) -> Option<Vec<BodyFragment>> {
        let mut holes = contexts.iter();
        pieces
            .iter()
            .map(|piece| {
                if let Piece::Literal(bytes) = piece {
                    return Some(BodyFragment::Literal {
                        value: text(bytes.clone())?,
                    });
                }
                let context = *holes.next()?;
                Some(BodyFragment::Template {
                    template: match piece {
                        Piece::Value(c) if *c < self.found.len() => {
                            let (_, expression, values) = &self.found[*c];
                            // Out of any JSON string, a value written as a JSON value: a number as it is, any
                            // other text as a string. Only where every value observed was a number, which it gives
                            // back as it was.
                            let numbers = values.iter().all(|v| is_json_number(v));
                            let pipe = match context {
                                Context::JsonBare if numbers => " | json_value",
                                _ => context.pipe()?,
                            };
                            format!("{}{pipe}}}}}", expression.trim_end_matches('}'))
                        }
                        _ => {
                            let fits = matches!(
                                context,
                                Context::XmlText | Context::JsonBare | Context::Text
                            );
                            fits.then(|| format!("{{{{script.{LIST_FIELD}}}}}"))?
                        }
                    },
                })
            })
            .collect()
    }
}

/// The skeleton of `pieces`: their literal text, `\0` for each hole; `None` when a literal holds a `\0` itself.
fn skeleton(pieces: &[Piece]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Literal(bytes) if bytes.contains(&0) => return None,
            Piece::Literal(bytes) => out.extend_from_slice(bytes),
            _ => out.push(0),
        }
    }
    Some(out)
}

/// `infer` with the request values looked for as `encoding` writes them. The lists are tried first: when every call
/// holds as many items, the values alone would read the blocks as fields of the items at fixed ranks (`/slots/2/type`),
/// which answers no request with another number of items.
fn infer_with(
    representatives: &[&Call],
    calls: &[&Call],
    headers: &[crate::models::HeaderEntry],
    format: Format,
    encoding: Encoding,
    renders: &Renders,
    lists: bool,
) -> Option<Repetition> {
    let attempt = Attempt {
        representatives,
        calls,
        headers,
        format,
        encoding,
        renders,
        found: candidates(representatives)
            .into_iter()
            .map(|(candidate, expression, raw)| {
                let encoded = raw.iter().map(|v| encoding.apply(v)).collect();
                (candidate, expression, encoded)
            })
            .collect(),
    };
    for source in list_sources(representatives).into_iter().filter(|_| lists) {
        if let Some(repetition) = try_list(&attempt, &source) {
            return Some(repetition);
        }
    }
    let streams: Vec<Stream> = representatives
        .iter()
        .enumerate()
        .map(|(i, call)| Stream {
            text: call.exchange().response_body.as_bytes(),
            values: attempt
                .found
                .iter()
                .map(|(_, _, v)| v[i].as_bytes())
                .collect(),
        })
        .collect();
    let starts = vec![0; streams.len()];
    let (pieces, _) = align(&streams, &starts, false, None)?;
    if !pieces.iter().any(|p| matches!(p, Piece::Value(_))) || attempt.in_headers(&pieces) {
        return None;
    }
    let contexts = match format {
        // Nothing to keep well formed: a text response may hold any byte, `\0` included.
        Format::Text => vec![Context::Text; pieces.len()],
        _ => contexts(format, &skeleton(&pieces)?)?,
    };
    let response = attempt.response(attempt.fragments(&pieces, &contexts)?);
    renders_back(&response, None, calls, renders).then(|| Repetition {
        body: response.body,
        fields: attempt.repeated(&pieces),
        list: None,
        script: None,
        script_texts: Vec::new(),
    })
}

/// The list at `source`, if the responses hold a block per item of it.
fn try_list(attempt: &Attempt, source: &ListSource) -> Option<Repetition> {
    let representatives = attempt.representatives;
    let encoding = attempt.encoding;
    // The fields of the first item are body fields already read: a response holding a block per item holds one of
    // them. Checked before the items are read, which costs a reading of the body.
    let first_shown = representatives.iter().all(|call| {
        let mut fields = call
            .body
            .fields
            .iter()
            .filter(|(field, _)| list_of(field).is_some_and(|(list, _)| list == *source))
            .peekable();
        let response = &call.exchange().response_body;
        fields.peek().is_none()
            || fields.any(|(_, v)| !v.is_empty() && response.contains(&encoding.apply(v)))
    });
    if !first_shown {
        return None;
    }
    let read: Vec<Vec<BTreeMap<String, String>>> = representatives
        .iter()
        .map(|call| read_items(source, &call.exchange().request_body))
        .collect::<Option<_>>()?;
    if read.iter().map(Vec::len).max().unwrap_or(0) < 2 {
        return None;
    }
    let fields: Vec<String> = read
        .iter()
        .flatten()
        .flat_map(|item| item.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // Items of one kind have a few fields; and a response holding a block per item holds the values of its first and
    // last items. Both checked before any reading side by side, which costs the items times their fields.
    let shown = |i: usize| {
        let (Some(first), Some(last)) = (read[i].first(), read[i].last()) else {
            return true;
        };
        let response = &representatives[i].exchange().response_body;
        fields.iter().any(|f| {
            [first, last].iter().all(|item| {
                item.get(f)
                    .is_some_and(|v| !v.is_empty() && response.contains(&encoding.apply(v)))
            })
        })
    };
    if fields.len() > MAX_ITEM_FIELDS || !(0..read.len()).all(shown) {
        return None;
    }
    infer_list(attempt, source, &fields, &read)
}

/// `infer` for one list: the responses of the calls with one item at least read side by side, the list taken where
/// they differ; the text that the list holds before its first block and after its last moved into the script, so that
/// a request with no item gets the text around the list alone.
fn infer_list(
    attempt: &Attempt,
    source: &ListSource,
    fields: &[String],
    read: &[Vec<BTreeMap<String, String>>],
) -> Option<Repetition> {
    let representatives = attempt.representatives;
    let found = &attempt.found;
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
                                    .map(|v| attempt.encoding.apply(v).into_bytes())
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
        if attempt.format == Format::Xml {
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
    let list = Piece::Value(found.len());
    if attempt.in_headers(&pieces) || !pieces.contains(&list) {
        return None;
    }

    let (outer, inner) = list_contexts(attempt.format, &pieces, &list, head, &block, tail)?;
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
        &inner,
        &text(head.to_vec())?,
        &text(tail.to_vec())?,
    );
    let response = attempt.response(attempt.fragments(&pieces, &outer)?);
    if !renders_back(&response, Some(&script), attempt.calls, attempt.renders) {
        return None;
    }
    let max_items = most_items(&script, source, fields, read, attempt.renders)?;
    Some(Repetition {
        body: response.body,
        fields: attempt.repeated(&pieces),
        list: Some(RepeatedList {
            items: match source {
                ListSource::Xml(path) => ConditionSource::XPath(path.clone()),
                ListSource::Json(pointer) => ConditionSource::JsonPointer(pointer.clone()),
            },
            fields: used,
            max_items,
        }),
        script: Some(script),
        script_texts: [head, tail, &block.gap]
            .into_iter()
            .chain(block.inner.iter().filter_map(|p| match p {
                Piece::Literal(text) => Some(text.as_slice()),
                _ => None,
            }))
            .map(|t| String::from_utf8_lossy(t).into_owned())
            .collect(),
    })
}

/// A request body holding `n` items at `source`, each field with a value of the length of `lengths`.
fn request_with(source: &ListSource, fields: &[(String, usize)], n: usize) -> String {
    match source {
        ListSource::Xml(path) => {
            let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            let Some((item, parents)) = segments.split_last() else {
                return String::new();
            };
            let fields: String = fields
                .iter()
                .map(|(name, length)| format!("<{name}>{}</{name}>", "x".repeat(*length)))
                .collect();
            let mut body: String = parents.iter().map(|p| format!("<{p}>")).collect();
            for _ in 0..n {
                body.push_str(&format!("<{item}>{fields}</{item}>"));
            }
            body.extend(parents.iter().rev().map(|p| format!("</{p}>")));
            body
        }
        ListSource::Json(pointer) => {
            let item: serde_json::Map<String, serde_json::Value> = fields
                .iter()
                .map(|(name, length)| (name.clone(), "x".repeat(*length).into()))
                .collect();
            let mut value = serde_json::Value::Array(vec![item.into(); n]);
            for segment in pointer
                .split('/')
                .skip(1)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                let key = segment.replace("~1", "/").replace("~0", "~");
                value = match key.parse::<usize>() {
                    Ok(index) if index < 16 => {
                        let mut array = vec![serde_json::Value::Null; index + 1];
                        array[index] = value;
                        serde_json::Value::Array(array)
                    }
                    _ => serde_json::json!({ key: value }),
                };
            }
            value.to_string()
        }
    }
}

/// The most items `script` writes for a request like the observed ones (`RepeatedList::max_items`): the largest `n`
/// whose request the engine runs it on without reaching a limit. The fields a script may read bound it from above;
/// one run there tells whether nothing else does (the usual case), else halving below it, in about ten runs.
fn most_items(
    script: &str,
    source: &ListSource,
    fields: &[String],
    read: &[Vec<BTreeMap<String, String>>],
    renders: &Renders,
) -> Option<usize> {
    let lengths: Vec<(String, usize)> = fields
        .iter()
        .map(|f| {
            let longest = read
                .iter()
                .flatten()
                .filter_map(|item| item.get(f).map(String::len))
                .max()
                .unwrap_or(1);
            (f.clone(), longest.max(1))
        })
        .collect();
    let runs = |n: usize| -> Option<bool> {
        if !renders.take(1) {
            return None;
        }
        let ctx = ScriptContext {
            body: request_with(source, &lengths, n),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            path_params: HashMap::new(),
        };
        Some(engine().execute(script, &ctx).is_ok())
    };
    let bound = crate::engine::script::MAX_LIST_ITEMS
        .min(crate::engine::script::MAX_READ_FIELDS / fields.len().max(1));
    if runs(bound)? {
        return Some(bound);
    }
    let (mut low, mut high) = (0, bound);
    // `runs(low)` holds and `runs(high)` does not.
    while high - low > 1 {
        let middle = low + (high - low) / 2;
        if runs(middle)? {
            low = middle;
        } else {
            high = middle;
        }
    }
    Some(low)
}

/// The contexts of the holes of a response with a list: those of `pieces` (the list among them, at `list`), and those
/// of the values of a block. The document is read with no block and with two: the holes outside the list must stand
/// where they stood without it, the values of both blocks in the same contexts, and the list where blocks of markup
/// may go (the text of an XML element, a JSON document out of strings); `None` otherwise.
fn list_contexts(
    format: Format,
    pieces: &[Piece],
    list: &Piece,
    head: &[u8],
    block: &Block,
    tail: &[u8],
) -> Option<(Vec<Context>, Vec<Context>)> {
    if format == Format::Text {
        return Some((
            vec![Context::Text; pieces.len()],
            vec![Context::Text; block.inner.len()],
        ));
    }
    let inner = skeleton(&block.inner)?;
    let values = inner.iter().filter(|&&b| b == 0).count();
    if [head, tail, &block.gap].iter().any(|t| t.contains(&0)) {
        return None;
    }
    let mut none = Vec::new();
    let mut two = Vec::new();
    let mut before = 0;
    for piece in pieces {
        match piece {
            Piece::Literal(bytes) if bytes.contains(&0) => return None,
            Piece::Literal(bytes) => {
                none.extend_from_slice(bytes);
                two.extend_from_slice(bytes);
            }
            p if p == list => {
                none.push(0);
                before = two.iter().filter(|&&b| b == 0).count();
                two.extend_from_slice(head);
                two.extend_from_slice(&inner);
                two.extend_from_slice(&block.gap);
                two.extend_from_slice(&inner);
                two.extend_from_slice(tail);
            }
            _ => {
                none.push(0);
                two.push(0);
            }
        }
    }
    let outer = contexts(format, &none)?;
    let all = contexts(format, &two)?;
    let first = all[before..before + values].to_vec();
    let second = &all[before + values..before + 2 * values];
    let around: Vec<Context> = all[..before]
        .iter()
        .chain(&all[before + 2 * values..])
        .copied()
        .collect();
    let without_list: Vec<Context> = outer
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != before)
        .map(|(_, c)| *c)
        .collect();
    (first == second && around == without_list).then_some((outer, first))
}

#[cfg(test)]
mod tests;
