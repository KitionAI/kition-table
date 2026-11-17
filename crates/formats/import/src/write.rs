//! Writing an import into an open table document: the target table (new, or an existing one whose
//! schema is adapted), the values converted to the field types, and the records written in
//! batches (appended, replacing the table's records, or upserted by key fields).

use std::collections::{HashMap, HashSet};

use kition_table_api::tables::{FieldSeed, TableSeed, ViewSeed, seed_to_spec};
use kition_table_file::{FIELD_TYPES, Field, FieldPatch, FieldSpec, Record, TableFile, TableFileError, TableSpec};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::nullable;
use crate::parse::{self, ImportField, ImportWarning, is_zero};

/// `DataImportResult`.
#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    #[serde(skip_serializing_if = "is_zero")]
    pub document_id: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub table_id: i64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    pub rows_total: usize,
    pub rows_created: usize,
    pub rows_updated: usize,
    pub rows_skipped: usize,
    pub fields_created: usize,
    pub fields_updated: usize,
    pub warnings: Vec<ImportWarning>,
}

/// The execute request's `target`. `document_id` is not checked: the kit has one document.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct Target {
    #[serde(default, deserialize_with = "nullable")]
    pub kind: String,
    #[serde(default, deserialize_with = "nullable")]
    pub path: String,
    #[serde(default, deserialize_with = "nullable")]
    pub table_title: String,
    #[serde(default, deserialize_with = "nullable")]
    pub document_id: i64,
    #[serde(default, deserialize_with = "nullable")]
    pub table_id: i64,
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct FieldOverride {
    #[serde(default, deserialize_with = "nullable")]
    pub index: i64,
    #[serde(default, deserialize_with = "nullable")]
    pub title: String,
    #[serde(default, rename = "type", deserialize_with = "nullable")]
    pub field_type: String,
    #[serde(default)]
    pub options: Option<Map<String, Value>>,
}

/// `DataImportExecuteRequest`. `import_token` is optional here (see [`crate::import_token`]);
/// `idempotency_key` is accepted and unused (a run is one synchronous request).
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ExecuteRequest {
    #[serde(default, deserialize_with = "nullable")]
    pub import_token: String,
    #[serde(default, deserialize_with = "nullable")]
    pub target: Target,
    #[serde(default, deserialize_with = "nullable")]
    pub write_mode: String,
    #[serde(default, deserialize_with = "nullable")]
    pub schema_mode: String,
    #[serde(default, deserialize_with = "nullable")]
    pub unique_by: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub field_overrides: Vec<FieldOverride>,
    #[serde(default, deserialize_with = "nullable")]
    pub idempotency_key: String,
}

/// Checks the request as Kition's route did. A `new_document` target is a new table in the open
/// document (the kit holds one document): it needs `table_title`, and its `path` is not used.
pub fn validate(request: &ExecuteRequest) -> Result<(), &'static str> {
    if !matches!(request.write_mode.as_str(), "append" | "replace" | "upsert") {
        return Err("write_mode must be append, replace, or upsert");
    }
    if !matches!(request.schema_mode.as_str(), "auto" | "preserve" | "replace") {
        return Err("schema_mode must be auto, preserve, or replace");
    }
    let target = &request.target;
    match target.kind.as_str() {
        "existing_table" if target.table_id <= 0 => return Err("existing_table target requires table_id"),
        "new_table" | "new_document" if target.table_title.trim().is_empty() => return Err("new_table target requires table_title"),
        "existing_table" | "new_table" | "new_document" => {}
        _ => return Err("unsupported import target"),
    }
    if request.write_mode == "upsert" && request.unique_by.is_empty() {
        return Err("upsert requires unique_by fields");
    }
    Ok(())
}

/// The preview's fields with the request's overrides applied (title, type, options). Types an
/// import cannot fill (`attachment`, `formula`, `ai`) and unknown ones are ignored.
pub fn apply_overrides(fields: &[ImportField], overrides: &[FieldOverride]) -> Vec<ImportField> {
    let mut out = fields.to_vec();
    for item in overrides {
        let Some(field) = usize::try_from(item.index).ok().and_then(|i| out.get_mut(i)) else {
            continue;
        };
        if !item.title.trim().is_empty() {
            field.title = item.title.trim().to_owned();
        }
        if FIELD_TYPES.contains(&item.field_type.as_str()) && !matches!(item.field_type.as_str(), "attachment" | "formula" | "ai") {
            field.field_type = item.field_type.clone();
        }
        if let Some(options) = &item.options {
            field.options = Some(options.clone());
        }
    }
    out
}

