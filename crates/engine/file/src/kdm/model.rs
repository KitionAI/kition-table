//! Typed writes of the `kit.table` document, on a [`Tx`] of its replica.
//!
//! The document is one `Base` holding ordered lists of `Table`s and `Dashboard`s. A `Table` holds
//! ordered lists of its `Field`s, `View`s, and `Record`s, and names its primary field. A
//! `Record`'s cells are a map keyed by field object id; record links are `Link` objects
//! `(from record, field, to record)`, so concurrent link edits merge as a set.
//!
//! A select's options are `Choice` objects its `Field.choices` lists (the values it does not list
//! are its `otherChoices`), and its cells hold choice ids; a long text's cell holds a `CellText` id; a field's AI prompt is its `aiPrompt` text (see
//! [`super::cells`]). The functions here take and give plain values (option names, texts) and
//! store them in that shape. View configs name fields by object id ([`super::view_refs`]).
//!
//! Every function here only stages operations: run them inside [`transact`] (or
//! [`kition_kdm::Replica::transact`]) so a failure anywhere leaves the document unchanged. Object
//! ids are UUID v7 except a record's, which is its `row_key` (a UUID v4). Options and configs
//! name tables and fields by object id; see [`super::options`] for the relational form.
//!
//! Lists can hold an id more than once after a merge (two replicas inserting one object); the
//! first occurrence counts and later ones are ignored, by readers and by positions here
//! ("the n-th field" counts live, distinct items). Functions that look up links scan the
//! document's `Link` objects once per call, O(objects); batch link edits go through
//! [`replace_links_many`].

use std::collections::{HashMap, HashSet};

use kition_kdm::{KdmError, ObjectId, Origin, Replica, Tx};
use serde_json::{Map, Value, json};

use super::cells::{self, CellKind, Encoder, Texts};
use super::view_refs;
use crate::json::{self, fallback};
use crate::source::{FIELD_TYPES, FieldPatch, FieldSpec, TablePatch, TableSpec, VIEW_TYPES, ViewPatch, ViewSpec};
use crate::{Result, TableFileError};

pub const BASE: &str = "Base";
pub const TABLE: &str = "Table";
pub const FIELD: &str = "Field";
pub const VIEW: &str = "View";
pub const RECORD: &str = "Record";
pub const LINK: &str = "Link";
pub const DASHBOARD: &str = "Dashboard";
pub const CHOICE: &str = "Choice";
pub const CELL_TEXT: &str = "CellText";

/// Field types whose values are derived, never stored in `Record.cells`. A `link_to_record`'s
/// display value is derived from its `Link` objects.
pub const COMPUTED_FIELD_TYPES: [&str; 9] =
    ["formula", "lookup", "rollup", "link_to_record", "auto_number", "created_time", "last_modified_time", "created_by", "last_modified_by"];

/// The column width range of a view, as the relational view configuration clamps it.
pub const MIN_WIDTH: i64 = 40;
pub const MAX_WIDTH: i64 = 1200;

/// Whether values of `field_type` are derived rather than stored.
pub fn is_computed(field_type: &str) -> bool {
    COMPUTED_FIELD_TYPES.contains(&field_type)
}

#[derive(Debug, Clone, Default)]
pub struct BaseSpec {
    pub title: String,
    pub description: String,
    pub icon: String,
    pub color: String,
    pub meta: Value,
}

/// The objects [`create_table`] created, in list order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableIds {
    pub table: ObjectId,
    pub fields: Vec<ObjectId>,
    pub views: Vec<ObjectId>,
}

/// Where [`create_records`] inserts, relative to a record of the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    Before(ObjectId),
    After(ObjectId),
}

