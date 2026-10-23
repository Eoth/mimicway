//! The value at a JSON pointer (RFC 6901) of a request body, read without building the document.
//!
//! `serde_json::from_slice::<Value>` then `Value::pointer` built the whole document for every condition and every
//! `{{body.…}}` variable: about 25 bytes of memory per byte of a document made of small objects, so one 10 MiB body
//! took 250 MiB, the memory the Kubernetes manifests give the whole pod. Here the parts beside the path are only
//! checked and skipped (`IgnoredAny`): memory follows the value found and the depth of the path, not the body. The
//! answer is the one `Value::pointer` gives, the last of duplicate keys included; the tests compare them.
use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

/// The value at `pointer` in the JSON document `body`, or `None` when the body is not JSON or nothing is there.
pub(crate) fn value_at(body: &[u8], pointer: &str) -> Option<Value> {
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return None;
    }
    let tokens: Vec<String> = pointer
        .split('/')
        .skip(1)
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect();
    let mut de = serde_json::Deserializer::from_slice(body);
    let found = PointerSeed { tokens: &tokens }.deserialize(&mut de).ok()?;
    de.end().ok()?;
    found
}

/// An array index as `Value::pointer` reads it: no sign, no leading zero.
fn parse_index(token: &str) -> Option<usize> {
    if token.starts_with('+') || (token.starts_with('0') && token.len() != 1) {
        return None;
    }
    token.parse().ok()
}

struct PointerSeed<'a> {
    tokens: &'a [String],
}

impl<'de> DeserializeSeed<'de> for PointerSeed<'_> {
    type Value = Option<Value>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        if self.tokens.is_empty() {
            return serde::Deserialize::deserialize(deserializer).map(Some);
        }
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for PointerSeed<'_> {
    type Value = Option<Value>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let (head, rest) = (&self.tokens[0], &self.tokens[1..]);
        let mut found = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == *head {
                // A later duplicate replaces the value, as in a parsed `Value::Object`.
                found = map.next_value_seed(PointerSeed { tokens: rest })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(found)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let index = parse_index(&self.tokens[0]);
        let rest = &self.tokens[1..];
        let mut found = None;
        let mut position = 0;
        loop {
            if Some(position) == index {
                match seq.next_element_seed(PointerSeed { tokens: rest })? {
                    Some(value) => found = value,
                    None => break,
                }
            } else if seq.next_element::<IgnoredAny>()?.is_none() {
                break;
            }
            position += 1;
        }
        Ok(found)
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(None)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
