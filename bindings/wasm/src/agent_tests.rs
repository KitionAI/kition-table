//! The agent-facing schemas (`agent.rs`) against what the commands really take and answer:
//! every command is run on a real document with params valid against its `inputSchema`, and every
//! answer, errors included (A18), validates against its `outputSchema`.

use std::collections::BTreeSet;

use base64::Engine as _;
use kition_kit::KitDocument;
use serde_json::{Value, json};

use crate::TableDocument;
use crate::agent::{commands_json, input_json_schema, input_schema, output_schema, to_json_schema};

/// `agent/commands.json`, which ships beside `kit.json`.
const COMMANDS_FILE: &str = include_str!("../../../agent/commands.json");

fn validator(schema: &Value) -> jsonschema::Validator {
    jsonschema::validator_for(schema).unwrap_or_else(|error| panic!("not a valid JSON Schema ({error}): {schema}"))
}

fn assert_valid(schema: &Value, value: &Value, what: &str) {
    let validator = validator(schema);
    let errors: Vec<String> = validator.iter_errors(value).map(|error| format!("{error} at {}", error.instance_path())).collect();
    assert!(errors.is_empty(), "{what}: {errors:?}\nvalue: {value}");
}

/// The keys of `value` that `schema` (JSON Schema) does not name, as paths: an output schema
/// describes the real answer key by key, wherever it lists an object's
/// properties. An object without listed properties (a record's values, a field's options) is an
/// open map.
fn undeclared(schema: &Value, value: &Value, at: &str, out: &mut Vec<String>) {
    if let Some(branches) = schema.get("oneOf").and_then(Value::as_array) {
        let found: Vec<Vec<String>> = branches
            .iter()
            .filter(|branch| validator(branch).is_valid(value))
            .map(|branch| {
                let mut inner = Vec::new();
                undeclared(branch, value, at, &mut inner);
                inner
            })
            .collect();
        if !found.iter().any(Vec::is_empty) {
            out.extend(found.into_iter().next().unwrap_or_default());
        }
        return;
    }
    match value {
        Value::Object(map) => {
            let Some(properties) = schema.get("properties").and_then(Value::as_object) else { return };
            for (key, inner) in map {
                match properties.get(key) {
                    Some(node) => undeclared(node, inner, &format!("{at}.{key}"), out),
                    None => out.push(format!("{at}.{key}")),
                }
            }
        }
        Value::Array(items) => {
            if let Some(node) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    undeclared(node, item, &format!("{at}[{index}]"), out);
                }
            }
        }
        _ => {}
    }
}

/// The error result an MCP binding answers for a failed command (A18), as Kition's kit tools do.
fn error_result(message: &str) -> Value {
    json!({ "error": { "code": "command-failed", "message": message } })
}

struct Runner {
    doc: TableDocument,
    ran: BTreeSet<String>,
}

impl Runner {
    /// Runs `command`: its params must be valid against the input schema, and its answer (or the
    /// error result) against the output schema. Answers the result, or the error message.
    fn run(&mut self, command: &str, params: Value) -> Result<Value, String> {
        let input = input_json_schema(command).unwrap_or_else(|| panic!("{command} has no input schema"));
        assert_valid(&input, &params, &format!("{command} params"));
        let output = to_json_schema(&output_schema(command).unwrap_or_else(|| panic!("{command} has no output schema")));
        self.ran.insert(command.to_owned());
        let before = self.doc.sealed_save().unwrap();
        match self.doc.execute(command, params) {
            Ok(answer) => {
                assert_valid(&output, &answer, &format!("{command} answer"));
                let mut unnamed = Vec::new();
                undeclared(&output, &answer, command, &mut unnamed);
                assert!(unnamed.is_empty(), "{command}: the output schema does not name {unnamed:?}\nanswer: {answer}");
                Ok(answer["result"].clone())
            }
            Err(message) => {
                assert_valid(&output, &error_result(&message), &format!("{command} error"));
                assert_eq!(self.doc.sealed_save().unwrap(), before, "{command} failed and changed the file");
                Err(message)
            }
        }
    }

    fn ok(&mut self, command: &str, params: Value) -> Value {
        self.run(command, params).unwrap_or_else(|message| panic!("{command}: {message}"))
    }
}

