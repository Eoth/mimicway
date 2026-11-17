// The judge of what a suggested rule does with requests it never saw. A model of the backend answers every request
// (`Backend`): some calls are observed and the rules suggested from them, then calls held out, with values no
// observed call had (quotes, `<`, `&`, `]]>`, control characters, long texts, a JSON number turned into a text or an
// object, a field missing, a list longer than a script can write), are answered by the rules as the mock would. Every
// answer must be a well-formed document (read whole by the XML or JSON reader); and a rule that reads values from the
// request (`repeats`, `list`) must answer what the backend answers, for a list as long as the rule says it holds.
//
// What it does not see: backends whose answers the model does not write (another escaping of the same characters, a
// value repeated in a comment or a processing instruction), and conditional rules, whose answers are observed ones.
use super::replay::{answer, as_rule, rules_of};
use super::suggest_for_service;
use crate::models::Rule;
use crate::server::observation::{ObservationKey, ObservedExchange};
use proptest::prelude::*;
use std::collections::HashMap;

/// Tame values, which the observed calls send, then the values held out for the calls never seen.
const TAME: &[&str] = &["R1", "R2", "R3", "R4", "2026-01-05", "A", "B"];
const WILD: &[&str] = &[
    "a\"b",
    "x\" admin=\"true",
    "it's",
    "<b>&amp;",
    "]]>",
    "\u{1}ctl",
    "line\nbreak",
    "tab\there",
];

/// A JSON value of the request at a place the backend writes as JSON, not as a text.
#[derive(Debug, Clone)]
enum Number {
    Int(i64),
    Text(&'static str),
    Null,
    Object,
    Missing,
}

impl Number {
    fn json(&self) -> Option<serde_json::Value> {
        match self {
            Number::Int(n) => Some((*n).into()),
            Number::Text(t) => Some((*t).into()),
            Number::Null => Some(serde_json::Value::Null),
            Number::Object => Some(serde_json::json!({ "a": 1 })),
            Number::Missing => None,
        }
    }
}

/// What the backend writes of the request, chosen once per case.
#[derive(Debug, Clone, Copy)]
struct Backend {
    json: bool,
    /// XML: the reference in an attribute, in the text of an element, in a CDATA section.
    attribute: bool,
    text: bool,
    cdata: bool,
    /// JSON: the count written as the JSON value of the request.
    count: bool,
}

#[derive(Debug, Clone)]
struct Request {
    reference: String,
    count: Number,
    slots: Vec<(String, String, Number)>,
}

fn xml_text(v: &str) -> String {
    crate::engine::escape::Escaping::XmlText.apply(v)
}

impl Backend {
    fn request_body(&self, r: &Request) -> String {
        if self.json {
            let mut order = serde_json::Map::new();
            order.insert("ref".into(), r.reference.clone().into());
            if let Some(count) = r.count.json() {
                order.insert("count".into(), count);
            }
            let lines: Vec<serde_json::Value> = r
                .slots
                .iter()
                .map(|(date, kind, qty)| {
                    let mut line = serde_json::Map::new();
                    line.insert("date".into(), date.clone().into());
                    line.insert("type".into(), kind.clone().into());
                    if let Some(qty) = qty.json() {
                        line.insert("qty".into(), qty);
                    }
                    line.into()
                })
                .collect();
            serde_json::json!({ "order": order, "lines": lines }).to_string()
        } else {
            let slots: String = r
                .slots
                .iter()
                .map(|(date, kind, _)| {
                    format!(
                        "<o:slot><o:date>{}</o:date><o:type>{}</o:type></o:slot>",
                        xml_text(date),
                        xml_text(kind)
                    )
                })
                .collect();
            format!(
                "<soap:Envelope xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:o=\"urn:o\">\
                 <soap:Body><o:Book><o:ref>{}</o:ref>{slots}</o:Book></soap:Body></soap:Envelope>",
                xml_text(&r.reference)
            )
        }
    }

