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

// The calendar as it is written, apart from the arithmetic of days_from_civil: the Gregorian leap year rule, the
// length of each month, and a day count walked year by year from 1970.
fn leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn month_length(year: i64, month: i64) -> i64 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn first_of_january(year: i64) -> i64 {
    let length = |y| if leap(y) { 366 } else { 365 };
    if year >= 1970 {
        (1970..year).map(length).sum()
    } else {
        -(year..1970).map(length).sum::<i64>()
    }
}

fn days_since_1970(year: i64, month: i64, day: i64) -> i64 {
    first_of_january(year) + (1..month).map(|m| month_length(year, m)).sum::<i64>() + day - 1
}

// Every month and day of two digits, in years where each leap rule decides (divisible by 4, by 100, by 400, neither),
// and at both ends of four digits: a date is read when it exists, at its own day, and refused otherwise.
#[test]
fn every_date_of_two_digit_month_and_day_is_read_exactly_when_it_exists() {
    let years = [
        0, 1, 4, 100, 1600, 1900, 1969, 1970, 1972, 2000, 2023, 2024, 2100, 2400, 9999,
    ];
    for year in years {
        for month in 0..=99 {
            for day in 0..=99 {
                let text = format!("{year:04}-{month:02}-{day:02}");
                let exists =
                    (1..=12).contains(&month) && (1..=month_length(year, month)).contains(&day);
                let expected = exists.then(|| days_since_1970(year, month, day) * 86_400_000);
                assert_eq!(iso_to_millis(&text), expected, "{text}");
            }
        }
    }
}

// Generated date-times in every form the reader takes: date alone, or a time with or without seconds and a fraction
// of 1 to 9 digits, after `T`, `t` or a space, then no offset, `Z`, `z`, `±HH`, `±HHMM` or `±HH:MM`, with blanks
// around. Each is read as the instant computed here from its parts, and written back by format_millis in UTC, then
// read again, to the same instant.
#[test]
fn generated_date_times_are_read_at_the_instant_their_parts_name() {
    let mut rng = fastrand::Rng::with_seed(0x6d69_6d69_6377_6179);
    for _ in 0..4096 {
        let (year, month) = (rng.i64(0..=9999), rng.i64(1..=12));
        let day = rng.i64(1..=month_length(year, month));
        let mut text = format!("{year:04}-{month:02}-{day:02}");
        let mut millis = days_since_1970(year, month, day) * 86_400_000;
        if rng.u8(..4) != 0 {
            let (hour, minute) = (rng.i64(0..24), rng.i64(0..60));
            text.push(['T', 't', ' '][rng.usize(..3)]);
            text += &format!("{hour:02}:{minute:02}");
            millis += hour * 3_600_000 + minute * 60_000;
            if rng.bool() {
                let second = rng.i64(0..60);
                text += &format!(":{second:02}");
                millis += second * 1000;
                if rng.bool() {
                    let fraction: String = (0..rng.usize(1..=9)).map(|_| rng.digit(10)).collect();
                    text.push(['.', ','][rng.usize(..2)]);
                    text += &fraction;
                    millis += format!("{:0<3}", &fraction[..fraction.len().min(3)])
                        .parse::<i64>()
                        .unwrap();
                }
            }
            let (sign, hours, minutes) =
                (rng.choice([1, -1]).unwrap(), rng.i64(0..24), rng.i64(0..60));
            let sign_char = if sign == 1 { '+' } else { '-' };
            match rng.u8(..6) {
                0 => {}
                1 => text.push(['Z', 'z'][rng.usize(..2)]),
                2 => {
                    text += &format!("{sign_char}{hours:02}");
                    millis -= sign * hours * 3_600_000;
                }
                3 => {
                    text += &format!("{sign_char}{hours:02}{minutes:02}");
                    millis -= sign * (hours * 60 + minutes) * 60_000;
                }
                _ => {
                    text += &format!("{sign_char}{hours:02}:{minutes:02}");
                    millis -= sign * (hours * 60 + minutes) * 60_000;
                }
            }
        }
        let blanks = ["", " ", "\t", "  \n"];
        let text = format!("{}{text}{}", blanks[rng.usize(..4)], blanks[rng.usize(..4)]);
        assert_eq!(iso_to_millis(&text), Some(millis), "{text:?}");
        let written = format_millis(millis, "yyyy-MM-ddTHH:mm:ss.SSSZ");
        if (0..=9999).contains(&written[..written.len() - 20].parse::<i64>().unwrap_or(-1)) {
            assert_eq!(
                iso_to_millis(&written),
                Some(millis),
                "{text:?} written {written}"
            );
        }
    }
}