#[test]
fn every_command_has_a_valid_parameter_and_output_schema() {
    let doc = TableDocument::create("Tasks.ktable", 640, 420).unwrap();
    for command in doc.commands() {
        let schema = command.schema.clone().unwrap_or_else(|| panic!("{} has no schema", command.id));
        assert_eq!(Some(&schema), input_schema(&command.id).as_ref());
        kition_kit::schema::validate(&schema).unwrap_or_else(|error| panic!("{}: {error}", command.id));
        let output = output_schema(&command.id).unwrap_or_else(|| panic!("{} has no output schema", command.id));
        kition_kit::schema::validate(&json!({ "output": output })).unwrap_or_else(|error| panic!("{} output: {error}", command.id));
        validator(&input_json_schema(&command.id).unwrap());
        let output = to_json_schema(&output);
        validator(&output);
        assert!(output.get("required").is_none(), "{}: an output schema requires nothing, so errors validate (A18)", command.id);
        assert_valid(&output, &error_result("boom"), &command.id);
        assert_valid(&output, &json!({ "error": { "code": "not-allowed", "message": "no", "index": 0, "retryable": false } }), &command.id);
        let bad = json!({ "error": { "code": "oops", "message": "no" } });
        assert!(!validator(&output).is_valid(&bad), "{}: an unknown error code is refused", command.id);
    }
}

#[test]
fn the_ids_and_bodies_come_from_the_request_types() {
    let records = input_schema("table.records.update").unwrap();
    assert_eq!(records["tableId"]["required"], true);
    // An id parameter takes the local integer or the stable id a receipt shows (one id space).
    assert_eq!(records["recordId"]["oneOf"], json!([{ "type": "integer" }, { "type": "string" }]));
    assert_eq!(records["values"]["type"], "object");
    assert_eq!(records["values"]["additionalProperties"], true);
    let transact = input_schema("table.transact").unwrap();
    assert_eq!(transact["ops"]["required"], true);
    let op = &transact["ops"]["items"];
    let names: Vec<&str> = op["oneOf"].as_array().unwrap().iter().map(|branch| branch["properties"]["op"]["enum"][0].as_str().unwrap()).collect();
    assert_eq!(names, ["cells.set", "records.create", "records.delete", "records.move", "fields.move"]);
    let filter = input_schema("table.views.putFilterTree").unwrap();
    assert_eq!(filter["tree"]["properties"]["conjunction"]["type"], "string");
    assert_eq!(input_schema("table.links.list").unwrap()["field_id"]["required"], true);
    assert_eq!(input_schema("table.attachments.read").unwrap()["hash"]["type"], "string");
    assert!(input_schema("table.records.create").unwrap().get("asOrigin").is_some());
    assert!(input_schema("table.records.list").unwrap().get("asOrigin").is_none(), "reads write nothing");
    assert_eq!(input_schema("edit.undo").unwrap()["scope"]["enum"], json!(["person", "agent"]), "whose step (M12a)");
    assert_eq!(input_schema("table.nope"), None);
}

#[test]
fn the_shipped_commands_file_is_the_generated_one() {
    let doc = TableDocument::create("Tasks.ktable", 640, 420).unwrap();
    let generated = format!("{}\n", serde_json::to_string_pretty(&commands_json(&doc.commands())).unwrap());
    if std::env::var_os("KITION_TABLE_WRITE_COMMANDS").is_some() {
        std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../agent/commands.json"), &generated).unwrap();
        return;
    }
    assert!(
        COMMANDS_FILE == generated,
        "agent/commands.json is stale: run KITION_TABLE_WRITE_COMMANDS=1 cargo test -p kition-table-kit shipped_commands_file"
    );
    let manifest: Value = serde_json::from_str(include_str!("../../../kit.json")).unwrap();
    assert_eq!(manifest["agent"]["prefix"], crate::agent::TOOL_PREFIX);
    let file: Value = serde_json::from_str(COMMANDS_FILE).unwrap();
    assert_eq!(file["commands"].as_array().unwrap().len(), doc.commands().len());
    assert_eq!(file["commands"][2]["tool"], "table_table_documents_get");
}

