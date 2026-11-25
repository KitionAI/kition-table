//! Records, their history and links, and CSV/Markdown transfer.

use kition_table_file::kdm::changes::HistoryPoint;
use kition_table_file::{HistoryEntry, LinkRef, Record, TableFile, TableFileError};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{ApiError, ApiResult, DocRef, csvutil, respond};

fn bad(err: TableFileError) -> ApiError {
    ApiError::bad_request(err.to_string())
}

fn table_not_found(_: TableFileError) -> ApiError {
    ApiError::not_found("Table does not exist")
}

/// `GET /records`: one page (see [`crate::list_window`]) and the table's record count.
pub fn list(file: &TableFile, doc: &DocRef, table_id: i64, limit: i64, offset: i64) -> ApiResult<Value> {
    let failed = |_| ApiError::internal("Failed to query records");
    let total = file.count_records(table_id).map_err(failed)?;
    let records = file.list_records_page(table_id, limit, offset).map_err(failed)?;
    let items: Vec<Value> = records.iter().map(|r| respond::record(doc, r)).collect();
    Ok(json!({ "items": items, "total": total, "offset": offset, "limit": limit }))
}

/// The body of `POST /records` and `PATCH /records/{recordId}`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct RecordRequest {
    /// The record's values, keyed by field name (not title). A key that names no field of the
    /// table is refused (`invalid_params` at `/values/<key>`) and nothing is written.
    #[serde(default)]
    pub values: Option<Map<String, Value>>,
    #[serde(default)]
    pub order: Option<i64>,
}

/// `POST /records`: the new record (answered with [`respond::record`]).
pub fn create(file: &mut TableFile, table_id: i64, request: RecordRequest) -> ApiResult<Record> {
    file.create_record(table_id, &request.values.unwrap_or_default()).map_err(bad)
}

/// A record's values and order before an update.
pub type Previous = Option<(Map<String, Value>, i64)>;

/// `PATCH /records/{recordId}`: the updated record and its values and order before the update,
/// when it could be read.
pub fn update(file: &mut TableFile, table_id: i64, record_id: i64, request: RecordRequest) -> ApiResult<(Record, Previous)> {
    let previous = file.get_record(table_id, record_id).ok().map(|r| (r.values, r.order));
    let record = file.update_record(table_id, record_id, &request.values.unwrap_or_default(), request.order).map_err(bad)?;
    Ok((record, previous))
}

/// `DELETE /records/{recordId}`.
pub fn delete(file: &mut TableFile, table_id: i64, record_id: i64) -> ApiResult<Value> {
    file.delete_record(table_id, record_id).map_err(|_| ApiError::not_found("Record does not exist"))?;
    Ok(json!({ "message": "Record deleted" }))
}

fn history_entry(entry: &HistoryEntry) -> Value {
    json!({
        "id": entry.id, "document_id": entry.document_id, "table_id": entry.table_id,
        "record_id": entry.record_id, "field_id": entry.field_id, "action": entry.action,
        "before": entry.before, "after": entry.after, "actor": entry.actor, "created_at": entry.created_at,
    })
}

/// `GET /records/{recordId}/history`; a `limit` of 0 means 100. Newest first: this session's
/// changes, then what came before it, told between the host's checkpoints `points`
/// ([`TableFile::stored_record_history`]), whose entries have negative ids.
pub fn history(file: &TableFile, table_id: i64, record_id: i64, limit: i64, offset: i64, points: &[HistoryPoint]) -> ApiResult<Value> {
    let limit = if limit <= 0 { 100 } else { limit };
    let failed = |_| ApiError::internal("Failed to query record history");
    let (mut entries, _) = file.list_record_history(table_id, record_id, 0, 0).map_err(failed)?;
    let stored = file.stored_record_history(table_id, record_id, points).map_err(failed)?;
    entries.extend(stored.into_iter().zip(1..).map(|(entry, at): (HistoryEntry, i64)| HistoryEntry { id: -at, ..entry }));
    let total = i64::try_from(entries.len()).unwrap_or(i64::MAX);
    let items: Vec<Value> = entries
        .iter()
        .skip(usize::try_from(offset.max(0)).unwrap_or(usize::MAX))
        .take(usize::try_from(limit).unwrap_or(usize::MAX))
        .map(history_entry)
        .collect();
    Ok(json!({ "items": items, "total": total, "limit": limit, "offset": offset }))
}

