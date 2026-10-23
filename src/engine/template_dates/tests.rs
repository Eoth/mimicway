use super::{format_millis, iso_to_millis};
use crate::engine::template::{TemplateContext, render_template};
use std::collections::HashMap;

fn render(template: &str, body: &str) -> String {
    let empty = HashMap::new();
    let ctx = TemplateContext {
        path_params: &empty,
        query_params: &empty,
        headers: &empty,
        request_body: body.as_bytes(),
        seq_counter: 0,
        script_result: None,
        pre_script_result: None,
        post_script_result: None,
    };
    render_template(template, &ctx)
}

#[test]
fn a_date_written_in_the_template_converts_to_every_unit() {
    // 2026-12-31T10:00:00Z, checked with `date -u -d 2026-12-31T10:00:00Z +%s`: 1798711200.
    assert_eq!(
        render(r#"{{"2026-12-31T10:00:00Z" | epoch_seconds}}"#, ""),
        "1798711200"
    );
    assert_eq!(
        render(r#"{{"2026-12-31T10:00:00Z" | epoch_ms}}"#, ""),
        "1798711200000"
    );
    assert_eq!(
        render(r#"{{"2026-12-31T10:00:00Z" | epoch_minutes}}"#, ""),
        "29978520"
    );
    assert_eq!(
        render(r#"{{"2026-12-31T10:00:00Z" | date("yyyy-MM-dd")}}"#, ""),
        "2026-12-31"
    );
    assert_eq!(
        render(r#"{"at": {{"2026-12-31" | epoch_seconds}}}"#, ""),
        r#"{"at": 1798675200}"#
    );
}

#[test]
fn a_date_of_the_request_converts_too() {
    let body = r#"{"created": "2026-03-01T08:30:15.250+01:00"}"#;
    assert_eq!(render("{{body.created | epoch_ms}}", body), "1772350215250");
    assert_eq!(
        render(
            r#"{{body.created | date("dd/MM/yyyy HH:mm:ss.SSS")}}"#,
            body
        ),
        "01/03/2026 07:30:15.250"
    );
}

#[test]
fn the_iso_forms_a_client_sends_are_read() {
    let noon = 1_798_718_400_000; // 2026-12-31T12:00:00Z
    for text in [
        "2026-12-31T12:00Z",
        "2026-12-31T12:00:00Z",
        "2026-12-31 12:00:00",
        "2026-12-31t12:00:00.000z",
        "2026-12-31T14:00:00+02:00",
        "2026-12-31T14:00:00+0200",
        "2026-12-31T07:00:00-05",
        "2026-12-31T12:00:00,0Z",
        "  2026-12-31T12:00:00Z  ",
    ] {
        assert_eq!(iso_to_millis(text), Some(noon), "{text}");
    }
    assert_eq!(
        iso_to_millis("2026-12-31T12:00:00.123456789Z"),
        Some(noon + 123)
    );
    assert_eq!(iso_to_millis("1970-01-01"), Some(0));
    assert_eq!(iso_to_millis("1969-12-31T23:59:59Z"), Some(-1000));
}

#[test]
fn what_is_not_a_date_is_refused_and_left_as_it_is() {
    for text in [
        "",
        "2026-02-30",
        "2026-13-01",
        "2026-12-31T24:00:00Z",
        "2026-12-31T12:60",
        "2026-12-31T12:00:00+25:00",
        "2026-12-31T12",
        "2026-12-31X12:00",
        "2026/12/31",
        "31/12/2026",
        "1798711200",
        "2026-12-31T12:00:00.Z",
        "2026-12-31T12:00:00 trailing",
        "2026-12-3é",
    ] {
        assert_eq!(iso_to_millis(text), None, "{text}");
    }
    // In a template, the value comes out unchanged, where it shows in the response.
    assert_eq!(
        render(r#"{{"31/12/2026" | epoch_seconds}}"#, ""),
        "31/12/2026"
    );
    assert_eq!(render(r#"{{"tomorrow" | date("yyyy")}}"#, ""), "tomorrow");
}

#[test]
fn a_pattern_keeps_what_is_not_a_token() {
    assert_eq!(
        format_millis(0, "yyyy-MM-dd'T'HH:mm:ss"),
        "1970-01-01'T'00:00:00"
    );
    assert_eq!(
        format_millis(1_798_718_400_123, "le dd/MM à HHhmm, SSS ms"),
        "le 31/12 à 12h00, 123 ms"
    );
    assert_eq!(
        format_millis(-1, "yyyy-MM-dd HH:mm:ss.SSS"),
        "1969-12-31 23:59:59.999"
    );
}

#[test]
fn a_quoted_text_may_hold_a_pipe() {
    assert_eq!(render(r#"{{"a|b" | upper}}"#, ""), "A|B");
    assert_eq!(render(r#"{{"plain"}}"#, ""), "plain");
}
