//! The fields of an observed request body that a suggestion may put in a condition, and the SOAP operation the body
//! names. Each field is read the way the condition written from it reads the request: an XML field is a path of local
//! names (`Envelope/Body/getStatus/ref`, namespace prefixes dropped by `MatchEngine::local_name_ref`) and holds what
//! `MatchEngine::extract_xpath` finds there; a JSON field is a JSON pointer (`/order/lines/0/sku`) and holds what
//! `json_pointer::value_at` finds there. A field whose value the condition would not find is never offered: a suggested
//! rule must match the calls it comes from (`replay`, and the property tests of this module).
//!
//! The body comes from a client or a backend that may be hostile, at most `observation::max_body_size()` bytes (10 MiB
//! when raised to the limit of a buffered body). Reading it costs its length, whatever its shape: one pass of the
//! reader, one lookup of a bounded name per element or key, and nothing copied before its length is checked against the
//! bounds below. A body cut by that size limit gives no field and no operation: its end, where a value or a second
//! operation may be, was never seen. Nor does a body that is not UTF-8 text: the conditions read its bytes, not the text
//! kept of it.
use crate::engine::MatchEngine;
use crate::engine::matcher::XML_DEPTH_LIMIT;
use crate::models::ConditionSource;
use serde::de::{
    DeserializeSeed, Deserializer, Error as _, IgnoredAny, MapAccess, SeqAccess, Visitor,
};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// Fields kept from one body, the first ones in the order of the document: enough for the parameters of an operation or
/// a JSON command, and a bound on what the choice of a field (`find_discriminator`) compares per call.
pub(super) const MAX_BODY_FIELDS: usize = 64;
/// Deepest field, counted in elements from the root of an XML body (`Envelope/Body/op/field` is 4) or in keys and
/// indexes from the top of a JSON body. SOAP puts the parameters of an operation at depth 4 and their parts a few
/// levels below; deeper values are rarely what decides an answer.
pub(super) const MAX_FIELD_DEPTH: usize = 16;
/// Longest path of a field (its XPath, or its JSON pointer once escaped), in bytes. Also the longest operation name.
pub(super) const MAX_FIELD_PATH: usize = 256;
/// Longest value of a field, in bytes: a value that decides an answer is a name, a code or an identifier.
pub(super) const MAX_FIELD_VALUE: usize = 256;
/// Distinct element paths followed in one XML body: each costs one entry and its path (`MAX_FIELD_PATH` at most).
const MAX_XML_PATHS: usize = 4 * MAX_BODY_FIELDS;

/// What a suggestion may use from one request body.
#[derive(Debug, Default, PartialEq)]
pub(super) struct BodyReading {
    /// Each field once, with its value, as a condition source (`JsonPointer` or `XPath`).
    pub fields: Vec<(ConditionSource, String)>,
    /// The SOAP operation: the local name of the one element that `Envelope/Body` holds. `None` when the body is not a
    /// SOAP envelope, or its `Body` holds no element or elements of more than one name: the condition
    /// `Envelope/Body/<operation>` exists then holds for this call exactly when the operation is this one.
    pub operation: Option<String>,
}

/// Reads `body` as JSON when it starts with `{` or `[`, as XML when it starts with `<`, and gives nothing otherwise or
/// when it is `unread` (cut at the size limit, or not UTF-8 text: `suggestion::unread`).
pub(super) fn read(body: &str, unread: bool) -> BodyReading {
    if unread {
        return BodyReading::default();
    }
    match body.trim_start().as_bytes().first() {
        Some(b'{' | b'[') => BodyReading {
            fields: json_fields(body),
            operation: None,
        },
        Some(b'<') => xml_fields(body),
        _ => BodyReading::default(),
    }
}

/// A path segment the XPath reader reads back as it is: it splits a path on `/` and drops empty segments.
fn plain_segment(name: &str) -> bool {
    !name.is_empty() && !name.contains('/')
}

/// The element paths of an XML body, as a tree of the paths seen: entry 0 is above the root, each other entry is one
/// path, reached from its parent's entry by its last local name.
struct Paths {
    path: Vec<String>,
    children: Vec<HashMap<String, usize>>,
}

impl Paths {
    fn new() -> Self {
        Self {
            path: vec![String::new()],
            children: vec![HashMap::new()],
        }
    }