/// The `checkpoints` of `records.history`: the host's checkpoints, oldest first, each
/// `{version, time, author?, snapshot?}` (`version` base64, as `replica/version` answers it;
/// `snapshot` base64, the checkpoint's snapshot as `host/checkpointRead` answers it).
pub fn history_points(value: Option<&Value>) -> ApiResult<Vec<HistoryPoint>> {
    use base64::Engine;
    let Some(value) = value.filter(|value| !value.is_null()) else { return Ok(Vec::new()) };
    let invalid = || ApiError::bad_request("checkpoints must be a list of {version, time, author?, snapshot?}");
    value
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|point| {
            let bytes = base64::engine::general_purpose::STANDARD.decode(point["version"].as_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
            let version = kition_kdm::Version::decode(&bytes).map_err(|_| invalid())?;
            let time = point.get("time").and_then(Value::as_i64).unwrap_or(0);
            let author = point.get("author").and_then(Value::as_str).filter(|author| !author.is_empty()).map(str::to_owned);
            let snapshot = match point.get("snapshot").and_then(Value::as_str) {
                Some(text) => Some(base64::engine::general_purpose::STANDARD.decode(text).map_err(|_| invalid())?),
                None => None,
            };
            Ok(HistoryPoint { version, time, author, snapshot })
        })
        .collect()
}

fn link_items(refs: &[LinkRef]) -> Value {
    json!({ "items": refs.iter().map(|r| json!({ "row_key": r.row_key, "display": r.display })).collect::<Vec<_>>() })
}

/// The `field_id` query value of `GET /records/{recordId}/links`: a positive integer.
pub fn link_field_id(value: Option<&str>) -> ApiResult<i64> {
    value.and_then(|v| v.parse::<i64>().ok()).filter(|id| *id > 0).ok_or_else(|| ApiError::bad_request("field_id is required"))
}

/// `GET /records/{recordId}/links`.
pub fn list_links(file: &TableFile, table_id: i64, record_id: i64, field_id: i64) -> ApiResult<Value> {
    let refs = file.list_record_links(table_id, record_id, field_id).map_err(|_| ApiError::internal("Failed to list record links"))?;
    Ok(link_items(&refs))
}

/// The body of `POST` and `DELETE /records/{recordId}/links`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct LinkRequest {
    #[serde(default)]
    pub field_id: Option<i64>,
    #[serde(default)]
    pub target_row_keys: Option<Vec<String>>,
}

impl LinkRequest {
    /// Refuses a request without a positive `field_id`.
    pub fn check(self) -> ApiResult<Self> {
        if self.field_id.unwrap_or_default() <= 0 {
            return Err(ApiError::bad_request("field_id is required"));
        }
        Ok(self)
    }
}

/// `POST /records/{recordId}/links`: replaces the record's links in the field.
pub fn replace_links(file: &mut TableFile, table_id: i64, record_id: i64, request: LinkRequest) -> ApiResult<Value> {
    let refs = file
        .replace_record_links(table_id, record_id, request.field_id.unwrap_or_default(), &request.target_row_keys.unwrap_or_default())
        .map_err(bad)?;
    Ok(link_items(&refs))
}

/// `DELETE /records/{recordId}/links`.
pub fn delete_links(file: &mut TableFile, table_id: i64, record_id: i64, request: LinkRequest) -> ApiResult<Value> {
    file.delete_record_links(table_id, record_id, request.field_id.unwrap_or_default(), &request.target_row_keys.unwrap_or_default())
        .map_err(|_| ApiError::internal("Failed to delete record links"))?;
    Ok(json!({ "ok": true }))
}

/// The `limit` query value of `GET /fields/{fieldId}/link-candidates`: positive, else 25.
pub fn candidate_limit(value: Option<&str>) -> i64 {
    value.and_then(|v| v.parse::<i64>().ok()).filter(|l| *l > 0).unwrap_or(25)
}

/// `GET /fields/{fieldId}/link-candidates`.
pub fn link_candidates(file: &TableFile, field_id: i64, query: &str, limit: i64) -> ApiResult<Value> {
    let refs = file.list_link_candidates(field_id, query, limit).map_err(|_| ApiError::internal("Failed to list link candidates"))?;
    Ok(link_items(&refs))
}

/// The body of `POST /import/csv`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct CsvRequest {
    #[serde(default)]
    pub content: Option<String>,
}

/// The rows of a CSV import: rectangular and not empty.
pub fn parse_csv(content: &str) -> ApiResult<Vec<Vec<String>>> {
    csvutil::read_all(content).filter(|rows| !rows.is_empty()).ok_or_else(|| ApiError::bad_request("Invalid CSV content"))
}

