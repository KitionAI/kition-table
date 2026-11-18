//! Cell values as the editor shows them: display text, numbers, dates in a field's format, and the
//! colours of select choices.

use std::cmp::Ordering;

use chrono::{DateTime, NaiveDate, Utc};
use kition_table_file::Field;
use serde_json::Value;

use crate::theme::{Mode, Rgba, Theme, hex};

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

/// Where dates are read: the UTC offset (minutes east) from each instant (Unix milliseconds) on,
/// oldest first, as the page's time zone has them; none is UTC (a picture, the same on every
/// machine). An instant before the first change takes the first offset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Zone {
    pub changes: Vec<(i64, i32)>,
}

impl Zone {
    /// The offset (minutes east of UTC) at `millis`.
    pub fn offset_at(&self, millis: i64) -> i32 {
        let after = self.changes.partition_point(|(at, _)| *at <= millis);
        self.changes.get(after.saturating_sub(1)).map_or(0, |(_, minutes)| *minutes)
    }
}

/// `GMT+8`, `GMT-3:30`, `GMT+0`: the label of an offset, as the editor writes it.
fn zone_label(minutes: i32) -> String {
    let sign = if minutes >= 0 { '+' } else { '-' };
    let (hours, rest) = (minutes.unsigned_abs() / 60, minutes.unsigned_abs() % 60);
    if rest == 0 { format!("GMT{sign}{hours}") } else { format!("GMT{sign}{hours}:{rest:02}") }
}

/// A date field's value in its `date_format` (the editor's formats), in UTC. Text that is no date
/// is shown as it is.
pub fn date(field: &Field, value: Option<&Value>) -> String {
    date_in(field, value, &Zone::default())
}

/// A date field's value in its `date_format`, read in `zone`: an instant (a number, or text with
/// its offset) shows its wall time there; a calendar day and a date-time without an offset show as
/// they are written.
pub fn date_in(field: &Field, value: Option<&Value>, zone: &Zone) -> String {
    let (millis, instant) = match value {
        Some(Value::Number(number)) => (number.as_f64().map(|n| n as i64), true),
        Some(Value::String(text)) => (parse_time(text), DateTime::parse_from_rfc3339(text.trim()).is_ok()),
        _ => (None, false),
    };
    let offset = match millis {
        Some(millis) if instant => zone.offset_at(millis),
        _ => 0,
    };
    let Some(time) = millis.and_then(|millis| DateTime::<Utc>::from_timestamp_millis(millis + i64::from(offset) * 60_000)) else {
        return display(value);
    };
    let default = if field.field_type == "date" { "year_month_day_slash" } else { "year_month_day_time_slash" };
    let format = field.options.get("date_format").and_then(Value::as_str).unwrap_or(default);
    let pattern = match format {
        "year_month_day_time_slash" => "%Y/%m/%d %H:%M",
        "year_month_day_time_zone_slash" => "%Y/%m/%d %H:%M (ZONE)",
        "year_month_day_dash" => "%Y-%m-%d",
        "year_month_day_time_dash" => "%Y-%m-%d %H:%M",
        "year_month_day_time_zone_dash" => "%Y-%m-%d %H:%M (ZONE)",
        "month_day_dash" => "%m-%d",
        "month_day_year_slash" => "%m/%d/%Y",
        "day_month_year_slash" => "%d/%m/%Y",
        _ => "%Y/%m/%d",
    };
    time.format(pattern).to_string().replace("ZONE", &zone_label(offset))
}

/// A select choice's colours: `(background, text)`.
pub type Tone = (Rgba, Rgba);

/// The editor's choice palette (light theme), in its order.
/// The select palette, light and dark (the editor's tones, `selectPalette`).
const TONES: [(&str, Tone, Tone); 6] = [
    ("green", (hex(0xD1FAE5), hex(0x065F46)), (hex(0x064E3B), hex(0xA7F3D0))),
    ("blue", (hex(0xE0F2FE), hex(0x075985)), (hex(0x0C4A6E), hex(0xBAE6FD))),
    ("yellow", (hex(0xFEF3C7), hex(0x92400E)), (hex(0x78350F), hex(0xFDE68A))),
    ("red", (hex(0xFFE4E6), hex(0x9F1239)), (hex(0x881337), hex(0xFECDD3))),
    ("purple", (hex(0xE6E0F5), hex(0x5B21B6)), (hex(0x4C1D95), hex(0xDDD6FE))),
    ("gray", (hex(0xE4E4E7), hex(0x27272A)), (hex(0x3F3F46), hex(0xF4F4F5))),
];