    /// The entry of `name` under `parent`, and whether this is its first element; `None` when the path is not followed
    /// (too deep, too long, a name the XPath reader would split, or too many paths already).
    fn child(&mut self, parent: usize, name: &str, depth: usize) -> Option<(usize, bool)> {
        if depth > MAX_FIELD_DEPTH || name.len() > MAX_FIELD_PATH || !plain_segment(name) {
            return None;
        }
        if let Some(&entry) = self.children[parent].get(name) {
            return Some((entry, false));
        }
        let separator = usize::from(parent != 0);
        if self.path.len() > MAX_XML_PATHS
            || self.path[parent].len() + separator + name.len() > MAX_FIELD_PATH
        {
            return None;
        }
        let mut path = String::with_capacity(self.path[parent].len() + separator + name.len());
        path.push_str(&self.path[parent]);
        if separator == 1 {
            path.push('/');
        }
        path.push_str(name);
        let entry = self.path.len();
        self.path.push(path);
        self.children.push(HashMap::new());
        self.children[parent].insert(name.to_string(), entry);
        Some((entry, true))
    }
}

/// Where an open element stands in a SOAP envelope.
#[derive(Clone, Copy, PartialEq)]
enum Soap {
    Outside,
    /// The root element, `Envelope`.
    Envelope,
    /// `Envelope/Body`: each element it holds names an operation.
    Body,
}

/// The names of the elements `Envelope/Body` holds, as far as they decide an operation.
enum Operations {
    None,
    One(String),
    Several,
}

/// The fields of an XML body: the elements without a child element, each read at the first element of its path in the
/// order of the document, as `extract_xpath` reads a path. The first element of a path that holds another element gives
/// no field: its XPath value is a text cut at the first end tag inside, not a value of its own. The reading stops where
/// `extract_xpath` stops (the first malformed part, the first element deeper than `XML_DEPTH_LIMIT`), keeping what was
/// read before; past `MAX_BODY_FIELDS` fields it goes on to the end for the operation alone.
fn xml_fields(body: &str) -> BodyReading {
    use quick_xml::events::Event;
    let mut reader = quick_xml::reader::Reader::from_str(body);
    let mut paths = Paths::new();
    // Per open element: its entry in `paths` (none when its path is not followed) and its place in an envelope.
    let mut open: Vec<(Option<usize>, Soap)> = Vec::new();
    // The first element of a path, while no child element has shown up in it: its entry, and its text so far.
    let mut leaf: Option<(usize, String)> = None;
    let mut fields = Vec::new();
    let mut operations = Operations::None;

    loop {
        let (element, self_closing) = match reader.read_event() {
            Ok(Event::Start(e)) => (e, false),
            Ok(Event::Empty(e)) => (e, true),
            Ok(event @ (Event::Text(_) | Event::CData(_) | Event::GeneralRef(_))) => {
                if let Some((_, text)) = &mut leaf {
                    if text.len() + MatchEngine::xml_text_len(&event) > MAX_FIELD_VALUE {
                        leaf = None;
                    } else if let Some(piece) = MatchEngine::xml_text(&event) {
                        text.push_str(&piece);
                    }
                }
                continue;
            }
            Ok(Event::End(_)) => {
                if let Some((entry, text)) = leaf.take()
                    && fields.len() < MAX_BODY_FIELDS
                {
                    fields.push((ConditionSource::XPath(paths.path[entry].clone()), text));
                }
                open.pop();
                continue;
            }
            Ok(Event::Eof) | Err(_) => break,
            Ok(_) => continue,
        };
        // Self-closing or not, no element deeper than the limit is read, as in `extract_xpath`.
        if open.len() >= XML_DEPTH_LIMIT {
            break;
        }
        // The element being read as a field holds an element: it is no field.
        leaf = None;
        let name = MatchEngine::local_name_ref(&element);
        let (parent_entry, parent_soap) = open.last().copied().unwrap_or((Some(0), Soap::Outside));
        let soap = match (open.is_empty(), parent_soap, name) {
            (true, _, "Envelope") => Soap::Envelope,
            (false, Soap::Envelope, "Body") => Soap::Body,
            _ => Soap::Outside,
        };
        if parent_soap == Soap::Body {
            operations = match operations {
                _ if !plain_segment(name) || name.len() > MAX_FIELD_PATH => Operations::Several,
                Operations::None => Operations::One(name.to_string()),
                Operations::One(known) if known == name => Operations::One(known),
                _ => Operations::Several,
            };
        }
        let entry = parent_entry.and_then(|parent| paths.child(parent, name, open.len() + 1));
        if self_closing {
            if let Some((entry, true)) = entry
                && fields.len() < MAX_BODY_FIELDS
            {
                fields.push((
                    ConditionSource::XPath(paths.path[entry].clone()),
                    String::new(),
                ));
            }
        } else {
            if let Some((entry, true)) = entry {
                leaf = Some((entry, String::new()));
            }
            open.push((entry.map(|(entry, _)| entry), soap));
        }
    }

    // The operation only when the reader of conditions finds it too: a body malformed or nested too deep past the
    // operation's start tag names an operation this reading saw, and that `Envelope/Body/<operation>` exists never
    // finds (it reads a value up to the first end tag, and stops at the first malformed part).
    let operation = match operations {
        Operations::One(name)
            if MatchEngine::extract_xpath(body.as_bytes(), &format!("Envelope/Body/{name}"))
                .is_some() =>
        {
            Some(name)
        }
        Operations::One(_) | Operations::None | Operations::Several => None,
    };
    BodyReading { fields, operation }
}

