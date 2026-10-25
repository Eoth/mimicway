use crate::engine::template::{TemplateContext, render_template};
use crate::models::{BodyFragment, ChaosConfig, FakeKind, HeaderEntry, MockResponse};
use axum::http::StatusCode;

pub struct TemplateRenderer;

impl TemplateRenderer {
    pub fn render_body(
        fragments: &[BodyFragment],
        path_segments: &[&str],
        ctx: &TemplateContext,
    ) -> String {
        let mut out = String::new();
        for frag in fragments {
            match frag {
                BodyFragment::Literal { value } => out.push_str(value),
                BodyFragment::Uuid => out.push_str(&Self::gen_uuid()),
                BodyFragment::PickFrom { values } => out.push_str(&Self::pick_from(values)),
                BodyFragment::FakeData { kind } => out.push_str(&Self::fake_data(kind)),
                BodyFragment::PathSegment { index } => {
                    if let Some(seg) = path_segments.get(*index) {
                        out.push_str(seg);
                    }
                }
                BodyFragment::Template { template } => {
                    out.push_str(&render_template(template, ctx));
                }
            }
        }
        out
    }

    /// A header value with `{{` is a template, rendered like a body with the same variables and pipes; any other
    /// value is sent as written, so that a value holding no expression costs no rendering. Names are never rendered.
    pub fn render_headers(headers: &[HeaderEntry], ctx: &TemplateContext) -> Vec<(String, String)> {
        headers
            .iter()
            .map(|h| {
                let value = if h.value.contains("{{") {
                    render_template(&h.value, ctx)
                } else {
                    h.value.clone()
                };
                (h.name.clone(), value)
            })
            .collect()
    }