/// Fills select choices from the data when a select field has no options.
pub fn ensure_select_options(fields: &mut [ImportField], rows: &[Vec<String>]) {
    for (index, field) in fields.iter_mut().enumerate() {
        if !matches!(field.field_type.as_str(), "single_select" | "multi_select") || field.options.is_some() {
            continue;
        }
        let mut seen = HashSet::new();
        let mut choices = Vec::new();
        for row in rows {
            let Some(cell) = row.get(index) else { continue };
            for raw in cell.split(',') {
                let value = raw.trim();
                if !value.is_empty() && choices.len() < 50 && seen.insert(value.to_owned()) {
                    choices.push(Value::String(value.to_owned()));
                }
            }
        }
        let mut options = Map::new();
        options.insert("choices".into(), Value::Array(choices));
        field.options = Some(options);
    }
}

fn field_seeds(fields: &[ImportField]) -> Vec<FieldSeed> {
    fields
        .iter()
        .enumerate()
        .map(|(index, f)| FieldSeed {
            title: f.title.clone(),
            field_type: f.field_type.clone(),
            primary: index == 0,
            options: f.options.clone().map_or(Value::Null, Value::Object),
            ..FieldSeed::default()
        })
        .collect()
}

/// The new table an import creates: one field per column (the first is primary) and an `All`
/// grid view.
pub fn import_table_spec(title: &str, fields: &[ImportField]) -> TableSpec {
    seed_to_spec(&TableSeed {
        title: title.to_owned(),
        fields: field_seeds(fields),
        views: vec![ViewSeed { title: "All".into(), view_type: "grid".into(), config: Value::Null }],
        ..TableSeed::default()
    })
}

/// Writes the import into `file` as its current origin. `rows` is the source's normalized rows,
/// header first; `fields` the preview's fields with the overrides applied.
pub fn write_import(
    file: &mut TableFile,
    document_id: i64,
    path: &str,
    mut request: ExecuteRequest,
    rows: &[Vec<String>],
    mut fields: Vec<ImportField>,
) -> Result<ImportResult, String> {
    if rows.is_empty() || fields.is_empty() {
        return Err("import source does not contain fields".into());
    }
    let data = rows.get(1..).unwrap_or_default();
    ensure_select_options(&mut fields, data);
    let (table, fields_created, fields_updated) = if matches!(request.target.kind.as_str(), "new_table" | "new_document") {
        let table = file.create_table(&import_table_spec(&request.target.table_title, &fields)).map_err(|e| e.to_string())?;
        (table, fields.len(), 0)
    } else {
        let table = file.get_table(request.target.table_id).map_err(|_| "Table does not exist".to_owned())?;
        let (created, updated, replaced) = prepare_schema(file, &table.fields, table.id, &fields, &request.schema_mode).map_err(|e| e.to_string())?;
        if replaced {
            request.write_mode = "replace".into();
        }
        (file.get_table(table.id).map_err(|e| e.to_string())?, created, updated)
    };
    let (created, updated, skipped) = write_rows(file, table.id, &table.fields, &fields, data, &request)?;
    Ok(ImportResult {
        document_id,
        table_id: table.id,
        path: path.to_owned(),
        rows_total: data.len(),
        rows_created: created,
        rows_updated: updated,
        rows_skipped: skipped,
        fields_created,
        fields_updated,
        warnings: Vec::new(),
    })
}

/// The default table shape every new document starts with.
fn is_default_schema(fields: &[Field]) -> bool {
    matches!(fields, [title, status, notes]
        if title.title == "Title" && title.field_type == "text"
            && status.title == "Status" && status.field_type == "single_select"
            && notes.title == "Notes" && notes.field_type == "long_text")
}

/// Adapts the target table's schema. Returns (created, updated, replaced).
///
/// `replace` (or `auto` on a table still in the default shape) retitles and retypes the
/// existing fields in order, adds the rest, and deletes the extra non-primary fields; otherwise
/// every column without a field of its title (case-insensitive) gets a new field.
pub fn prepare_schema(
    file: &mut TableFile,
    existing: &[Field],
    table_id: i64,
    fields: &[ImportField],
    schema_mode: &str,
) -> kition_table_file::Result<(usize, usize, bool)> {
    let replaced = schema_mode == "replace" || (schema_mode == "auto" && is_default_schema(existing));
    let (mut created, mut updated) = (0, 0);
    let options = |f: &ImportField| f.options.clone().map_or(Value::Null, Value::Object);
    if replaced {
        for (current, field) in existing.iter().zip(fields) {
            file.update_field(
                table_id,
                current.id,
                &FieldPatch {
                    title: Some(field.title.clone()),
                    field_type: Some(field.field_type.clone()),
                    options: Some(options(field)),
                    ..FieldPatch::default()
                },
            )?;
            updated += 1;
        }
        for (index, field) in fields.iter().enumerate().skip(existing.len()) {
            file.create_field(
                table_id,
                &FieldSpec {
                    title: field.title.clone(),
                    field_type: field.field_type.clone(),
                    options: options(field),
                    order: Some(i64::try_from(index + 1).unwrap_or(i64::MAX)),
                    ..FieldSpec::default()
                },
            )?;
            created += 1;
        }
        for current in existing.iter().skip(fields.len()) {
            if !current.is_primary {
                file.delete_field(table_id, current.id)?;
            }
        }
        return Ok((created, updated, true));
    }
    let titles: HashSet<String> = existing.iter().map(|f| f.title.to_lowercase()).collect();
    for field in fields {
        if titles.contains(&field.title.to_lowercase()) {
            continue;
        }
        file.create_field(
            table_id,
            &FieldSpec { title: field.title.clone(), field_type: field.field_type.clone(), options: options(field), ..FieldSpec::default() },
        )?;
        created += 1;
    }
    Ok((created, updated, false))
}

