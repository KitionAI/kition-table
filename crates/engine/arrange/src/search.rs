//! The editor's search: whether a record shows for a search, and which of its cells hold it.
//!
//! A cell holds the search when its text ([`display`]), lower-cased, contains the search,
//! lower-cased and trimmed. A record shows when any of its table's fields holds it.
//!
//! A whole arrangement tests every record of the table, so it reads each record's values as the
//! JSON text the file stores and never builds them as values: [`Search::matches_json`] first
//! looks for the search in the text itself (a record whose text does not hold it cannot show,
//! when nothing in the search could be written differently in JSON: see [`Search::new`]), then
//! reads the cells' text straight from the JSON, stopping at the first that holds it.

use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt;

use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use crate::value::{display, number};

/// A search over the fields of one table.
#[derive(Clone, Debug)]
pub struct Search {
    /// Lower-cased and trimmed; never empty.
    needle: String,
    /// The fields' names: a record shows when one of them holds the search.
    names: HashSet<String>,
    /// Whether a record whose JSON text does not hold the search can be left out unread.
    text_first: bool,
}

impl Search {
    /// A search for `query` over the fields `names`; `None` for a blank query (every record shows).
    pub fn new(query: &str, names: impl IntoIterator<Item = String>) -> Option<Self> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return None;
        }
        // A cell's text is in its record's JSON text as it is, except: a list's items are joined
        // with ", ", a number may be written otherwise (an exponent: `1e21`), and a string may
        // hold escapes. A search without commas, digits, `.`, `-`, quotes, backslashes or control
        // characters is found in the JSON text whenever it is in a cell's text (escaped records
        // are read whole).
        let text_first = !needle.chars().any(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '-' | '"' | '\\') || c.is_control());
        Some(Self { needle, names: names.into_iter().collect(), text_first })
    }

    /// Whether a cell's text holds the search.
    fn holds_text(&self, text: &str) -> bool {
        contains_lowercase(text, &self.needle)
    }

    /// Whether the cell `value` holds the search.
    pub fn holds(&self, value: Option<&Value>) -> bool {
        self.holds_text(&display(value))
    }

    /// Whether a record with `values` shows.
    pub fn matches(&self, values: &Map<String, Value>) -> bool {
        values.iter().any(|(name, value)| self.names.contains(name) && self.holds(Some(value)))
    }

    /// Whether a record whose values the file stores as the JSON text `json` shows: the same
    /// answer as [`Search::matches`] on the values it holds.
    pub fn matches_json(&self, json: &str) -> bool {
        if self.text_first && !json.contains('\\') && !contains_lowercase(json, &self.needle) {
            return false;
        }
        let mut reader = serde_json::Deserializer::from_str(json);
        reader.deserialize_map(Record(self)).unwrap_or(false)
    }
}

/// Whether `text`, lower-cased, contains `needle` (lower-cased already).
fn contains_lowercase(text: &str, needle: &str) -> bool {
    // Lower-casing turns no character but these two into ASCII: the KELVIN SIGN into `k`, and
    // the dotted capital I into `i` and a combining dot. Without them, an ASCII needle is in the
    // lower-cased text where it is in the text's ASCII, case ignored, byte by byte.
    const KELVIN: &str = "\u{212A}";
    const DOTTED_I: &str = "\u{130}";
    if needle.is_ascii() && (text.is_ascii() || !(text.contains(KELVIN) || text.contains(DOTTED_I))) {
        return contains_ascii_case(text.as_bytes(), needle.as_bytes());
    }
    text.to_lowercase().contains(needle)
}

/// Whether `text` contains the lower-case ASCII `needle`, ASCII case ignored.
fn contains_ascii_case(text: &[u8], needle: &[u8]) -> bool {
    let Some((&first, rest)) = needle.split_first() else { return true };
    if text.len() < needle.len() {
        return false;
    }
    (0..=text.len() - needle.len()).any(|at| text[at].to_ascii_lowercase() == first && text[at + 1..at + needle.len()].eq_ignore_ascii_case(rest))
}

/// A record's values: whether one of the searched fields holds the search.
struct Record<'s>(&'s Search);

impl<'de> Visitor<'de> for Record<'_> {
    type Value = bool;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a record's values")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<bool, A::Error> {
        let mut found = false;
        while let Some(name) = map.next_key::<Cow<'de, str>>()? {
            if found || !self.0.names.contains(name.as_ref()) {
                map.next_value::<IgnoredAny>()?;
            } else {
                found = self.0.holds_text(&map.next_value_seed(Text)?);
            }
        }
        Ok(found)
    }
}

/// A cell's text ([`display`]) read from its JSON.
struct Text;

impl<'de> DeserializeSeed<'de> for Text {
    type Value = Cow<'de, str>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Text {
    type Value = Cow<'de, str>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a cell")
    }
    fn visit_borrowed_str<E>(self, text: &'de str) -> Result<Self::Value, E> {
        Ok(Cow::Borrowed(text))
    }
    fn visit_str<E>(self, text: &str) -> Result<Self::Value, E> {
        Ok(Cow::Owned(text.to_owned()))
    }
    fn visit_string<E>(self, text: String) -> Result<Self::Value, E> {
        Ok(Cow::Owned(text))
    }
    fn visit_bool<E>(self, flag: bool) -> Result<Self::Value, E> {
        Ok(Cow::Borrowed(if flag { "true" } else { "false" }))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Cow::Owned(number(value as f64)))
    }
    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Cow::Owned(number(value as f64)))
    }
    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(Cow::Owned(number(value)))
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Cow::Borrowed(""))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Self::Value, A::Error> {
        let mut joined = String::new();
        while let Some(item) = items.next_element_seed(Text)? {
            if !item.is_empty() {
                if !joined.is_empty() {
                    joined.push_str(", ");
                }
                joined.push_str(&item);
            }
        }
        Ok(Cow::Owned(joined))
    }
    /// An object's label: its first non-empty text of these keys, in this order.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        const LABELS: [&str; 6] = ["name", "title", "display", "url", "href", "row_key"];
        let mut best: Option<(usize, Cow<'de, str>)> = None;
        while let Some(key) = map.next_key::<Cow<'de, str>>()? {
            match LABELS.iter().position(|label| *label == key.as_ref()) {
                Some(rank) => {
                    if let Some(text) = map.next_value_seed(Label)?.filter(|text| !text.is_empty())
                        && best.as_ref().is_none_or(|(known, _)| rank < *known)
                    {
                        best = Some((rank, text));
                    }
                }
                None => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(best.map_or(Cow::Borrowed(""), |(_, text)| text))
    }
}

