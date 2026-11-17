// Each escaping judged by the reader of its format: quick-xml reads back the text of an element, the value of an
// attribute between either quote and a CDATA section, and serde_json writes every string as `json_string` does.
use super::*;
use proptest::prelude::*;

const ALL: [Escaping; 5] = [
    Escaping::XmlText,
    Escaping::XmlAttr,
    Escaping::Cdata,
    Escaping::JsonString,
    Escaping::JsonValue,
];

/// The text quick-xml reads in the first element of `xml`, its attribute `a` when it has one, entities resolved and
/// CDATA sections read as text; `None` when the document is not well formed.
fn read_back(xml: &str) -> Option<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::reader::Reader::from_str(xml);
    let mut text = String::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) => {
                if let Some(a) = e.try_get_attribute("a").ok()? {
                    return Some(
                        a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .ok()?
                            .into_owned(),
                    );
                }
            }
            event @ (Event::Text(_) | Event::GeneralRef(_) | Event::CData(_)) => {
                text.push_str(&crate::engine::MatchEngine::xml_text(&event)?)
            }
            Event::Eof => return Some(text),
            _ => {}
        }
    }
}

fn arb_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "\\PC{0,24}",
        "[<>&\"'\\]\\[!CDATA{}\\\\ a\u{0}-\u{1f}\u{7f}é]{0,24}",
    ]
}

#[test]
fn every_character_each_escaping_writes_otherwise() {
    assert_eq!(
        Escaping::XmlText.apply("a&b<c>d\"e'f"),
        "a&amp;b&lt;c&gt;d\"e'f"
    );
    assert_eq!(
        Escaping::XmlAttr.apply("a&b<c>d\"e'f"),
        "a&amp;b&lt;c&gt;d&quot;e&apos;f"
    );
    assert_eq!(
        Escaping::JsonString.apply("\"\\\u{8}\u{c}\n\r\t\u{0}\u{1f}\u{7f}é/"),
        "\\\"\\\\\\b\\f\\n\\r\\t\\u0000\\u001f\u{7f}é/"
    );
    assert_eq!(Escaping::JsonString.apply("\u{1a}"), "\\u001a");
    assert_eq!(
        Escaping::Cdata.apply("a]]>b]]>"),
        "a]]]]><![CDATA[>b]]]]><![CDATA[>"
    );
    assert_eq!(Escaping::Cdata.apply("]]]>&<"), "]]]]]><![CDATA[>&<");
    for escaping in ALL {
        assert_eq!(Escaping::from_name(escaping.name()), Some(escaping));
        if escaping != Escaping::JsonValue {
            assert_eq!(escaping.apply("plain text"), "plain text");
        }
    }
    assert_eq!(Escaping::from_name("xml"), None);
}

#[test]
fn json_value_writes_a_json_number_as_it_is_and_any_other_text_as_a_string() {
    for number in [
        "0",
        "-0",
        "7",
        "-12",
        "1.0",
        "0.5",
        "1e999",
        "1E+2",
        "2e-3",
        "-0.0e0",
        "123456789012345678901234567890",
    ] {
        assert_eq!(Escaping::JsonValue.apply(number), number);
    }
    for text in [
        "+1", "01", "-01", ".5", "1.", "1.e2", "NaN", "Infinity", "-", "1e", "1e+", "0x1", " 1",
        "1 ", "", "true", "null", "a\"b",
    ] {
        let written = Escaping::JsonValue.apply(text);
        assert_eq!(
            serde_json::from_str::<String>(&written).unwrap(),
            text,
            "{written}"
        );
    }
}

proptest! {
    #![proptest_config(crate::test_seed::config(512, 0x6573_6361_7065_2131))]

    #[test]
    fn json_string_writes_what_serde_json_writes(text in arb_text()) {
        let quoted = serde_json::to_string(&text).unwrap();
        prop_assert_eq!(Escaping::JsonString.apply(&text), &quoted[1..quoted.len() - 1]);
    }

    #[test]
    fn each_xml_escaping_reads_back_its_text(text in arb_text()) {
        // XML holds no control character but tab, line feed and carriage return; and a carriage return is read as a
        // line feed. Those are no matter of escaping.
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\t' || *c == '\n')
            .collect();
        let element = format!("<r>{}</r>", Escaping::XmlText.apply(&text));
        prop_assert_eq!(read_back(&element), Some(text.clone()), "{}", element);
        let cdata = format!("<r><![CDATA[{}]]></r>", Escaping::Cdata.apply(&text));
        prop_assert_eq!(read_back(&cdata), Some(text.clone()), "{}", cdata);
        let attribute = text.replace(['\t', '\n'], " ");
        for quote in ['"', '\''] {
            let xml = format!("<r a={quote}{}{quote}/>", Escaping::XmlAttr.apply(&attribute));
            prop_assert_eq!(read_back(&xml), Some(attribute.clone()), "{}", xml);
        }
    }

    #[test]
    fn json_value_is_a_value_serde_json_reads_the_same_number_or_the_text(
        text in prop_oneof![
            arb_text(),
            "-?(0|[1-9][0-9]{0,3})(\\.[0-9]{1,3})?([eE][+-]?[0-9]{1,3})?",
            "[-+.eE0-9]{0,6}",
        ]
    ) {
        let written = Escaping::JsonValue.apply(&text);
        match serde_json::from_str::<serde_json::Value>(&written) {
            // A number out of the range of a float (`1e999`) is still a JSON number: serde_json refuses to build it.
            Err(_) => prop_assert!(written == text && is_json_number(&text), "{}", written),
            Ok(serde_json::Value::String(read)) => prop_assert_eq!(read, text),
            Ok(serde_json::Value::Number(_)) => prop_assert_eq!(&written, &text),
            Ok(other) => prop_assert!(false, "{} read as {:?}", written, other),
        }
    }

    #[test]
    fn the_length_is_said_before_the_text_is_built(text in arb_text()) {
        for escaping in ALL {
            prop_assert_eq!(escaping.escaped_len(&text), escaping.apply(&text).len());
        }
    }
}
