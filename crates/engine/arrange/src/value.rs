//! Cell values as the arrangement reads them: their text, numbers, truth, dates, and the order
//! of their text.

use std::cmp::Ordering;

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use serde_json::Value;

/// A cell's text: a link's or a person's label, a list joined with `, `, a number as JavaScript
/// writes it, nothing for an empty cell. Search, sorts and group labels read this text.
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

/// Whether a value counts as set: `false`, `0`, `""`, `"false"`, `[]` and nothing do not.
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
        if let Ok(time) = NaiveDateTime::parse_from_str(text, layout) {
            return Some(time.and_utc().timestamp_millis());
        }
    }
    None
}

/// A text as [`natural_cmp`] orders it: every character lower-cased, once, so a sort of 100,000
/// records compares keys instead of lower-casing them at every comparison.
pub fn sort_key(text: &str) -> Box<str> {
    text.chars().flat_map(char::to_lowercase).collect::<String>().into_boxed_str()
}

/// The order of two [`sort_key`]s: runs of ASCII digits compare as the numbers they write
/// (`2` before `10`, `01` the same as `1`), every other character by its code point. A total
/// order: a run of digits sits where the digits sit among the characters (after `/`, before `:`).
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    loop {
        match (a.get(i), b.get(j)) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let start = (i, j);
                while a.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
                while b.get(j).is_some_and(u8::is_ascii_digit) {
                    j += 1;
                }
                let trim = |digits: &[u8]| -> usize { digits.iter().take_while(|digit| **digit == b'0').count() };
                let (left, right) = (&a[start.0..i], &b[start.1..j]);
                let (left, right) = (&left[trim(left)..], &right[trim(right)..]);
                let order = left.len().cmp(&right.len()).then_with(|| left.cmp(right));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(_), Some(_)) => {
                // UTF-8 keeps the order of code points, byte by byte; a digit byte is never part of
                // another character.
                let order = a[i].cmp(&b[j]);
                if order != Ordering::Equal {
                    return order;
                }
                i += 1;
                j += 1;
            }
        }
    }
}

/// Whether two cells hold the same value for grouping: equal JSON, numbers by their value.
pub fn same_value(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a.unwrap_or(&Value::Null), b.unwrap_or(&Value::Null)) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_value(Some(x), Some(y))),
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|((xk, xv), (yk, yv))| xk == yk && same_value(Some(xv), Some(yv)))
        }
        (x, y) => x == y,
    }
}

/// A value as JavaScript's `JSON.stringify` writes it (numbers as `String(n)` writes them), for
/// group ids.
pub fn js_json(value: Option<&Value>) -> String {
    match value.unwrap_or(&Value::Null) {
        Value::Number(n) => n.as_f64().filter(|n| n.is_finite()).map_or_else(|| "null".to_owned(), number),
        Value::Array(items) => format!("[{}]", items.iter().map(|item| js_json(Some(item))).collect::<Vec<_>>().join(",")),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter().map(|(key, item)| format!("{}:{}", Value::String(key.clone()), js_json(Some(item)))).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn cmp(a: &str, b: &str) -> Ordering {
        natural_cmp(&sort_key(a), &sort_key(b))
    }

    // Ported from web/src/editor/table/lib/tableEditorShared.spec.ts: displayValue's phase 2 ref
    // shapes ("so filter/sort/search/group see link cells as text").
    #[test]
    fn a_link_or_a_person_reads_as_its_label() {
        assert_eq!(display(Some(&json!({ "row_key": "r_1", "display": "Project Alpha" }))), "Project Alpha");
        assert_eq!(display(Some(&json!([{ "row_key": "r_1", "display": "Alpha" }, { "row_key": "r_2", "display": "Beta" }]))), "Alpha, Beta");
        assert_eq!(display(Some(&json!({ "row_key": "r_999" }))), "r_999");
        assert_eq!(display(Some(&json!([{ "id": "u1", "name": "Alice" }, { "id": "u2", "name": "Bob" }]))), "Alice, Bob");
        assert_eq!(display(Some(&json!([]))), "");
    }

    // Ported from tableEditorShared.spec.ts: compareRecordValues (the multi-sort building block).
    #[test]
    fn text_orders_alphabetically_and_numbers_by_value() {
        assert_eq!(cmp("apple", "banana"), Ordering::Less);
        assert_eq!(cmp("banana", "apple"), Ordering::Greater);
        assert_eq!(cmp("apple", "apple"), Ordering::Equal);
        assert_eq!(cmp(&display(Some(&json!(2))), &display(Some(&json!(10)))), Ordering::Less);
        assert_eq!(cmp(&display(Some(&json!(10))), &display(Some(&json!(2)))), Ordering::Greater);
    }

    #[test]
    fn case_is_ignored_and_leading_zeros_do_not_count() {
        assert_eq!(cmp("Apple", "apple"), Ordering::Equal);
        assert_eq!(cmp("item 01", "item 1"), Ordering::Equal);
        assert_eq!(cmp("item 9", "item 10"), Ordering::Less);
        assert_eq!(cmp("", "a"), Ordering::Less);
        assert_eq!(cmp("a1", "ab"), Ordering::Less);
        assert_eq!(cmp("a/", "a1"), Ordering::Less);
        assert_eq!(cmp("a:", "a1"), Ordering::Greater);
    }

    #[test]
    fn the_order_is_total() {
        let texts = ["", "a", "A1", "a01", "a10", "a2", "a/", "a:", "é", "Zeta", "10", "9", "x9y", "x10", "x09z"];
        for a in texts {
            for b in texts {
                assert_eq!(cmp(a, b), cmp(b, a).reverse(), "{a} / {b}");
                for c in texts {
                    if cmp(a, b) != Ordering::Greater && cmp(b, c) != Ordering::Greater {
                        assert_ne!(cmp(a, c), Ordering::Greater, "{a} / {b} / {c}");
                    }
                }
            }
        }
    }

    #[test]
    fn group_ids_write_values_as_javascript_does() {
        assert_eq!(js_json(Some(&json!("Done"))), "\"Done\"");
        assert_eq!(js_json(None), "null");
        assert_eq!(js_json(Some(&json!(3.0))), "3");
        assert_eq!(js_json(Some(&json!(["a", 1.5]))), "[\"a\",1.5]");
        assert!(same_value(Some(&json!(1)), Some(&json!(1.0))));
        assert!(same_value(None, Some(&Value::Null)));
        assert!(!same_value(Some(&json!("1")), Some(&json!(1))));
    }
}
