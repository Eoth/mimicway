// Judges of script_json.rs. serde_json's own tree reads every generated document, valid or broken, and says what
// `parse` must give and what `to_text` must write back; the limits are judged at their edge, the last item, entry or
// byte accepted and the next one refused, wherever in the document they fall.
//
// Equivalent mutants (cargo-mutants, excluded in .cargo/mutants.toml):
// - `expecting` of `Value` and of `Key` written empty: serde_json reads it only to word an error, which `parse` turns
//   into `()` without reading it.
// - `visit_unit` returning `Default::default()`: the default Rhai value is `()` itself.
use super::{parse, to_text};
use crate::engine::script_budget::{
    ARRAY_TOO_LARGE, MAP_TOO_LARGE, MAX_ARRAY_SIZE, MAX_MAP_SIZE, MAX_STRING_SIZE, TEXT_TOO_LARGE,
};
use rhai::{Array, Dynamic, EvalAltResult, Map};
use serde_json::Value as Doc;
use std::collections::BTreeMap;

// Each value with its type, so that `1` and `1.0`, or `"1"` and `1`, never compare equal.
fn describe(value: &Dynamic) -> String {
    if value.is_unit() {
        "()".into()
    } else if let Ok(flag) = value.as_bool() {
        format!("b:{flag}")
    } else if let Ok(number) = value.as_int() {
        format!("i:{number}")
    } else if let Ok(number) = value.as_float() {
        format!("f:{number:?}")
    } else if let Some(array) = value.read_lock::<Array>() {
        let items: Vec<String> = array.iter().map(describe).collect();
        format!("[{}]", items.join(","))
    } else if let Some(map) = value.read_lock::<Map>() {
        let entries: Vec<String> = map
            .iter()
            .map(|(key, item)| format!("{:?}={}", key.as_str(), describe(item)))
            .collect();
        format!("{{{}}}", entries.join(","))
    } else if value.is_string() {
        format!("s:{:?}", value.clone().into_string().unwrap())
    } else {
        format!("?{}", value.type_name())
    }
}

