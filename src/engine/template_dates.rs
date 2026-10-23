// Dates in templates: the pipes epoch_ms, epoch_seconds, epoch_minutes and date("pattern") read an ISO 8601 date or
// date-time (`2026-12-31`, `2026-12-31T10:00:00Z`, `2026-12-31 10:00:00.250+02:00`), so that a response can hold a date
// in the unit a client expects without converting it by hand. A date-time without an offset is UTC, as now_iso is.
// Nothing is guessed: a value that is not such a date comes out of the pipe unchanged, where it shows in the response,
// rather than as an empty or wrong number. The calendar arithmetic is template.rs's (civil_from_days, days_from_civil).
use crate::engine::template::{civil_from_days, days_from_civil};

/// Milliseconds since 1970-01-01T00:00:00Z of an ISO 8601 date or date-time; `None` when `text` is not one, or names
/// a day or a time that does not exist.
pub(crate) fn iso_to_millis(text: &str) -> Option<i64> {
    let text = text.trim();
    let date = text.get(..10)?;
    let (year, month, day) = (
        number(date, 0, 4)?,
        number(date, 5, 2)?,
        number(date, 8, 2)?,
    );
    if date.as_bytes()[4] != b'-'
        || date.as_bytes()[7] != b'-'
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
    {
        return None;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    // A day past the end of its month (February 30) does not come back as the same date.
    if civil_from_days(days) != (year, month as u32, day as u32) {
        return None;
    }
    let rest = &text[10..];
    let (time_of_day, offset_minutes) = if rest.is_empty() {
        (0, 0)
    } else {
        let rest = rest.strip_prefix(['T', 't', ' '])?;
        time_and_offset(rest)?
    };
    (days * 86_400_000 + time_of_day).checked_sub(offset_minutes * 60_000)
}

/// `millis` (since 1970, UTC) written with `pattern`: yyyy, MM, dd, HH, mm, ss and SSS are replaced, anything else is
/// kept, the same tokens as parse_date in scripts.
pub(crate) fn format_millis(millis: i64, pattern: &str) -> String {
    let days = millis.div_euclid(86_400_000);
    let time = millis.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut rest = pattern;
    while !rest.is_empty() {
        let (token, value) = [
            ("yyyy", format!("{year:04}")),
            ("MM", format!("{month:02}")),
            ("dd", format!("{day:02}")),
            ("HH", format!("{:02}", time / 3_600_000)),
            ("mm", format!("{:02}", time / 60_000 % 60)),
            ("ss", format!("{:02}", time / 1000 % 60)),
            ("SSS", format!("{:03}", time % 1000)),
        ]
        .into_iter()
        .find(|(token, _)| rest.starts_with(token))
        .unwrap_or_default();
        if token.is_empty() {
            let c = rest.chars().next().unwrap_or_default();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        } else {
            out.push_str(&value);
            rest = &rest[token.len()..];
        }
    }
    out
}

// `HH:MM`, then optionally `:SS` and a fraction, then optionally `Z` or an offset (`+02:00`, `+0200`, `-05`).
fn time_and_offset(text: &str) -> Option<(i64, i64)> {
    let (hour, minute) = (number(text, 0, 2)?, number(text, 3, 2)?);
    if text.as_bytes().get(2) != Some(&b':') || hour > 23 || minute > 59 {
        return None;
    }
    let mut rest = &text[5..];
    let mut second = 0;
    let mut millis = 0;
    if let Some(after) = rest.strip_prefix(':') {
        second = number(after, 0, 2)?;
        if second > 59 {
            return None;
        }
        rest = &after[2..];
        if let Some(fraction) = rest.strip_prefix(['.', ',']) {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            // Milliseconds: the first three digits, the rest dropped (an ISO fraction may hold nanoseconds).
            let first_three = format!("{:0<3}", &fraction[..digits.min(3)]);
            millis = first_three.parse().ok()?;
            rest = &fraction[digits..];
        }
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits = rest[1..].replace(':', "");
            if !(digits.len() == 2 || digits.len() == 4) {
                return None;
            }
            let (hours, minutes) = (
                number(&digits, 0, 2)?,
                number(&digits, 2, digits.len() - 2).unwrap_or(0),
            );
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 60 + minutes)
        }
    };
    Some((
        hour * 3_600_000 + minute * 60_000 + second * 1000 + millis,
        offset,
    ))
}

// The `width` ASCII digits of `text` at `at`, as a number; `None` when one is missing or is not a digit.
fn number(text: &str, at: usize, width: usize) -> Option<i64> {
    if width == 0 {
        return None;
    }
    let digits = text.get(at..at + width)?;
    digits
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| digits.parse().ok())?
}

#[cfg(test)]
mod tests;
