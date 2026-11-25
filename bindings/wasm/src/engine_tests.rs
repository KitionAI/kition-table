//! The engine's own commands: import, AI fields, `change.list`, and `asOrigin` writes.

use base64::Engine as _;
use kition_kit::KitDocument;
use serde_json::{Value, json};

use crate::{ENGINE_METHODS, READ_METHODS, TableDocument, known_method};

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn new_document() -> TableDocument {
    TableDocument::create("Tasks.ktable", 0, 0).unwrap()
}

/// The first table's id and its fields' names by title.
fn first_table(doc: &mut TableDocument) -> (i64, Vec<(String, String, i64)>) {
    let document = doc.call("file.get", &Value::Null).unwrap()["result"].clone();
    let table = &document["tables"][0];
    let fields = table["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| (field["title"].as_str().unwrap().to_owned(), field["name"].as_str().unwrap().to_owned(), field["id"].as_i64().unwrap()))
        .collect();
    (table["id"].as_i64().unwrap(), fields)
}

fn field_name(fields: &[(String, String, i64)], title: &str) -> String {
    fields.iter().find(|(found, _, _)| found == title).unwrap().1.clone()
}

fn version(doc: &TableDocument) -> String {
    b64(&doc.version())
}

fn since(doc: &mut TableDocument, version: &str) -> Value {
    doc.execute("change.list", json!({ "version": version })).unwrap()["result"].clone()
}

#[test]
fn engine_methods_are_commands_and_the_reads_do_not_mutate() {
    let doc = new_document();
    let commands = doc.commands();
    for (method, _) in ENGINE_METHODS {
        assert!(known_method(method), "{method}");
        let command = commands.iter().find(|command| command.id == *method).unwrap();
        assert_eq!(command.mutates, !READ_METHODS.contains(method), "{method}");
    }
    for read in ["file.previewImport", "ai.prepare", "change.list"] {
        assert!(READ_METHODS.contains(&read));
    }
    let methods: Vec<(String, String)> = serde_json::from_str(&crate::methods_json()).unwrap();
    assert!(methods.iter().any(|(method, _)| method == "change.list"));
}

const PEOPLE: &[u8] = b"Name,Score\nAda,10\nGrace,7\n";

#[test]
fn import_previews_and_runs_as_one_request_outside_the_undo_history() {
    let mut doc = new_document();
    let start = version(&doc);
    let preview = doc.execute("file.previewImport", json!({ "fileName": "people.csv", "content": b64(PEOPLE) })).unwrap();
    assert_eq!(preview["change"], Value::Null);
    let preview = preview["result"].clone();
    assert_eq!(preview["row_count"], 2);
    assert!(!doc.dirty(), "a preview reads only");
    let run = doc
        .execute(
            "file.runImport",
            json!({
                "fileName": "people.csv",
                "content": b64(PEOPLE),
                "import_token": preview["import_token"],
                "target": { "kind": "new_table", "document_id": 1, "table_title": "People" },
                "write_mode": "append",
                "schema_mode": "auto",
            }),
        )
        .unwrap();
    let result = &run["result"];
    assert_eq!((result["rows_created"].as_i64(), result["fields_created"].as_i64()), (Some(2), Some(2)));
    assert_eq!(result["document_id"], 1);
    assert_eq!(result["path"], "Tasks.ktable");
    assert_eq!(run["change"]["origin"], "import:import");
    assert!(doc.dirty());
    assert!(!doc.can_undo(), "an import is no undo step");
    let changes = since(&mut doc, &start);
    let created = changes["created"].as_array().unwrap();
    assert_eq!(created.len(), 2);
    assert!(created.iter().all(|item| item["origin"] == "import:import" && item["tableId"] == result["table_id"]));
    assert_eq!(changes["version"], version(&doc));
}

#[test]
fn a_failed_import_leaves_the_document_as_it_was() {
    let mut doc = new_document();
    let (table_id, fields) = first_table(&mut doc);
    let bytes = doc.saved_bytes().unwrap();
    let mut doc = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    // `Extra` is created first; the rows then fail: the starter table's `Title` is required.
    let failed = doc.execute(
        "file.runImport",
        json!({
            "fileName": "notes.csv",
            "content": b64(b"Extra,Notes\na,b\n"),
            "target": { "kind": "existing_table", "table_id": table_id },
            "write_mode": "append",
            "schema_mode": "preserve",
        }),
    );
    assert!(failed.is_err(), "{failed:?}");
    let (_, after) = first_table(&mut doc);
    assert_eq!(after, fields);
    assert!(!doc.dirty());
    assert!(doc.execute("record.add", json!({ "tableId": table_id, "values": { field_name(&fields, "Title"): "Still works" } })).is_ok());
}

#[test]
fn ai_fields_prepare_prompts_and_apply_results_as_the_ai_field_origin() {
    let mut doc = new_document();
    let (table_id, fields) = first_table(&mut doc);
    let notes_id = fields.iter().find(|(title, _, _)| title == "Notes").unwrap().2;
    let summary = doc
        .execute(
            "field.add",
            json!({ "tableId": table_id, "title": "Summary", "type": "text", "options": { "ai_config": { "type": "summarize", "source_field_id": notes_id } } }),
        )
        .unwrap()["result"]
        .clone();
    let summary_id = summary["id"].as_i64().unwrap();
    let summary_name = summary["name"].as_str().unwrap().to_owned();
    let values = json!({ field_name(&fields, "Title"): "Plan", field_name(&fields, "Notes"): "Write the plan, then the code." });
    let record = doc.execute("record.add", json!({ "tableId": table_id, "values": values })).unwrap()["result"].clone();
    let record_id = record["id"].as_i64().unwrap();
    let start = version(&doc);

    let prepared = doc.execute("ai.prepare", json!({ "tableId": table_id, "fieldId": summary_id, "record_ids": [record_id] })).unwrap();
    assert_eq!(prepared["change"], Value::Null);
    let request = &prepared["result"]["requests"][0];
    assert_eq!(request["recordId"], record_id);
    assert!(request["messages"][0]["parts"][0]["text"].as_str().unwrap().ends_with("Write the plan, then the code."));

    // The person retitles the record, then the AI cell is written.
    let title = field_name(&fields, "Title");
    doc.execute("record.set", json!({ "tableId": table_id, "recordId": record_id, "values": { title.clone(): "Plan v2" } })).unwrap();
    let applied = doc
        .execute(
            "ai.apply",
            json!({ "tableId": table_id, "fieldId": summary_id, "results": [{ "recordId": record_id, "text": "Plan, then code." }] }),
        )
        .unwrap();
    assert_eq!(applied["result"]["completed"], 1);
    assert_eq!(applied["result"]["items"][0]["record"]["values"][&summary_name], "Plan, then code.");
    assert_eq!(applied["change"]["origin"], "aifield");
    let changes = since(&mut doc, &start);
    let updated = changes["updated"].as_array().unwrap();
    assert_eq!(updated[1], json!({ "tableId": table_id, "recordId": record_id, "fieldIds": [summary_id], "origin": "system:aifield" }));

    // The person's undo reverts the person's edit, not the AI cell.
    doc.execute("edit.undo", json!({})).unwrap();
    let after = doc.call("record.list", &json!({ "tableId": table_id })).unwrap()["result"]["items"][0].clone();
    assert_eq!(after["values"][&title], "Plan");
    assert_eq!(after["values"][&summary_name], "Plan, then code.");
}

#[test]
fn as_origin_writes_as_a_system_feature_outside_the_persons_undo() {
    let bytes = new_document().saved_bytes().unwrap();
    let mut doc = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    let (table_id, fields) = first_table(&mut doc);
    let (title, notes) = (field_name(&fields, "Title"), field_name(&fields, "Notes"));
    let start = version(&doc);
    let mine = doc.execute("record.add", json!({ "tableId": table_id, "values": { title.clone(): "Mine" } })).unwrap();
    let mine_id = mine["result"]["id"].as_i64().unwrap();
    let synced = doc
        .execute("record.add", json!({ "tableId": table_id, "values": { title.clone(): "From a workflow" }, "asOrigin": "system:workflow" }))
        .unwrap();
    assert_eq!(synced["change"]["origin"], "workflow");
    let synced_id = synced["result"]["id"].as_i64().unwrap();
    doc.execute(
        "record.set",
        json!({ "tableId": table_id, "recordId": mine_id, "values": { notes.clone(): "Seen" }, "asOrigin": "system:workflow" }),
    )
    .unwrap();

    let changes = since(&mut doc, &start);
    assert_eq!(
        changes["created"],
        json!([
            { "tableId": table_id, "recordId": mine_id, "origin": "person:kit" },
            { "tableId": table_id, "recordId": synced_id, "origin": "system:workflow" },
        ])
    );
    let notes_id = fields.iter().find(|(found, _, _)| found == "Notes").unwrap().2;
    assert_eq!(changes["updated"], json!([{ "tableId": table_id, "recordId": mine_id, "fieldIds": [notes_id], "origin": "system:workflow" }]));
    assert_eq!(changes["deleted"], json!([]));

    // Undo reverts the person's record, never the system writes.
    let before_undo = version(&doc);
    doc.execute("edit.undo", json!({})).unwrap();
    let left: Vec<Value> = doc.call("record.list", &json!({ "tableId": table_id })).unwrap()["result"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(left, [json!(synced_id)]);
    assert!(!doc.can_undo());
    assert!(doc.dirty(), "the system writes are not in the file");
    assert_eq!(since(&mut doc, &before_undo)["deleted"], json!([{ "tableId": table_id, "recordId": mine_id, "origin": "person:kit" }]));

    // Only `system:<name>` origins, and never for undo or redo.
    for refused in [json!("agent:turn-1"), json!("person:someone"), json!("system:"), json!("system:a b"), json!(1)] {
        let answer = doc.execute("record.add", json!({ "tableId": table_id, "values": { title.clone(): "No" }, "asOrigin": refused }));
        assert!(answer.is_err(), "{refused}");
    }
    assert!(doc.call("history.undo", &json!({ "asOrigin": "system:workflow" })).is_err());
}

#[test]
fn changes_since_the_start_and_since_now() {
    let mut doc = new_document();
    let (table_id, fields) = first_table(&mut doc);
    let record = doc.execute("record.add", json!({ "tableId": table_id, "values": { field_name(&fields, "Title"): "Ada" } })).unwrap();
    let all = since(&mut doc, "");
    assert_eq!(all["created"], json!([{ "tableId": table_id, "recordId": record["result"]["id"], "origin": "person:kit" }]));
    let now = all["version"].as_str().unwrap().to_owned();
    assert_eq!(since(&mut doc, &now), json!({ "version": now, "created": [], "updated": [], "deleted": [] }));
    assert!(doc.execute("change.list", json!({ "version": "not base64!" })).is_err());
    let other = new_document();
    assert!(doc.execute("change.list", json!({ "version": version(&other) })).is_err());
}