/// The colours of choice `value` of a select field in `theme`: a value the field's choices do not
/// list (every value of a field that lists none) is neutral, the theme's chip, as the old grid
/// drew a value it found no choice for; a listed one takes its `choice_tones` entry, else the
/// editor's hash of the text (each character's first UTF-16 unit added in, modulo the palette),
/// in the palette of the theme's mode.
pub fn tone(field: &Field, value: &str, theme: &Theme) -> Tone {
    let listed = match field.options.get("choices") {
        Some(Value::Array(items)) => items.iter().any(|item| display(Some(item)).trim() == value),
        _ => false,
    };
    if !listed {
        return (theme.chip, theme.text);
    }
    let pick = |entry: &(&str, Tone, Tone)| if theme.mode == Mode::Dark { entry.2 } else { entry.1 };
    let named = field.options.get("choice_tones").and_then(|tones| tones.get(value)).and_then(Value::as_str);
    if let Some(entry) = named.and_then(|name| TONES.iter().find(|(tone, _, _)| *tone == name)) {
        return pick(entry);
    }
    let mut hash = 0usize;
    for c in value.chars() {
        // `charCodeAt(0)` of each code point: its first UTF-16 unit.
        let mut units = [0u16; 2];
        let first = c.encode_utf16(&mut units).first().copied().unwrap_or_default();
        hash = (hash + usize::from(first)) % TONES.len();
    }
    pick(TONES.get(hash).unwrap_or(&TONES[5]))
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

/// The text a cell of `field` shows, for cells drawn as text (dates in UTC).
pub fn cell_text(field: &Field, value: Option<&Value>) -> String {
    cell_text_in(field, value, &Zone::default())
}

/// The text a cell of `field` shows, for cells drawn as text, its dates read in `zone`.
pub fn cell_text_in(field: &Field, value: Option<&Value>, zone: &Zone) -> String {
    match field.field_type.as_str() {
        "date" | "datetime" | "created_time" | "last_modified_time" => date_in(field, value, zone),
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
        let mut field = Field::default();
        field.options.insert("choices".into(), serde_json::json!(["A"]));
        // "A" is 65: 65 % 6 = 5, gray.
        assert_eq!(tone(&field, "A", &Theme::LIGHT), TONES[5].1);
        let mut toned = field.clone();
        toned.options.insert("choice_tones".into(), serde_json::json!({ "A": "green" }));
        assert_eq!(tone(&toned, "A", &Theme::LIGHT), TONES[0].1);
    }

    /// The editor reads dates in the person's time zone (the old grid formatted them with the
    /// browser's): an instant shows its wall time there, with its offset as the zone formats
    /// write it; a calendar day and a date-time without an offset show as written; a picture
    /// (no zone) shows UTC.
    #[test]
    fn dates_read_in_the_page_s_zone() {
        let mut field = Field { field_type: "datetime".into(), ..Field::default() };
        let shanghai = Zone { changes: vec![(i64::MIN, 480)] };
        let instant = Value::String("2026-07-23T04:14:47Z".into());
        assert_eq!(date_in(&field, Some(&instant), &shanghai), "2026/07/23 12:14");
        assert_eq!(date(&field, Some(&instant)), "2026/07/23 04:14");
        assert_eq!(date_in(&field, Some(&Value::String("2026-07-23T04:14".into())), &shanghai), "2026/07/23 04:14");
        // Daylight saving time: the offset of each instant.
        let new_york = Zone { changes: vec![(i64::MIN, -300), (1_772_953_200_000, -240), (1_793_512_800_000, -300)] };
        assert_eq!(date_in(&field, Some(&Value::String("2026-01-15T12:00:00Z".into())), &new_york), "2026/01/15 07:00");
        assert_eq!(date_in(&field, Some(&Value::String("2026-07-15T12:00:00Z".into())), &new_york), "2026/07/15 08:00");
        field.options.insert("date_format".into(), Value::String("year_month_day_time_zone_dash".into()));
        assert_eq!(date_in(&field, Some(&instant), &Zone { changes: vec![(i64::MIN, 330)] }), "2026-07-23 09:44 (GMT+5:30)");
        assert_eq!(date(&field, Some(&instant)), "2026-07-23 04:14 (GMT+0)");
        let day = Field { field_type: "date".into(), ..Field::default() };
        assert_eq!(date_in(&day, Some(&Value::String("2026-11-28".into())), &new_york), "2026/11/28");
    }

    /// A select field that lists no choices draws every value neutral, in both themes, whatever
    /// `choice_tones` says: the old grid found no choice for the value and fell back to the
    /// theme's chip (case regressions/issue-select-without-choices).
    #[test]
    fn a_select_without_choices_draws_its_values_neutral() {
        let mut field = Field { field_type: "single_select".into(), ..Field::default() };
        for theme in [Theme::LIGHT, Theme::DARK] {
            assert_eq!(tone(&field, "Lead", &theme), (theme.chip, theme.text));
            assert_eq!(tone(&field, "A", &theme), (theme.chip, theme.text));
        }
        field.options.insert("choices".into(), serde_json::json!([]));
        assert_eq!(tone(&field, "Lead", &Theme::LIGHT), (Theme::LIGHT.chip, Theme::LIGHT.text), "an empty list lists nothing");
        field.options.insert("choice_tones".into(), serde_json::json!({ "Lead": "green" }));
        assert_eq!(tone(&field, "Lead", &Theme::LIGHT), (Theme::LIGHT.chip, Theme::LIGHT.text), "a tone for a value no choice lists");
    }

    /// The old grid's selectChoiceColors.spec: a tone in the dark theme is the dark palette's, and
    /// a value the field's choices do not list is neutral.
    #[test]
    fn choices_are_toned_for_the_theme_and_unlisted_values_are_neutral() {
        let mut field = Field::default();
        field.options.insert("choices".into(), serde_json::json!(["Ready", "Done"]));
        field.options.insert("choice_tones".into(), serde_json::json!({ "Ready": "red" }));
        assert_eq!(tone(&field, "Ready", &Theme::LIGHT), (hex(0xFFE4E6), hex(0x9F1239)));
        assert_eq!(tone(&field, "Ready", &Theme::DARK), (hex(0x881337), hex(0xFECDD3)));
        assert_eq!(tone(&field, "Later", &Theme::LIGHT), (Theme::LIGHT.chip, Theme::LIGHT.text));
        assert_eq!(tone(&field, "Later", &Theme::DARK), (Theme::DARK.chip, Theme::DARK.text));
    }
}
