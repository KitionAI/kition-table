//! What a record's cells and a field's choices hold in the document, and the plain values
//! readers and writers see.
//!
//! Most cells hold their value (`Record.cells`, keyed by field object id). Three kinds of field
//! keep their content in objects, so concurrent edits merge inside it:
//!
//! - A select's options are `Choice` objects (`{field, name, color}`) its `Field.choices` lists in
//!   order. A `single_select` cell holds a choice id, a `multi_select` cell a list of choice ids, so
//!   renaming an option renames it in every cell, and two replicas adding options both keep
//!   theirs. A value that names no option of the field is a choice the field does not list (an
//!   "unlisted" choice, which is what a removed option becomes): its cells keep showing it, and the
//!   kit's check reports it (`table.select-option-missing`). Values that are no text (a number
//!   written to a select) are stored as they are.
//! - A `long_text` cell holds the id of a `CellText` object (`{record, field, text}`), whose `text`
//!   merges per character: two replicas typing in one cell both keep their typing.
//! - A field's AI prompt is its `aiPrompt` text.
//!
//! Readers ([`decode`]) answer the plain values (option names, the text); writers give plain
//! values and [`Encoder`] stores them. The relational projection, the API and the editor only see
//! plain values, and a field's options in the projection list its choices by name
//! (`options.choices`, and `options.choice_tones` by name for the colors).

use std::collections::HashMap;

use kition_kdm::{ObjectId, Replica, Tx};
use serde_json::{Map, Value, json};

use super::model::{CELL_TEXT, CHOICE, Listing, is_a, require, string};
use crate::{Result, TableFileError};

/// How a field's cells are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// The value itself.
    Plain,
    /// A choice id (`single_select`).
    Single,
    /// A list of choice ids (`multi_select`).
    Multi,
    /// A `CellText` id (`long_text`).
    LongText,
}

impl CellKind {
    pub fn of(field_type: &str) -> Self {
        match field_type {
            "single_select" => Self::Single,
            "multi_select" => Self::Multi,
            "long_text" => Self::LongText,
            _ => Self::Plain,
        }
    }

    pub fn is_select(self) -> bool {
        matches!(self, Self::Single | Self::Multi)
    }
}

/// Whether a field of `field_type` lists its choices in its options even when it has none.
pub fn always_lists_choices(field_type: &str) -> bool {
    CellKind::of(field_type).is_select()
}

/// One of a field's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceData {
    pub id: ObjectId,
    pub name: String,
    /// `None` when it has no color.
    pub color: Option<String>,
}

/// The choices `Field.choices` lists, in order (live and first occurrence only).
pub fn listed_choices(replica: &Replica, field: &ObjectId) -> Result<Vec<ChoiceData>> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for id in replica.list(field, "choices")? {
        if !seen.insert(id.clone()) || !replica.object_type(&id).is_ok_and(|ty| ty == CHOICE) {
            continue;
        }
        out.push(ChoiceData {
            name: replica.get(&id, "name")?.as_str().unwrap_or_default().to_owned(),
            color: replica.get(&id, "color")?.as_str().map(str::to_owned),
            id,
        });
    }
    Ok(out)
}

