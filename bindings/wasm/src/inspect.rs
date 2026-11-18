//! What an agent reads of a table document (KHP A52): `inspect()` answers its tables with their
//! fields, views, record counts and first records, and its dashboards; `table.read` answers the
//! records of one table or one view, a page at a time (A24a, A25). The kits server passes an
//! inspect `target` on to no kit command but a spreadsheet's or a text document's reader, so
//! the part is read with that command instead, as the answer's `next` step says.
//!
//! Objects expose stable UUIDs as `tool_id`, accepted by every corresponding command parameter;
//! numeric ids remain local projection addresses. Dashboards use their UUID directly, and
//! inspect's link chips retain local record addresses. Records are listed under
//! `columns`, the field names their values are keyed by in `table.records.create` and
//! `table.records.update`, one `cells` entry per column.
//!
//! Hosts that relay `inspect()` over the Kit Protocol leave out every `title` key (a session key
//! there), so each object's title is answered as `label`; a table's and a dashboard's `title`
//! stays beside it for the editor's shell, which reads it.
//!
//! The answer stays within a budget: [`ROWS_PER_TABLE`] records of a table and [`ROWS_IN_ALL`] of
//! the document, each text cut at [`CELL_CHARS`] characters. A table whose records are cut says
//! so and names the `table.read` step that lists the rest (`next`).

use kition_table_file::{Field, FilterTree, Record, Table, TableFile, View};
use kition_table_picture::Arrangement;
use serde_json::{Map, Value, json};

use crate::error;

/// The most records `inspect()` lists of one table.
pub const ROWS_PER_TABLE: usize = 20;
/// The most records `inspect()` lists in all.
pub const ROWS_IN_ALL: usize = 100;
/// The most characters of a text value `inspect()` and `table.read` answer; longer ones end in `…`.
pub const CELL_CHARS: usize = 200;
/// The page `table.read` answers without a `limit`, and the largest it takes.
pub const READ_LIMIT: usize = 50;
pub const MAX_READ_LIMIT: usize = 200;

/// The field options an agent needs to write values and follow links; the editor's display
/// settings are left out.
const OPTIONS: &[&str] = &[
    "choices",
    "target_table_id",
    "allow_multiple",
    "symmetric_field_id",
    "link_field_id",
    "target_field_id",
    "multi_value_mode",
    "aggregation",
    "max",
];

/// The document as `inspect()` answers it, without the session's keys.
pub fn overview(file: &TableFile) -> Result<Value, String> {
    let mut budget = ROWS_IN_ALL;
    let mut tables = Vec::new();
    for table in file.list_tables().map_err(error)? {
        let limit = budget.min(ROWS_PER_TABLE);
        let (listed, shown) = table_json(file, &table, limit)?;
        budget -= shown;
        tables.push(listed);
    }
    let dashboards: Vec<Value> = file
        .list_dashboards()
        .map_err(error)?
        .iter()
        .map(|dashboard| {
            let mut entry = json!({ "id": dashboard.id, "title": dashboard.name, "label": dashboard.name, "widgets": dashboard.widgets.len() });
            if let Some(source) = dashboard.config.get("source_table_id").filter(|source| !source.is_null()) {
                entry["sourceTableId"] = source.clone();
            }
            entry
        })
        .collect();
    Ok(json!({ "tables": tables, "dashboards": dashboards }))
}

/// One table: its fields, views, record count and up to `limit` records; and how many it listed.
fn table_json(file: &TableFile, table: &Table, limit: usize) -> Result<(Value, usize), String> {
    let total = usize::try_from(file.count_records(table.id).map_err(error)?).unwrap_or_default();
    let records = if limit == 0 { Vec::new() } else { file.list_records_page(table.id, limit as i64, 0).map_err(error)? };
    let views = table.views.iter().map(|view| view_json(file, table, view)).collect::<Result<Vec<_>, _>>()?;
    let mut listed = json!({
        "id": table.id,
        "tool_id": crate::address::tool(file, "Table", &json!(table.id)),
        "name": table.name,
        "title": table.title,
        "label": table.title,
        "primaryFieldId": table.primary_field_id,
        "recordCount": total,
        "fields": table.fields.iter().map(|field| field_json(file, field)).collect::<Vec<_>>(),
        "views": views,
    });
    if !table.description.is_empty() {
        listed["description"] = Value::String(table.description.clone());
    }
    let shown = records.len();
    listed["records"] = rows_json(file, &table.fields, &records, 0, total, json!({ "tableId": table.id }));
    Ok((listed, shown))
}