/// The scalar fields of a JSON body (strings, numbers, booleans; `null` left out), at any depth up to
/// `MAX_FIELD_DEPTH`, each named by its JSON pointer. Read with the same reader as `json_pointer::value_at`, without
/// building the document: a body that reader refuses (malformed, nested past serde_json's limit, trailing text) gives
/// no field, and so does one with a key written twice in an object, where `value_at` keeps the last one only.
fn json_fields(body: &str) -> Vec<(ConditionSource, String)> {
    let mut fields = Vec::new();
    let mut path = String::new();
    let mut de = serde_json::Deserializer::from_str(body);
    let walked = JsonFields {
        path: &mut path,
        depth: 0,
        out: &mut fields,
    }
    .deserialize(&mut de);
    if walked.is_err() || de.end().is_err() {
        return Vec::new();
    }
    fields
}

struct JsonFields<'a> {
    /// The pointer of the value being read.
    path: &'a mut String,
    depth: usize,
    out: &'a mut Vec<(ConditionSource, String)>,
}

impl JsonFields<'_> {
    fn record(self, value: String) {
        if !self.path.is_empty() && self.out.len() < MAX_BODY_FIELDS {
            self.out
                .push((ConditionSource::JsonPointer(self.path.clone()), value));
        }
    }
}

/// An object key, copied only when it fits in a field path: a longer one comes back as `None`.
struct Key;

impl<'de> DeserializeSeed<'de> for Key {
    type Value = Option<String>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for Key {
    type Value = Option<String>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object key")
    }

    fn visit_str<E>(self, key: &str) -> Result<Self::Value, E> {
        Ok((key.len() <= MAX_FIELD_PATH).then(|| key.to_string()))
    }
}

impl<'de> DeserializeSeed<'de> for JsonFields<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

/// Whether the value at the current path, `extra` bytes longer once its token is appended, may hold a field.
fn worth_reading(path: &str, extra: usize, depth: usize, found: usize) -> bool {
    depth < MAX_FIELD_DEPTH && path.len() + extra <= MAX_FIELD_PATH && found < MAX_BODY_FIELDS
}

impl<'de> Visitor<'de> for JsonFields<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<(), E> {
        self.record(value.to_string());
        Ok(())
    }

    fn visit_i64<E>(self, value: i64) -> Result<(), E> {
        self.record(serde_json::Number::from(value).to_string());
        Ok(())
    }

    fn visit_u64<E>(self, value: u64) -> Result<(), E> {
        self.record(serde_json::Number::from(value).to_string());
        Ok(())
    }

    fn visit_f64<E>(self, value: f64) -> Result<(), E> {
        // A `Value` holds no infinite number: it reads one as `null`, which gives no field.
        if let Some(number) = serde_json::Number::from_f64(value) {
            self.record(number.to_string());
        }
        Ok(())
    }

    fn visit_str<E>(self, value: &str) -> Result<(), E> {
        if value.len() <= MAX_FIELD_VALUE {
            self.record(value.to_string());
        }
        Ok(())
    }

    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let JsonFields { path, depth, out } = self;
        let mut seen: HashSet<String> = HashSet::new();
        while let Some(key) = map.next_key_seed(Key)? {
            let Some(key) = key else {
                map.next_value::<IgnoredAny>()?;
                continue;
            };
            let token = key.replace('~', "~0").replace('/', "~1");
            if !seen.insert(key) {
                return Err(A::Error::custom("a key written twice"));
            }
            if worth_reading(path, 1 + token.len(), depth, out.len()) {
                let start = path.len();
                path.push('/');
                path.push_str(&token);
                map.next_value_seed(JsonFields {
                    path: &mut *path,
                    depth: depth + 1,
                    out: &mut *out,
                })?;
                path.truncate(start);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let JsonFields { path, depth, out } = self;
        let mut index = 0usize;
        loop {
            let token = index.to_string();
            let more = if worth_reading(path, 1 + token.len(), depth, out.len()) {
                let start = path.len();
                path.push('/');
                path.push_str(&token);
                let more = seq
                    .next_element_seed(JsonFields {
                        path: &mut *path,
                        depth: depth + 1,
                        out: &mut *out,
                    })?
                    .is_some();
                path.truncate(start);
                more
            } else {
                seq.next_element::<IgnoredAny>()?.is_some()
            };
            if !more {
                return Ok(());
            }
            index += 1;
        }
    }
}

#[cfg(test)]
mod tests;
