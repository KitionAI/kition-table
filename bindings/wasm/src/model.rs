//! The object model (`KitObjectModel 1.0`) for the headless executable and the editor's engine
//! (`modelDescribe`, `modelBatch`): the document's own replica exchanges changes with a scratch
//! replica on its line of changes (`kition_kit::session::model`), so a batch arrives here as that
//! replica's changes and is imported, repaired, and projected like any import.

use std::sync::{Arc, OnceLock};

use kition_kit::KitDocument;
use kition_kit::kdm::{Origin, Schema};
use kition_kit::session::model::{Exchange, ReplicaPort};
use serde_json::Value;

use crate::{MODEL_SCHEMA, TableDocument};

impl ReplicaPort for TableDocument {
    fn model_schema(&self) -> Arc<Schema> {
        static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
        Arc::clone(SCHEMA.get_or_init(|| match Schema::parse(MODEL_SCHEMA) {
            Ok(schema) => Arc::new(schema),
            // A constant of this crate; its tests parse it.
            Err(err) => panic!("model/schema.json is invalid: {err}"),
        }))
    }

    fn model_export(&mut self, since: &[u8]) -> Result<Vec<u8>, String> {
        self.export_changes(since)
    }

    fn model_import(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.import_changes(bytes)?;
        // No editor takes the queued changes here.
        self.take_imported();
        Ok(())
    }

    fn model_command(&mut self, command: &str, params: Value, origin: &Origin) -> Result<Value, String> {
        let origin = kition_kdm::Origin::decode(&origin.encode()).ok_or("the origin is not readable")?;
        self.execute_as(command, params, &origin)
    }

    fn model_commands(&self) -> Vec<kition_kit::KitCommand> {
        self.commands()
    }

    /// The attachments a co-editor's changes name, stored before `model.import` takes them.
    fn model_add_blobs(&mut self, blobs: &[u8]) -> Result<(), String> {
        self.add_blobs(blobs).map(|_| ())
    }

    /// The attachments the changes since `since` name, for `model.export` to send beside them.
    fn model_blobs_since(&mut self, since: &[u8]) -> Result<Vec<u8>, String> {
        self.blobs_since(since)
    }

    /// A document with no file yet (an agent's headless draft, a server's headless kit opening a
    /// sync room): the replica alone, with its attachments. A snapshot that is no replica fails;
    /// the kit never opens an empty document in its place.
    fn model_open_replica(name: &str, snapshot: &[u8], blobs: &[u8]) -> Result<Self, String> {
        Self::open_with_replica(name, &[], snapshot, blobs)
    }
}

impl TableDocument {
    /// `model/describe` (`modelDescribe`).
    pub fn model_describe(&mut self) -> Result<Value, String> {
        let mut exchange = std::mem::take(&mut self.exchange);
        let answer = exchange.describe(self);
        self.exchange = exchange;
        answer.map_err(|error| error.to_json().to_string())
    }

    /// `model/batch` (`modelBatch`); a refusal's message is the error's JSON (`{"code", "index",
    /// "message"}`).
    pub fn model_batch(&mut self, request: &Value) -> Result<Value, String> {
        let mut exchange: Exchange = std::mem::take(&mut self.exchange);
        let answer = exchange.batch(self, request, false);
        self.exchange = exchange;
        answer.map_err(|error| error.to_json().to_string())
    }
}

#[cfg(test)]
mod tests {
    use kition_kit::KitDocument;
    use kition_kit::session::model::{Exchange, ModelErrorCode};
    use serde_json::{Value, json};

    use crate::TableDocument;

    fn first_table(doc: &mut TableDocument) -> Value {
        doc.call("documents.get", &Value::Null).unwrap()["result"]["tables"][0].clone()
    }

    /// `kition-table` serves the object model through `ReplicaPort`: a command and a model edit in
    /// batches reach the engine (projected, as a co-editor's change), and a read-only batch
    /// refuses writes.
    #[test]
    fn serves_the_object_model_over_its_change_exchange() {
        let mut doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
        let mut exchange = Exchange::new();
        let described = exchange.describe(&mut doc).unwrap();
        assert_eq!(described["capability"], "KitObjectModel 1.0");
        assert!(described["commands"].as_array().unwrap().iter().any(|command| command["id"] == "record.add"));

        let table = first_table(&mut doc);
        let table_id = table["id"].as_i64().unwrap();
        let primary = table["fields"].as_array().unwrap().iter().find(|field| field["id"] == table["primary_field_id"]).unwrap()["name"].clone();
        let values = json!({ primary.as_str().unwrap(): "From a script" });
        let command = json!({ "ops": [{ "op": "command", "id": "record.add", "params": { "tableId": table_id, "values": values } }] });
        assert_eq!(exchange.batch(&mut doc, &command, true).unwrap_err().code, ModelErrorCode::ReadOnly);
        exchange.batch(&mut doc, &command, false).unwrap();
        let page = doc.call("records.list", &json!({ "tableId": table_id })).unwrap();
        assert_eq!(page["result"]["total"], 1, "the command ran in the engine");

        let loaded = exchange.batch(&mut doc, &json!({ "ops": [], "load": [{ "target": "root", "fields": ["tables"] }] }), false).unwrap();
        let object = loaded["loaded"][0]["fields"]["tables"][0].as_str().unwrap().to_owned();
        let rename = json!({ "ops": [{ "op": "set", "target": object, "field": "title", "value": "Renamed by a script" }] });
        exchange.batch(&mut doc, &rename, false).unwrap();
        assert_eq!(first_table(&mut doc)["title"], "Renamed by a script", "the model edit is projected");
        let saved = TableDocument::open("Tasks.ktable", &doc.saved_bytes().unwrap()).unwrap();
        assert_eq!(saved.doc_id(), doc.doc_id());
    }
}