/// A field: its id, name (the key of its values), label, type, and the options that matter.
fn field_json(file: &TableFile, field: &Field) -> Value {
    let mut listed = json!({ "id": field.id, "name": field.name, "label": field.title, "type": field.field_type });
    crate::address::label(file, "Field", &mut listed);
    if field.is_primary {
        listed["primary"] = Value::Bool(true);
    }
    if field.required {
        listed["required"] = Value::Bool(true);
    }
    if field.readonly {
        listed["readonly"] = Value::Bool(true);
    }
    let options: Map<String, Value> = OPTIONS
        .iter()
        .filter_map(|key| field.options.get(*key).filter(|value| !value.is_null()).map(|value| ((*key).to_owned(), value.clone())))
        .collect();
    if !options.is_empty() {
        listed["options"] = Value::Object(options);
    }
    if !field.formula.is_empty() {
        listed["formula"] = Value::String(field.formula.clone());
    }
    listed
}

/// A view: its id, label and type, and what it filters, sorts, groups by and hides, by field name.
fn view_json(file: &TableFile, table: &Table, view: &View) -> Result<Value, String> {
    let mut listed = json!({ "id": view.id, "label": view.title, "type": view.view_type });
    crate::address::label(file, "View", &mut listed);
    let arrangement = Arrangement::of(file, &table.fields, Some(view))?;
    if let Some(tree) = &arrangement.filter
        && arrangement.filters() > 0
    {
        listed["filter"] = Value::String(filter_text(tree, &table.fields));
    }
    let ordered = |items: &[(Field, bool)]| -> Vec<Value> {
        items.iter().map(|(field, descending)| Value::String(format!("{} {}", field.name, if *descending { "desc" } else { "asc" }))).collect()
    };
    if !arrangement.sorts.is_empty() {
        listed["sorts"] = Value::Array(ordered(&arrangement.sorts));
    }
    if !arrangement.groups.is_empty() {
        listed["groups"] = Value::Array(ordered(&arrangement.groups));
    }
    let named: Vec<&str> = view.config.get("hidden_field_names").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let layouts = file.list_view_fields(view.id).map_err(error)?;
    let hidden: Vec<Value> = table
        .fields
        .iter()
        .filter(|field| named.contains(&field.name.as_str()) || layouts.iter().any(|layout| layout.field_id == field.id && !layout.visible))
        .map(|field| Value::String(field.name.clone()))
        .collect();
    if !hidden.is_empty() {
        listed["hidden"] = Value::Array(hidden);
    }
    Ok(listed)
}

/// A filter tree in one line, by field name: `status neq "Done" and (priority gt 2 or …)`.
fn filter_text(tree: &FilterTree, fields: &[Field]) -> String {
    let joiner = if tree.conjunction == "or" { " or " } else { " and " };
    let parts: Vec<String> = tree
        .children
        .iter()
        .filter_map(|child| match (&child.group, &child.condition) {
            (Some(group), _) => Some(format!("({})", filter_text(group, fields))),
            (None, Some(rule)) => {
                let name = fields
                    .iter()
                    .find(|field| field.id == rule.field_id)
                    .map_or_else(|| format!("field {}", rule.field_id), |field| field.name.clone());
                Some(if rule.value.is_null() { format!("{name} {}", rule.operator) } else { format!("{name} {} {}", rule.operator, rule.value) })
            }
            _ => None,
        })
        .collect();
    parts.join(joiner)
}

/// Records under their columns: `{columns, rows: [{id, cells}], from, to, total, next?}`; `from`
/// and `to` count the listed records from 1 among `total`, and `next` is the `table.read` step for
/// the records after them (`part` names the table or view: `{tableId}` or `{viewId}`).
fn rows_json(file: &TableFile, fields: &[Field], records: &[Record], offset: usize, total: usize, part: Value) -> Value {
    let columns: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    let rows: Vec<Value> = records
        .iter()
        .map(|record| {
            let cells: Vec<Value> = fields.iter().map(|field| cell(file, field, record.values.get(&field.name))).collect();
            json!({ "id": record.id, "tool_id": record.row_key, "cells": cells })
        })
        .collect();
    let mut listed = json!({ "columns": columns, "rows": rows, "total": total });
    if !records.is_empty() {
        listed["from"] = json!(offset + 1);
        listed["to"] = json!(offset + records.len());
    }
    let after = offset + records.len();
    if after < total {
        let mut params = part;
        params["cursor"] = Value::String(after.to_string());
        listed["next"] = json!({ "command": READ_COMMAND, "params": params });
    }
    listed
}

/// The command that lists one table's or view's records.
pub const READ_COMMAND: &str = "table.read";

