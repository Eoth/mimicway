// Escaping of a text for the place it lands in a response, the same in templates (the pipes `xml_text`, `xml_attr`,
// `cdata`, `json_string`) and in scripts (the functions of the same names): a value read from the request and written
// into an XML or JSON document keeps that document well formed, whatever the value.
//
// - `xml_text`: the text of an element. `&`, `<` and `>` become entities (`>` too, so that a value never ends a CDATA
//   marker `]]>` written around it).
// - `xml_attr`: the value of an attribute, between either kind of quote. `&`, `<`, `>`, `"` and `'` become entities.
// - `cdata`: the inside of a CDATA section, where entities are not read: `]]>`, which would end the section, is written
//   by ending the section between `]]` and `>` and opening another one (`]]]]><![CDATA[>`).
// - `json_string`: the inside of a JSON string, as serde_json writes one: `"` and `\` escaped, the control characters
//   U+0000 to U+001F as `\b`, `\f`, `\n`, `\r`, `\t` or `\u00XX`.
// - `json_value` (template pipe only): a JSON value, out of any string. A text that is exactly a JSON number (RFC 8259:
//   no `+1`, `01`, `.5`, `1.`, `NaN` nor `Infinity`) as it is, any other text as a JSON string, quotes included; `true`,
//   `false` and `null` too, as the texts they are.
//
// Each function says the length of its result before building it (`escaped_len`), so that a script can refuse a result
// past the size of a text before it exists (script_budget.rs).

/// The kinds of escaping, by the name of their pipe and script function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Escaping {
    XmlText,
    XmlAttr,
    Cdata,
    JsonString,
    JsonValue,
}

impl Escaping {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Escaping::XmlText => "xml_text",
            Escaping::XmlAttr => "xml_attr",
            Escaping::Cdata => "cdata",
            Escaping::JsonString => "json_string",
            Escaping::JsonValue => "json_value",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        [
            Escaping::XmlText,
            Escaping::XmlAttr,
            Escaping::Cdata,
            Escaping::JsonString,
            Escaping::JsonValue,
        ]
        .into_iter()
        .find(|e| e.name() == name)
    }

    /// `text` escaped.
    pub(crate) fn apply(self, text: &str) -> String {
        let mut out = String::with_capacity(self.escaped_len(text));
        match self {
            Escaping::JsonValue if is_json_number(text) => out.push_str(text),
            Escaping::JsonValue => {
                out.push('"');
                out.push_str(&Escaping::JsonString.apply(text));
                out.push('"');
            }
            Escaping::Cdata => {
                let mut rest = text;
                while let Some(at) = rest.find("]]>") {
                    out.push_str(&rest[..at]);
                    out.push_str(CDATA_END);
                    rest = &rest[at + 3..];
                }
                out.push_str(rest);
            }
            _ => {
                for c in text.chars() {
                    match self.piece(c) {
                        Some(Piece::Fixed(piece)) => out.push_str(piece),
                        Some(Piece::Unicode) => {
                            out.push_str("\\u00");
                            out.push(HEX[(u32::from(c) >> 4) as usize]);
                            out.push(HEX[(u32::from(c) & 0xf) as usize]);
                        }
                        None => out.push(c),
                    }
                }
            }
        }
        out
    }

    /// The length in bytes of `apply(text)`, without building it.
    pub(crate) fn escaped_len(self, text: &str) -> usize {
        match self {
            Escaping::JsonValue if is_json_number(text) => text.len(),
            Escaping::JsonValue => Escaping::JsonString.escaped_len(text) + 2,
            Escaping::Cdata => text.len() + text.matches("]]>").count() * (CDATA_END.len() - 3),
            _ => text
                .chars()
                .map(|c| match self.piece(c) {
                    Some(Piece::Fixed(piece)) => piece.len(),
                    Some(Piece::Unicode) => 6,
                    None => c.len_utf8(),
                })
                .sum(),
        }
    }

    fn piece(self, c: char) -> Option<Piece> {
        match (self, c) {
            (Escaping::XmlText | Escaping::XmlAttr, '&') => Some(Piece::Fixed("&amp;")),
            (Escaping::XmlText | Escaping::XmlAttr, '<') => Some(Piece::Fixed("&lt;")),
            (Escaping::XmlText | Escaping::XmlAttr, '>') => Some(Piece::Fixed("&gt;")),
            (Escaping::XmlAttr, '"') => Some(Piece::Fixed("&quot;")),
            (Escaping::XmlAttr, '\'') => Some(Piece::Fixed("&apos;")),
            (Escaping::JsonString, '"') => Some(Piece::Fixed("\\\"")),
            (Escaping::JsonString, '\\') => Some(Piece::Fixed("\\\\")),
            (Escaping::JsonString, '\u{8}') => Some(Piece::Fixed("\\b")),
            (Escaping::JsonString, '\u{c}') => Some(Piece::Fixed("\\f")),
            (Escaping::JsonString, '\n') => Some(Piece::Fixed("\\n")),
            (Escaping::JsonString, '\r') => Some(Piece::Fixed("\\r")),
            (Escaping::JsonString, '\t') => Some(Piece::Fixed("\\t")),
            (Escaping::JsonString, '\u{0}'..='\u{1f}') => Some(Piece::Unicode),
            _ => None,
        }
    }
}

/// Whether `text` is exactly a JSON number (RFC 8259, section 6): `-`? then `0` or a digit 1-9 and digits, then
/// optionally `.` and digits, then optionally `e` or `E`, a sign, digits.
pub(crate) fn is_json_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut i = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => i += digits(i),
        _ => return false,
    }
    if bytes.get(i) == Some(&b'.') {
        let fraction = digits(i + 1);
        if fraction == 0 {
            return false;
        }
        i += 1 + fraction;
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exponent = digits(i);
        if exponent == 0 {
            return false;
        }
        i += exponent;
    }
    i == bytes.len()
}

/// How one character is written.
enum Piece {
    Fixed(&'static str),
    /// `\u00XX`, lowercase hexadecimal digits, as serde_json writes them.
    Unicode,
}

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// What `]]>` becomes inside a CDATA section.
const CDATA_END: &str = "]]]]><![CDATA[>";

#[cfg(test)]
mod tests;
