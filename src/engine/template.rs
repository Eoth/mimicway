// Response templates: replaces {{var}} and {{var | pipe}} in the text. Single braces are ordinary characters (JSON,
// XML); double braces hold an expression.
// Variables: path.*, query.*, header.*, body.* (JSON pointer), xpath.* (simplified path, XML/SOAP), fake.*, uuid,
//   now_ms, now_iso, now_epoch, seq, script.*, pre_script.*, post_script.*
// Pipes: lower, upper, trim, capitalize, length, first(N), last(N), substr(s,l), default("v"), replace("a","b"),
//   prepend("p"), append("s")
use crate::engine::renderer::TemplateRenderer;
use std::collections::HashMap;

use crate::engine::script::ScriptResult;

pub struct TemplateContext<'a> {
    pub path_params: &'a HashMap<String, String>,
    pub query_params: &'a HashMap<String, String>,
    pub headers: &'a HashMap<String, String>,
    pub request_body: &'a [u8],
    pub seq_counter: u64,
    pub script_result: Option<&'a ScriptResult>,
    // The pre_script and post_script slots run independently of `script` (same context, no chaining, see `Rule`) and
    // are available as {{pre_script}}, {{pre_script.field}}, {{post_script}} and {{post_script.field}}.
    pub pre_script_result: Option<&'a ScriptResult>,
    pub post_script_result: Option<&'a ScriptResult>,
}