/// A label's text: a string, or nothing for any other value.
struct Label;

impl<'de> DeserializeSeed<'de> for Label {
    type Value = Option<Cow<'de, str>>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Label {
    type Value = Option<Cow<'de, str>>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a value")
    }
    fn visit_borrowed_str<E>(self, text: &'de str) -> Result<Self::Value, E> {
        Ok(Some(Cow::Borrowed(text)))
    }
    fn visit_str<E>(self, text: &str) -> Result<Self::Value, E> {
        Ok(Some(Cow::Owned(text.to_owned())))
    }
    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Self::Value, A::Error> {
        while items.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use serde_json::json;

    fn search(query: &str) -> Search {
        Search::new(query, ["a", "b", "c"].map(str::to_owned)).unwrap()
    }

    /// Whether the record's JSON text and its parsed values give the same answer, and which.
    fn both(query: &str, values: &Value) -> bool {
        let search = search(query);
        let parsed = search.matches(values.as_object().unwrap());
        assert_eq!(search.matches_json(&values.to_string()), parsed, "{query:?} in {values}");
        parsed
    }

    #[test]
    fn a_blank_search_is_none() {
        assert!(Search::new("  ", ["a".to_owned()]).is_none());
    }

    #[test]
    fn a_record_shows_when_one_of_the_fields_text_holds_the_search_case_ignored() {
        assert!(both("APOLLO", &json!({ "a": "Project Apollo" })));
        assert!(both("urgent", &json!({ "b": ["red", "Urgent"] })));
        assert!(!both("zeta", &json!({ "a": "alpha", "b": 1 })));
        // Only the fields searched: another key holding it does not count.
        assert!(!both("alpha", &json!({ "z": "alpha" })));
        assert!(!both("a", &json!({})));
    }

    #[test]
    fn the_text_read_from_json_is_the_cell_s_text() {
        // Numbers as JavaScript writes them, whatever the JSON says.
        assert!(both("1000000000000000000000", &json!({ "a": 1e21 })));
        assert!(both("0.0000001", &json!({ "a": 1e-7 })));
        assert!(both("5", &json!({ "a": 5.0 })));
        assert!(!both("5.0", &json!({ "a": 5.0 })));
        assert!(both("-3", &json!({ "a": -3 })));
        // Lists joined with ", ", empty items left out; nested lists too.
        assert!(both("red, blue", &json!({ "a": ["red", "", null, "blue"] })));
        assert!(!both("red, , blue", &json!({ "a": ["red", "", "blue"] })));
        assert!(both("x, y", &json!({ "a": [["x"], ["y"]] })));
        // A person or a link: its label, by the first key in order that has one.
        assert!(both("ada", &json!({ "a": [{ "id": "u1", "name": "Ada" }] })));
        assert!(!both("u1", &json!({ "a": [{ "id": "u1", "name": "Ada" }] })));
        assert!(both("alpha", &json!({ "a": { "row_key": "r_1", "display": "Alpha" } })));
        assert!(!both("r_1", &json!({ "a": { "row_key": "r_1", "display": "Alpha" } })));
        assert!(both("r_9", &json!({ "a": { "row_key": "r_9", "display": "" } })));
        assert!(both("deck", &json!({ "a": { "url": "https://x", "title": 3, "name": "Deck.png" } })));
        assert!(!both("3", &json!({ "a": { "title": 3, "nested": { "name": "3" } } })));
        // Flags, nothing, escapes, and text that lower-cases otherwise.
        assert!(both("true", &json!({ "c": true })));
        assert!(!both("null", &json!({ "c": null })));
        assert!(both("say \"hi\"", &json!({ "a": "They say \"hi\"" })));
        assert!(both("a\\b", &json!({ "a": "a\\b" })));
        assert!(both("line\ntwo", &json!({ "a": "line\ntwo" })));
        assert!(both("k", &json!({ "a": "\u{212A}elvin" })));
        assert!(both("i\u{307}", &json!({ "a": "\u{130}stanbul" })));
        assert!(both("ς", &json!({ "a": "ΟΔΟΣ" })));
        assert!(both("项目", &json!({ "b": ["项目进度"] })));
        assert!(both("😀", &json!({ "b": "go 😀" })));
    }

    #[test]
    fn escaped_json_text_is_read_whole() {
        // A writer that escapes every non-ASCII character: the text holds no "é", the cell does.
        let accent = search("é");
        assert!(accent.matches_json(r#"{"a":"caf\u00e9"}"#));
        assert!(search("CAFÉ").matches_json(r#"{"a":"caf\u00e9"}"#));
        assert!(!accent.matches_json(r#"{"a":"cafe"}"#));
        assert!(!accent.matches_json("not json"));
    }
}