/// Runs `f` as one transaction of `replica`, like [`Replica::transact`] but with this crate's
/// errors: an error from `f` aborts the transaction (nothing is applied) and is returned as is.
pub fn transact<R>(replica: &mut Replica, origin: &Origin, f: impl FnOnce(&mut Tx<'_>) -> Result<R>) -> Result<R> {
    let mut failure = None;
    let outcome = replica.transact(origin, |tx| {
        f(tx).map_err(|err| {
            failure = Some(err);
            KdmError::Engine("the transaction was aborted".into())
        })
    });
    outcome.map_err(|err| failure.take().unwrap_or_else(|| err.into()))
}

// ---------------------------------------------------------------------------
// Base.

pub fn create_base(tx: &mut Tx<'_>, spec: &BaseSpec) -> Result<ObjectId> {
    let mut fields = registers(json!({
        "title": spec.title.trim(),
        "description": spec.description.trim(),
        "icon": spec.icon.trim(),
        "color": spec.color.trim(),
    }));
    put_json(&mut fields, "meta", &spec.meta)?;
    Ok(tx.create_with_id(BASE, &crate::ids::object(), fields)?)
}

// ---------------------------------------------------------------------------
// Tables.

/// Creates a table at `index` of the base's tables (`None` or past the end: last). Without
/// `spec.fields` it gets a required primary "Title" text field; without `spec.views`, an "All"
/// grid view. The primary field is the last field spec marked `primary`, else the first field.
pub fn create_table(tx: &mut Tx<'_>, base: &ObjectId, spec: &TableSpec, index: Option<usize>) -> Result<TableIds> {
    create_table_as(tx, base, spec, index, true)
}

/// [`create_table`] without seeding a default field or view when `spec` has none: for migrations
/// and other bulk builds that must reproduce a source exactly, including a table a user left with
/// no live fields or no live views.
pub fn create_table_unseeded(tx: &mut Tx<'_>, base: &ObjectId, spec: &TableSpec, index: Option<usize>) -> Result<TableIds> {
    create_table_as(tx, base, spec, index, false)
}

fn create_table_as(tx: &mut Tx<'_>, base: &ObjectId, spec: &TableSpec, index: Option<usize>, seed: bool) -> Result<TableIds> {
    require(tx, base, BASE)?;
    let tables = Listing::read(tx, base, "tables")?;
    let title = fallback(&spec.title, "Table");
    let taken = names_of(tx, &tables.ids())?;
    let name = unique_name(&json::internal_name(&fallback(&spec.name, &title)), &taken);
    let mut fields = registers(json!({
        "name": name,
        "title": title,
        "description": spec.description.trim(),
    }));
    put_json(&mut fields, "meta", &spec.meta)?;
    let table = tx.create_with_id(TABLE, &crate::ids::object(), fields)?;
    let at = tables.insert_index(index.unwrap_or(usize::MAX));
    tx.list_insert(base, "tables", at, &table)?;

    let default_fields =
        [FieldSpec { title: "Title".to_owned(), field_type: "text".to_owned(), primary: true, required: true, ..FieldSpec::default() }];
    let field_specs: &[FieldSpec] = if spec.fields.is_empty() && seed { &default_fields } else { &spec.fields };
    let mut primary = false;
    for field_spec in field_specs {
        // A migration reproduces the source as it is, circular references included.
        create_field_as(tx, &table, field_spec, seed)?;
        primary |= field_spec.primary;
    }
    // Fields inserted at an `order` may not be in creation order.
    let in_order = tx.list_items(&table, "fields")?;
    if !primary && let Some(first) = in_order.first() {
        tx.set(&table, "primaryField", json!(first.as_str()))?;
    }

    let default_views = [ViewSpec { title: "All".to_owned(), view_type: "grid".to_owned(), config: Value::Null }];
    let view_specs: &[ViewSpec] = if spec.views.is_empty() && seed { &default_views } else { &spec.views };
    let mut views = Vec::with_capacity(view_specs.len());
    for view_spec in view_specs {
        views.push(create_view(tx, &table, view_spec)?);
    }
    Ok(TableIds { table, fields: in_order, views })
}

/// Patches a table. The primary field is `primary_field`, which must be one of the table's
/// fields ([`TableFileError::NotFound`] otherwise); `None` keeps the current one.
/// `TablePatch::primary_field_id` is a relational id and is refused ([`TableFileError::Invalid`]):
/// resolve it to the field's object id and pass that.
pub fn update_table(tx: &mut Tx<'_>, table: &ObjectId, patch: &TablePatch, primary_field: Option<&ObjectId>) -> Result<()> {
    require(tx, table, TABLE)?;
    if patch.primary_field_id.is_some() {
        return Err(TableFileError::invalid("primary_field_id is a relational id; pass the primary field's object id"));
    }
    if let Some(title) = patch.title.as_deref().map(str::trim)
        && !title.is_empty()
    {
        set_if_changed(tx, table, "title", json!(title))?;
    }
    if let Some(description) = &patch.description {
        set_if_changed(tx, table, "description", json!(description))?;
    }
    if let Some(meta) = &patch.meta {
        set_if_changed(tx, table, "meta", decode_json(meta)?)?;
    }
    if let Some(primary) = primary_field {
        if !Listing::read(tx, table, "fields")?.contains(primary) {
            return Err(TableFileError::NotFound);
        }
        set_if_changed(tx, table, "primaryField", json!(primary.as_str()))?;
    }
    Ok(())
}

/// Deletes a table with its fields, views, records, and every link from, to, or through them.
pub fn delete_table(tx: &mut Tx<'_>, base: &ObjectId, table: &ObjectId) -> Result<()> {
    require(tx, base, BASE)?;
    require(tx, table, TABLE)?;
    if !remove_all(tx, base, "tables", table)? {
        return Err(TableFileError::NotFound);
    }
    let fields = tx.list_items(table, "fields")?;
    let views = tx.list_items(table, "views")?;
    let records = tx.list_items(table, "records")?;
    let field_set: HashSet<&ObjectId> = fields.iter().collect();
    let record_set: HashSet<&ObjectId> = records.iter().collect();
    for link in all_links(tx)? {
        if field_set.contains(&link.field) || record_set.contains(&link.from) || record_set.contains(&link.to) {
            tx.delete(&link.id)?;
        }
    }
    // The table's choices, long texts and views' filters and sorts go with it.
    let mut doomed: Vec<ObjectId> = Vec::new();
    for view in &views {
        for list in VIEW_ITEM_LISTS {
            doomed.extend(tx.list_items(view, list).unwrap_or_default());
        }
    }
    for field in &fields {
        for list in ["choices", "otherChoices"] {
            doomed.extend(tx.list_items(field, list).unwrap_or_default());
        }
    }
    for id in tx.objects_of_type(CELL_TEXT) {
        let record = tx.get(&id, "record").ok().and_then(|value| value.as_str().map(ObjectId::from_string));
        if record.is_some_and(|record| record_set.contains(&record)) {
            doomed.push(id);
        }
    }
    let mut seen = HashSet::new();
    for id in fields.iter().chain(&views).chain(&records).chain(&doomed) {
        if seen.insert(id) && tx.exists(id) {
            tx.delete(id)?;
        }
    }
    tx.delete(table)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Fields.

/// Creates a field. Its name is `spec.name` (else its title) as an internal name, suffixed
/// `_2`, `_3`, ... until unique in the table. `spec.order` is a 1-based position as in
/// [`crate::TableFile::create_field`]: the field becomes the `order`-th field (clamped);
/// `None` appends. `spec.primary` makes it the table's primary field.
///
/// Field names are immutable internal keys: view configs and formulas name fields by them. Two
/// replicas can still create one name concurrently; [`super::repair`] then renames the field with
/// the higher id (`status` to `status_2`), which can happen again in a later merge (`status_2`
/// to `status_3`), and references by name follow the field that keeps the name.
///
/// A derived field whose value would depend on itself is refused ([`TableFileError::Invalid`],
/// "circular reference"); see [`check_acyclic`].
pub fn create_field(tx: &mut Tx<'_>, table: &ObjectId, spec: &FieldSpec) -> Result<ObjectId> {
    create_field_as(tx, table, spec, true)
}

fn create_field_as(tx: &mut Tx<'_>, table: &ObjectId, spec: &FieldSpec, check_cycles: bool) -> Result<ObjectId> {
    require(tx, table, TABLE)?;
    let field_type = fallback(&spec.field_type, "text");
    check_field_type(&field_type)?;
    let title = fallback(&spec.title, "Field");
    let fields = Listing::read(tx, table, "fields")?;
    let taken = names_of(tx, &fields.ids())?;
    let name = unique_name(&json::internal_name(&fallback(&spec.name, &title)), &taken);
    let readonly = spec.readonly || forces_readonly(&field_type);
    let mut values = registers(json!({
        "name": name,
        "title": title,
        "type": field_type,
        "required": spec.required,
        "readonly": readonly,
        "formula": spec.formula.trim(),
    }));
    let (options, choices) = cells::split_choices(&object_ids_only(decode_json(&spec.options)?)?);
    let (options, instruction) = split_instruction(options);
    put_json(&mut values, "options", &options)?;
    put_json(&mut values, "meta", &spec.meta)?;
    let field = tx.create_with_id(FIELD, &crate::ids::object(), values)?;
    let prompt = instruction.as_deref().map(str::trim).filter(|text| !text.is_empty()).unwrap_or(spec.ai_prompt.trim());
    if !prompt.is_empty() {
        tx.text_insert(&field, "aiPrompt", 0, prompt)?;
    }
    if !choices.is_empty() {
        cells::write_choices(tx, &field, &choices)?;
    }
    let position = match spec.order {
        Some(order) => usize::try_from(order.max(1) - 1).unwrap_or(usize::MAX),
        None => usize::MAX,
    };
    tx.list_insert(table, "fields", fields.insert_index(position), &field)?;
    if spec.primary {
        tx.set(table, "primaryField", json!(field.as_str()))?;
    }
    if check_cycles {
        check_acyclic(tx, &field)?;
    }
    Ok(field)
}

/// Patches a field. Field names are immutable. `is_primary: Some(true)` makes it its table's
/// primary field; `Some(false)` on the primary field is refused ([`TableFileError::Invalid`]), and
/// so is a change that makes a derived field depend on itself ([`check_acyclic`]).
///
/// Options that name `choices` make the field list them ([`cells::write_choices`]); a new
/// `ai_prompt` replaces the text, all of it (see [`update_field_known`]). A new type stores the
/// cells as the type stores them: a select's names become choices, a long text's text a
/// `CellText`, and back.
pub fn update_field(tx: &mut Tx<'_>, field: &ObjectId, patch: &FieldPatch) -> Result<()> {
    update_field_known(tx, field, patch, &Texts::default())
}

/// [`update_field`] knowing what the field's texts read now (its `aiPrompt`, and the long texts
/// of its cells): a new prompt changes only what differs, and a long text field can change type
/// (its cells' texts are read from `texts`; a text it does not know is
/// [`TableFileError::Invalid`]).
pub fn update_field_known(tx: &mut Tx<'_>, field: &ObjectId, patch: &FieldPatch, texts: &Texts) -> Result<()> {
    require(tx, field, FIELD)?;
    let old_type = string(tx, field, "type")?;
    if let Some(title) = patch.title.as_deref().map(str::trim)
        && !title.is_empty()
    {
        set_if_changed(tx, field, "title", json!(title))?;
    }
    let mut field_type = string(tx, field, "type")?;
    if let Some(requested) = patch.field_type.as_deref().map(str::trim).filter(|requested| !requested.is_empty()) {
        check_field_type(requested)?;
        field_type = requested.to_owned();
        set_if_changed(tx, field, "type", json!(requested))?;
    }
    if let Some(required) = patch.required {
        set_if_changed(tx, field, "required", json!(required))?;
    }
    let readonly = patch.readonly.unwrap_or(tx.get(field, "readonly")? == json!(true));
    set_if_changed(tx, field, "readonly", json!(readonly || forces_readonly(&field_type)))?;
    if let Some(options) = &patch.options {
        let (options, choices) = cells::split_choices(&object_ids_only(decode_json(options)?)?);
        let (options, instruction) = split_instruction(options);
        set_if_changed(tx, field, "options", options)?;
        cells::write_choices(tx, field, &choices)?;
        if let Some(instruction) = instruction
            && patch.ai_prompt.is_none()
        {
            cells::write_text(tx, field, "aiPrompt", texts, &instruction)?;
        }
    }
    if let Some(formula) = &patch.formula {
        set_if_changed(tx, field, "formula", json!(formula))?;
    }
    if let Some(prompt) = &patch.ai_prompt {
        cells::write_text(tx, field, "aiPrompt", texts, prompt)?;
    }
    if field_type != old_type && !is_computed(&field_type) {
        convert_cells(tx, field, CellKind::of(&field_type), texts)?;
    }
    if let Some(meta) = &patch.meta {
        set_if_changed(tx, field, "meta", decode_json(meta)?)?;
    }
    if let Some(primary) = patch.is_primary {
        let table = table_of_field(tx, field)?.ok_or(TableFileError::NotFound)?;
        let is_primary = tx.get(&table, "primaryField")? == json!(field.as_str());
        if primary {
            set_if_changed(tx, &table, "primaryField", json!(field.as_str()))?;
        } else if is_primary {
            return Err(TableFileError::invalid("primary field cannot be unset directly"));
        }
    }
    // A new primary field changes what link chips of other tables read, so it can close a cycle too.
    if patch.field_type.is_some() || patch.options.is_some() || patch.formula.is_some() || patch.is_primary.is_some() {
        check_acyclic(tx, field)?;
    }
    Ok(())
}

/// Stores every cell of `field` as a field of `kind` stores it, after a change of type: each
/// record's value, read whatever shape it has ([`cells::decode_in`]), is written again.
fn convert_cells(tx: &mut Tx<'_>, field: &ObjectId, kind: CellKind, texts: &Texts) -> Result<()> {
    let Some(table) = table_of_field(tx, field)? else {
        return Ok(());
    };
    let key = field.as_str();
    let mut encoder = Encoder::default();
    for record in Listing::read(tx, &table, "records")?.ids() {
        let current = tx.map_get(&record, "cells", key)?;
        if current.is_null() {
            continue;
        }
        let old_text = cells::cell_text_in(tx, &current);
        if kind == CellKind::LongText && old_text.is_some() {
            continue;
        }
        let plain = cells::decode_in(tx, &current, texts)?;
        // A long text kept as a long text keeps its `CellText`.
        let keep = if kind == CellKind::LongText { current.clone() } else { Value::Null };
        let stored = encoder.encode(tx, &record, field, kind, &keep, &plain, texts)?;
        if !json::equal(&current, &stored) {
            tx.map_set(&record, "cells", key, stored)?;
            if let Some(old) = old_text {
                tx.delete(&old)?;
            }
        }
    }
    Ok(())
}

/// Refuses ([`TableFileError::Invalid`], "circular reference") when the derived value of `field`
/// depends on itself through the document's field dependencies, as spreadsheets refuse circular
/// references. A formula depends on the fields of its table it names; a lookup on its target
/// field; a rollup on its target field, or without one on every field of its link field's target
/// table; a `link_to_record` field on the primary field of its target table. The graph is per
/// field, not per record: a running total (a rollup of the previous record's formula that adds
/// the rollup) is refused too. Concurrent edits can still merge into a cycle; the projector
/// gives such fields bounded values ([`super::project`]).
pub fn check_acyclic(tx: &Tx<'_>, field: &ObjectId) -> Result<()> {
    if !is_a(tx, field, FIELD) {
        return Ok(());
    }
    let derived = |ty: &str| matches!(ty, "formula" | "lookup" | "rollup" | "link_to_record");
    if !derived(&string(tx, field, "type")?) {
        return Ok(());
    }
    struct Node {
        table: ObjectId,
        name: String,
        ty: String,
        options: Value,
        formula: String,
    }
    let mut nodes: HashMap<ObjectId, Node> = HashMap::new();
    let mut table_fields: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    let mut primaries: HashMap<ObjectId, ObjectId> = HashMap::new();
    for base in tx.objects_of_type(BASE) {
        for table in Listing::read(tx, &base, "tables")?.ids() {
            let fields = Listing::read(tx, &table, "fields")?.ids();
            for id in &fields {
                nodes.entry(id.clone()).or_insert(Node {
                    table: table.clone(),
                    name: string(tx, id, "name")?,
                    ty: string(tx, id, "type")?,
                    options: tx.get(id, "options")?,
                    formula: string(tx, id, "formula")?,
                });
            }
            if let Some(primary) = tx.get(&table, "primaryField")?.as_str() {
                primaries.insert(table.clone(), ObjectId::from_string(primary));
            }
            table_fields.insert(table, fields);
        }
    }
    let option = |node: &Node, key: &str| -> Option<ObjectId> { node.options.get(key).and_then(Value::as_str).map(ObjectId::from_string) };
    let edges = |id: &ObjectId| -> Vec<ObjectId> {
        let Some(node) = nodes.get(id) else {
            return Vec::new();
        };
        let table_of_link = |link: Option<ObjectId>| -> Option<ObjectId> { option(nodes.get(&link?)?, "target_table") };
        match node.ty.as_str() {
            "formula" => {
                let names = crate::computed::formula_references(&node.formula);
                table_fields
                    .get(&node.table)
                    .into_iter()
                    .flatten()
                    .filter(|other| nodes.get(*other).is_some_and(|other| names.contains(&other.name)))
                    .cloned()
                    .collect()
            }
            "lookup" => option(node, "target_field").into_iter().collect(),
            "rollup" => match option(node, "target_field").filter(|f| nodes.contains_key(f)) {
                Some(target) => vec![target],
                None => table_of_link(option(node, "link_field")).and_then(|table| table_fields.get(&table).cloned()).unwrap_or_default(),
            },
            "link_to_record" => option(node, "target_table").and_then(|table| primaries.get(&table).cloned()).into_iter().collect(),
            _ => Vec::new(),
        }
    };
    let mut seen = HashSet::new();
    let mut stack = edges(field);
    while let Some(next) = stack.pop() {
        if next == *field {
            let name = nodes.get(field).map(|node| node.name.clone()).unwrap_or_default();
            return Err(TableFileError::invalid(format!("circular reference: field {name} depends on itself")));
        }
        if seen.insert(next.clone()) {
            stack.extend(edges(&next));
        }
    }
    Ok(())
}

/// Deletes a field and the links through it. The primary field is refused
/// ([`TableFileError::Invalid`]).
///
/// Its cells stay in the records: readers ignore cells of fields that are gone. Clearing them
/// would cost a write per record and would not merge cleanly with concurrent cell edits, which
/// another replica may still be making; keeping them keeps every concurrent edit mergeable.
pub fn delete_field(tx: &mut Tx<'_>, table: &ObjectId, field: &ObjectId) -> Result<()> {
    require(tx, table, TABLE)?;
    if !Listing::read(tx, table, "fields")?.contains(field) {
        return Err(TableFileError::NotFound);
    }
    if tx.get(table, "primaryField")? == json!(field.as_str()) {
        return Err(TableFileError::invalid("primary field cannot be deleted"));
    }
    remove_all(tx, table, "fields", field)?;
    for link in all_links(tx)? {
        if link.field == *field {
            tx.delete(&link.id)?;
        }
    }
    tx.delete(field)?;
    Ok(())
}

/// Moves `fields` (in the given order) so that the first becomes the `to_index`-th of the
/// table's other fields (past the end: last). Every field must be in the table
/// ([`TableFileError::NotFound`]).
pub fn move_fields(tx: &mut Tx<'_>, table: &ObjectId, fields: &[ObjectId], to_index: usize) -> Result<()> {
    require(tx, table, TABLE)?;
    let listing = Listing::read(tx, table, "fields")?;
    let moved = distinct(fields);
    if moved.iter().any(|field| !listing.contains(field)) {
        return Err(TableFileError::NotFound);
    }
    let moving: HashSet<&ObjectId> = moved.iter().collect();
    let anchor = listing.ids().into_iter().filter(|item| !moving.contains(item)).nth(to_index);
    place_items(tx, table, "fields", listing.raw, &moved, anchor)
}

// ---------------------------------------------------------------------------
// Views.

/// Creates a view at the end of the table's views. An unknown view type becomes `grid`.
pub fn create_view(tx: &mut Tx<'_>, table: &ObjectId, spec: &ViewSpec) -> Result<ObjectId> {
    require(tx, table, TABLE)?;
    let mut view_type = fallback(&spec.view_type, "grid");
    if !VIEW_TYPES.contains(&view_type.as_str()) {
        view_type = "grid".to_owned();
    }
    let values = registers(json!({
        "type": view_type,
        "title": fallback(&spec.title, "All"),
        "locked": false,
    }));
    let config = config_in(tx, table, &decode_json(&spec.config)?)?;
    let view = tx.create_with_id(VIEW, &crate::ids::object(), values)?;
    let len = tx.list_items(table, "views")?.len();
    tx.list_insert(table, "views", len, &view)?;
    if !config.is_null() {
        super::view_store::store(tx, &view, &config)?;
    }
    Ok(view)
}

/// Patches a view. An unknown view type is refused ([`TableFileError::Invalid`]).
pub fn update_view(tx: &mut Tx<'_>, view: &ObjectId, patch: &ViewPatch) -> Result<()> {
    require(tx, view, VIEW)?;
    if let Some(title) = patch.title.as_deref().map(str::trim)
        && !title.is_empty()
    {
        set_if_changed(tx, view, "title", json!(title))?;
    }
    if let Some(requested) = patch.view_type.as_deref().map(str::trim).filter(|requested| !requested.is_empty()) {
        if !VIEW_TYPES.contains(&requested) {
            return Err(TableFileError::invalid(format!("unsupported view type {requested:?}: `type` must be one of {}", VIEW_TYPES.join(", "))));
        }
        set_if_changed(tx, view, "type", json!(requested))?;
    }
    if let Some(config) = &patch.config {
        let config = decode_json(config)?;
        let config = match table_of_view(tx, view)? {
            Some(table) => config_in(tx, &table, &config)?,
            None => view_refs::config_to_kdm(&config, &|_| None),
        };
        // A view without a config reads as `{}`: writing that back changes nothing (`store`).
        super::view_store::store(tx, view, &config)?;
    }
    Ok(())
}

/// A view config in the document's form: fields named by name become object ids of `table`'s
/// fields ([`view_refs::config_to_kdm`]), and the editor's private keys are left out.
fn config_in(tx: &Tx<'_>, table: &ObjectId, config: &Value) -> Result<Value> {
    let mut names: HashMap<String, ObjectId> = HashMap::new();
    for field in Listing::read(tx, table, "fields")?.ids() {
        names.entry(string(tx, &field, "name")?).or_insert(field);
    }
    Ok(view_refs::config_to_kdm(config, &|name| names.get(name).cloned()))
}

/// The table whose views list holds `view`.
fn table_of_view(tx: &Tx<'_>, view: &ObjectId) -> Result<Option<ObjectId>> {
    for table in tx.objects_of_type(TABLE) {
        if tx.list_items(&table, "views")?.contains(view) {
            return Ok(Some(table));
        }
    }
    Ok(None)
}

/// The lists of a view whose items are its own: its filters, sorts and groups.
const VIEW_ITEM_LISTS: [&str; 3] = ["filters", "sorts", "groups"];

/// Deletes a view's filters, sorts and groups.
fn delete_view_items(tx: &mut Tx<'_>, view: &ObjectId) -> Result<()> {
    for list in VIEW_ITEM_LISTS {
        for item in tx.list_items(view, list)? {
            if tx.exists(&item) {
                tx.delete(&item)?;
            }
        }
    }
    Ok(())
}

/// Sets a column width, clamped to [`MIN_WIDTH`]`..=`[`MAX_WIDTH`].
pub fn set_view_width(tx: &mut Tx<'_>, view: &ObjectId, field: &ObjectId, width: i64) -> Result<()> {
    require(tx, view, VIEW)?;
    let width = json!(width.clamp(MIN_WIDTH, MAX_WIDTH));
    if tx.map_get(view, "widths", field.as_str())? != width {
        tx.map_set(view, "widths", field.as_str(), width)?;
    }
    Ok(())
}

/// Hides or shows a column: an entry of the view's hidden columns (`View.hiddenFields`, field id
/// to its place among them), added after the others or removed.
pub fn set_view_hidden(tx: &mut Tx<'_>, view: &ObjectId, field: &ObjectId, hidden: bool) -> Result<()> {
    require(tx, view, VIEW)?;
    let map = super::view_store::HIDDEN_FIELDS;
    let held = !tx.map_get(view, map, field.as_str())?.is_null();
    if hidden && !held {
        let next = tx.map_entries(view, map)?.iter().filter_map(|(_, place)| place.as_i64()).max().map_or(0, |last| last + 1);
        tx.map_set(view, map, field.as_str(), json!(next))?;
    } else if !hidden && held {
        tx.map_delete(view, map, field.as_str())?;
    }
    Ok(())
}

pub fn delete_view(tx: &mut Tx<'_>, table: &ObjectId, view: &ObjectId) -> Result<()> {
    require(tx, table, TABLE)?;
    if !remove_all(tx, table, "views", view)? {
        return Err(TableFileError::NotFound);
    }
    if tx.exists(view) {
        delete_view_items(tx, view)?;
        tx.delete(view)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Records.

/// Creates one record per row at `at` (`None`: at the end), in order, with `createdAt = now`
/// (Unix milliseconds) and `createdBy = created_by`. Rows are keyed by field object id; keys that
/// are not stored fields of the table (unknown or computed) and `null` values are skipped.
/// Required fields are not checked here. An anchor that is not a record of the table is
/// [`TableFileError::NotFound`].
pub fn create_records(
    tx: &mut Tx<'_>,
    table: &ObjectId,
    at: Option<&Anchor>,
    rows: &[Map<String, Value>],
    now: i64,
    created_by: &str,
) -> Result<Vec<ObjectId>> {
    require(tx, table, TABLE)?;
    let stored = stored_fields(tx, table)?;
    // As `Listing` reads it, without looking up every listed record: an anchor's raw index is its
    // first entry, when the anchor exists.
    let records = tx.list_items(table, "records")?;
    let raw_of = |anchor: &ObjectId| records.iter().position(|item| item == anchor).filter(|_| tx.exists(anchor)).ok_or(TableFileError::NotFound);
    let start = match at {
        None => records.len(),
        Some(Anchor::Before(anchor)) => raw_of(anchor)?,
        Some(Anchor::After(anchor)) => raw_of(anchor)? + 1,
    };
    let mut ids = Vec::with_capacity(rows.len());
    let mut encoder = Encoder::default();
    for (index, row) in (start..).zip(rows) {
        let id = crate::ids::record();
        let values = registers(json!({ "createdAt": now, "createdBy": created_by }));
        tx.create_with_id(RECORD, &id, values)?;
        tx.list_insert(table, "records", index, &id)?;
        for (key, value) in row {
            if let Some(kind) = stored.get(key.as_str()).copied()
                && !value.is_null()
            {
                let field = ObjectId::from_string(key.clone());
                let value = encoder.encode(tx, &id, &field, kind, &Value::Null, value, &Texts::default())?;
                tx.map_set(&id, "cells", key, value)?;
            }
        }
        ids.push(id);
    }
    Ok(ids)
}

/// Gives every object what a create gives it and a raw model create may leave out (the
/// schema's `optional` fields of [`TABLE`], [`FIELD`] and [`RECORD`]): a table's `description`
/// and a field's `formula` are `""` (a receipt leaves empty strings out of a created object's
/// values), and a record without `createdAt` gets the time now and `created_by`, as
/// [`create_records`] gives them. Writes nothing when every object has them.
pub fn settle_created(tx: &mut Tx<'_>, created_by: &str) -> Result<()> {
    for (ty, fields) in [(TABLE, &["description"][..]), (FIELD, &["formula"][..])] {
        for id in tx.objects_of_type(ty) {
            for field in fields {
                if tx.get(&id, field).unwrap_or(Value::Null).is_null() {
                    tx.set(&id, field, json!(""))?;
                }
            }
        }
    }
    for id in tx.objects_of_type(RECORD) {
        if tx.get(&id, "createdAt").unwrap_or(Value::Null).is_null() {
            tx.set(&id, "createdAt", json!(kition_time::now_millis()))?;
            if tx.get(&id, "createdBy").unwrap_or(Value::Null).is_null() {
                tx.set(&id, "createdBy", json!(created_by))?;
            }
        }
    }
    Ok(())
}

/// Writes cells of one record of `table`, keyed by field object id: one write per cell whose
/// value differs from the stored one (`null` clears). Keys that are not stored fields of the
/// table (unknown or computed) are skipped. A record the table does not list is
/// [`TableFileError::NotFound`].
pub fn set_cells(tx: &mut Tx<'_>, table: &ObjectId, record: &ObjectId, cells: &Map<String, Value>) -> Result<()> {
    set_cells_many(tx, table, &[(record.clone(), cells.clone())])
}

/// [`set_cells`] for many records of one table, reading the table once. A long text is written
/// whole (see [`set_cells_known`]).
pub fn set_cells_many(tx: &mut Tx<'_>, table: &ObjectId, edits: &[(ObjectId, Map<String, Value>)]) -> Result<()> {
    set_cells_known(tx, table, edits, &Texts::default())
}

/// [`set_cells_many`] knowing what the records' long texts read now ([`Texts::of_records`]): a
/// long text then changes only where it differs, so concurrent typing elsewhere in it stays.
/// Cleared cells drop their `CellText`.
pub fn set_cells_known(tx: &mut Tx<'_>, table: &ObjectId, edits: &[(ObjectId, Map<String, Value>)], texts: &Texts) -> Result<()> {
    require(tx, table, TABLE)?;
    // The edited records the table lists and that exist (`Listing::contains`), without looking up
    // every listed record.
    let edited: HashSet<&ObjectId> = edits.iter().map(|(record, _)| record).collect();
    let listed: HashSet<ObjectId> = tx.list_items(table, "records")?.into_iter().filter(|item| edited.contains(item)).collect();
    let stored = stored_fields(tx, table)?;
    let mut encoder = Encoder::default();
    for (record, values) in edits {
        if !listed.contains(record) || !tx.exists(record) {
            return Err(TableFileError::NotFound);
        }
        for (key, value) in values {
            let Some(kind) = stored.get(key.as_str()).copied() else {
                continue;
            };
            let current = tx.map_get(record, "cells", key)?;
            let old_text = cells::cell_text_in(tx, &current);
            if value.is_null() {
                if !current.is_null() {
                    tx.map_delete(record, "cells", key)?;
                }
                if let Some(old) = old_text {
                    tx.delete(&old)?;
                }
                continue;
            }
            let field = ObjectId::from_string(key.clone());
            let value = encoder.encode(tx, record, &field, kind, &current, value, texts)?;
            if !json::equal(&current, &value) {
                tx.map_set(record, "cells", key, value)?;
                if let Some(old) = old_text {
                    tx.delete(&old)?;
                }
            }
        }
    }
    Ok(())
}

/// Deletes records of the table and every link from or to them. Ids that are not records of the
/// table are skipped. Answers how many records were deleted.
pub fn delete_records(tx: &mut Tx<'_>, table: &ObjectId, records: &[ObjectId]) -> Result<usize> {
    require(tx, table, TABLE)?;
    let doomed: HashSet<&ObjectId> = records.iter().collect();
    let items = tx.list_items(table, "records")?;
    let mut deleted = HashSet::new();
    for (index, item) in items.iter().enumerate().rev() {
        if doomed.contains(item) {
            tx.list_remove(table, "records", index)?;
            deleted.insert(item.clone());
        }
    }
    if deleted.is_empty() {
        return Ok(0);
    }
    for link in all_links(tx)? {
        if deleted.contains(&link.from) || deleted.contains(&link.to) {
            tx.delete(&link.id)?;
        }
    }
    let mut count = 0;
    for record in &deleted {
        if tx.exists(record) {
            for (_, raw) in tx.map_entries(record, "cells")? {
                if let Some(text) = cells::cell_text_in(tx, &raw) {
                    tx.delete(&text)?;
                }
            }
            tx.delete(record)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Moves `records` (in the given order) to just before `before` (`None`: to the end), with one
/// list move per record that is not already in place. Every record, and `before`, must be in the
/// table ([`TableFileError::NotFound`]); a `before` among the moved records means the first record
/// after it that is not moved.
pub fn move_records(tx: &mut Tx<'_>, table: &ObjectId, records: &[ObjectId], before: Option<&ObjectId>) -> Result<()> {
    require(tx, table, TABLE)?;
    let listing = Listing::read(tx, table, "records")?;
    let moved = distinct(records);
    if moved.iter().any(|record| !listing.contains(record)) {
        return Err(TableFileError::NotFound);
    }
    let moving: HashSet<&ObjectId> = moved.iter().collect();
    let anchor = match before {
        None => None,
        Some(before) => {
            let live = listing.ids();
            let start = live.iter().position(|item| item == before).ok_or(TableFileError::NotFound)?;
            live.into_iter().skip(start).find(|item| !moving.contains(item))
        }
    };
    place_items(tx, table, "records", listing.raw, &moved, anchor)
}

// ---------------------------------------------------------------------------
// Links.

/// Makes the links of `field` from `from` go to `targets` (duplicates ignored), creating and
/// deleting `Link` objects only for the difference. Links read back in creation order, so new
/// targets follow kept ones. `field` must be a `link_to_record` field
/// ([`TableFileError::Invalid`]); `from` and every target must be records
/// ([`TableFileError::NotFound`]).
///
/// Concurrent replaces of one field of one record merge as a union: each replica only adds and
/// removes its own difference. For a single-target field (`allow_multiple: false`),
/// [`super::repair`] then keeps the link with the lowest id.
pub fn replace_links(tx: &mut Tx<'_>, from: &ObjectId, field: &ObjectId, targets: &[ObjectId]) -> Result<()> {
    replace_links_many(tx, field, &[(from.clone(), targets.to_vec())])
}

/// [`replace_links`] for many records and one field, scanning the links once.
///
/// A link field with a symmetric field (its `symmetric_field`, or a field of the target table
/// whose `symmetric_field` is this one, linking back to this field's table) shows the other side
/// of each link: every link added or removed here is added to or removed from the target's
/// symmetric field. For a single-target symmetric field the new link replaces the target's other
/// link, and that record loses its link back.
pub fn replace_links_many(tx: &mut Tx<'_>, field: &ObjectId, edits: &[(ObjectId, Vec<ObjectId>)]) -> Result<()> {
    replace_links_mirrored(tx, field, edits, 0)
}

/// How many times an edit is mirrored: the symmetric field, then back for the links a
/// single-target symmetric field replaced.
const MIRROR_DEPTH: usize = 2;

fn replace_links_mirrored(tx: &mut Tx<'_>, field: &ObjectId, edits: &[(ObjectId, Vec<ObjectId>)], depth: usize) -> Result<()> {
    let changes = replace_links_once(tx, field, edits)?;
    if depth >= MIRROR_DEPTH || changes.is_empty() {
        return Ok(());
    }
    let Some(partner) = symmetric_partner(tx, field)? else {
        return Ok(());
    };
    // Each touched target's links in the partner field, with the change applied.
    let mut current: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for link in all_links(tx)? {
        if link.field == partner {
            current.entry(link.from).or_default().push(link.to);
        }
    }
    let mut mirrored: Vec<(ObjectId, Vec<ObjectId>)> = Vec::new();
    for (from, target, added) in changes {
        let at = match mirrored.iter().position(|(record, _)| *record == target) {
            Some(at) => at,
            None => {
                mirrored.push((target.clone(), current.remove(&target).unwrap_or_default()));
                mirrored.len() - 1
            }
        };
        let Some((_, targets)) = mirrored.get_mut(at) else { continue };
        if added {
            if is_single_target(tx, &partner)? {
                targets.clear();
            }
            if !targets.contains(&from) {
                targets.push(from);
            }
        } else {
            targets.retain(|other| *other != from);
        }
    }
    replace_links_mirrored(tx, &partner, &mirrored, depth + 1)
}

/// The link field that shows the other side of `field`'s links, if any: its `symmetric_field`,
/// else a link field that names it as its own; either way one of the target table's link fields
/// that links back to `field`'s table.
fn symmetric_partner(tx: &Tx<'_>, field: &ObjectId) -> Result<Option<ObjectId>> {
    let option = |id: &ObjectId, key: &str| -> Result<Option<ObjectId>> {
        Ok(tx.get(id, "options")?.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(ObjectId::from_string))
    };
    let is_link = |id: &ObjectId| -> Result<bool> { Ok(is_a(tx, id, FIELD) && string(tx, id, "type")? == "link_to_record") };
    let declared = match option(field, "symmetric_field")? {
        Some(partner) if is_link(&partner)? => Some(partner),
        _ => {
            let mut naming = None;
            for other in tx.objects_of_type(FIELD) {
                if other != *field && is_link(&other)? && option(&other, "symmetric_field")?.as_ref() == Some(field) {
                    naming = Some(other);
                    break;
                }
            }
            naming
        }
    };
    let Some(partner) = declared else {
        return Ok(None);
    };
    let (Some(own_table), Some(partner_table)) = (table_of_field(tx, field)?, table_of_field(tx, &partner)?) else {
        return Ok(None);
    };
    let links_back =
        option(&partner, "target_table")?.as_ref() == Some(&own_table) && option(field, "target_table")?.as_ref() == Some(&partner_table);
    Ok(links_back.then_some(partner))
}

/// Replaces the links of one field; answers each change as `(from, to, added)`.
fn replace_links_once(tx: &mut Tx<'_>, field: &ObjectId, edits: &[(ObjectId, Vec<ObjectId>)]) -> Result<Vec<(ObjectId, ObjectId, bool)>> {
    require(tx, field, FIELD)?;
    if string(tx, field, "type")? != "link_to_record" {
        return Err(TableFileError::invalid(format!("field {} is not link_to_record", string(tx, field, "name")?)));
    }
    let single = is_single_target(tx, field)?;
    let mut changes = Vec::new();
    let mut existing: HashMap<ObjectId, Vec<LinkEnds>> = HashMap::new();
    for link in all_links(tx)? {
        if link.field == *field {
            existing.entry(link.from.clone()).or_default().push(link);
        }
    }
    for (from, targets) in edits {
        require(tx, from, RECORD)?;
        let targets = distinct(targets);
        if single && targets.len() > 1 {
            return Err(TableFileError::invalid("field does not allow multiple link targets"));
        }
        for target in &targets {
            require(tx, target, RECORD)?;
        }
        let wanted: HashSet<&ObjectId> = targets.iter().collect();
        let mut kept = HashSet::new();
        for link in existing.remove(from).unwrap_or_default() {
            if wanted.contains(&link.to) && !kept.contains(&link.to) {
                kept.insert(link.to);
            } else {
                tx.delete(&link.id)?;
                if !wanted.contains(&link.to) {
                    changes.push((from.clone(), link.to, false));
                }
            }
        }
        for target in &targets {
            if !kept.contains(target) {
                add_link(tx, from, field, target)?;
                changes.push((from.clone(), target.clone(), true));
            }
        }
    }
    Ok(changes)
}

/// Creates one `Link` object without looking at existing links: for bulk builds (imports,
/// migrations) that know they create each link once. Edits use [`replace_links`].
pub fn add_link(tx: &mut Tx<'_>, from: &ObjectId, field: &ObjectId, to: &ObjectId) -> Result<ObjectId> {
    let values = registers(json!({
        "from": from.as_str(),
        "field": field.as_str(),
        "to": to.as_str(),
    }));
    Ok(tx.create_with_id(LINK, &crate::ids::object(), values)?)
}

/// A `Link` object's ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkEnds {
    pub id: ObjectId,
    pub from: ObjectId,
    pub field: ObjectId,
    pub to: ObjectId,
}

/// Every link of the document, sorted by id. One scan of the document's objects.
pub(crate) fn all_links(tx: &Tx<'_>) -> Result<Vec<LinkEnds>> {
    let mut links = Vec::new();
    for id in tx.objects_of_type(LINK) {
        let end = |name: &str| -> Result<ObjectId> { Ok(ObjectId::from_string(tx.get(&id, name)?.as_str().unwrap_or_default())) };
        let (from, field, to) = (end("from")?, end("field")?, end("to")?);
        links.push(LinkEnds { id, from, field, to });
    }
    Ok(links)
}

/// Whether a link field takes one target (`options.allow_multiple: false`).
pub(crate) fn is_single_target(tx: &Tx<'_>, field: &ObjectId) -> Result<bool> {
    Ok(tx.get(field, "options")?.get("allow_multiple") == Some(&Value::Bool(false)))
}

// ---------------------------------------------------------------------------
// Dashboards.

/// Creates or updates a dashboard of the base. `config` is the dashboard without its widgets
/// (title, order, layout, `source_table` as an object id; see [`super::options`]); `widgets` are
/// keyed by widget id, their queries naming fields by object id ([`super::widgets`]). The layout's
/// entries are kept with their widgets (`Widget.layout`).
/// `id: None` creates one with a new id; an id that names no object creates one under that id
/// (it must match the KDM object id pattern). On update, `name` and `config` are set when they
/// differ and widgets are diffed key by key: changed widgets are written, missing ones deleted.
/// `null` widgets are skipped. A dashboard missing from the base's list is (re)listed last.
pub fn upsert_dashboard(
    tx: &mut Tx<'_>,
    base: &ObjectId,
    id: Option<&ObjectId>,
    name: &str,
    config: Value,
    widgets: &Map<String, Value>,
) -> Result<ObjectId> {
    require(tx, base, BASE)?;
    let config = object_ids_only(decode_json(&config)?)?;
    let (config, layout) = super::widgets::split_layout(&config);
    let config = if config.as_object().is_some_and(Map::is_empty) { Value::Null } else { config };
    let dashboard = match id {
        Some(id) if tx.exists(id) => {
            require(tx, id, DASHBOARD)?;
            set_if_changed(tx, id, "name", json!(name))?;
            set_if_changed(tx, id, "config", config)?;
            id.clone()
        }
        chosen => {
            let mut values = registers(json!({ "name": name }));
            if !config.is_null() {
                values.insert("config".into(), config);
            }
            match chosen {
                Some(id) => tx.create_with_id(DASHBOARD, id, values)?,
                None => tx.create_with_id(DASHBOARD, &crate::ids::object(), values)?,
            }
        }
    };
    super::widgets::store(tx, &dashboard, widgets, &layout)?;
    let listed = tx.list_items(base, "dashboards")?;
    if !listed.contains(&dashboard) {
        tx.list_insert(base, "dashboards", listed.len(), &dashboard)?;
    }
    Ok(dashboard)
}

pub fn delete_dashboard(tx: &mut Tx<'_>, base: &ObjectId, id: &ObjectId) -> Result<()> {
    require(tx, base, BASE)?;
    if !remove_all(tx, base, "dashboards", id)? {
        return Err(TableFileError::NotFound);
    }
    if tx.exists(id) {
        for widget in tx.list_items(id, "widgets")? {
            if tx.exists(&widget) {
                tx.delete(&widget)?;
            }
        }
        tx.delete(id)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers shared with `repair`.

/// A list as the model reads it: the raw items, and the live ones (naming existing objects,
/// first occurrence only) with their raw index.
pub(crate) struct Listing {
    pub raw: Vec<ObjectId>,
    pub live: Vec<(usize, ObjectId)>,
    first: HashMap<ObjectId, usize>,
}

impl Listing {
    pub fn read(tx: &Tx<'_>, id: &ObjectId, field: &str) -> Result<Self> {
        let raw = tx.list_items(id, field)?;
        let mut first = HashMap::with_capacity(raw.len());
        let mut live = Vec::with_capacity(raw.len());
        for (index, item) in raw.iter().enumerate() {
            if !first.contains_key(item) && tx.exists(item) {
                first.insert(item.clone(), index);
                live.push((index, item.clone()));
            }
        }
        Ok(Self { raw, live, first })
    }

    /// The live items in order.
    pub fn ids(&self) -> Vec<ObjectId> {
        self.live.iter().map(|(_, id)| id.clone()).collect()
    }

    pub fn contains(&self, id: &ObjectId) -> bool {
        self.first.contains_key(id)
    }

    /// The raw index at which an insert makes the new item the `position`-th live item (past
    /// the end: last).
    pub fn insert_index(&self, position: usize) -> usize {
        self.live.get(position).map_or(self.raw.len(), |(index, _)| *index)
    }
}

/// Whether `id` is a live object of type `ty`.
pub(crate) fn is_a(tx: &Tx<'_>, id: &ObjectId, ty: &str) -> bool {
    tx.object_type(id).is_ok_and(|found| found == ty)
}

/// [`TableFileError::NotFound`] unless `id` is a live object of type `ty`.
pub(crate) fn require(tx: &Tx<'_>, id: &ObjectId, ty: &str) -> Result<()> {
    if is_a(tx, id, ty) { Ok(()) } else { Err(TableFileError::NotFound) }
}

pub(crate) fn string(tx: &Tx<'_>, id: &ObjectId, field: &str) -> Result<String> {
    Ok(tx.get(id, field)?.as_str().unwrap_or_default().to_owned())
}

/// `base`, or `base_2`, `base_3`, ... : the first that is not taken.
pub(crate) fn unique_name(base: &str, taken: &HashSet<String>) -> String {
    let mut name = base.to_owned();
    let mut suffix = 2;
    while taken.contains(&name) {
        name = format!("{base}_{suffix}");
        suffix += 1;
    }
    name
}

/// Sets a register unless it already holds an equal value.
pub(crate) fn set_if_changed(tx: &mut Tx<'_>, id: &ObjectId, field: &str, value: Value) -> Result<()> {
    if !json::equal(&tx.get(id, field)?, &value) {
        tx.set(id, field, value)?;
    }
    Ok(())
}

/// The names of the objects `ids`.
fn names_of(tx: &Tx<'_>, ids: &[ObjectId]) -> Result<HashSet<String>> {
    ids.iter().map(|id| string(tx, id, "name")).collect()
}

/// The ids of the table's live fields whose values are stored in cells, with how they store them.
fn stored_fields(tx: &Tx<'_>, table: &ObjectId) -> Result<HashMap<String, CellKind>> {
    let mut stored = HashMap::new();
    for field in Listing::read(tx, table, "fields")?.ids() {
        let field_type = string(tx, &field, "type")?;
        if !is_computed(&field_type) {
            stored.insert(field.to_string(), CellKind::of(&field_type));
        }
    }
    Ok(stored)
}

/// The table whose fields list holds `field`.
fn table_of_field(tx: &Tx<'_>, field: &ObjectId) -> Result<Option<ObjectId>> {
    for table in tx.objects_of_type(TABLE) {
        if tx.list_items(&table, "fields")?.contains(field) {
            return Ok(Some(table));
        }
    }
    Ok(None)
}

/// Removes every occurrence of `item` from a list; answers whether there was one.
fn remove_all(tx: &mut Tx<'_>, id: &ObjectId, field: &str, item: &ObjectId) -> Result<bool> {
    let items = tx.list_items(id, field)?;
    let mut found = false;
    for (index, current) in items.iter().enumerate().rev() {
        if current == item {
            tx.list_remove(id, field, index)?;
            found = true;
        }
    }
    Ok(found)
}

/// Moves `moved` (in order, all in `items`, `items` being the list's raw content) to just before
/// `anchor` (an item not moved; `None`: the end), with at most one list move per occurrence.
/// Each moved item, from the last, is placed right before its successor in the target order, so
/// items not moved keep their order. Every occurrence of a moved item moves, so its first
/// occurrence (the one readers use) is where it was put; no occurrence is removed, which keeps
/// concurrent moves from losing an item.
pub(crate) fn place_items(
    tx: &mut Tx<'_>,
    id: &ObjectId,
    field: &str,
    mut items: Vec<ObjectId>,
    moved: &[ObjectId],
    anchor: Option<ObjectId>,
) -> Result<()> {
    let first = |items: &[ObjectId], wanted: &ObjectId| items.iter().position(|item| item == wanted).ok_or(TableFileError::NotFound);
    let mut successor = anchor;
    for item in moved.iter().rev() {
        let count = items.iter().filter(|current| *current == item).count();
        for placed in 0..count {
            let before = match &successor {
                Some(next) => first(&items, next)?,
                None => items.len(),
            };
            // The occurrences already placed sit just before `before`.
            let block = before.saturating_sub(placed)..before;
            let from =
                items.iter().enumerate().position(|(index, current)| current == item && !block.contains(&index)).ok_or(TableFileError::NotFound)?;
            let to = if from < before { before - 1 } else { before };
            if from != to {
                tx.list_move(id, field, from, to)?;
                let value = items.remove(from);
                items.insert(to, value);
            }
        }
        successor = Some(item.clone());
    }
    Ok(())
}

/// The ids, first occurrence only.
fn distinct(ids: &[ObjectId]) -> Vec<ObjectId> {
    let mut seen = HashSet::new();
    ids.iter().filter(|id| seen.insert(*id)).cloned().collect()
}

fn check_field_type(field_type: &str) -> Result<()> {
    if FIELD_TYPES.contains(&field_type) {
        Ok(())
    } else {
        Err(TableFileError::invalid(format!("unsupported field type {field_type:?}: `type` must be one of {}", FIELD_TYPES.join(", "))))
    }
}

/// The `ai_config` key that holds the instruction of an AI action, which the document keeps as
/// the field's `aiPrompt` text (so it merges per character), not in the options.
pub(crate) fn instruction_key(ai_config: &Value) -> Option<&'static str> {
    let action = ai_config.get("type").or_else(|| ai_config.get("action_type")).and_then(Value::as_str)?;
    match action.trim() {
        "customize" | "text_generation" | "image_generation" | "image_customization" => Some("prompt"),
        "extract" => Some("extract_instruction"),
        "improve" => Some("improve_instruction"),
        "summarize" => Some("summary_instruction"),
        _ => None,
    }
}

/// Takes the instruction out of `options.ai_config` (`prompt`, or `custom_prompt`, for the
/// actions that write one; the action's own instruction key otherwise): the options left and the
/// instruction, `Some("")` for an action whose config has none, `None` when the options configure
/// no action with an instruction.
fn split_instruction(options: Value) -> (Value, Option<String>) {
    let mut options = options;
    let Some(config) = options.get_mut("ai_config").and_then(Value::as_object_mut) else {
        return (options, None);
    };
    let Some(key) = instruction_key(&Value::Object(config.clone())) else {
        return (options, None);
    };
    let mut text = config.remove(key).and_then(|value| value.as_str().map(str::to_owned));
    if key == "prompt"
        && let Some(alias) = config.remove("custom_prompt")
        && text.as_deref().is_none_or(str::is_empty)
    {
        text = alias.as_str().map(str::to_owned);
    }
    (options, Some(text.unwrap_or_default()))
}

/// Puts the field's instruction (`aiPrompt`) back into `options.ai_config` under its action's key,
/// as readers see the options.
pub(crate) fn with_instruction(options: Value, prompt: &str) -> Value {
    let mut options = options;
    if prompt.is_empty() {
        return options;
    }
    if let Some(config) = options.get_mut("ai_config").and_then(Value::as_object_mut)
        && let Some(key) = instruction_key(&Value::Object(config.clone()))
    {
        config.insert(key.to_owned(), json!(prompt));
    }
    options
}

/// Formula and AI values are written by Kition, never typed in.
fn forces_readonly(field_type: &str) -> bool {
    field_type == "formula" || field_type == "ai"
}

/// The registers of a new object, from a JSON object literal.
fn registers(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Adds an optional JSON register unless it is empty.
fn put_json(values: &mut Map<String, Value>, field: &str, value: &Value) -> Result<()> {
    let value = decode_json(value)?;
    if !value.is_null() {
        values.insert(field.to_owned(), value);
    }
    Ok(())
}

/// Refuses options or a config naming tables or fields by relational id
/// ([`TableFileError::Invalid`]): the document holds object ids; convert with
/// [`super::options::options_to_kdm`].
fn object_ids_only(value: Value) -> Result<Value> {
    if let Some(key) = super::options::relational_key(&value) {
        return Err(TableFileError::invalid(format!("option {key} is a relational id; convert options with options_to_kdm")));
    }
    Ok(value)
}

/// A JSON document as the relational API accepts it: a value, or a string holding encoded JSON
/// (blank: none). `null` means none. An invalid string is [`TableFileError::Invalid`].
fn decode_json(value: &Value) -> Result<Value> {
    match value {
        Value::String(raw) => {
            let raw = raw.trim();
            if raw.is_empty() {
                return Ok(Value::Null);
            }
            serde_json::from_str(raw).map_err(|_| TableFileError::invalid("invalid json string"))
        }
        other => Ok(other.clone()),
    }
}
