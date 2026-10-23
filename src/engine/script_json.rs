// JSON for rule scripts: `parse_json` turns a text into Rhai values, `to_json` turns values back into text. Both stop
// at the limits of a value (script_budget.rs) instead of building their whole result first. Parsing goes straight
// from the text to Rhai values, without a serde_json tree of the whole document (about 25 bytes of memory per byte of
// JSON), and stops at the first item, entry or byte over a limit: a 10 MiB request body then costs no more memory
// than the value a script may hold. Map keys count with the text, unlike in Rhai's own checks, so that no part of a
// parsed document escapes the limits.
use super::script_budget::{
    ARRAY_TOO_LARGE, MAP_TOO_LARGE, MAX_ARRAY_SIZE, MAX_MAP_SIZE, MAX_STRING_SIZE, TEXT_TOO_LARGE,
    too_large,
};
use rhai::{Array, Dynamic, EvalAltResult, ImmutableString, Map};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};

/// The Rhai value of a JSON text: `()` when the text is not JSON, an error naming the limit a value would break.
pub(super) fn parse(text: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    let mut sizes = Sizes::default();
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let parsed = Value(&mut sizes)
        .deserialize(&mut deserializer)
        .and_then(|value| deserializer.end().map(|()| value));
    match (parsed, sizes.exceeded) {
        (_, Some(kind)) => Err(too_large(kind)),
        (Ok(value), None) => Ok(value),
        (Err(_), None) => Ok(Dynamic::UNIT),
    }
}

/// JSON text of a Rhai value: unit as null, scalars as themselves, a string or any type without a JSON counterpart as
/// text. An error names the text limit when the result would be longer.
pub(super) fn to_text(value: &Dynamic) -> Result<String, Box<EvalAltResult>> {
    let mut out = BoundedText::default();
    match serde_json::to_writer(&mut out, &Json(value)) {
        Ok(()) => Ok(String::from_utf8(out.bytes).unwrap_or_default()),
        Err(_) if out.over => Err(too_large(TEXT_TOO_LARGE)),
        Err(_) => Ok(String::new()),
    }
}

// Totals over the whole document, as Rhai counts a value: items of every array, entries of every map, bytes of text.
#[derive(Default)]
struct Sizes {
    items: usize,
    entries: usize,
    bytes: usize,
    exceeded: Option<&'static str>,
}

impl Sizes {
    fn add<E: de::Error>(&mut self, kind: &'static str) -> Result<(), E> {
        let over = match kind {
            ARRAY_TOO_LARGE => {
                self.items += 1;
                self.items > MAX_ARRAY_SIZE
            }
            _ => {
                self.entries += 1;
                self.entries > MAX_MAP_SIZE
            }
        };
        self.stop_if(over, kind)
    }

    fn add_text<E: de::Error>(&mut self, length: usize) -> Result<(), E> {
        self.bytes = self.bytes.saturating_add(length);
        self.stop_if(self.bytes > MAX_STRING_SIZE, TEXT_TOO_LARGE)
    }

    fn stop_if<E: de::Error>(&mut self, over: bool, kind: &'static str) -> Result<(), E> {
        if over {
            self.exceeded = Some(kind);
            return Err(E::custom(kind));
        }
        Ok(())
    }
}

struct Value<'a>(&'a mut Sizes);

impl<'de> DeserializeSeed<'de> for Value<'_> {
    type Value = Dynamic;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Dynamic, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Value<'_> {
    type Value = Dynamic;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E>(self) -> Result<Dynamic, E> {
        Ok(Dynamic::UNIT)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Dynamic, E> {
        Ok(value.into())
    }

    fn visit_i64<E>(self, value: i64) -> Result<Dynamic, E> {
        Ok(value.into())
    }

    // Integers past i64 become floats, as serde_json's own `as_i64` then `as_f64` would read them.
    fn visit_u64<E>(self, value: u64) -> Result<Dynamic, E> {
        Ok(i64::try_from(value).map_or_else(|_| Dynamic::from(value as f64), Dynamic::from))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Dynamic, E> {
        Ok(value.into())
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Dynamic, E> {
        self.0.add_text(value.len())?;
        Ok(value.into())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Dynamic, A::Error> {
        let sizes = self.0;
        let mut array = Array::new();
        while let Some(item) = seq.next_element_seed(Value(&mut *sizes))? {
            sizes.add(ARRAY_TOO_LARGE)?;
            array.push(item);
        }
        Ok(Dynamic::from_array(array))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Dynamic, A::Error> {
        let sizes = self.0;
        let mut map = Map::new();
        while let Some(key) = access.next_key_seed(Key(&mut *sizes))? {
            let value = access.next_value_seed(Value(&mut *sizes))?;
            sizes.add(MAP_TOO_LARGE)?;
            map.insert(key.into(), value);
        }
        Ok(Dynamic::from_map(map))
    }
}

struct Key<'a>(&'a mut Sizes);

impl<'de> DeserializeSeed<'de> for Key<'_> {
    type Value = String;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for Key<'_> {
    type Value = String;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a JSON object key")
    }

    fn visit_str<E: de::Error>(self, key: &str) -> Result<String, E> {
        self.0.add_text(key.len())?;
        Ok(key.to_string())
    }
}

// Serializes a Rhai value in place: arrays and maps are read, never copied.
struct Json<'a>(&'a Dynamic);

impl Serialize for Json<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let value = self.0;
        if value.is_unit() {
            serializer.serialize_unit()
        } else if let Ok(flag) = value.as_bool() {
            serializer.serialize_bool(flag)
        } else if let Ok(number) = value.as_int() {
            serializer.serialize_i64(number)
        } else if let Ok(number) = value.as_float() {
            // NaN and infinities have no JSON form.
            if number.is_finite() {
                serializer.serialize_f64(number)
            } else {
                serializer.serialize_unit()
            }
        } else if let Some(array) = value.read_lock::<Array>() {
            let mut seq = serializer.serialize_seq(Some(array.len()))?;
            for item in array.iter() {
                seq.serialize_element(&Json(item))?;
            }
            seq.end()
        } else if let Some(map) = value.read_lock::<Map>() {
            let mut out = serializer.serialize_map(Some(map.len()))?;
            for (key, item) in map.iter() {
                out.serialize_entry(key.as_str(), &Json(item))?;
            }
            out.end()
        } else if let Some(text) = value.read_lock::<ImmutableString>() {
            serializer.serialize_str(text.as_str())
        } else {
            serializer.serialize_str(&value.to_string())
        }
    }
}

#[derive(Default)]
struct BoundedText {
    bytes: Vec<u8>,
    over: bool,
}

impl std::io::Write for BoundedText {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len() + buf.len() > MAX_STRING_SIZE {
            self.over = true;
            return Err(std::io::Error::other(TEXT_TOO_LARGE));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