// Each separator of `2026-12-31T12:00:00.250` replaced by every other printable ASCII character: the date-time is
// refused, wherever the wrong one stands.
#[test]
fn a_date_time_with_one_wrong_separator_is_refused() {
    let text = "2026-12-31T12:00:00.250";
    assert!(iso_to_millis(text).is_some());
    let separators: [(usize, &[u8]); 6] = [
        (4, b"-"),
        (7, b"-"),
        (10, b"Tt "),
        (13, b":"),
        (16, b":"),
        (19, b".,"),
    ];
    for (at, allowed) in separators {
        for other in (b' '..=b'~').filter(|c| !allowed.contains(c)) {
            let mut wrong = text.as_bytes().to_vec();
            wrong[at] = other;
            let wrong = String::from_utf8(wrong).unwrap();
            assert_eq!(iso_to_millis(&wrong), None, "{wrong:?}");
        }
    }
}

// Every hour and minute of two digits, alone in a time and in each form of offset (`±HH:MM`, `±HHMM`, `±HH`): a time
// is read when its hour is below 24 and its minute below 60, as is an offset, which moves the instant by its own
// length; anything else is refused.
#[test]
fn every_two_digit_hour_and_minute_of_a_time_or_an_offset_is_read_exactly_when_it_exists() {
    let midnight = days_since_1970(2026, 12, 31) * 86_400_000;
    for first in 0..=99 {
        for second in 0..=99 {
            let exists = first < 24 && second < 60;
            let time = format!("2026-12-31T{first:02}:{second:02}");
            let expected = exists.then(|| midnight + (first * 60 + second) * 60_000);
            assert_eq!(iso_to_millis(&time), expected, "{time}");
            for (sign_char, sign) in [('+', 1), ('-', -1)] {
                let shift = |length: i64| exists.then(|| midnight - sign * length * 60_000);
                for offset in [
                    format!("{sign_char}{first:02}:{second:02}"),
                    format!("{sign_char}{first:02}{second:02}"),
                ] {
                    let text = format!("2026-12-31T00:00{offset}");
                    assert_eq!(iso_to_millis(&text), shift(first * 60 + second), "{text}");
                }
                let text = format!("2026-12-31T00:00{sign_char}{first:02}");
                let expected = (first < 24).then(|| midnight - sign * first * 3_600_000);
                assert_eq!(iso_to_millis(&text), expected, "{text}");
            }
        }
    }
    let second_of = |s: i64| iso_to_millis(&format!("2026-12-31T00:00:{s:02}"));
    for s in 0..=99 {
        assert_eq!(
            second_of(s),
            (s < 60).then(|| midnight + s * 1000),
            "second {s}"
        );
    }
}

// An offset is `±HH`, `±HHMM` or `±HH:MM`, nothing else: `+02:00` with one character replaced by any other printable
// ASCII one, or with a colon added anywhere, is refused, except where the result is itself one of those forms.
#[test]
fn an_offset_in_any_other_form_is_refused() {
    let base = "2026-12-31T12:00:00+02:00";
    assert!(iso_to_millis(base).is_some());
    let forms = regex::Regex::new(r"^[+-]([0-9]{2}|[0-9]{4}|[0-9]{2}:[0-9]{2})$").unwrap();
    let at = base.len() - 6;
    let mut variants = Vec::new();
    for position in at..base.len() {
        for other in b' '..=b'~' {
            let mut changed = base.as_bytes().to_vec();
            changed[position] = other;
            variants.push(String::from_utf8(changed).unwrap());
        }
    }
    for position in at + 1..=base.len() {
        variants.push(format!("{}:{}", &base[..position], &base[position..]));
    }
    let mut refused = 0;
    for text in variants {
        let offset = &text[at..];
        // A sign replaced by `Z`, a digit or a separator of the seconds makes another time, not another offset.
        if !offset.starts_with(['+', '-']) {
            continue;
        }
        let readable = forms.is_match(offset) && {
            let digits: String = offset[1..].chars().filter(char::is_ascii_digit).collect();
            digits[..2].parse::<u32>().unwrap() < 24
                && digits
                    .get(2..)
                    .is_none_or(|m| m.parse::<u32>().unwrap() < 60)
        };
        assert_eq!(iso_to_millis(&text).is_some(), readable, "{text:?}");
        refused += usize::from(!readable);
    }
    assert!(refused > 400, "{refused}");
}
