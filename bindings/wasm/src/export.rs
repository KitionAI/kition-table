//! `file.export`: one table as a CSV or Markdown file's bytes, base64, for the host to write
//! as a new file (Kit UI protocol `host/write` mode `export`, KHP A41). The editor's Export and the
//! agent's `table_export` (`kit.json` `agent.export`) run it; the engine writes no file and the
//! document does not change. It answers the file at the top level (`data`), not as a data-document
//! request's `{ result, change }`, because the host reads the file from the answer's `data`.

use base64::Engine as _;
use kition_table_api::records;
use kition_table_file::TableFile;
use serde_json::{Value, json};

use crate::error;

/// The command id.
pub const EXPORT_COMMAND: &str = "file.export";

/// The formats it writes: extension and MIME type, as `kit.json` `exports` declares them.
pub const EXPORT_FORMATS: &[(&str, &str)] = &[("csv", "text/csv"), ("md", "text/markdown")];

/// Its parameters (a `ParameterSchemaSpec`, KHP A5).
pub fn input_schema() -> Value {
    json!({
        "format": {
            "type": "string",
            "required": true,
            "enum": EXPORT_FORMATS.iter().map(|(extension, _)| *extension).collect::<Vec<_>>(),
            "description": "csv, or md for a Markdown table.",
        },
        "tableId": { "type": "integer", "description": "The table to export; default the first table." },
    })
}

/// Its answer (a `ValueSchemaSpec`, KHP A1), or `{ error }` (A18).
pub fn output_schema(error_spec: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "format": { "type": "string" },
            "extension": { "type": "string" },
            "mime": { "type": "string" },
            "name": { "type": "string", "description": "The table's title." },
            "tableId": { "type": "integer" },
            "bytes": { "type": "integer", "description": "The file's size." },
            "data": { "type": "string", "description": "The file's bytes, base64." },
            "error": error_spec,
        },
    })
}

/// The file `params` asks for: `{ format, extension, mime, name, tableId, bytes, data }`.
pub fn export(file: &TableFile, params: &Value) -> Result<Value, String> {
    let format = params["format"].as_str().unwrap_or_default();
    let Some((extension, mime)) = EXPORT_FORMATS.iter().find(|(extension, _)| *extension == format) else {
        return Err(if format.is_empty() {
            "missing `format`: csv or md".to_owned()
        } else {
            format!("cannot export `{format}`: the formats are csv or md")
        });
    };
    let table_id = match params.get("tableId") {
        None | Some(Value::Null) => file.list_tables().map_err(error)?.first().map(|table| table.id).ok_or("the document has no table to export")?,
        Some(id) => id.as_i64().ok_or("`tableId` must be a table's id")?,
    };
    let name = file.get_table(table_id).map_err(|_| format!("no table {table_id}"))?.title;
    let content = match *extension {
        "csv" => records::export_csv(file, table_id).map_err(error)?.1,
        _ => records::export_markdown(file, table_id).map_err(error)?["content"].as_str().unwrap_or_default().to_owned(),
    };
    Ok(json!({
        "format": extension,
        "extension": extension,
        "mime": mime,
        "name": name,
        "tableId": table_id,
        "bytes": content.len(),
        "data": base64::engine::general_purpose::STANDARD.encode(content.as_bytes()),
    }))
}