/// Text form used for upsert keys, like the legacy `%v` formatting.
fn key_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "<nil>".to_owned(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(n)) => n.as_f64().map(crate::format_float).unwrap_or_else(|| n.to_string()),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Array(items)) => format!("[{}]", items.iter().map(|v| key_text(Some(v))).collect::<Vec<_>>().join(" ")),
        Some(other) => other.to_string(),
    }
}

/// The upsert key of `values` over the `unique_by` fields (by title or name, case-insensitive);
/// empty when a key field is unknown or empty.
pub fn values_key(fields: &[Field], values: &Map<String, Value>, unique_by: &[String]) -> String {
    let mut names: HashMap<String, String> = HashMap::new();
    for field in fields {
        names.insert(field.title.to_lowercase(), field.name.clone());
        names.insert(field.name.to_lowercase(), field.name.clone());
    }
    let mut parts = Vec::with_capacity(unique_by.len());
    for title in unique_by {
        let Some(name) = names.get(&title.trim().to_lowercase()) else {
            return String::new();
        };
        let value = key_text(values.get(name)).trim().to_owned();
        if value.is_empty() || value == "<nil>" {
            return String::new();
        }
        parts.push(value);
    }
    parts.join("\u{1f}")
}

/// Rows written per transaction by an import.
pub const IMPORT_BATCH: usize = 500;

/// Maps one source row through the same title and type resolution the import writes.
pub fn row_values(fields: &[Field], source_fields: &[ImportField], row: &[String]) -> Map<String, Value> {
    let by_title: HashMap<String, &Field> = fields.iter().map(|f| (f.title.trim().to_lowercase(), f)).collect();
    source_fields
        .iter()
        .enumerate()
        .filter_map(|(column, source)| {
            let field = by_title.get(&source.title.trim().to_lowercase())?;
            let cell = row.get(column).filter(|cell| !cell.is_empty())?;
            Some((field.name.clone(), parse::coerce_value(cell, &field.field_type)))
        })
        .collect()
}

/// Writes `rows` (data rows, no header) into the table: `replace` deletes its records first,
/// `upsert` updates the record with the same key (and skips rows without one), and new records
/// are created [`IMPORT_BATCH`] at a time. Answers (created, updated, skipped).
pub fn write_rows(
    file: &mut TableFile,
    table_id: i64,
    fields: &[Field],
    source_fields: &[ImportField],
    rows: &[Vec<String>],
    request: &ExecuteRequest,
) -> Result<(usize, usize, usize), String> {
    let map_err = |e: TableFileError| e.to_string();
    if request.write_mode == "replace" {
        let keys: Vec<String> = file.list_records(table_id).map_err(map_err)?.into_iter().map(|record| record.row_key).collect();
        file.bulk_delete_records(table_id, &keys).map_err(map_err)?;
    }
    let mut existing: HashMap<String, Record> = HashMap::new();
    if !request.unique_by.is_empty() {
        for record in file.list_records(table_id).map_err(map_err)? {
            let key = values_key(fields, &record.values, &request.unique_by);
            if !key.is_empty() {
                existing.insert(key, record);
            }
        }
    }
    let (mut created, mut updated, mut skipped) = (0, 0, 0);
    // New records are written in batches: one transaction each, not one per row.
    let mut pending: Vec<Map<String, Value>> = Vec::new();
    for row in rows {
        let values = row_values(fields, source_fields, row);
        if request.write_mode == "upsert" {
            let key = values_key(fields, &values, &request.unique_by);
            if key.is_empty() {
                skipped += 1;
                continue;
            }
            if let Some(record) = existing.get(&key) {
                let mut merged = record.values.clone();
                merged.extend(values);
                file.update_record(table_id, record.id, &merged, None).map_err(map_err)?;
                updated += 1;
                continue;
            }
        }
        pending.push(values);
        created += 1;
        if pending.len() >= IMPORT_BATCH {
            file.create_records(table_id, &pending).map_err(map_err)?;
            pending.clear();
        }
    }
    if !pending.is_empty() {
        file.create_records(table_id, &pending).map_err(map_err)?;
    }
    Ok((created, updated, skipped))
}
