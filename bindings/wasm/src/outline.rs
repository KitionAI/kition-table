//! `file.outline`: the document's outline (`kit.json` `outline.command`, platform K24), the
//! parts of a table file a host lists under it and opens the file at: its tables, then its
//! dashboards, each in the document's order. An item is `{ id, title, kind, view }`: `kind` is
//! `table` or `dashboard`, and `view` is the place as the editor takes it (Kit UI protocol
//! `document.view` and `document/reveal`: `{ "path": "tables/<id>" }`, `{ "path":
//! "dashboards/<id>" }`). A title is never empty: a table without a title goes by its name, and a
//! dashboard without a name by its id. It takes no parameters, reads only, and answers at the top level
//! (`items`), not as a data-document request's `{ result, change }`, because the host reads the
//! outline from the answer's `items`.

use kition_table_file::TableFile;
use serde_json::{Value, json};

use crate::error;

/// The command id.
pub const OUTLINE_COMMAND: &str = "file.outline";

/// Its parameters (a `ParameterSchemaSpec`, KHP A5): none.
pub fn input_schema() -> Value {
    json!({})
}

/// Its answer (a `ValueSchemaSpec`, KHP A1), or `{ error }` (A18).
pub fn output_schema(error_spec: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "items": {
                "type": "array",
                "description": "The file's tables, then its dashboards, in the document's order.",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "id": { "type": "string", "description": "The table's or dashboard's id." },
                        "title": { "type": "string", "description": "What people call it." },
                        "kind": { "type": "string", "enum": ["table", "dashboard"] },
                        "view": { "type": "json", "description": "The place, as the editor opens the file at it: { path }." },
                    },
                },
            },
            "error": error_spec,
        },
    })
}

/// The outline: `{ items }`.
pub fn outline(file: &TableFile) -> Result<Value, String> {
    let mut items = Vec::new();
    for table in file.list_tables().map_err(error)? {
        // A table without a title goes by its name.
        let title = if table.title.trim().is_empty() { table.name } else { table.title };
        items.push(json!({ "id": table.id.to_string(), "title": title, "kind": "table", "view": { "path": format!("tables/{}", table.id) } }));
    }
    for dashboard in file.list_dashboards().map_err(error)? {
        // A dashboard without a name goes by its id, which is never empty.
        let title = if dashboard.name.trim().is_empty() { dashboard.id.clone() } else { dashboard.name };
        items.push(json!({ "id": dashboard.id, "title": title, "kind": "dashboard", "view": { "path": format!("dashboards/{}", dashboard.id) } }));
    }
    Ok(json!({ "items": items }))
}
