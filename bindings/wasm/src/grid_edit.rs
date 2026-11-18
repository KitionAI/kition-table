//! What the editor's grid reads to edit (grid plan sections 3.8 and 4b): `gridRecords`, the
//! records of some ids with their row keys, for the cell editors, copying, pasting, filling and
//! clearing. The grid shows records from the arrangement's ids and the bands; it never holds the
//! whole table on the page, so a write starts by reading just the records it touches. Writes go
//! through the engine's commands (`transact` with `cells.set`, `records.create`, `records.move`,
//! `fields.move`; `views.patchField`, `views.patchConfig`), so undo, collaboration and the agent's
//! receipts stay as they are.

use serde_json::{Map, Value, json};

impl crate::TableDocument {
    /// `gridRecords`: `{tableId, ids, fields?}` → the records `ids` of the table, in the order
    /// asked, as `[{id, row_key, values}]`. With `fields` (field names) a record's values keep
    /// only those. An id that names no record of the table is left out.
    pub fn grid_records(&self, params: &Value) -> Result<Value, String> {
        let table_id = params.get("tableId").and_then(Value::as_i64).ok_or("invalid params: tableId is a table's id")?;
        let ids = params.get("ids").and_then(Value::as_array).ok_or("invalid params: ids is a list of record ids")?;
        let names: Option<Vec<&str>> = params.get("fields").and_then(Value::as_array).map(|names| names.iter().filter_map(Value::as_str).collect());
        let mut records = Vec::with_capacity(ids.len());
        for id in ids.iter().filter_map(Value::as_i64) {
            let Ok(record) = self.file.get_record(table_id, id) else { continue };
            let values: Map<String, Value> = match &names {
                Some(names) => record.values.into_iter().filter(|(name, _)| names.contains(&name.as_str())).collect(),
                None => record.values,
            };
            records.push(json!({ "id": record.id, "row_key": record.row_key, "values": values }));
        }
        Ok(Value::Array(records))
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use kition_kit::wasm_bindgen::prelude::*;

    use crate::__kition_kit_export::WebDocument;

    #[wasm_bindgen(js_class = "TableDocument", wasm_bindgen = kition_kit::wasm_bindgen)]
    impl WebDocument {
        /// The records of some ids (`{tableId, ids, fields?}`), as JSON `[{id, row_key, values}]`.
        #[wasm_bindgen(js_name = gridRecords)]
        pub fn grid_records(&self, params: &str) -> Result<String, JsError> {
            let params = kition_kit::parse_params(params).map_err(|message| JsError::new(&message))?;
            self.0.grid_records(&params).map(|value| value.to_string()).map_err(|message| JsError::new(&message))
        }
    }
}
