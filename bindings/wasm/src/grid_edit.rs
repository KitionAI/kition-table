//! What the editor's grid reads to edit (grid plan sections 3.8 and 4b): `gridRecords`, the
//! records of some ids with their row keys, for the cell editors, copying, pasting, filling and
//! clearing. The grid shows records from the arrangement's ids and the bands; it never holds the
//! whole table on the page, so a write starts by reading just the records it touches. Writes go
//! through the engine's commands (`transact` with `cells.set`, `records.create`, `records.move`,
//! `fields.move`; `views.patchField`, `views.patchConfig`), so undo, collaboration and the agent's
//! receipts stay as they are.
//!
//! A clear of many records (a whole column of a large table) is `gridClear`: the page hands over
//! the records' ids as numbers, and the engine finds their row keys in one read and writes the
//! one `cells.set` itself (plan 3.8), so the page neither reads every record nor builds that JSON.

use std::collections::HashMap;

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

    /// `gridClear`: `{tableId, values}` and the ids of records of the table → `values` (field
    /// name → the field's empty value) set on every one of those records, as one `transact` of
    /// one `cells.set` (one undo step, the same receipts as a `transact` the page sends). Ids
    /// that name no record of the table are left out. Answers what `transact` answers, with
    /// `cleared`: how many records it wrote.
    pub fn grid_clear(&mut self, params: &Value, ids: &[i64]) -> Result<Value, String> {
        let table_id = params.get("tableId").and_then(Value::as_i64).ok_or("invalid params: tableId is a table's id")?;
        let values = params.get("values").and_then(Value::as_object).ok_or("invalid params: values maps field names to empty values")?;
        if values.is_empty() {
            return Err("invalid params: values names no field".into());
        }
        // Every row key of the table in one read (no cell is parsed), then the ids looked up.
        let keys: HashMap<i64, String> = self
            .file
            .list_record_values(table_id, &[])
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|record| (record.id, record.row_key))
            .collect();
        let records: Vec<Value> = ids.iter().filter_map(|id| keys.get(id)).map(|key| json!({ "rowKey": key, "values": values })).collect();
        let cleared = records.len();
        if cleared == 0 {
            return Ok(json!({ "result": { "cleared": 0 } }));
        }
        let mut answer = self.call("transact", &json!({ "ops": [{ "op": "cells.set", "tableId": table_id, "records": records }] }))?;
        answer["result"]["cleared"] = json!(cleared);
        Ok(answer)
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

        /// Clears records of a table (`{tableId, values}`, the records' ids as numbers) in one
        /// `transact`; answers JSON `{result, change}`.
        #[wasm_bindgen(js_name = gridClear)]
        pub fn grid_clear(&mut self, params: &str, ids: &[f64]) -> Result<String, JsError> {
            let params = kition_kit::parse_params(params).map_err(|message| JsError::new(&message))?;
            // Record ids are whole numbers well inside an f64's exact range.
            #[allow(clippy::cast_possible_truncation)]
            let ids: Vec<i64> = ids.iter().map(|id| *id as i64).collect();
            self.0.grid_clear(&params, &ids).map(|value| value.to_string()).map_err(|message| JsError::new(&message))
        }
    }
}