/// `POST /import/csv`: the first row names fields (by title or name); every other row becomes
/// a record, all in one transaction.
pub fn import_csv(file: &mut TableFile, table_id: i64, rows: &[Vec<String>]) -> ApiResult<Value> {
    let table = file.get_table(table_id).map_err(table_not_found)?;
    let Some((headers, rows)) = rows.split_first() else {
        return Err(ApiError::bad_request("Invalid CSV content"));
    };
    let mut batch = Vec::with_capacity(rows.len());
    for row in rows {
        let mut values = Map::new();
        for (index, header) in headers.iter().enumerate() {
            let Some(cell) = row.get(index) else { continue };
            if let Some(field) = table.fields.iter().find(|f| &f.title == header || &f.name == header)
                && let Some(value) = csv_value(&field.field_type, cell)
            {
                values.insert(field.name.clone(), value);
            }
        }
        batch.push(values);
    }
    // One transaction for the whole file.
    let created = file.create_records(table_id, &batch).map_err(|_| ApiError::internal("Failed to import CSV"))?.len();
    Ok(json!({ "created": created }))
}

/// A CSV cell as a value of a field of `field_type`: numbers and ratings as numbers, checkboxes
/// as booleans, multiple choices as the comma-separated list; an empty cell is no value. A cell
/// that does not read as its type stays text (the check reports it).
pub fn csv_value(field_type: &str, cell: &str) -> Option<Value> {
    let text = cell.trim();
    if text.is_empty() {
        return None;
    }
    let as_text = || Value::String(cell.to_owned());
    Some(match field_type {
        "number" | "rating" => match text.replace(',', "").parse::<f64>() {
            Ok(number) if number.is_finite() && number.fract() == 0.0 && number.abs() < 9.0e15 => json!(number as i64),
            Ok(number) if number.is_finite() => json!(number),
            _ => as_text(),
        },
        "checkbox" => match text.to_lowercase().as_str() {
            "true" | "yes" | "y" | "1" | "x" | "checked" => Value::Bool(true),
            "false" | "no" | "n" | "0" | "unchecked" => Value::Bool(false),
            _ => as_text(),
        },
        "multi_select" => {
            Value::Array(text.split(',').map(str::trim).filter(|item| !item.is_empty()).map(|item| Value::String(item.to_owned())).collect())
        }
        _ => as_text(),
    })
}

/// Text form of a cell for CSV and Markdown exports.
pub fn stringify_cell(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(stringify_cell).collect::<Vec<_>>().join(", "),
        Value::Object(map) => ["display", "name", "title", "url", "href", "row_key"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str).filter(|s| !s.is_empty()))
            .unwrap_or_default()
            .to_owned(),
        Value::Number(number) => match (number.as_i64(), number.as_f64()) {
            (Some(int), _) => int.to_string(),
            (None, Some(float)) => float.to_string(),
            _ => number.to_string(),
        },
        Value::Bool(flag) => flag.to_string(),
    }
}

fn escape_markdown_cell(value: &str) -> String {
    value.replace('\n', "<br>").replace('|', "\\|")
}

/// `GET /export/csv`: the table's name and its records as CSV, one column per field.
pub fn export_csv(file: &TableFile, table_id: i64) -> ApiResult<(String, String)> {
    let table = file.get_table(table_id).map_err(table_not_found)?;
    let records = file.list_records(table_id).map_err(|_| ApiError::internal("Failed to export CSV"))?;
    let mut out = String::new();
    csvutil::write_record(&mut out, &table.fields.iter().map(|f| f.title.clone()).collect::<Vec<_>>());
    for record in &records {
        let row: Vec<String> = table.fields.iter().map(|f| stringify_cell(record.values.get(&f.name).unwrap_or(&Value::Null))).collect();
        csvutil::write_record(&mut out, &row);
    }
    Ok((table.name, out))
}

/// `GET /export/markdown`: `{ "content" }`, the table as a Markdown table.
pub fn export_markdown(file: &TableFile, table_id: i64) -> ApiResult<Value> {
    let table = file.get_table(table_id).map_err(table_not_found)?;
    let records = file.list_records(table_id).map_err(|_| ApiError::internal("Failed to export Markdown"))?;
    let mut out = String::from("|");
    for field in &table.fields {
        out.push_str(&format!(" {} |", escape_markdown_cell(&field.title)));
    }
    out.push_str("\n|");
    for _ in &table.fields {
        out.push_str(" --- |");
    }
    out.push('\n');
    for record in &records {
        out.push('|');
        for field in &table.fields {
            let cell = stringify_cell(record.values.get(&field.name).unwrap_or(&Value::Null));
            out.push_str(&format!(" {} |", escape_markdown_cell(&cell)));
        }
        out.push('\n');
    }
    Ok(json!({ "content": out }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stringifies_cells() {
        assert_eq!(stringify_cell(&json!([1, "a", {"display": "D"}])), "1, a, D");
        assert_eq!(stringify_cell(&json!(2.5)), "2.5");
        assert_eq!(stringify_cell(&json!({"x": 1})), "");
        assert_eq!(stringify_cell(&json!(true)), "true");
        assert_eq!(escape_markdown_cell("a|b\nc"), "a\\|b<br>c");
    }
}