pub fn render_template(template: &str, ctx: &TemplateContext) -> String {
    let mut out = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '{'
            && i + 1 < len
            && chars[i + 1] == '{'
            && let Some(end) = find_closing_double_brace(&chars, i + 2)
        {
            let expr: String = chars[i + 2..end].iter().collect();
            out.push_str(&eval_expression(expr.trim(), ctx));
            i = end + 2;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn find_closing_double_brace(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    let mut in_quotes = false;
    let mut paren_depth: u32 = 0;
    while i < chars.len() {
        match chars[i] {
            '"' if paren_depth == 0 => in_quotes = !in_quotes,
            '(' if !in_quotes => paren_depth += 1,
            ')' if !in_quotes => paren_depth = paren_depth.saturating_sub(1),
            '}' if !in_quotes && i + 1 < chars.len() && chars[i + 1] == '}' && paren_depth == 0 => {
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn eval_expression(expr: &str, ctx: &TemplateContext) -> String {
    let parts: Vec<&str> = expr.splitn(2, '|').collect();
    let var_name = parts[0].trim();
    let raw_value = resolve_variable(var_name, ctx);
    if parts.len() == 1 {
        return raw_value;
    }
    apply_pipes(&raw_value, parts[1], ctx)
}

fn resolve_variable(name: &str, ctx: &TemplateContext) -> String {
    if let Some(key) = name.strip_prefix("path.") {
        return ctx.path_params.get(key).cloned().unwrap_or_default();
    }
    if let Some(key) = name.strip_prefix("query.") {
        return ctx.query_params.get(key).cloned().unwrap_or_default();
    }
    if let Some(key) = name.strip_prefix("header.") {
        let lower = key.to_ascii_lowercase();
        return ctx
            .headers
            .iter()
            .find(|(k, _)| k.to_ascii_lowercase() == lower)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
    }
    if let Some(pointer) = name.strip_prefix("body.") {
        return extract_body_json(ctx.request_body, pointer);
    }
    if let Some(path) = name.strip_prefix("xpath.") {
        return extract_body_xpath(ctx.request_body, path);
    }
    if let Some(kind_str) = name.strip_prefix("fake.") {
        return resolve_fake(kind_str);
    }
    if name == "script" {
        return ctx
            .script_result
            .map(|r| r.value.clone())
            .unwrap_or_default();
    }
    if let Some(field) = name.strip_prefix("script.") {
        return ctx
            .script_result
            .and_then(|r| r.fields.get(field).cloned())
            .unwrap_or_default();
    }
    if name == "pre_script" {
        return ctx
            .pre_script_result
            .map(|r| r.value.clone())
            .unwrap_or_default();
    }
    if let Some(field) = name.strip_prefix("pre_script.") {
        return ctx
            .pre_script_result
            .and_then(|r| r.fields.get(field).cloned())
            .unwrap_or_default();
    }
    if name == "post_script" {
        return ctx
            .post_script_result
            .map(|r| r.value.clone())
            .unwrap_or_default();
    }
    if let Some(field) = name.strip_prefix("post_script.") {
        return ctx
            .post_script_result
            .and_then(|r| r.fields.get(field).cloned())
            .unwrap_or_default();
    }
    match name {
        "uuid" => TemplateRenderer::gen_uuid(),
        "now_ms" => now_millis().to_string(),
        "now_epoch" => now_secs().to_string(),
        "now_iso" => epoch_to_iso(now_secs()),
        "seq" => ctx.seq_counter.to_string(),
        _ => String::new(),
    }
}

pub fn resolve_fake_public(kind: &str) -> String {
    resolve_fake(kind)
}

fn resolve_fake(kind: &str) -> String {
    use crate::models::FakeKind;
    let fk = match kind {
        "FirstName" => FakeKind::FirstName,
        "LastName" => FakeKind::LastName,
        "Email" => FakeKind::Email,
        "PhoneNumberFR" => FakeKind::PhoneNumberFR,
        "CompanyName" => FakeKind::CompanyName,
        "StreetName" => FakeKind::StreetName,
        "CityFR" => FakeKind::CityFR,
        "PostcodeFR" => FakeKind::PostcodeFR,
        "Siren" => FakeKind::Siren,
        "Siret" => FakeKind::Siret,
        "FullAddressFR" => FakeKind::FullAddressFR,
        "DatePast" => FakeKind::DatePast,
        "DateFuture" => FakeKind::DateFuture,
        "TimestampMs" => FakeKind::TimestampMs,
        "BoolRandom" => FakeKind::BoolRandom,
        "LoremSentence" => FakeKind::LoremSentence,
        "CountryFR" => FakeKind::CountryFR,
        "IbanFR" => FakeKind::IbanFR,
        _ => return String::new(),
    };
    TemplateRenderer::fake_data(&fk)
}

fn extract_body_json(body: &[u8], pointer: &str) -> String {
    let pointer = if pointer.starts_with('/') {
        pointer.to_string()
    } else {
        format!("/{pointer}")
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return String::new();
    };
    match value.pointer(&pointer) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

// The XML counterpart of extract_body_json, for {{xpath.path}}. It reuses MatchEngine::extract_xpath, the parser of
// XPath conditions, so the path has the same syntax: no namespace prefixes.
fn extract_body_xpath(body: &[u8], path: &str) -> String {
    crate::engine::matcher::MatchEngine::extract_xpath(body, path).unwrap_or_default()
}

fn apply_pipes(value: &str, pipes_str: &str, ctx: &TemplateContext) -> String {
    let mut result = value.to_string();
    for pipe in split_pipes(pipes_str) {
        let pipe = pipe.trim();
        if pipe.is_empty() {
            continue;
        }
        result = apply_single_pipe(&result, pipe, ctx);
    }
    result
}

fn split_pipes(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'|' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

fn apply_single_pipe(value: &str, pipe: &str, _ctx: &TemplateContext) -> String {
    match pipe {
        "lower" => value.to_ascii_lowercase(),
        "upper" => value.to_ascii_uppercase(),
        "trim" => value.trim().to_string(),
        "capitalize" => {
            let mut chars = value.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => format!(
                    "{}{}",
                    c.to_uppercase(),
                    chars.as_str().to_ascii_lowercase()
                ),
            }
        }
        "length" => value.chars().count().to_string(),
        _ => {
            if let Some(arg) = extract_fn_arg(pipe, "first") {
                let n: usize = arg.parse().unwrap_or(0);
                value.chars().take(n).collect()
            } else if let Some(arg) = extract_fn_arg(pipe, "last") {
                let n: usize = arg.parse().unwrap_or(0);
                let chars: Vec<char> = value.chars().collect();
                let skip = chars.len().saturating_sub(n);
                chars.into_iter().skip(skip).collect()
            } else if let Some(arg) = extract_fn_arg(pipe, "default") {
                let fallback = arg.trim_matches('"').trim_matches('\'');
                if value.is_empty() {
                    fallback.to_string()
                } else {
                    value.to_string()
                }
            } else if let Some(arg) = extract_fn_arg(pipe, "substr") {
                let parts: Vec<&str> = arg.split(',').map(|s| s.trim()).collect();
                let start: usize = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                let len: usize = parts
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(usize::MAX);
                value.chars().skip(start).take(len).collect()
            } else if let Some(arg) = extract_fn_arg(pipe, "replace") {
                let (from, to) = parse_two_string_args(arg);
                value.replace(&from, &to)
            } else if let Some(arg) = extract_fn_arg(pipe, "prepend") {
                let prefix = arg.trim_matches('"').trim_matches('\'');
                format!("{prefix}{value}")
            } else if let Some(arg) = extract_fn_arg(pipe, "append") {
                let suffix = arg.trim_matches('"').trim_matches('\'');
                format!("{value}{suffix}")
            } else {
                value.to_string()
            }
        }
    }
}

fn parse_two_string_args(arg: &str) -> (String, String) {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';
    let mut escaped = false;
    for ch in arg.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if !in_quotes && (ch == '"' || ch == '\'') {
            in_quotes = true;
            quote_char = ch;
            continue;
        }
        if in_quotes && ch == quote_char {
            in_quotes = false;
            continue;
        }
        if !in_quotes && ch == ',' {
            parts.push(std::mem::take(&mut current));
            continue;
        }
        current.push(ch);
    }
    parts.push(current);
    let a = parts.first().map(|s| s.to_string()).unwrap_or_default();
    let b = parts
        .get(1)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    (a, b)
}

fn extract_fn_arg<'a>(pipe: &'a str, name: &str) -> Option<&'a str> {
    let pipe = pipe.trim();
    if !pipe.starts_with(name) {
        return None;
    }
    let rest = &pipe[name.len()..].trim_start();
    let rest = rest.strip_prefix('(')?;
    let rest = rest.strip_suffix(')')?;
    Some(rest.trim())
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn epoch_to_iso(epoch_secs: u64) -> String {
    let secs = epoch_secs as i64;
    let days = secs.div_euclid(86400);
    let time_secs = secs.rem_euclid(86400);
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;

    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// Also used by the date functions of scripts, so the calendar arithmetic (Howard Hinnant's) exists once.
pub(crate) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d)
}

// The exact inverse of civil_from_days (Howard Hinnant, http://howardhinnant.github.io/date_algorithms.html),
// used by parse_date in scripts. The caller keeps `m` in [1,12] and `d` in [1,31]. A day beyond the month's real
// length (February 31) does not panic: it gives a day count that does not convert back to the same date, which is
// how parse_date detects it without repeating the month length and leap year rules.
pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = if m > 2 { m as i64 - 3 } else { m as i64 + 9 }; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx<'a>(
        path: &'a HashMap<String, String>,
        query: &'a HashMap<String, String>,
        headers: &'a HashMap<String, String>,
        body: &'a [u8],
        seq: u64,
    ) -> TemplateContext<'a> {
        TemplateContext {
            path_params: path,
            query_params: query,
            headers,
            request_body: body,
            seq_counter: seq,
            script_result: None,
            pre_script_result: None,
            post_script_result: None,
        }
    }

    fn empty_ctx() -> (
        HashMap<String, String>,
        HashMap<String, String>,
        HashMap<String, String>,
    ) {
        (HashMap::new(), HashMap::new(), HashMap::new())
    }

    // One argument per field of the context, so that each test shows what it sets.
    #[allow(clippy::too_many_arguments)]
    fn make_ctx_full<'a>(
        path: &'a HashMap<String, String>,
        query: &'a HashMap<String, String>,
        headers: &'a HashMap<String, String>,
        body: &'a [u8],
        seq: u64,
        script_result: Option<&'a ScriptResult>,
        pre_script_result: Option<&'a ScriptResult>,
        post_script_result: Option<&'a ScriptResult>,
    ) -> TemplateContext<'a> {
        TemplateContext {
            path_params: path,
            query_params: query,
            headers,
            request_body: body,
            seq_counter: seq,
            script_result,
            pre_script_result,
            post_script_result,
        }
    }

    #[test]
    fn script_variable_resolves_from_populated_result() {
        let (p, q, h) = empty_ctx();
        let sr = ScriptResult {
            value: "hello".into(),
            fields: HashMap::new(),
        };
        let ctx = make_ctx_full(&p, &q, &h, b"", 0, Some(&sr), None, None);
        assert_eq!(render_template("{{script}}", &ctx), "hello");
    }

    #[test]
    fn script_field_variable_resolves_from_populated_result() {
        let (p, q, h) = empty_ctx();
        let mut fields = HashMap::new();
        fields.insert("role".to_string(), "admin".to_string());
        let sr = ScriptResult {
            value: String::new(),
            fields,
        };
        let ctx = make_ctx_full(&p, &q, &h, b"", 0, Some(&sr), None, None);
        assert_eq!(render_template("{{script.role}}", &ctx), "admin");
    }

    #[test]
    fn pre_script_variable_resolves_independently() {
        let (p, q, h) = empty_ctx();
        let sr = ScriptResult {
            value: "pre-val".into(),
            fields: HashMap::new(),
        };
        let ctx = make_ctx_full(&p, &q, &h, b"", 0, None, Some(&sr), None);
        assert_eq!(render_template("{{pre_script}}", &ctx), "pre-val");
    }

    #[test]
    fn post_script_field_variable_resolves_independently() {
        let (p, q, h) = empty_ctx();
        let mut fields = HashMap::new();
        fields.insert("total".to_string(), "42".to_string());
        let sr = ScriptResult {
            value: String::new(),
            fields,
        };
        let ctx = make_ctx_full(&p, &q, &h, b"", 0, None, None, Some(&sr));
        assert_eq!(render_template("{{post_script.total}}", &ctx), "42");
    }

    #[test]
    fn pre_script_and_post_script_do_not_leak_into_each_other() {
        let (p, q, h) = empty_ctx();
        let pre = ScriptResult {
            value: "PRE".into(),
            fields: HashMap::new(),
        };
        let post = ScriptResult {
            value: "POST".into(),
            fields: HashMap::new(),
        };
        let ctx = make_ctx_full(&p, &q, &h, b"", 0, None, Some(&pre), Some(&post));
        assert_eq!(
            render_template("{{pre_script}}-{{post_script}}", &ctx),
            "PRE-POST"
        );
        assert_eq!(render_template("{{script}}", &ctx), "");
    }

    #[test]
    fn literal_text_passthrough() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("hello world", &ctx), "hello world");
    }

    #[test]
    fn path_variable() {
        let mut p = HashMap::new();
        p.insert("siret".into(), "44306184100047".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{path.siret}}", &ctx), "44306184100047");
    }

    #[test]
    fn query_variable() {
        let (p, h) = (HashMap::new(), HashMap::new());
        let mut q = HashMap::new();
        q.insert("page".into(), "3".into());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("page={{query.page}}", &ctx), "page=3");
    }

    #[test]
    fn header_variable_case_insensitive() {
        let (p, q) = (HashMap::new(), HashMap::new());
        let mut h = HashMap::new();
        h.insert("x-request-id".into(), "abc123".into());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{header.X-Request-Id}}", &ctx), "abc123");
    }

    #[test]
    fn uuid_variable() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{uuid}}", &ctx);
        assert_eq!(out.len(), 36);
        assert_eq!(out.chars().filter(|c| *c == '-').count(), 4);
    }

    #[test]
    fn now_ms_is_numeric() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{now_ms}}", &ctx);
        assert!(out.parse::<u128>().is_ok());
        assert!(out.len() >= 13);
    }

    #[test]
    fn now_iso_format() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{now_iso}}", &ctx);
        assert!(out.ends_with('Z'));
        assert!(out.contains('T'));
        assert_eq!(out.len(), 20);
    }

    #[test]
    fn now_epoch_numeric() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{now_epoch}}", &ctx);
        assert!(out.parse::<u64>().is_ok());
    }

    #[test]
    fn seq_counter() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 42);
        assert_eq!(render_template("{{seq}}", &ctx), "42");
    }

    #[test]
    fn fake_variable() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{fake.FirstName}}", &ctx);
        assert!(!out.is_empty());
    }

    #[test]
    fn body_echo_json_pointer() {
        let (p, q, h) = empty_ctx();
        let body = br#"{"user":{"name":"Alice"}}"#;
        let ctx = make_ctx(&p, &q, &h, body, 0);
        assert_eq!(render_template("{{body./user/name}}", &ctx), "Alice");
    }

    #[test]
    fn body_echo_without_leading_slash() {
        let (p, q, h) = empty_ctx();
        let body = br#"{"id":99}"#;
        let ctx = make_ctx(&p, &q, &h, body, 0);
        assert_eq!(render_template("{{body.id}}", &ctx), "99");
    }

    // --- xpath.path: the XML counterpart of body.pointer above, through MatchEngine::extract_xpath.

    #[test]
    fn xpath_echo_extracts_value_from_xml_body() {
        let (p, q, h) = empty_ctx();
        let body = br#"<Envelope><Body><id>123</id></Body></Envelope>"#;
        let ctx = make_ctx(&p, &q, &h, body, 0);
        assert_eq!(render_template("{{xpath.Envelope/Body/id}}", &ctx), "123");
    }

    #[test]
    fn xpath_echo_ignores_namespace_prefixes_in_body() {
        let (p, q, h) = empty_ctx();
        let body = br#"<soap:Envelope><soap:Body><ns:id>abc</ns:id></soap:Body></soap:Envelope>"#;
        let ctx = make_ctx(&p, &q, &h, body, 0);
        assert_eq!(render_template("{{xpath.Envelope/Body/id}}", &ctx), "abc");
    }

    #[test]
    fn xpath_echo_survives_non_self_closing_sibling_before_target() {
        // An empty <Header></Header> written in full next to <Body> must not break the extraction (walk_xml, matcher.rs).
        let (p, q, h) = empty_ctx();
        let body = br#"<SOAP:Envelope><SOAP-ENV:Header></SOAP-ENV:Header><SOAP-ENV:Body><ns3:recherche><ns3:Siret>12345678901234</ns3:Siret></ns3:recherche></SOAP-ENV:Body></SOAP:Envelope>"#;
        let ctx = make_ctx(&p, &q, &h, body, 0);
        assert_eq!(
            render_template("{{xpath.Envelope/Body/recherche/Siret}}", &ctx),
            "12345678901234"
        );
    }

    #[test]
    fn xpath_echo_with_substr_pipe_truncates_value() {
        let (p, q, h) = empty_ctx();
        let body = br#"<Envelope><Body><recherche><Siret>12345678901234</Siret></recherche></Body></Envelope>"#;
        let ctx = make_ctx(&p, &q, &h, body, 0);
        assert_eq!(
            render_template(
                "{{xpath.Envelope/Body/recherche/Siret | substr(0,9)}}",
                &ctx
            ),
            "123456789"
        );
    }

    #[test]
    fn xpath_echo_no_match_renders_empty() {
        let (p, q, h) = empty_ctx();
        let body = br#"<Envelope><Body><mode>x</mode></Body></Envelope>"#;
        let ctx = make_ctx(&p, &q, &h, body, 0);
        assert_eq!(
            render_template("{{xpath.Envelope/Body/recherche}}", &ctx),
            ""
        );
    }

    #[test]
    fn pipe_lower() {
        let mut p = HashMap::new();
        p.insert("name".into(), "ALICE".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{path.name | lower}}", &ctx), "alice");
    }

    #[test]
    fn pipe_upper() {
        let mut p = HashMap::new();
        p.insert("name".into(), "alice".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{path.name | upper}}", &ctx), "ALICE");
    }

    #[test]
    fn pipe_trim() {
        let mut p = HashMap::new();
        p.insert("v".into(), "  hello  ".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{path.v | trim}}", &ctx), "hello");
    }

    #[test]
    fn pipe_first() {
        let mut p = HashMap::new();
        p.insert("siret".into(), "44306184100047".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(
            render_template("{{path.siret | first(9)}}", &ctx),
            "443061841"
        );
    }

    #[test]
    fn pipe_default_empty() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(
            render_template(r#"{{path.missing | default("N/A")}}"#, &ctx),
            "N/A"
        );
    }

    #[test]
    fn pipe_default_present() {
        let mut p = HashMap::new();
        p.insert("x".into(), "real".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(
            render_template(r#"{{path.x | default("fallback")}}"#, &ctx),
            "real"
        );
    }

    #[test]
    fn pipe_chain() {
        let mut p = HashMap::new();
        p.insert("siret".into(), "44306184100047".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(
            render_template("{{path.siret | first(9) | upper}}", &ctx),
            "443061841"
        );
    }

    #[test]
    fn literal_braces_passthrough() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{literal}", &ctx), "{literal}");
    }

    #[test]
    fn unknown_variable_empty() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{unknown.foo}}", &ctx), "");
    }

    #[test]
    fn complex_insee_template() {
        let mut p = HashMap::new();
        p.insert("siret".into(), "44306184100047".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 5);
        let tpl =
            r#"{"siret":"{{path.siret}}","siren":"{{path.siret | first(9)}}","seq":"{{seq}}"}"#;
        let out = render_template(tpl, &ctx);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["siret"], "44306184100047");
        assert_eq!(parsed["siren"], "443061841");
        assert_eq!(parsed["seq"], "5");
    }

    #[test]
    fn epoch_to_iso_known_date() {
        assert_eq!(epoch_to_iso(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn days_from_civil_is_inverse_of_civil_from_days_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn days_from_civil_round_trips_for_a_normal_date() {
        let days = days_from_civil(2026, 3, 15);
        assert_eq!(civil_from_days(days), (2026, 3, 15));
    }

    #[test]
    fn days_from_civil_round_trips_for_leap_day() {
        let days = days_from_civil(2028, 2, 29);
        assert_eq!(civil_from_days(days), (2028, 2, 29));
    }

    #[test]
    fn days_from_civil_does_not_round_trip_for_invalid_february_31() {
        let days = days_from_civil(2026, 2, 31);
        assert_ne!(civil_from_days(days), (2026, 2, 31));
    }

    #[test]
    fn pipe_capitalize() {
        let mut p = HashMap::new();
        p.insert("v".into(), "hELLO".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{path.v | capitalize}}", &ctx), "Hello");
    }

    #[test]
    fn pipe_length() {
        let mut p = HashMap::new();
        p.insert("v".into(), "abcde".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{path.v | length}}", &ctx), "5");
    }

    #[test]
    fn pipe_substr() {
        let mut p = HashMap::new();
        p.insert("v".into(), "abcdefgh".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{path.v | substr(2, 3)}}", &ctx), "cde");
    }

    #[test]
    fn pipe_replace() {
        let mut p = HashMap::new();
        p.insert("v".into(), "hello world".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(
            render_template(r#"{{path.v | replace("world", "rust")}}"#, &ctx),
            "hello rust"
        );
    }

    #[test]
    fn pipe_prepend() {
        let mut p = HashMap::new();
        p.insert("v".into(), "world".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(
            render_template(r#"{{path.v | prepend("hello ")}}"#, &ctx),
            "hello world"
        );
    }

    #[test]
    fn pipe_append() {
        let mut p = HashMap::new();
        p.insert("v".into(), "hello".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(
            render_template(r#"{{path.v | append(" world")}}"#, &ctx),
            "hello world"
        );
    }

    #[test]
    fn fake_bool_random() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{fake.BoolRandom}}", &ctx);
        assert!(out == "true" || out == "false");
    }

    #[test]
    fn fake_lorem_sentence() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{fake.LoremSentence}}", &ctx);
        assert!(out.len() > 10 && out.ends_with('.'));
    }

    #[test]
    fn fake_country_fr() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{fake.CountryFR}}", &ctx);
        assert!(!out.is_empty());
    }

    #[test]
    fn fake_iban_fr() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template("{{fake.IbanFR}}", &ctx);
        assert!(out.starts_with("FR76"));
        assert!(out.len() >= 20);
    }

    #[test]
    fn json_roundtrip_simple() {
        let tpl = r#"{"name":"Alice","age":30}"#;
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let out = render_template(tpl, &ctx);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["name"], "Alice");
        assert_eq!(parsed["age"], 30);
    }

    #[test]
    fn json_nested_with_vars() {
        let mut p = HashMap::new();
        p.insert("siret".into(), "12345678901234".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 7);
        let tpl = r#"{"data":{"siret":"{{path.siret}}","siren":"{{path.siret | first(9)}}","seq":{{seq}}}}"#;
        let out = render_template(tpl, &ctx);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["data"]["siret"], "12345678901234");
        assert_eq!(parsed["data"]["siren"], "123456789");
        assert_eq!(parsed["data"]["seq"], 7);
    }

    #[test]
    fn json_array_of_objects() {
        let (p, q, h) = empty_ctx();
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        let tpl = r#"{"items":[{"id":"{{uuid}}","status":"active"}]}"#;
        let out = render_template(tpl, &ctx);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed["items"][0]["id"].as_str().unwrap().len() == 36);
        assert_eq!(parsed["items"][0]["status"], "active");
    }

    #[test]
    fn template_with_all_pipe_types() {
        let mut p = HashMap::new();
        p.insert("v".into(), "Hello World".into());
        let (q, h) = (HashMap::new(), HashMap::new());
        let ctx = make_ctx(&p, &q, &h, b"", 0);
        assert_eq!(render_template("{{path.v | lower}}", &ctx), "hello world");
        assert_eq!(render_template("{{path.v | upper}}", &ctx), "HELLO WORLD");
        assert_eq!(
            render_template("{{path.v | capitalize}}", &ctx),
            "Hello world"
        );
        assert_eq!(render_template("{{path.v | first(5)}}", &ctx), "Hello");
        assert_eq!(render_template("{{path.v | last(5)}}", &ctx), "World");
        assert_eq!(render_template("{{path.v | length}}", &ctx), "11");
        assert_eq!(render_template("{{path.v | substr(6, 5)}}", &ctx), "World");
        assert_eq!(
            render_template(r#"{{path.v | replace("World", "Rust")}}"#, &ctx),
            "Hello Rust"
        );
        assert_eq!(
            render_template(r#"{{path.v | prepend(">> ")}}"#, &ctx),
            ">> Hello World"
        );
        assert_eq!(
            render_template(r#"{{path.v | append(" <<")}}"#, &ctx),
            "Hello World <<"
        );
    }
}