#[test]
fn empty_params_answer_a_result_or_an_error_and_leave_the_file() {
    // A36: every command called with `{}` answers a typed error or a result, never a crash.
    let ids: Vec<String> = TableDocument::create("Tasks.ktable", 640, 420).unwrap().commands().into_iter().map(|command| command.id).collect();
    for id in ids {
        let mut runner = Runner { doc: TableDocument::create("Tasks.ktable", 640, 420).unwrap(), ran: BTreeSet::new() };
        let input = input_json_schema(&id).unwrap();
        if validator(&input).is_valid(&json!({})) {
            let _ = runner.run(&id, json!({}));
        } else {
            // Params the schema refuses still answer an error the output schema accepts.
            let output = to_json_schema(&output_schema(&id).unwrap());
            let before = runner.doc.sealed_save().unwrap();
            match runner.doc.execute(&id, json!({})) {
                Ok(answer) => assert_valid(&output, &answer, &id),
                Err(message) => {
                    assert_valid(&output, &error_result(&message), &id);
                    assert_eq!(runner.doc.sealed_save().unwrap(), before, "{id} failed and changed the file");
                }
            }
        }
    }
}

#[test]
fn real_answers_and_errors_of_every_command_match_their_schemas() {
    let mut r = Runner { doc: TableDocument::create("Tasks.ktable", 640, 420).unwrap(), ran: BTreeSet::new() };
    let document = r.ok("table.documents.get", json!({}));
    let table = document["tables"][0]["id"].as_i64().unwrap();
    let primary = document["tables"][0]["primary_field_id"].as_i64().unwrap();
    let title = document["tables"][0]["fields"].as_array().unwrap().iter().find(|field| field["id"].as_i64() == Some(primary)).unwrap()["name"]
        .as_str()
        .unwrap()
        .to_owned();
    r.ok("table.tables.list", json!({}));

    let other = r.ok("table.tables.create", json!({ "title": "People", "fields": [{ "title": "Name", "type": "text", "primary": true }] }))["id"]
        .as_i64()
        .unwrap();
    r.ok("table.tables.update", json!({ "tableId": other, "title": "Persons", "description": "Who" }));
    r.ok("table.fields.list", json!({ "tableId": table }));
    let score = r.ok("table.fields.create", json!({ "tableId": table, "title": "Score", "type": "number" }))["id"].as_i64().unwrap();
    r.ok("table.fields.update", json!({ "tableId": table, "fieldId": score, "title": "Points" }));
    let copy = r.ok("table.fields.duplicate", json!({ "tableId": table, "fieldId": score }))["id"].as_i64().unwrap();
    r.ok("table.fields.delete", json!({ "tableId": table, "fieldId": copy }));
    let _ = r.run("table.fields.linkCandidates", json!({ "tableId": table, "fieldId": primary, "q": "a", "limit": 5 }));

    r.ok("table.views.list", json!({ "tableId": table }));
    let view = r.ok("table.views.create", json!({ "tableId": table, "title": "Board", "type": "grid" }))["id"].as_i64().unwrap();
    r.ok("table.views.update", json!({ "tableId": table, "viewId": view, "title": "Main" }));
    r.ok("table.views.getFilterTree", json!({ "tableId": table, "viewId": view }));
    r.ok(
        "table.views.putFilterTree",
        json!({ "tableId": table, "viewId": view, "tree": { "conjunction": "and", "children": [{ "condition": { "field_id": primary, "operator": "is_not_empty", "value": null } }] } }),
    );
    r.ok("table.views.getSorts", json!({ "tableId": table, "viewId": view }));
    r.ok("table.views.putSorts", json!({ "tableId": table, "viewId": view, "items": [{ "field_id": primary, "direction": "asc" }] }));
    r.ok("table.views.getGroups", json!({ "tableId": table, "viewId": view }));
    r.ok("table.views.putGroups", json!({ "tableId": table, "viewId": view, "items": [{ "field_id": primary, "direction": "desc" }] }));
    r.ok("table.views.patchConfig", json!({ "tableId": table, "viewId": view, "row_height": "short", "frozen_column_count": 1 }));
    r.ok("table.views.listFields", json!({ "tableId": table, "viewId": view }));
    r.ok("table.views.patchField", json!({ "tableId": table, "viewId": view, "fieldId": score, "visible": false, "width": 120 }));

    let record = r.ok("table.records.create", json!({ "tableId": table, "values": { title.clone(): "Write the plan" } }));
    let (record_id, row_key) = (record["id"].as_i64().unwrap(), record["row_key"].as_str().unwrap().to_owned());
    r.ok("table.records.update", json!({ "tableId": table, "recordId": record_id, "values": { title.clone(): "Ship it" } }));
    r.ok("table.records.list", json!({ "tableId": table, "limit": 10, "offset": 0 }));
    r.ok("table.records.history", json!({ "tableId": table, "recordId": record_id }));
    r.ok("table.records.bulkCopy", json!({ "tableId": table, "row_keys": [row_key.clone()], "target_position": 0 }));
    let _ = r.run("table.links.list", json!({ "tableId": table, "recordId": record_id, "field_id": primary }));
    let _ = r.run("table.links.replace", json!({ "tableId": table, "recordId": record_id, "field_id": primary, "target_row_keys": [] }));
    let _ = r.run("table.links.delete", json!({ "tableId": table, "recordId": record_id, "field_id": primary, "target_row_keys": [] }));

    r.ok("table.csv.import", json!({ "tableId": table, "content": format!("{title}\nFrom CSV\n") }));
    r.ok("table.csv.export", json!({ "tableId": table }));
    r.ok("table.markdown.export", json!({ "tableId": table }));
    r.ok("table.file.export", json!({ "format": "csv", "tableId": table }));
    r.ok("table.file.export", json!({ "format": "md" }));
    assert!(r.run("table.file.export", json!({ "format": "csv", "tableId": 999 })).is_err());
    r.ok(
        "table.transact",
        json!({ "ops": [{ "op": "cells.set", "tableId": table, "records": [{ "rowKey": row_key, "values": { title.clone(): "Edited" } }] }] }),
    );
    r.ok(
        "table.transact",
        json!({ "ops": [{ "op": "records.create", "tableId": table, "at": { "after": row_key }, "records": [{ title.clone(): "Next" }] }] }),
    );
    r.ok("table.historyState", json!({}));
    r.ok("table.undo", json!({}));
    r.ok("table.redo", json!({}));
    r.ok("edit.undo", json!({}));
    r.ok("edit.redo", json!({}));

    r.ok("table.dashboards.upsert", json!({ "dashboardId": "d1", "title": "Overview", "order": 1, "source_table_id": table, "layout": [], "widgets": [{ "id": "w1", "type": "metric" }] }));
    r.ok("table.dashboards.list", json!({}));
    r.ok("table.dashboards.delete", json!({ "dashboardId": "d1" }));

    let data = base64::engine::general_purpose::STANDARD.encode(b"hello");
    let attachment = r.ok("table.attachments.put", json!({ "tableId": table, "name": "note.txt", "mimeType": "text/plain", "data": data }));
    let hash = attachment["url"].as_str().unwrap().trim_start_matches("kition-blob:").to_owned();
    r.ok("table.attachments.read", json!({ "hash": hash }));

    let csv = base64::engine::general_purpose::STANDARD.encode("Name,Age\nAda,36\n");
    let _ = r.run("table.import.preview", json!({ "fileName": "people.csv", "content": csv }));
    let _ = r.run(
        "table.import.run",
        json!({ "fileName": "people.csv", "content": csv, "write_mode": "append", "schema_mode": "create", "target": { "kind": "existing_table", "table_id": other } }),
    );
    let _ = r.run("table.ai.prepare", json!({ "tableId": table, "fieldId": primary, "recordId": record_id }));
    let _ = r.run(
        "table.ai.apply",
        json!({ "tableId": table, "fieldId": primary, "recordId": record_id, "results": [{ "recordId": record_id, "text": "x" }] }),
    );
    r.ok("table.changes.since", json!({ "version": "" }));
    r.ok("table.read", json!({ "tableId": table, "limit": 1 }));
    assert!(r.run("table.read", json!({ "viewId": 999 })).is_err());
    r.ok("table.file.render", json!({ "target": { "path": format!("tables/{table}") }, "scale": 2 }));
    r.ok("table.file.outline", json!({}));
    r.ok("table.preview.show", json!({ "tableId": table }));
    assert!(r.run("table.preview.show", json!({ "viewId": 999 })).is_err());

    // Errors: a missing table, a bad op, a missing record; each validates and leaves the file.
    assert!(r.run("table.fields.list", json!({ "tableId": 999 })).is_err());
    assert!(r.run("table.records.update", json!({ "tableId": table, "recordId": 999, "values": {} })).is_err());
    assert!(r.run("table.transact", json!({ "ops": [] })).is_err());
    assert!(r.run("table.changes.since", json!({ "version": "not base64!" })).is_err());
    assert!(r.run("table.file.render", json!({ "target": { "path": "tables/999" } })).is_err());

    let spare =
        r.ok("table.records.create", json!({ "tableId": table, "values": { title.clone(): "Spare" } }))["row_key"].as_str().unwrap().to_owned();
    r.ok("table.records.bulkDelete", json!({ "tableId": table, "row_keys": [spare] }));
    r.ok("table.records.delete", json!({ "tableId": table, "recordId": record_id }));
    r.ok("table.views.delete", json!({ "tableId": table, "viewId": view }));
    r.ok("table.tables.delete", json!({ "tableId": other }));

    let all: BTreeSet<String> = r.doc.commands().into_iter().map(|command| command.id).collect();
    let missing: Vec<&String> = all.difference(&r.ran).collect();
    assert!(missing.is_empty(), "commands never run: {missing:?}");
}