    pub fn gen_uuid() -> String {
        let mut bytes = [0u8; 16];
        for b in &mut bytes {
            *b = fastrand::u8(..);
        }
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;

        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
            bytes[8],
            bytes[9],
            bytes[10],
            bytes[11],
            bytes[12],
            bytes[13],
            bytes[14],
            bytes[15],
        )
    }

    fn pick_from(values: &[String]) -> String {
        if values.is_empty() {
            return String::new();
        }
        let idx = fastrand::usize(..values.len());
        values[idx].clone()
    }

    pub fn fake_data(kind: &FakeKind) -> String {
        match kind {
            FakeKind::FirstName => Self::pick_first_name(),
            FakeKind::LastName => Self::pick_last_name(),
            FakeKind::Email => Self::gen_email(),
            FakeKind::PhoneNumberFR => Self::gen_phone_fr(),
            FakeKind::Integer { min, max } => Self::gen_integer(*min, *max),
            FakeKind::CompanyName => Self::pick_company_name(),
            FakeKind::StreetName => Self::pick_street_name(),
            FakeKind::CityFR => Self::pick_city_fr(),
            FakeKind::PostcodeFR => Self::gen_postcode_fr(),
            FakeKind::Siren => Self::gen_n_digits(9),
            FakeKind::Siret => Self::gen_n_digits(14),
            FakeKind::FullAddressFR => Self::gen_full_address_fr(),
            FakeKind::DatePast => Self::gen_date_offset(-1825, -1),
            FakeKind::DateFuture => Self::gen_date_offset(1, 1825),
            FakeKind::TimestampMs => Self::gen_timestamp_ms(),
            FakeKind::BoolRandom => if fastrand::bool() { "true" } else { "false" }.into(),
            FakeKind::LoremSentence => Self::pick_lorem(),
            FakeKind::CountryFR => Self::pick_country_fr(),
            FakeKind::IbanFR => Self::gen_iban_fr(),
        }
    }

    fn pick_first_name() -> String {
        const NAMES: &[&str] = &[
            "Alice",
            "Bob",
            "Claire",
            "David",
            "Emma",
            "François",
            "Gabrielle",
            "Hugo",
            "Isabelle",
            "Julien",
            "Karine",
            "Lucas",
            "Marie",
            "Nicolas",
            "Olivia",
            "Pierre",
            "Quentin",
            "Rose",
            "Sophie",
            "Thomas",
            "Ursule",
            "Victor",
            "Wendy",
            "Xavier",
        ];
        NAMES[fastrand::usize(..NAMES.len())].to_string()
    }

    fn pick_last_name() -> String {
        const NAMES: &[&str] = &[
            "Martin", "Bernard", "Dubois", "Thomas", "Robert", "Richard", "Petit", "Durand",
            "Leroy", "Moreau", "Simon", "Laurent", "Lefebvre", "Michel", "Garcia", "David",
            "Bertrand", "Roux", "Vincent", "Fournier", "Morel", "Girard", "Andre", "Mercier",
        ];
        NAMES[fastrand::usize(..NAMES.len())].to_string()
    }

    fn gen_email() -> String {
        let first = Self::pick_first_name().to_ascii_lowercase();
        let last = Self::pick_last_name().to_ascii_lowercase();
        let n = fastrand::u16(1..999);
        const DOMAINS: &[&str] = &["example.com", "test.fr", "mock.dev", "fake.org"];
        let domain = DOMAINS[fastrand::usize(..DOMAINS.len())];
        format!("{first}.{last}{n}@{domain}")
    }

    fn gen_phone_fr() -> String {
        let prefix = ["06", "07"][fastrand::usize(..2)];
        let mut num = String::from(prefix);
        for _ in 0..8 {
            num.push(char::from(b'0' + fastrand::u8(..10)));
        }
        num
    }

    fn gen_integer(min: i64, max: i64) -> String {
        if min >= max {
            return min.to_string();
        }
        let val = fastrand::i64(min..=max);
        val.to_string()
    }

    fn pick_company_name() -> String {
        const NAMES: &[&str] = &[
            "Nexora",
            "Voltaire Industries",
            "Lumea Tech",
            "Groupe Ariane",
            "Solaris SARL",
            "EcoVert Solutions",
            "DataPulse",
            "Meridian SAS",
            "Altiore Conseil",
            "BioSphera",
            "CyberNova",
            "Hexagone Digital",
            "Nova Logistique",
            "Prisme Analytics",
            "Quantum Services",
            "Riviera Holding",
            "Sigma Ingenierie",
            "Triton Energies",
            "Zenith Constructions",
            "Omega Pharma",
        ];
        NAMES[fastrand::usize(..NAMES.len())].to_string()
    }

    fn pick_street_name() -> String {
        const TYPES: &[&str] = &["Rue", "Avenue", "Boulevard", "Place", "Impasse", "Allee"];
        const NAMES: &[&str] = &[
            "de la Republique",
            "Victor Hugo",
            "Jean Jaures",
            "du General de Gaulle",
            "Pasteur",
            "des Lilas",
            "du Commerce",
            "Gambetta",
            "de la Liberte",
            "Voltaire",
            "Emile Zola",
            "des Roses",
            "du Marechal Foch",
            "de la Paix",
            "Saint-Michel",
            "des Champs",
            "Clemenceau",
            "Pierre Curie",
            "de Verdun",
            "du Moulin",
        ];
        let t = TYPES[fastrand::usize(..TYPES.len())];
        let n = NAMES[fastrand::usize(..NAMES.len())];
        format!("{t} {n}")
    }

    fn pick_city_fr() -> String {
        const CITIES: &[&str] = &[
            "Paris",
            "Marseille",
            "Lyon",
            "Toulouse",
            "Nice",
            "Nantes",
            "Montpellier",
            "Strasbourg",
            "Bordeaux",
            "Lille",
            "Rennes",
            "Reims",
            "Saint-Etienne",
            "Toulon",
            "Le Havre",
            "Grenoble",
            "Dijon",
            "Angers",
            "Nimes",
            "Clermont-Ferrand",
        ];
        CITIES[fastrand::usize(..CITIES.len())].to_string()
    }

    fn gen_postcode_fr() -> String {
        format!("{:05}", fastrand::u32(1000..99999))
    }

    fn gen_n_digits(n: usize) -> String {
        let mut s = String::with_capacity(n);
        for _ in 0..n {
            s.push(char::from(b'0' + fastrand::u8(..10)));
        }
        s
    }

    fn gen_full_address_fr() -> String {
        let num = fastrand::u16(1..200);
        let street = Self::pick_street_name();
        let postcode = Self::gen_postcode_fr();
        let city = Self::pick_city_fr();
        format!("{num} {street}, {postcode} {city}")
    }

    fn gen_date_offset(min_days: i64, max_days: i64) -> String {
        use crate::engine::template::epoch_to_iso;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let offset_days = fastrand::i64(min_days..=max_days);
        let target = (now as i64 + offset_days * 86400) as u64;
        let iso = epoch_to_iso(target);
        iso[..10].to_string()
    }

    fn gen_timestamp_ms() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string()
    }

    fn pick_lorem() -> String {
        const SENTENCES: &[&str] = &[
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit.",
            "Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.",
            "Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.",
            "Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore.",
            "Excepteur sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt.",
        ];
        SENTENCES[fastrand::usize(..SENTENCES.len())].to_string()
    }

    fn pick_country_fr() -> String {
        const COUNTRIES: &[&str] = &[
            "France",
            "Belgique",
            "Suisse",
            "Canada",
            "Luxembourg",
            "Monaco",
            "Allemagne",
            "Espagne",
            "Italie",
            "Portugal",
        ];
        COUNTRIES[fastrand::usize(..COUNTRIES.len())].to_string()
    }

    fn gen_iban_fr() -> String {
        let bank: u64 = fastrand::u64(10000..99999);
        let branch: u64 = fastrand::u64(10000..99999);
        let account: u64 = fastrand::u64(10000000000..99999999999);
        let key: u8 = fastrand::u8(10..99);
        format!("FR76{bank:05}{branch:05}{account:011}{key:02}")
    }
}

