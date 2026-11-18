//! Cell values as the editor shows them: display text, numbers, dates in a field's format, and the
//! colours of select choices.

use std::cmp::Ordering;

use chrono::{DateTime, NaiveDate, Utc};
use kition_table_file::Field;
use serde_json::Value;

use crate::theme::{Rgba, hex};

/// A value's text, as the editor's `displayValue`: lists joined with commas, objects by their
/// name, title, display, url, href or row key.
pub fn display(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.as_f64().map_or_else(|| number.to_string(), self::number),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Array(items)) => items.iter().map(|item| display(Some(item))).filter(|text| !text.is_empty()).collect::<Vec<_>>().join(", "),
        Some(Value::Object(map)) => ["name", "title", "display", "url", "href", "row_key"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str).filter(|text| !text.is_empty()))
            .unwrap_or_default()
            .to_owned(),
    }
}

/// A number as JavaScript writes it (`String(n)`): integers without a fraction, others in their
/// shortest form.
pub fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value}");
        text.strip_suffix(".0").map_or(text.clone(), str::to_owned)
    }
}

/// Whether a value counts as set (the editor's `isTruthyValue`).
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty() && text != "false",
        Value::Array(items) => !items.is_empty(),
        Value::Object(_) => true,
    }
}

/// The editor's ordering (`localeCompare` with numeric collation, case and accents ignored):
/// runs of digits compare as numbers, everything else letter by letter, ignoring case.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut left = String::new();
                while let Some(c) = a.peek().copied().filter(char::is_ascii_digit) {
                    left.push(c);
                    a.next();
                }
                let mut right = String::new();
                while let Some(c) = b.peek().copied().filter(char::is_ascii_digit) {
                    right.push(c);
                    b.next();
                }
                let (left, right) = (left.trim_start_matches('0'), right.trim_start_matches('0'));
                let order = left.len().cmp(&right.len()).then_with(|| left.cmp(right));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// Unix milliseconds of a date (`2026-11-28`, a date-time with an offset, or one without, read as
/// UTC).
pub fn parse_time(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return date.and_hms_opt(0, 0, 0).map(|time| time.and_utc().timestamp_millis());
    }
    if let Ok(time) = DateTime::parse_from_rfc3339(text) {
        return Some(time.timestamp_millis());
    }
    for layout in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(time) = chrono::NaiveDateTime::parse_from_str(text, layout) {
            return Some(time.and_utc().timestamp_millis());
        }
    }
    None
}

/// A date field's value in its `date_format` (the editor's formats; times in UTC, so a picture is
/// the same on every machine). Text that is no date is shown as it is.
pub fn date(field: &Field, value: Option<&Value>) -> String {
    let millis = match value {
        Some(Value::Number(number)) => number.as_f64().map(|n| n as i64),
        Some(Value::String(text)) => parse_time(text),
        _ => None,
    };
    let Some(time) = millis.and_then(DateTime::<Utc>::from_timestamp_millis) else {
        return display(value);
    };
    let default = if field.field_type == "date" { "year_month_day_slash" } else { "year_month_day_time_slash" };
    let format = field.options.get("date_format").and_then(Value::as_str).unwrap_or(default);
    let pattern = match format {
        "year_month_day_time_slash" => "%Y/%m/%d %H:%M",
        "year_month_day_time_zone_slash" => "%Y/%m/%d %H:%M (GMT+0)",
        "year_month_day_dash" => "%Y-%m-%d",
        "year_month_day_time_dash" => "%Y-%m-%d %H:%M",
        "year_month_day_time_zone_dash" => "%Y-%m-%d %H:%M (GMT+0)",
        "month_day_dash" => "%m-%d",
        "month_day_year_slash" => "%m/%d/%Y",
        "day_month_year_slash" => "%d/%m/%Y",
        _ => "%Y/%m/%d",
    };
    time.format(pattern).to_string()
}

/// A select choice's colours: `(background, text)`.
pub type Tone = (Rgba, Rgba);

/// The editor's choice palette (light theme), in its order.
const TONES: [(&str, Tone); 6] = [
    ("green", (hex(0xD1FAE5), hex(0x065F46))),
    ("blue", (hex(0xE0F2FE), hex(0x075985))),
    ("yellow", (hex(0xFEF3C7), hex(0x92400E))),
    ("red", (hex(0xFFE4E6), hex(0x9F1239))),
    ("purple", (hex(0xE6E0F5), hex(0x5B21B6))),
    ("gray", (hex(0xE4E4E7), hex(0x27272A))),
];

/// The colours of choice `value` of a select field: its `choice_tones` entry, else the editor's
/// hash of the text (each character's first UTF-16 unit added in, modulo the palette).
pub fn tone(field: &Field, value: &str) -> Tone {
    let named = field.options.get("choice_tones").and_then(|tones| tones.get(value)).and_then(Value::as_str);
    if let Some((_, tone)) = named.and_then(|name| TONES.iter().find(|(tone, _)| *tone == name)) {
        return *tone;
    }
    let mut hash = 0usize;
    for c in value.chars() {
        // `charCodeAt(0)` of each code point: its first UTF-16 unit.
        let mut units = [0u16; 2];
        let first = c.encode_utf16(&mut units).first().copied().unwrap_or_default();
        hash = (hash + usize::from(first)) % TONES.len();
    }
    TONES.get(hash).map_or(TONES[5].1, |(_, tone)| *tone)
}

/// A select value's choices, as a list.
pub fn choices(value: Option<&Value>) -> Vec<String> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(|item| display(Some(item))).filter(|text| !text.is_empty()).collect(),
        Some(Value::String(text)) if text.contains(',') => {
            text.split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect()
        }
        Some(other) => {
            let text = display(Some(other));
            if text.is_empty() { Vec::new() } else { vec![text] }
        }
    }
}

/// The text a cell of `field` shows, for cells drawn as text.
pub fn cell_text(field: &Field, value: Option<&Value>) -> String {
    match field.field_type.as_str() {
        "date" | "datetime" | "created_time" | "last_modified_time" => date(field, value),
        _ => display(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_reads_numbers() {
        let mut items = vec!["Item 10", "item 9", "Item 1", "", "48000", "9000"];
        items.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(items, vec!["", "9000", "48000", "Item 1", "item 9", "Item 10"]);
    }

    #[test]
    fn numbers_and_dates_read_as_the_editor_writes_them() {
        assert_eq!(number(48000.0), "48000");
        assert_eq!(number(24.5), "24.5");
        let field = Field { field_type: "date".into(), ..Field::default() };
        assert_eq!(date(&field, Some(&Value::from("2026-11-28"))), "2026/11/28");
        let mut dashed = field.clone();
        dashed.options.insert("date_format".into(), Value::from("year_month_day_dash"));
        assert_eq!(date(&dashed, Some(&Value::from("2026-11-28"))), "2026-11-28");
        assert_eq!(date(&field, Some(&Value::from("soon"))), "soon");
    }

    #[test]
    fn choices_take_the_editor_tones() {
        let field = Field::default();
        // "A" is 65: 65 % 6 = 5, gray.
        assert_eq!(tone(&field, "A"), TONES[5].1);
        let mut toned = field.clone();
        toned.options.insert("choice_tones".into(), serde_json::json!({ "A": "green" }));
        assert_eq!(tone(&toned, "A"), TONES[0].1);
    }
}