/// A value as the commands take it: a link's records by record id (`{id, display}`), and text cut
/// at [`CELL_CHARS`] characters.
fn cell(file: &TableFile, field: &Field, value: Option<&Value>) -> Value {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Value::Null;
    };
    if field.field_type == "link_to_record" {
        let linked = value.as_array().into_iter().flatten().map(|item| {
            let key = item.get("row_key").and_then(Value::as_str).unwrap_or_default();
            let id = file
                .conn()
                .query_row("SELECT id FROM kition_records WHERE row_key = ?1 AND deleted_at IS NULL", [key], |row| row.get::<_, i64>(0))
                .ok();
            json!({ "id": id, "tool_id": key, "display": item.get("display").cloned().unwrap_or(Value::Null) })
        });
        return Value::Array(linked.collect());
    }
    cut(value)
}

/// `value` with every string longer than [`CELL_CHARS`] characters cut, ending in `…`.
fn cut(value: &Value) -> Value {
    match value {
        Value::String(text) if text.chars().count() > CELL_CHARS => Value::String(format!("{}…", text.chars().take(CELL_CHARS).collect::<String>())),
        Value::Array(items) => Value::Array(items.iter().map(cut).collect()),
        Value::Object(map) => Value::Object(map.iter().map(|(key, value)| (key.clone(), cut(value))).collect()),
        other => other.clone(),
    }
}

/// `table.read`: `{ tableId? | viewId?, limit?, cursor? }` answers the records of a table (in
/// order) or of a view (filtered and in its order), `limit` at a time (default [`READ_LIMIT`], at
/// most [`MAX_READ_LIMIT`]) from `cursor` (a previous answer's `next`), as
/// `{ tableId, viewId?, columns, rows, from, to, total, next? }`.
pub fn read(file: &TableFile, params: &Value) -> Result<Value, String> {
    let id = |key: &str| -> Result<Option<i64>, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value.as_i64().filter(|id| *id > 0).map(Some).ok_or_else(|| format!("invalid params: {key} is a positive integer")),
        }
    };
    let (table_id, view_id) = (id("tableId")?, id("viewId")?);
    let tables = file.list_tables().map_err(error)?;
    let table = match (table_id, view_id) {
        (Some(table_id), _) => tables.iter().find(|table| table.id == table_id).ok_or_else(|| format!("not found: table {table_id}"))?,
        (None, Some(view_id)) => {
            tables.iter().find(|table| table.views.iter().any(|view| view.id == view_id)).ok_or_else(|| format!("not found: view {view_id}"))?
        }
        (None, None) => return Err("invalid params: name a table (tableId) or a view (viewId)".into()),
    };
    let limit = match params.get("limit") {
        None | Some(Value::Null) => READ_LIMIT,
        Some(value) => value
            .as_u64()
            .and_then(|limit| usize::try_from(limit).ok())
            .filter(|limit| (1..=MAX_READ_LIMIT).contains(limit))
            .ok_or_else(|| format!("invalid params: limit is an integer from 1 to {MAX_READ_LIMIT}"))?,
    };
    let offset = match params.get("cursor") {
        None | Some(Value::Null) => 0,
        Some(value) => value
            .as_str()
            .and_then(|cursor| cursor.parse::<usize>().ok())
            .ok_or("invalid params: cursor is the `next` cursor of a previous answer, such as \"50\"")?,
    };
    let records = kition_table_picture::view_records(file, table.id, view_id)?;
    let total = records.len();
    let page: Vec<Record> = records.into_iter().skip(offset).take(limit).collect();
    let part = match view_id {
        Some(view_id) => json!({ "viewId": view_id }),
        None => json!({ "tableId": table.id }),
    };
    let mut answer = rows_json(file, &table.fields, &page, offset, total, part);
    answer["tableId"] = json!(table.id);
    if let Some(view_id) = view_id {
        answer["viewId"] = json!(view_id);
    }
    Ok(answer)
}

/// The document in a line, for hosts' summaries: `2 tables (12 records), 1 dashboard`.
pub fn summary(file: &TableFile) -> String {
    let tables = file.list_tables().unwrap_or_default();
    let records: i64 = tables.iter().map(|table| file.count_records(table.id).unwrap_or_default()).sum();
    let dashboards = file.list_dashboards().map(|dashboards| dashboards.len()).unwrap_or_default();
    let count = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    let mut line =
        format!("{} ({})", count(tables.len(), "table", "tables"), count(usize::try_from(records).unwrap_or_default(), "record", "records"));
    if dashboards > 0 {
        line.push_str(&format!(", {}", count(dashboards, "dashboard", "dashboards")));
    }
    line
}