pub struct ChaosMode;

impl ChaosMode {
    pub fn should_inject_error(config: &ChaosConfig) -> bool {
        match config.error_rate {
            Some(rate) if rate > 0.0 => fastrand::f64() < rate,
            _ => false,
        }
    }

    pub fn error_status(config: &ChaosConfig) -> StatusCode {
        StatusCode::from_u16(config.error_status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub async fn maybe_delay(config: &ChaosConfig) {
        let delay = if let (Some(min), Some(max)) = (config.delay_min_ms, config.delay_max_ms) {
            if min < max {
                Some(fastrand::u64(min..=max))
            } else {
                Some(min)
            }
        } else {
            config.delay_ms
        };
        if let Some(ms) = delay
            && ms > 0
        {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }
    }
}

pub async fn apply_chaos_and_render(
    response: &MockResponse,
    path_segments: &[&str],
    ctx: &TemplateContext<'_>,
) -> Result<(StatusCode, Vec<(String, String)>, String), StatusCode> {
    if let Some(ref chaos) = response.chaos {
        ChaosMode::maybe_delay(chaos).await;
        if ChaosMode::should_inject_error(chaos) {
            let status = ChaosMode::error_status(chaos);
            tracing::warn!(status = %status.as_u16(), "chaos: injecting error");
            return Err(status);
        }
    }

    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK);
    let headers = TemplateRenderer::render_headers(&response.headers, ctx);
    let body = TemplateRenderer::render_body(&response.body, path_segments, ctx);

    Ok((status, headers, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;
    use std::collections::HashMap;

    fn default_ctx() -> TemplateContext<'static> {
        static EMPTY: std::sync::LazyLock<HashMap<String, String>> =
            std::sync::LazyLock::new(HashMap::new);
        static EMPTY_BODY: &[u8] = b"";
        TemplateContext {
            path_params: &EMPTY,
            query_params: &EMPTY,
            headers: &EMPTY,
            request_body: EMPTY_BODY,
            seq_counter: 0,
            script_result: None,
        }
    }

    #[test]
    fn render_literal_only() {
        let frags = vec![BodyFragment::Literal {
            value: "hello world".into(),
        }];
        assert_eq!(
            TemplateRenderer::render_body(&frags, &[], &default_ctx()),
            "hello world"
        );
    }

    #[test]
    fn render_uuid_format() {
        let frags = vec![BodyFragment::Uuid];
        let out = TemplateRenderer::render_body(&frags, &[], &default_ctx());
        assert_eq!(out.len(), 36);
        assert_eq!(out.chars().filter(|c| *c == '-').count(), 4);
        assert_eq!(out.as_bytes()[14], b'4');
        let ninth = out.as_bytes()[19];
        assert!(
            ninth == b'8' || ninth == b'9' || ninth == b'a' || ninth == b'b',
            "variant bits incorrect: {}",
            ninth as char
        );
    }

    #[test]
    fn render_pick_from() {
        let values = vec!["a".into(), "b".into(), "c".into()];
        let frags = vec![BodyFragment::PickFrom {
            values: values.clone(),
        }];
        let out = TemplateRenderer::render_body(&frags, &[], &default_ctx());
        assert!(values.contains(&out));
    }

    #[test]
    fn render_pick_from_empty() {
        let frags = vec![BodyFragment::PickFrom { values: vec![] }];
        assert_eq!(
            TemplateRenderer::render_body(&frags, &[], &default_ctx()),
            ""
        );
    }

    #[test]
    fn render_fake_first_name() {
        let frags = vec![BodyFragment::FakeData {
            kind: FakeKind::FirstName,
        }];
        let out = TemplateRenderer::render_body(&frags, &[], &default_ctx());
        assert!(!out.is_empty());
        assert!(out.chars().next().unwrap().is_uppercase());
    }

    #[test]
    fn render_fake_last_name() {
        let frags = vec![BodyFragment::FakeData {
            kind: FakeKind::LastName,
        }];
        let out = TemplateRenderer::render_body(&frags, &[], &default_ctx());
        assert!(!out.is_empty());
    }

    #[test]
    fn render_fake_email() {
        let frags = vec![BodyFragment::FakeData {
            kind: FakeKind::Email,
        }];
        let out = TemplateRenderer::render_body(&frags, &[], &default_ctx());
        assert!(out.contains('@'));
        assert!(out.contains('.'));
    }

    #[test]
    fn render_fake_phone_fr() {
        let frags = vec![BodyFragment::FakeData {
            kind: FakeKind::PhoneNumberFR,
        }];
        let out = TemplateRenderer::render_body(&frags, &[], &default_ctx());
        assert_eq!(out.len(), 10);
        assert!(out.starts_with("06") || out.starts_with("07"));
        assert!(out.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn render_fake_integer_range() {
        let frags = vec![BodyFragment::FakeData {
            kind: FakeKind::Integer { min: 10, max: 20 },
        }];
        for _ in 0..100 {
            let out = TemplateRenderer::render_body(&frags, &[], &default_ctx());
            let val: i64 = out.parse().unwrap();
            assert!((10..=20).contains(&val));
        }
    }

    #[test]
    fn render_fake_integer_min_equals_max() {
        let frags = vec![BodyFragment::FakeData {
            kind: FakeKind::Integer { min: 5, max: 5 },
        }];
        assert_eq!(
            TemplateRenderer::render_body(&frags, &[], &default_ctx()),
            "5"
        );
    }

    #[test]
    fn render_composite_body() {
        let frags = vec![
            BodyFragment::Literal {
                value: r#"{"id":""#.into(),
            },
            BodyFragment::Uuid,
            BodyFragment::Literal {
                value: r#"","name":""#.into(),
            },
            BodyFragment::FakeData {
                kind: FakeKind::FirstName,
            },
            BodyFragment::Literal {
                value: r#""}"#.into(),
            },
        ];
        let out = TemplateRenderer::render_body(&frags, &[], &default_ctx());
        assert!(out.starts_with(r#"{"id":""#));
        assert!(out.ends_with(r#""}"#));
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed["id"].is_string());
        assert!(parsed["name"].is_string());
    }

    #[test]
    fn render_path_segment() {
        let frags = vec![
            BodyFragment::Literal {
                value: r#"{"siret":""#.into(),
            },
            BodyFragment::PathSegment { index: 2 },
            BodyFragment::Literal {
                value: r#""}"#.into(),
            },
        ];
        let segments = vec!["v4", "api", "insee", "12345678901234"];
        let out = TemplateRenderer::render_body(&frags, &segments, &default_ctx());
        assert_eq!(out, r#"{"siret":"insee"}"#);
    }

    #[test]
    fn render_path_segment_last() {
        let frags = vec![BodyFragment::PathSegment { index: 3 }];
        let segments = vec!["v4", "api", "insee", "12345678901234"];
        let out = TemplateRenderer::render_body(&frags, &segments, &default_ctx());
        assert_eq!(out, "12345678901234");
    }

    #[test]
    fn render_path_segment_out_of_bounds() {
        let frags = vec![BodyFragment::PathSegment { index: 99 }];
        let out = TemplateRenderer::render_body(&frags, &["a", "b"], &default_ctx());
        assert_eq!(out, "");
    }

    #[test]
    fn chaos_error_rate_zero_never_triggers() {
        let config = ChaosConfig {
            delay_ms: None,
            delay_min_ms: None,
            delay_max_ms: None,
            error_rate: Some(0.0),
            error_status: 500,
        };
        for _ in 0..1000 {
            assert!(!ChaosMode::should_inject_error(&config));
        }
    }

    #[test]
    fn chaos_error_rate_one_always_triggers() {
        let config = ChaosConfig {
            delay_ms: None,
            delay_min_ms: None,
            delay_max_ms: None,
            error_rate: Some(1.0),
            error_status: 503,
        };
        for _ in 0..100 {
            assert!(ChaosMode::should_inject_error(&config));
        }
    }

    #[test]
    fn chaos_no_error_rate() {
        let config = ChaosConfig {
            delay_ms: Some(100),
            delay_min_ms: None,
            delay_max_ms: None,
            error_rate: None,
            error_status: 500,
        };
        assert!(!ChaosMode::should_inject_error(&config));
    }

    #[test]
    fn chaos_error_status_custom() {
        let config = ChaosConfig {
            delay_ms: None,
            delay_min_ms: None,
            delay_max_ms: None,
            error_rate: Some(1.0),
            error_status: 429,
        };
        assert_eq!(
            ChaosMode::error_status(&config),
            StatusCode::TOO_MANY_REQUESTS
        );
    }

    #[tokio::test]
    async fn chaos_delay_zero_is_instant() {
        let config = ChaosConfig {
            delay_ms: Some(0),
            delay_min_ms: None,
            delay_max_ms: None,
            error_rate: None,
            error_status: 500,
        };
        let start = std::time::Instant::now();
        ChaosMode::maybe_delay(&config).await;
        assert!(start.elapsed().as_millis() < 50);
    }

    #[tokio::test]
    async fn apply_chaos_and_render_nominal() {
        let response = MockResponse {
            status: 201,
            headers: vec![HeaderEntry {
                name: "X-Custom".into(),
                value: "test".into(),
            }],
            body: vec![BodyFragment::Literal { value: "ok".into() }],
            chaos: None,
        };
        let (status, headers, body) = apply_chaos_and_render(&response, &[], &default_ctx())
            .await
            .unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].0, "X-Custom");
        assert_eq!(body, "ok");
    }

    #[test]
    fn a_header_value_with_an_expression_is_rendered_and_any_other_is_sent_as_written() {
        let path = HashMap::from([("job".to_string(), "billing".to_string())]);
        let headers = HashMap::from([("Host".to_string(), "mock:7342".to_string())]);
        let empty = HashMap::new();
        let ctx = TemplateContext {
            path_params: &path,
            query_params: &empty,
            headers: &headers,
            request_body: b"",
            seq_counter: 4,
            script_result: None,
        };
        let entries = vec![
            HeaderEntry {
                name: "Location".into(),
                value: "http://{{header.host}}/queue/item/{{path.job | upper}}-{{seq}}/".into(),
            },
            HeaderEntry {
                name: "{{path.job}}".into(),
                value: "{ \"single\": \"braces\" } {path.job}".into(),
            },
        ];
        assert_eq!(
            TemplateRenderer::render_headers(&entries, &ctx),
            vec![
                (
                    "Location".to_string(),
                    "http://mock:7342/queue/item/BILLING-4/".to_string()
                ),
                (
                    "{{path.job}}".to_string(),
                    "{ \"single\": \"braces\" } {path.job}".to_string()
                ),
            ]
        );
    }

    #[tokio::test]
    async fn apply_chaos_and_render_with_error_injection() {
        let response = MockResponse {
            status: 200,
            headers: vec![],
            body: vec![BodyFragment::Literal { value: "ok".into() }],
            chaos: Some(ChaosConfig {
                delay_ms: None,
                delay_min_ms: None,
                delay_max_ms: None,
                error_rate: Some(1.0),
                error_status: 503,
            }),
        };
        let result = apply_chaos_and_render(&response, &[], &default_ctx()).await;
        assert_eq!(result.unwrap_err(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
