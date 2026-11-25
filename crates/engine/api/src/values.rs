//! A request's record `values` name fields of their table.
//!
//! Record values are keyed by a field's `name`. A key that names no field of the table would be
//! dropped on the way into the file, so a request that has one is refused before anything is
//! written, with one [`ParamIssue`] per such key: `record.add` and `record.set`
//! (`/values/<key>`), and `transact`'s `cells.set` (`/ops/<i>/records/<j>/values/<key>`) and
//! `records.create` (`/ops/<i>/records/<j>/<key>`) ops. A key that is a field's title, or its
//! name or title in another case or with spaces for underscores, is told the field's name.

use std::collections::HashMap;

use kition_table_file::{Field, TableFile};
use serde_json::{Map, Value};

use crate::coerce::parse_id;
use crate::{ApiError, ApiResult, ParamIssue};

/// Refuses `method`'s request when a record `values` key names no field of its table
/// ([`ApiError::InvalidParams`], one issue per key). Requests whose table does not exist, or
/// whose values are not objects, are left to the method, which refuses them itself. Each table's
/// fields are read once per request, however many records it writes.
pub(crate) fn refuse_unknown_keys(file: &TableFile, method: &str, params: &Map<String, Value>) -> ApiResult<()> {
    let mut issues = Vec::new();
    let mut tables = Tables { file, read: HashMap::new() };
    match method {
        "record.add" | "record.set" => {
            let table_id = params.get("tableId").and_then(id);
            if let (Some(table_id), Some(Value::Object(values))) = (table_id, params.get("values")) {
                check(&mut tables, table_id, values, "/values", "values", &mut issues);
            }
        }
        "edit.transact" => {
            let ops = params.get("ops").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
            for (i, op) in ops.iter().enumerate() {
                let (Some(kind), Some(table_id)) = (op.get("op").and_then(Value::as_str), op.get("tableId").and_then(id)) else { continue };
                let records = op.get("records").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
                for (j, record) in records.iter().enumerate() {
                    let (values, pointer, shown) = match kind {
                        "cells.set" => (record.get("values"), format!("/ops/{i}/records/{j}/values"), format!("ops[{i}].records[{j}].values")),
                        "records.create" => (Some(record), format!("/ops/{i}/records/{j}"), format!("ops[{i}].records[{j}]")),
                        _ => break,
                    };
                    if let Some(Value::Object(values)) = values {
                        check(&mut tables, table_id, values, &pointer, &shown, &mut issues);
                    }
                }
            }
        }
        _ => {}
    }
    if issues.is_empty() { Ok(()) } else { Err(ApiError::InvalidParams(issues)) }
}

/// A table id given as a number or a decimal string.
fn id(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => parse_id(text).ok(),
        _ => None,
    }
}

/// The fields of the tables a request names, each read once (`None`: no such table).
struct Tables<'a> {
    file: &'a TableFile,
    read: HashMap<i64, Option<Vec<Field>>>,
}

impl Tables<'_> {
    fn fields(&mut self, table_id: i64) -> Option<&[Field]> {
        let file = self.file;
        self.read.entry(table_id).or_insert_with(|| file.list_fields(table_id).ok()).as_deref()
    }
}

/// One issue per key of `values` that names no field of table `table_id`; `pointer` is where
/// `values` is in the params, `shown` the same in the dotted form messages use.
fn check(tables: &mut Tables<'_>, table_id: i64, values: &Map<String, Value>, pointer: &str, shown: &str, issues: &mut Vec<ParamIssue>) {
    let Some(fields) = tables.fields(table_id) else { return };
    let unknown: Vec<&String> = values.keys().filter(|key| !fields.iter().any(|field| &field.name == *key)).collect();
    if unknown.is_empty() {
        return;
    }
    let names = fields.iter().map(|field| format!("`{}`", field.name)).collect::<Vec<_>>().join(", ");
    for key in unknown {
        let mut message = format!("`{shown}.{key}` names no field of table {table_id}");
        if let Some(field) = resembled(fields, key) {
            message.push_str(&format!("; the field titled \"{}\" is named `{}`", field.title, field.name));
        }
        issues.push(ParamIssue {
            path: format!("{pointer}/{}", pointer_token(key)),
            message,
            expected: format!("a field name of table {table_id} (not its title): {names}"),
        });
    }
}

/// The field whose title `key` is, or whose name or title it is in another case or with spaces
/// for underscores.
fn resembled<'a>(fields: &'a [Field], key: &str) -> Option<&'a Field> {
    let loose = |text: &str| {
        text.trim().split(|c: char| c.is_whitespace() || c == '_').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("_").to_lowercase()
    };
    let wanted = loose(key);
    fields
        .iter()
        .find(|field| field.title == key)
        .or_else(|| fields.iter().find(|field| loose(&field.title) == wanted || loose(&field.name) == wanted))
}

/// `key` as one JSON Pointer reference token (RFC 6901): `~` as `~0`, `/` as `~1`.
fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_a_key_as_a_pointer_token() {
        assert_eq!(pointer_token("a/b~c"), "a~1b~0c");
        assert_eq!(pointer_token("Primary channel"), "Primary channel");
    }
}