    fn response_body(&self, r: &Request) -> String {
        use crate::engine::escape::Escaping;
        if self.json {
            let to_json = |n: &Number| n.json().unwrap_or(serde_json::Value::Null).to_string();
            let string = |v: &str| Escaping::JsonString.apply(v);
            let lines: Vec<String> = r
                .slots
                .iter()
                .map(|(date, kind, qty)| {
                    format!(
                        "{{\"date\":\"{}\",\"type\":\"{}\",\"qty\":{}}}",
                        string(date),
                        string(kind),
                        to_json(qty)
                    )
                })
                .collect();
            let count = if self.count {
                format!(",\"count\":{}", to_json(&r.count))
            } else {
                String::new()
            };
            format!(
                "{{\"ref\":\"{}\"{count},\"booked\":[{}]}}",
                string(&r.reference),
                lines.join(",")
            )
        } else {
            let id = if self.attribute {
                format!(" id=\"{}\"", Escaping::XmlAttr.apply(&r.reference))
            } else {
                String::new()
            };
            let text = if self.text {
                format!("<ref>{}</ref>", xml_text(&r.reference))
            } else {
                String::new()
            };
            let cdata = if self.cdata {
                format!(
                    "<note><![CDATA[{}]]></note>",
                    Escaping::Cdata.apply(&r.reference)
                )
            } else {
                String::new()
            };
            let blocks: String = r
                .slots
                .iter()
                .map(|(date, kind, _)| {
                    format!(
                        "<booked date=\"{}\"><type>{}</type></booked>",
                        Escaping::XmlAttr.apply(date),
                        xml_text(kind)
                    )
                })
                .collect();
            format!(
                "<soap:Envelope xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\"><soap:Body>\
                 <BookResponse{id}>{text}{cdata}{blocks}</BookResponse></soap:Body></soap:Envelope>"
            )
        }
    }

