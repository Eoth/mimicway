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
}

impl Escaping {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Escaping::XmlText => "xml_text",
            Escaping::XmlAttr => "xml_attr",
            Escaping::Cdata => "cdata",
            Escaping::JsonString => "json_string",
        }
    }

    pub(crate) fn from_name(name: &str) -> Option<Self> {
        [
            Escaping::XmlText,
            Escaping::XmlAttr,
            Escaping::Cdata,
            Escaping::JsonString,
        ]
        .into_iter()
        .find(|e| e.name() == name)
    }

    /// `text` escaped.
    pub(crate) fn apply(self, text: &str) -> String {
        let mut out = String::with_capacity(self.escaped_len(text));
        match self {
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