/// The answers the run above meets only as errors (links, AI fields, an import), answered for real
/// and described key by key.
#[test]
fn link_ai_and_import_answers_match_their_schemas_key_by_key() {
    let mut r = Runner { doc: TableDocument::create("Tasks.ktable", 640, 420).unwrap(), ran: BTreeSet::new() };
    let people = r.ok("table.tables.create", json!({ "title": "People", "fields": [{ "title": "Name", "type": "text", "primary": true }] }));
    let people_id = people["id"].as_i64().unwrap();
    let ada = r.ok("table.records.create", json!({ "tableId": people_id, "values": { "name": "Ada" } }))["row_key"].as_str().unwrap().to_owned();
    let owner =
        r.ok("table.fields.create", json!({ "tableId": 1, "title": "Owner", "type": "link_to_record", "options": { "target_table_id": people_id } }));
    let owner_id = owner["id"].as_i64().unwrap();
    let task = r.ok("table.records.create", json!({ "tableId": 1, "values": { "title": "Plan" } }))["id"].as_i64().unwrap();
    r.ok("table.links.replace", json!({ "tableId": 1, "recordId": task, "field_id": owner_id, "target_row_keys": [ada] }));
    r.ok("table.links.list", json!({ "tableId": 1, "recordId": task, "field_id": owner_id }));
    r.ok("table.fields.linkCandidates", json!({ "tableId": 1, "fieldId": owner_id, "q": "A" }));
    r.ok("table.links.delete", json!({ "tableId": 1, "recordId": task, "field_id": owner_id, "target_row_keys": [ada] }));

    let summary = r.ok(
        "table.fields.create",
        json!({ "tableId": 1, "title": "Summary", "type": "ai", "options": { "ai_config": { "type": "summarize", "source_field_id": 1 } } }),
    )["id"]
        .as_i64()
        .unwrap();
    r.ok("table.ai.prepare", json!({ "tableId": 1, "fieldId": summary, "recordId": task }));
    r.ok("table.ai.prepare", json!({ "tableId": 1, "fieldId": summary, "record_ids": [task] }));
    r.ok("table.ai.apply", json!({ "tableId": 1, "fieldId": summary, "recordId": task, "results": [{ "recordId": task, "text": "A plan." }] }));
    r.ok(
        "table.ai.apply",
        json!({ "tableId": 1, "fieldId": summary, "results": [{ "recordId": task, "text": "A plan." }, { "recordId": 999, "error": "no" }] }),
    );

    let csv = base64::engine::general_purpose::STANDARD.encode("Name,Age\nAda,36\nGrace,45\n");
    r.ok("table.import.preview", json!({ "fileName": "people.csv", "content": csv }));
    r.ok(
        "table.import.run",
        json!({ "fileName": "people.csv", "content": csv, "write_mode": "append", "schema_mode": "auto", "target": { "kind": "new_table", "table_title": "Imported" } }),
    );
    r.ok(
        "table.import.run",
        json!({ "fileName": "people.csv", "content": csv, "write_mode": "append", "schema_mode": "auto", "target": { "kind": "existing_table", "table_id": people_id } }),
    );
}

/// Every command says what it does and every reading command declares what it answers
/// (kition-sdk `check_commands`).
#[test]
fn every_command_is_described() {
    let doc = TableDocument::create("Tasks.ktable", 640, 420).unwrap();
    kition_kit::testing::check_commands(&doc.commands(), &[]).unwrap();
}