/// A field's options as readers see them: the stored options with `choices` (the listed choices'
/// names, in order) and, when any choice has a color, `choice_tones` (color by name). A select
/// field always has `choices`; another field only when it lists some.
pub fn options_with_choices(stored: &Value, field_type: &str, choices: &[ChoiceData]) -> Value {
    if choices.is_empty() && !always_lists_choices(field_type) {
        return stored.clone();
    }
    let mut options = match stored {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    options.insert("choices".into(), Value::Array(choices.iter().map(|choice| json!(choice.name)).collect()));
    let tones: Map<String, Value> =
        choices.iter().filter_map(|choice| choice.color.as_ref().map(|color| (choice.name.clone(), json!(color)))).collect();
    if !tones.is_empty() {
        options.insert("choice_tones".into(), Value::Object(tones));
    }
    Value::Object(options)
}

/// The plain value of a stored cell: option names for the choices it names, the text of the
/// `CellText` it names. Whatever the field's type, so a cell written under another type (by a
/// replica that had not seen a type change yet) still reads as its value. A stored value that
/// names no choice or text is answered as it is.
pub fn decode(replica: &Replica, raw: &Value) -> Value {
    match raw {
        Value::Array(items) => Value::Array(items.iter().map(|item| choice_name(replica, item)).collect()),
        Value::String(text) => {
            let id = ObjectId::from_string(text.clone());
            match replica.object_type(&id).as_deref() {
                Ok(CHOICE) => replica.get(&id, "name").unwrap_or(Value::Null),
                Ok(CELL_TEXT) => Value::String(replica.text(&id, "text").unwrap_or_default()),
                _ => raw.clone(),
            }
        }
        other => other.clone(),
    }
}

fn choice_name(replica: &Replica, raw: &Value) -> Value {
    match raw.as_str().map(ObjectId::from_string) {
        Some(id) if replica.object_type(&id).is_ok_and(|ty| ty == CHOICE) => replica.get(&id, "name").unwrap_or(Value::Null),
        _ => raw.clone(),
    }
}

/// The `CellText` objects a stored cell names (none for other fields).
pub fn cell_text_of(replica: &Replica, raw: &Value) -> Option<ObjectId> {
    raw.as_str().map(ObjectId::from_string).filter(|id| replica.object_type(id).is_ok_and(|ty| ty == CELL_TEXT))
}

/// The texts a write may edit, read from the replica before its transaction (a transaction reads
/// a text's length, not its characters): a write that knows a text's characters changes only the
/// part that differs, so concurrent typing elsewhere in it stays; one that does not replaces it.
#[derive(Debug, Clone, Default)]
pub struct Texts(HashMap<(ObjectId, &'static str), String>);

impl Texts {
    /// The `CellText`s of `records`' cells.
    pub fn of_records<'a>(replica: &Replica, records: impl IntoIterator<Item = &'a ObjectId>) -> Self {
        let mut texts = Self::default();
        for record in records {
            for (_, raw) in replica.map_entries(record, "cells").unwrap_or_default() {
                if let Some(id) = cell_text_of(replica, &raw) {
                    texts.read(replica, &id, "text");
                }
            }
        }
        texts
    }

    /// Adds the text `field` of `id`.
    pub fn read(&mut self, replica: &Replica, id: &ObjectId, field: &'static str) {
        if let Ok(text) = replica.text(id, field) {
            self.0.insert((id.clone(), field), text);
        }
    }

    pub fn get(&self, id: &ObjectId, field: &'static str) -> Option<&str> {
        self.0.get(&(id.clone(), field)).map(String::as_str)
    }
}

/// Makes the text `field` of `id` read `now`. When `texts` knows what it reads now (and the
/// transaction has not changed its length), only the changed middle is deleted and inserted;
/// otherwise the whole text is replaced. Answers whether anything was written.
pub fn write_text(tx: &mut Tx<'_>, id: &ObjectId, field: &'static str, texts: &Texts, now: &str) -> Result<bool> {
    let len = tx.text_len(id, field)?;
    let was = texts.get(id, field).filter(|was| was.chars().count() == len);
    let Some(was) = was else {
        if len == 0 && now.is_empty() {
            return Ok(false);
        }
        if len > 0 {
            tx.text_delete(id, field, 0, len)?;
        }
        if !now.is_empty() {
            tx.text_insert(id, field, 0, now)?;
        }
        return Ok(true);
    };
    if was == now {
        return Ok(false);
    }
    let a: Vec<char> = was.chars().collect();
    let b: Vec<char> = now.chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix =
        a.get(prefix..).unwrap_or_default().iter().rev().zip(b.get(prefix..).unwrap_or_default().iter().rev()).take_while(|(x, y)| x == y).count();
    let removed = a.len() - prefix - suffix;
    if removed > 0 {
        tx.text_delete(id, field, prefix, removed)?;
    }
    let inserted: String = b.get(prefix..b.len() - suffix).unwrap_or_default().iter().collect();
    if !inserted.is_empty() {
        tx.text_insert(id, field, prefix, &inserted)?;
    }
    Ok(true)
}

/// A field's choices in a transaction: the listed ones in order, then the unlisted ones (choices of
/// the field no list names) by id.
fn field_choices(tx: &Tx<'_>, field: &ObjectId) -> Result<(Vec<ObjectId>, Vec<ObjectId>)> {
    let listed = Listing::read(tx, field, "choices")?.ids();
    let mut unlisted: Vec<ObjectId> = tx
        .objects_of_type(CHOICE)
        .into_iter()
        .filter(|id| {
            !listed.contains(id) && tx.get(id, "field").ok().and_then(|value| value.as_str().map(ObjectId::from_string)).as_ref() == Some(field)
        })
        .collect();
    unlisted.sort();
    Ok((listed, unlisted))
}

/// A choice as options name it: its name, and its color (`None`: none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantedChoice {
    pub name: String,
    pub color: Option<String>,
}

/// Takes the choices out of options as writers give them: `choices` is a list of names or of
/// `{name, color}` objects, and `choice_tones` gives colors by name. Answers the options left
/// (`null` when nothing is left) and the choices; options are replaced whole, so options without
/// `choices` name none.
pub fn split_choices(options: &Value) -> (Value, Vec<WantedChoice>) {
    let Value::Object(map) = options else {
        return (options.clone(), Vec::new());
    };
    let mut rest = map.clone();
    let tones = rest.remove("choice_tones").and_then(|tones| tones.as_object().cloned()).unwrap_or_default();
    let items = match rest.remove("choices") {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    let wanted = items
        .iter()
        .filter_map(|item| {
            let (name, own) = match item {
                Value::String(name) => (name.clone(), None),
                Value::Object(object) => {
                    let name = object.get("name").and_then(Value::as_str)?.to_owned();
                    (name, object.get("color").and_then(Value::as_str).map(str::to_owned))
                }
                Value::Number(_) | Value::Bool(_) => (crate::json::to_text(item), None),
                _ => return None,
            };
            let color = own.or_else(|| tones.get(&name).and_then(Value::as_str).map(str::to_owned));
            Some(WantedChoice { name, color })
        })
        .collect();
    (if rest.is_empty() && !map.is_empty() { Value::Null } else { Value::Object(rest) }, wanted)
}

/// Makes `field` list `wanted`, in order. A wanted name the field has a choice of (listed first,
/// else unlisted) keeps that choice; a wanted name at the place of a listed choice no wanted name
/// kept renames that choice (an option renamed in place keeps its cells); the other names are new
/// choices. Listed choices nobody wants are no longer listed: the cells holding them keep showing
/// them. Colors are written where given.
pub fn write_choices(tx: &mut Tx<'_>, field: &ObjectId, wanted: &[WantedChoice]) -> Result<()> {
    require(tx, field, super::model::FIELD)?;
    let (listed, unlisted) = field_choices(tx, field)?;
    let mut taken: Vec<bool> = vec![false; listed.len()];
    let mut taken_unlisted: Vec<bool> = vec![false; unlisted.len()];
    let mut chosen: Vec<Option<ObjectId>> = vec![None; wanted.len()];
    // Same name, listed.
    for (slot, choice) in chosen.iter_mut().zip(wanted) {
        for (index, id) in listed.iter().enumerate() {
            if !taken.get(index).copied().unwrap_or(true) && string(tx, id, "name")? == choice.name {
                taken[index] = true;
                *slot = Some(id.clone());
                break;
            }
        }
    }
    // Same name, unlisted: listed again.
    for (slot, choice) in chosen.iter_mut().zip(wanted) {
        if slot.is_some() {
            continue;
        }
        for (index, id) in unlisted.iter().enumerate() {
            if !taken_unlisted.get(index).copied().unwrap_or(true) && string(tx, id, "name")? == choice.name {
                taken_unlisted[index] = true;
                *slot = Some(id.clone());
                break;
            }
        }
    }
    // Renamed in place.
    for (index, (slot, choice)) in chosen.iter_mut().zip(wanted).enumerate() {
        if slot.is_some() {
            continue;
        }
        if let Some(id) = listed.get(index)
            && !taken.get(index).copied().unwrap_or(true)
        {
            taken[index] = true;
            tx.set(id, "name", json!(choice.name))?;
            *slot = Some(id.clone());
        }
    }
    // New.
    for (slot, choice) in chosen.iter_mut().zip(wanted) {
        if slot.is_none() {
            *slot = Some(create_choice(tx, field, &choice.name)?);
        }
    }
    let order: Vec<ObjectId> = chosen.into_iter().flatten().collect();
    // Colors.
    for (id, choice) in order.iter().zip(wanted) {
        let value = choice.color.as_ref().map_or(Value::Null, |color| json!(color));
        if tx.get(id, "color")? != value {
            tx.set(id, "color", value)?;
        }
    }
    // The list: drop what is no longer wanted, add what is new, then put it in order.
    let items = tx.list_items(field, "choices")?;
    for (index, item) in items.iter().enumerate().rev() {
        if !order.contains(item) {
            tx.list_remove(field, "choices", index)?;
        }
    }
    let present = tx.list_items(field, "choices")?;
    for id in &order {
        if !present.contains(id) {
            let len = tx.list_items(field, "choices")?.len();
            tx.list_insert(field, "choices", len, id)?;
        }
    }
    let raw = tx.list_items(field, "choices")?;
    super::model::place_items(tx, field, "choices", raw, &order, None)?;
    Ok(())
}

/// A new choice of `field`, listed nowhere.
fn create_choice(tx: &mut Tx<'_>, field: &ObjectId, name: &str) -> Result<ObjectId> {
    let mut values = Map::new();
    values.insert("field".into(), json!(field.as_str()));
    values.insert("name".into(), json!(name));
    Ok(tx.create_with_id(CHOICE, &crate::ids::object(), values)?)
}

/// Stores plain cell values in a transaction: it finds a field's choices once per write and makes
/// the choices and texts the values need.
#[derive(Default)]
pub struct Encoder {
    /// Each field's choices by name: listed first, then unlisted.
    choices: HashMap<ObjectId, Vec<(String, ObjectId)>>,
}

impl Encoder {
    /// The id of `field`'s choice named `name`, made (unlisted) when it has none.
    fn choice(&mut self, tx: &mut Tx<'_>, field: &ObjectId, name: &str) -> Result<ObjectId> {
        if !self.choices.contains_key(field) {
            let (listed, unlisted) = field_choices(tx, field)?;
            let mut named = Vec::with_capacity(listed.len() + unlisted.len());
            for id in listed.into_iter().chain(unlisted) {
                named.push((string(tx, &id, "name")?, id));
            }
            self.choices.insert(field.clone(), named);
        }
        if let Some((_, id)) = self.choices.get(field).and_then(|named| named.iter().find(|(known, _)| known == name)) {
            return Ok(id.clone());
        }
        let id = create_choice(tx, field, name)?;
        self.choices.entry(field.clone()).or_default().push((name.to_owned(), id.clone()));
        Ok(id)
    }

    /// The value to store for the plain `value` (not `null`) of `field`'s cell of `record`, which
    /// holds `current` now: what the cell should hold (unchanged when it is `current`). A long
    /// text's characters are written into its `CellText` (made when the cell has none).
    pub fn encode(
        &mut self,
        tx: &mut Tx<'_>,
        record: &ObjectId,
        field: &ObjectId,
        kind: CellKind,
        current: &Value,
        value: &Value,
        texts: &Texts,
    ) -> Result<Value> {
        match kind {
            CellKind::Plain => Ok(value.clone()),
            CellKind::Single => match value {
                Value::String(name) if !name.is_empty() => Ok(json!(self.choice(tx, field, name)?.as_str())),
                other => Ok(other.clone()),
            },
            CellKind::Multi => match value {
                Value::Array(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        out.push(match item {
                            Value::String(name) if !name.is_empty() => json!(self.choice(tx, field, name)?.as_str()),
                            other => other.clone(),
                        });
                    }
                    Ok(Value::Array(out))
                }
                other => Ok(other.clone()),
            },
            CellKind::LongText => {
                let Value::String(text) = value else {
                    return Ok(value.clone());
                };
                if let Some(id) = own_text(tx, record, field, current) {
                    write_text(tx, &id, "text", texts, text)?;
                    return Ok(current.clone());
                }
                if text.is_empty() {
                    return Ok(value.clone());
                }
                let mut values = Map::new();
                values.insert("record".into(), json!(record.as_str()));
                values.insert("field".into(), json!(field.as_str()));
                let id = tx.create_with_id(CELL_TEXT, &crate::ids::object(), values)?;
                tx.text_insert(&id, "text", 0, text)?;
                Ok(json!(id.as_str()))
            }
        }
    }
}

/// The `CellText` a stored cell names when it is this record's text of this field.
fn own_text(tx: &Tx<'_>, record: &ObjectId, field: &ObjectId, current: &Value) -> Option<ObjectId> {
    let id = current.as_str().map(ObjectId::from_string)?;
    if !is_a(tx, &id, CELL_TEXT) {
        return None;
    }
    let names =
        |key: &str, wanted: &ObjectId| tx.get(&id, key).ok().and_then(|value| value.as_str().map(ObjectId::from_string)).as_ref() == Some(wanted);
    (names("record", record) && names("field", field)).then_some(id)
}

/// The `CellText` a stored cell names, if any (in a transaction).
pub fn cell_text_in(tx: &Tx<'_>, raw: &Value) -> Option<ObjectId> {
    raw.as_str().map(ObjectId::from_string).filter(|id| is_a(tx, id, CELL_TEXT))
}

/// The plain value of a stored cell in a transaction: what [`decode`] answers. A long text whose
/// characters `texts` does not know is [`TableFileError::Invalid`].
pub fn decode_in(tx: &Tx<'_>, raw: &Value, texts: &Texts) -> Result<Value> {
    let name = |raw: &Value| -> Result<Value> {
        match raw.as_str().map(ObjectId::from_string) {
            Some(id) if is_a(tx, &id, CHOICE) => Ok(tx.get(&id, "name")?),
            _ => Ok(raw.clone()),
        }
    };
    match raw {
        Value::Array(items) => Ok(Value::Array(items.iter().map(name).collect::<Result<Vec<_>>>()?)),
        Value::String(_) => match cell_text_in(tx, raw) {
            Some(id) => {
                let text = texts.get(&id, "text").ok_or_else(|| TableFileError::invalid("the long text of a cell was not read before the write"))?;
                Ok(json!(text))
            }
            None => name(raw),
        },
        other => Ok(other.clone()),
    }
}

/// Every choice of `field`, listed or not: the ones `Field.choices` lists in order, then the
/// others by id.
pub fn all_choices(replica: &Replica, field: &ObjectId) -> Vec<ObjectId> {
    let mut ids: Vec<ObjectId> = listed_choices(replica, field).unwrap_or_default().into_iter().map(|choice| choice.id).collect();
    let mut unlisted: Vec<ObjectId> = replica
        .objects_of_type(CHOICE)
        .into_iter()
        .filter(|id| {
            !ids.contains(id) && replica.get(id, "field").ok().and_then(|value| value.as_str().map(ObjectId::from_string)).as_ref() == Some(field)
        })
        .collect();
    unlisted.sort();
    ids.extend(unlisted);
    ids
}