    fn exchange(&self, r: &Request) -> ObservedExchange {
        ObservedExchange::new(
            HashMap::new(),
            HashMap::new(),
            self.request_body(r).as_bytes(),
            None,
            200,
            HashMap::new(),
            self.response_body(r).as_bytes(),
            Some(
                if self.json {
                    "application/json"
                } else {
                    "text/xml"
                }
                .into(),
            ),
        )
    }
}

/// Whether `body` is a well-formed document of its format: read whole, every attribute and every reference too.
fn well_formed(json: bool, body: &str) -> bool {
    if json {
        return serde_json::from_str::<serde_json::Value>(body).is_ok();
    }
    use quick_xml::events::Event;
    let mut reader = quick_xml::reader::Reader::from_str(body);
    let mut depth = 0usize;
    let mut roots = 0usize;
    loop {
        match reader.read_event() {
            Ok(event @ (Event::Start(_) | Event::Empty(_))) => {
                let (Event::Start(e) | Event::Empty(e)) = &event else {
                    return false;
                };
                for attribute in e.attributes() {
                    let Ok(attribute) = attribute else {
                        return false;
                    };
                    if attribute
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .is_err()
                    {
                        return false;
                    }
                }
                if depth == 0 {
                    roots += 1;
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Ok(Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(event @ (Event::Text(_) | Event::GeneralRef(_) | Event::CData(_))) => {
                let Some(text) = crate::engine::MatchEngine::xml_text(&event) else {
                    return false;
                };
                if depth == 0 && !text.trim().is_empty() {
                    return false;
                }
                if text
                    .chars()
                    .any(|c| c.is_control() && !"\t\n\r".contains(c))
                {
                    return false;
                }
            }
            Ok(Event::Eof) => return depth == 0 && roots == 1,
            Err(_) => return false,
            Ok(_) => {}
        }
    }
}

fn arb_number(wild: bool) -> impl Strategy<Value = Number> {
    if wild {
        prop_oneof![
            (0i64..1000).prop_map(Number::Int),
            Just(Number::Text("abc")),
            Just(Number::Null),
            Just(Number::Object),
            Just(Number::Missing),
        ]
        .boxed()
    } else {
        (0i64..1000).prop_map(Number::Int).boxed()
    }
}

/// A value: tame, or for a call held out anything of `WILD` (no control character in XML, which none may hold), or a
/// long text.
fn arb_value(wild: bool, json: bool) -> BoxedStrategy<String> {
    let tame = prop::sample::select(TAME).prop_map(str::to_string);
    if !wild {
        return tame.boxed();
    }
    let wild_values: Vec<&'static str> = WILD
        .iter()
        .copied()
        .filter(|v| json || !v.contains('\u{1}'))
        .collect();
    prop_oneof![
        tame,
        prop::sample::select(wild_values).prop_map(str::to_string),
        Just("v".repeat(300)),
    ]
    .boxed()
}

fn arb_request(wild: bool, json: bool) -> impl Strategy<Value = Request> {
    let slots = prop_oneof![
        prop::collection::vec(
            (
                arb_value(wild, json),
                arb_value(wild, json),
                arb_number(wild)
            ),
            0..4
        ),
        // Longer than a script holds, for a call held out only.
        if wild {
            Just(400usize).boxed()
        } else {
            Just(2usize).boxed()
        }
        .prop_map(|n| {
            (0..n)
                .map(|i| (format!("2026-{i:04}"), "A".to_string(), Number::Int(1)))
                .collect()
        }),
    ];
    (arb_value(wild, json), arb_number(wild), slots).prop_map(|(reference, count, slots)| Request {
        reference,
        count,
        slots,
    })
}

/// The observed calls: every value differs from call to call and from item to item, so that each field the backend
/// writes is seen with several contents (a value seen with one content stays as written, and rightly so).
fn observed(slot_counts: &[usize]) -> Vec<Request> {
    slot_counts
        .iter()
        .enumerate()
        .map(|(c, &n)| Request {
            reference: format!("R{c}"),
            count: Number::Int(100 + 7 * c as i64),
            slots: (0..n)
                .map(|j| {
                    (
                        format!("2026-0{c}-1{j}"),
                        format!("T{c}{j}"),
                        Number::Int(500 + 10 * c as i64 + j as i64),
                    )
                })
                .collect(),
        })
        .collect()
}

fn arb_case() -> impl Strategy<Value = (Backend, Vec<Request>, Vec<Request>)> {
    (
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_flat_map(|(json, attribute, text, cdata, count)| {
            let backend = Backend {
                json,
                attribute,
                text: text || !(attribute || cdata),
                cdata,
                count,
            };
            (
                Just(backend),
                prop::collection::vec(0usize..4, 3..6).prop_map(|counts| observed(&counts)),
                prop::collection::vec(arb_request(true, json), 4..8),
            )
        })
}

/// Whether a rule says it writes a list of `items` (`RepeatedList::max_items`).
fn holds(rule: &super::SuggestedRule, items: usize) -> bool {
    rule.list
        .as_ref()
        .is_none_or(|list| items <= list.max_items)
}

#[test]
fn rules_answer_calls_never_seen_with_well_formed_documents_and_as_the_backend() {
    use proptest::test_runner::TestRunner;
    use std::cell::Cell;
    let config = crate::test_seed::config(256, 0x6865_6c64_5f6f_7574);
    let (generalized, answered) = (Cell::new(0), Cell::new(0));
    let result = TestRunner::new(config).run(&arb_case(), |(backend, observed, held_out)| {
        let key = ObservationKey {
            group_name: None,
            service_name: "svc".into(),
            method: "POST".into(),
            sub_path: "/ws".into(),
            operation: None,
        };
        let exchanges: Vec<ObservedExchange> =
            observed.iter().map(|r| backend.exchange(r)).collect();
        let endpoints = vec![(key.clone(), exchanges)];
        let suggestions = suggest_for_service(&endpoints);
        let suggested: Vec<&super::SuggestedRule> = suggestions
            .iter()
            .flat_map(|s| rules_of(&s.outcome))
            .collect();
        let rules: Vec<Rule> = suggested
            .iter()
            .enumerate()
            .map(|(i, r)| as_rule(i, r))
            .collect();
        generalized.set(
            generalized.get()
                + usize::from(
                    suggested
                        .iter()
                        .any(|r| !r.repeats.is_empty() || r.list.is_some()),
                ),
        );
        for request in &held_out {
            let exchange = backend.exchange(request);
            let body = exchange.request_body.clone();
            let Some((_, got)) = answer(&rules, &key, &exchange, body.as_bytes()) else {
                continue;
            };
            answered.set(answered.get() + 1);
            prop_assert!(
                well_formed(backend.json, &got),
                "not well formed for {request:?}: {got}\nrules: {suggested:?}"
            );
            let index = rules
                .iter()
                .position(|r| {
                    crate::engine::MatchEngine::first_match(
                        std::slice::from_ref(r),
                        &crate::engine::RequestData {
                            query_params: HashMap::new(),
                            headers: HashMap::new(),
                            body: body.as_bytes().to_vec(),
                            content_type: None,
                            path_params: HashMap::new(),
                            method: "POST".into(),
                            remaining_path: "/ws".into(),
                        },
                    )
                    .is_some()
                })
                .unwrap_or(0);
            let rule = suggested[index];
            // A rule without a list answers as the backend only the number of items it was observed with.
            let items = request.slots.len();
            let covered = match &rule.list {
                Some(_) => holds(rule, items),
                None => observed.iter().any(|r| r.slots.len() == items),
            };
            if (!rule.repeats.is_empty() || rule.list.is_some()) && covered {
                prop_assert_eq!(
                    &got,
                    &exchange.response_body,
                    "for {:?}\nrule: {:?}",
                    request,
                    rule
                );
            }
        }
        Ok(())
    });
    if let Err(failure) = result {
        panic!("{failure}");
    }
    assert!(
        generalized.get() >= 64 && answered.get() >= 256,
        "generalized {generalized:?}, answered {answered:?}"
    );
}
