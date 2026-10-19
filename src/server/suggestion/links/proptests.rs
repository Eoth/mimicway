// Property-based tests: the scanned bodies come from a real backend, possibly hostile. A body is generated as a sequence
// of parts whose fate is known in advance (an address a service covers, one it does not, Mimicway's own, a namespace
// declaration, plain text), so the expected answer is built next to it, independently of the scanner: rendered for a
// request that reached Mimicway, the rewritten rule must answer the observed body with exactly the covered addresses
// replaced, and nothing else changed.
use super::*;
use crate::engine::{TemplateContext, TemplateRenderer};
use proptest::prelude::*;
use std::collections::HashMap;

const MIMICWAY: &str = "mimic.test:7342";

#[derive(Debug, Clone)]
enum Part {
    Text(String),
    /// Covered by `orders` (group code `team`), whose target is the origin.
    Orders(String),
    /// Covered by `stock`, whose target is `/v1` on its origin.
    StockV1(String),
    /// On `stock`'s origin, outside its target.
    StockV2(String),
    Elsewhere(String),
    Own(String),
    Namespace(String),
}

impl Part {
    /// The part as the backend sent it, and as the rewritten rule must answer it.
    fn sent_and_expected(&self) -> (String, String) {
        let same = |s: String| (s.clone(), s);
        match self {
            Part::Text(t) => same(t.clone()),
            Part::Orders(p) => (
                format!("http://orders.internal:8080{p}"),
                format!("http://{MIMICWAY}/team/orders{p}"),
            ),
            Part::StockV1(p) => (
                format!("https://stock.internal/v1{p}"),
                format!("http://{MIMICWAY}/stock{p}"),
            ),
            Part::StockV2(p) => same(format!("https://stock.internal/v2{p}")),
            Part::Elsewhere(p) => same(format!("http://elsewhere.test{p}")),
            Part::Own(p) => same(format!("http://{MIMICWAY}{p}")),
            Part::Namespace(p) => same(format!(r#"xmlns:o="http://orders.internal:8080{p}""#)),
        }
    }
}

fn arb_path() -> impl Strategy<Value = String> {
    ("(/[a-z0-9]{1,6}){0,3}", prop::option::of("[0-9]{1,3}")).prop_map(
        |(path, query)| match query {
            Some(q) => format!("{path}?q={q}"),
            None => path,
        },
    )
}

fn arb_part() -> impl Strategy<Value = Part> {
    prop_oneof![
        "[a-z0-9 /{}]{0,10}".prop_map(Part::Text),
        arb_path().prop_map(Part::Orders),
        arb_path().prop_map(Part::StockV1),
        arb_path().prop_map(Part::StockV2),
        arb_path().prop_map(Part::Elsewhere),
        arb_path().prop_map(Part::Own),
        arb_path().prop_map(Part::Namespace),
    ]
}

/// Parts joined by separators that end an address, as in JSON, XML or text.
fn arb_body() -> impl Strategy<Value = Vec<(char, Part)>> {
    prop::collection::vec(
        (
            prop::sample::select(vec![' ', '"', '<', '>', '\n']),
            arb_part(),
        ),
        0..12,
    )
}

fn covers() -> Covers {
    let config: MockConfig = serde_json::from_value(serde_json::json!({
        "services": [
            { "name": "orders", "listen_path": "", "real_target_url": "http://orders.internal:8080",
              "is_mocked": false, "rewrite_directory_urls": false, "group_name": "Team", "wsdl_mode": "auto",
              "rules": [] },
            { "name": "stock", "listen_path": "/*", "real_target_url": "https://stock.internal/v1/",
              "is_mocked": true, "rewrite_directory_urls": false, "group_name": null, "wsdl_mode": "auto",
              "rules": [] },
        ],
        "groups": [{ "name": "Team", "code": "team" }],
    }))
    .unwrap();
    Covers::from_config(&config)
}

fn render(response: &MockResponse) -> String {
    let empty = HashMap::new();
    let headers = HashMap::from([("Host".to_string(), MIMICWAY.to_string())]);
    let ctx = TemplateContext {
        path_params: &empty,
        query_params: &empty,
        headers: &headers,
        request_body: b"",
        seq_counter: 0,
        script_result: None,
        pre_script_result: None,
        post_script_result: None,
    };
    TemplateRenderer::render_body(&response.body, &[], &ctx)
}

fn literal(text: &str) -> MockResponse {
    MockResponse {
        status: 200,
        headers: vec![],
        body: vec![BodyFragment::Literal { value: text.into() }],
        chaos: None,
    }
}

proptest! {
    #[test]
    fn a_rewritten_rule_answers_the_observed_body_with_only_the_covered_addresses_replaced(parts in arb_body()) {
        let mut sent = String::new();
        let mut expected = String::new();
        let mut services = BTreeSet::new();
        for (separator, part) in &parts {
            let (s, e) = part.sent_and_expected();
            sent.push(*separator);
            sent.push_str(&s);
            expected.push(*separator);
            expected.push_str(&e);
            match part {
                Part::Orders(_) => { services.insert("orders"); }
                Part::StockV1(_) => { services.insert("stock"); }
                _ => {}
            }
        }
        let mut response = literal(&sent);
        let redirected = redirect_response(&mut response, &covers(), None);
        prop_assert_eq!(render(&response), expected);
        let names: BTreeSet<&str> = redirected.iter().map(|s| s.name.as_str()).collect();
        prop_assert_eq!(names, services);
    }

    #[test]
    fn any_text_is_scanned_without_panic_and_answers_itself_when_nothing_is_covered(
        text in "(\\PC|http://|https://|xmlns=\"|orders.internal:8080|\\{\\{|\\.|:)*"
    ) {
        let mut response = literal(&text);
        let redirected = redirect_response(&mut response, &covers(), None);
        if redirected.is_empty() {
            prop_assert_eq!(render(&response), text);
        }
    }
}