// What a script must see of a document: integers that fit an i64 as integers, every other number as a float.
fn expected(doc: &Doc) -> String {
    match doc {
        Doc::Null => "()".into(),
        Doc::Bool(flag) => format!("b:{flag}"),
        Doc::Number(number) => match number.as_i64() {
            Some(int) => format!("i:{int}"),
            None => format!("f:{:?}", number.as_f64().unwrap()),
        },
        Doc::String(text) => format!("s:{text:?}"),
        Doc::Array(items) => {
            let items: Vec<String> = items.iter().map(expected).collect();
            format!("[{}]", items.join(","))
        }
        Doc::Object(entries) => {
            let sorted: BTreeMap<&String, &Doc> = entries.iter().collect();
            let entries: Vec<String> = sorted
                .iter()
                .map(|(key, item)| format!("{:?}={}", key.as_str(), expected(item)))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
    }
}

// The JSON text `to_text` must write for a parsed document: the same document, its large integers as floats.
fn written(doc: &Doc) -> String {
    fn as_script_sees(doc: &Doc) -> Doc {
        match doc {
            Doc::Number(number) if number.as_i64().is_none() => {
                serde_json::Number::from_f64(number.as_f64().unwrap())
                    .map_or(Doc::Null, Doc::Number)
            }
            Doc::Array(items) => Doc::Array(items.iter().map(as_script_sees).collect()),
            Doc::Object(entries) => Doc::Object(
                entries
                    .iter()
                    .map(|(key, item)| (key.clone(), as_script_sees(item)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    serde_json::to_string(&as_script_sees(doc)).unwrap()
}

fn limit_of(error: &EvalAltResult) -> &str {
    match error {
        EvalAltResult::ErrorDataTooLarge(kind, _) => kind,
        other => panic!("not a limit: {other}"),
    }
}

// Scalars as JSON writes them, odd forms included: escapes, surrogate pairs, exponents, the edges of i64 and u64.
const SCALARS: &[&str] = &[
    "null",
    "true",
    "false",
    "0",
    "-0",
    "7",
    "-12",
    "1.5",
    "-0.0",
    "1e3",
    "2E-2",
    "1.0",
    "9223372036854775807",
    "-9223372036854775808",
    "9223372036854775808",
    "18446744073709551615",
    "18446744073709551616",
    "-9223372036854775809",
    "1e308",
    "\"\"",
    "\"x\"",
    r#""caf\u00e9 \"q\" \\ \/ \n\t""#,
    r#""\ud83d\ude00""#,
    "\"é\"",
];

// A document of at most `depth` levels, keys sometimes repeated (the last one wins), blanks sometimes around tokens.
fn document(rng: &mut fastrand::Rng, depth: u32) -> String {
    let blank = |rng: &mut fastrand::Rng| [" ", "", "\n", "\t", ""][rng.usize(..5)];
    let kind = if depth == 0 { 0 } else { rng.u8(..3) };
    match kind {
        0 => SCALARS[rng.usize(..SCALARS.len())].to_string(),
        1 => {
            let items: Vec<String> = (0..rng.usize(..4))
                .map(|_| format!("{}{}", blank(rng), document(rng, depth - 1)))
                .collect();
            format!("[{}{}]", items.join(","), blank(rng))
        }
        _ => {
            let entries: Vec<String> = (0..rng.usize(..4))
                .map(|_| {
                    let key = ["a", "b", "", "k\\u00e9", "a"][rng.usize(..5)];
                    format!(
                        "{}\"{key}\"{}:{}",
                        blank(rng),
                        blank(rng),
                        document(rng, depth - 1)
                    )
                })
                .collect();
            format!("{{{}{}}}", entries.join(","), blank(rng))
        }
    }
}

#[test]
fn every_scalar_reads_and_writes_as_serde_json_does() {
    for text in SCALARS {
        let doc: Doc = serde_json::from_str(text).unwrap();
        let value = parse(text).unwrap();
        assert_eq!(describe(&value), expected(&doc), "{text}");
        assert_eq!(to_text(&value).unwrap(), written(&doc), "{text}");
    }
}

// Generated documents, then every prefix of some of them and each with a byte added at its end: what serde_json
// refuses reads as `()`, what it reads reads as its tree, and writes back as serde_json writes it.
#[test]
fn generated_documents_and_their_broken_forms_read_as_serde_json_reads_them() {
    let mut rng = fastrand::Rng::with_seed(0x6a73_6f6e_6a75_6467);
    let mut refused = 0;
    for round in 0..2048 {
        let text = document(&mut rng, 4);
        let mut texts = vec![text.clone()];
        if round % 16 == 0 {
            texts.extend(
                (0..text.len())
                    .filter(|&end| text.is_char_boundary(end))
                    .map(|end| text[..end].to_string()),
            );
            texts.extend([" ", "]", "}", ",", "x", "0", "\""].map(|tail| format!("{text}{tail}")));
        }
        for text in texts {
            let value = parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            match serde_json::from_str::<Doc>(&text) {
                Ok(doc) => {
                    assert_eq!(describe(&value), expected(&doc), "{text}");
                    assert_eq!(to_text(&value).unwrap(), written(&doc), "{text}");
                }
                Err(_) => {
                    refused += 1;
                    assert!(value.is_unit(), "{text}: {}", describe(&value));
                }
            }
        }
    }
    assert!(refused > 1000, "too few broken documents: {refused}");
}

// Values that JSON has no form for: a float that is not finite is written null, any other type as its text.
#[test]
fn values_without_a_json_form_are_written_as_null_or_as_their_text() {
    let cases: Vec<(Dynamic, &str)> = vec![
        (Dynamic::from(f64::NAN), "null"),
        (Dynamic::from(f64::INFINITY), "null"),
        (Dynamic::from(f64::NEG_INFINITY), "null"),
        (Dynamic::from('c'), "\"c\""),
        (Dynamic::from(-0.0_f64), "-0.0"),
        (Dynamic::UNIT, "null"),
        (
            Dynamic::from_array(vec![Dynamic::from(f64::NAN), Dynamic::from('\n')]),
            "[null,\"\\n\"]",
        ),
    ];
    for (value, text) in cases {
        assert_eq!(to_text(&value).unwrap(), text, "{}", describe(&value));
    }
}

// A document made of `parts` arrays or maps nested in each other at random, holding `count` items or entries in all
// (the containers themselves counted where they are an item or an entry).
fn spread(rng: &mut fastrand::Rng, kind: &str, count: usize) -> String {
    let parts = rng.usize(1..=6).min(count.max(1));
    // Each nested container is itself an item (or an entry) of the one around it: `parts - 1` of the count.
    let mut left = count - (parts - 1);
    let mut text = String::new();
    for part in 0..parts {
        let share = if part == parts - 1 {
            left
        } else {
            rng.usize(..=left)
        };
        left -= share;
        let filler: Vec<String> = (0..share)
            .map(|i| match kind {
                ARRAY_TOO_LARGE => "0".to_string(),
                _ => format!("\"{part}-{i}\":0"),
            })
            .collect();
        text += &filler.join(",");
        let inner = if part == parts - 1 {
            String::new()
        } else if kind == ARRAY_TOO_LARGE {
            format!("{}[", if share > 0 { "," } else { "" })
        } else {
            format!("{}\"in{part}\":{{", if share > 0 { "," } else { "" })
        };
        text += &inner;
    }
    let (open, close) = if kind == ARRAY_TOO_LARGE {
        ("[", "]")
    } else {
        ("{", "}")
    };
    format!("{open}{text}{}", close.repeat(parts))
}

#[test]
fn the_item_and_entry_limits_hold_to_the_last_one_wherever_they_fall() {
    let mut rng = fastrand::Rng::with_seed(0x6c69_6d69_7473_0001);
    for (kind, limit) in [
        (ARRAY_TOO_LARGE, MAX_ARRAY_SIZE),
        (MAP_TOO_LARGE, MAX_MAP_SIZE),
    ] {
        for _ in 0..24 {
            let within = spread(&mut rng, kind, limit);
            let doc: Doc = serde_json::from_str(&within).unwrap();
            assert_eq!(describe(&parse(&within).unwrap()), expected(&doc));
            let over = spread(&mut rng, kind, limit + 1);
            assert!(serde_json::from_str::<Doc>(&over).is_ok(), "{over}");
            assert_eq!(limit_of(&parse(&over).unwrap_err()), kind, "{over}");
        }
    }
    // Maps inside an array, arrays inside a map, both limits reached at once: each counts its own kind only.
    let items = format!(
        "[{}]",
        (0..MAX_ARRAY_SIZE)
            .map(|i| if i < MAX_MAP_SIZE {
                format!("{{\"{i}\":0}}")
            } else {
                "0".into()
            })
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(parse(&items).is_ok());
    let entries = format!(
        "{{{}}}",
        (0..MAX_MAP_SIZE)
            .map(|i| format!("\"{i}\":[0,1]"))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert!(parse(&entries).is_ok());
}

// Texts and keys spread over an array, `total` bytes of them in all: each piece a text item or the key of a map.
fn texts_of(rng: &mut fastrand::Rng, total: usize) -> String {
    let mut left = total;
    let mut items = Vec::new();
    while left > 0 {
        let share = if rng.u8(..4) == 0 {
            left
        } else {
            rng.usize(..=left)
        };
        left -= share;
        let piece = "t".repeat(share);
        items.push(if rng.bool() {
            format!("{{\"{piece}\":0}}")
        } else {
            format!("\"{piece}\"")
        });
    }
    format!("[{}]", items.join(","))
}

#[test]
fn the_text_limit_holds_to_the_last_byte_keys_included() {
    let mut rng = fastrand::Rng::with_seed(0x6c69_6d69_7473_0002);
    for _ in 0..8 {
        for total in [MAX_STRING_SIZE, MAX_STRING_SIZE + 1] {
            let text = texts_of(&mut rng, total);
            let doc: Doc = serde_json::from_str(&text).unwrap();
            assert_eq!(text_bytes(&doc), total);
            let result = parse(&text);
            if total > MAX_STRING_SIZE {
                assert_eq!(limit_of(&result.unwrap_err()), TEXT_TOO_LARGE);
            } else {
                assert_eq!(describe(&result.unwrap()), expected(&doc));
            }
        }
    }
}

fn text_bytes(doc: &Doc) -> usize {
    match doc {
        Doc::String(text) => text.len(),
        Doc::Array(items) => items.iter().map(text_bytes).sum(),
        Doc::Object(entries) => entries
            .iter()
            .map(|(key, item)| key.len() + text_bytes(item))
            .sum(),
        _ => 0,
    }
}

// The JSON of generated documents padded to the limit and one byte past it: the first is written whole, the second
// refused.
#[test]
fn to_json_writes_up_to_the_last_byte_of_the_limit() {
    let mut rng = fastrand::Rng::with_seed(0x6c69_6d69_7473_0003);
    for _ in 0..16 {
        let doc: Doc = serde_json::from_str(&document(&mut rng, 3)).unwrap();
        let inner = written(&doc);
        for length in [MAX_STRING_SIZE, MAX_STRING_SIZE + 1] {
            // `[inner,"pad"]`: three brackets and commas, two quotes.
            let pad = "p".repeat(length - inner.len() - 5);
            let value = Dynamic::from_array(vec![parse(&inner).unwrap(), Dynamic::from(pad)]);
            match to_text(&value) {
                Ok(text) => {
                    assert_eq!(length, MAX_STRING_SIZE);
                    assert_eq!(text.len(), MAX_STRING_SIZE);
                }
                Err(error) => {
                    assert_eq!(length, MAX_STRING_SIZE + 1);
                    assert_eq!(limit_of(&error), TEXT_TOO_LARGE);
                }
            }
        }
    }
}
