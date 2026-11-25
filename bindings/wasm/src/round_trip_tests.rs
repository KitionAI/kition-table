//! Command-level round trips (plan phase P2): each test edits a new table file only with commands,
//! checks the edit took, saves the `.ktable`, reopens it and compares the whole document as the
//! API reads it (every table with its fields, every record, every view with its filters, sorts,
//! groups and columns, every dashboard); then it saves the reopened file and compares once more.
//! Nothing is ignored: the file stores every value these reads answer.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use base64::Engine;
use kition_kit::KitDocument;
use serde_json::{Value, json};

use crate::TableDocument;

fn new_document() -> TableDocument {
    TableDocument::create("Tasks.ktable", 0, 0).unwrap()
}

/// Runs a command and answers its `result` (an answer also carries the `change` it made).
fn run(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params.clone()).unwrap_or_else(|error| panic!("{command} {params}: {error}"))["result"].clone()
}

fn read(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    run(doc, command, params)
}

/// The first table's id and its primary field's id and name.
fn first_table(doc: &mut TableDocument) -> (i64, i64, String) {
    let document = read(doc, "file.get", json!({}));
    let table = &document["tables"][0];
    let primary = table["primary_field_id"].as_i64().unwrap();
    let field = table["fields"].as_array().unwrap().iter().find(|field| field["id"].as_i64() == Some(primary)).unwrap();
    (table["id"].as_i64().unwrap(), primary, field["name"].as_str().unwrap().to_owned())
}

/// The name of a table's primary field, the key its records' values use.
fn primary_name(doc: &mut TableDocument, table_id: i64) -> String {
    let document = read(doc, "file.get", json!({}));
    let table = document["tables"].as_array().unwrap().iter().find(|table| table["id"].as_i64() == Some(table_id)).unwrap().clone();
    let primary = table["primary_field_id"].as_i64().unwrap();
    table["fields"].as_array().unwrap().iter().find(|field| field["id"].as_i64() == Some(primary)).unwrap()["name"].as_str().unwrap().to_owned()
}

/// The whole document as the API reads it.
fn view(doc: &mut TableDocument) -> Value {
    let document = read(doc, "file.get", json!({}));
    let mut tables = Vec::new();
    for table in document["tables"].as_array().unwrap() {
        let id = table["id"].as_i64().unwrap();
        let records = read(doc, "record.list", json!({ "tableId": id, "limit": 1000, "offset": 0 }));
        let mut views = Vec::new();
        for view in read(doc, "tableView.list", json!({ "tableId": id })).as_array().cloned().unwrap_or_default() {
            let view_id = view["id"].as_i64().unwrap();
            let at = json!({ "tableId": id, "viewId": view_id });
            views.push(json!({
                "view": view,
                "filters": read(doc, "tableView.getFilterTree", at.clone()),
                "sorts": read(doc, "tableView.getSorts", at.clone()),
                "groups": read(doc, "tableView.getGroups", at.clone()),
                "fields": read(doc, "tableView.listFields", at),
            }));
        }
        tables.push(json!({ "table": table, "records": records, "views": views }));
    }
    json!({ "document": document, "tables": tables, "dashboards": read(doc, "dashboard.list", json!({})) })
}

/// Saves, reopens and compares; then saves the reopened file and compares once more.
fn round_trip(doc: &mut TableDocument) -> TableDocument {
    let before = view(doc);
    let bytes = doc.saved_bytes().unwrap();
    let mut back = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    assert_eq!(view(&mut back), before, "the reopened file differs from the saved one");
    let again = back.saved_bytes().unwrap();
    let mut twice = TableDocument::open("Tasks.ktable", &again).unwrap();
    assert_eq!(view(&mut twice), before, "a second save of the reopened file differs");
    back
}

#[test]
fn tables_and_fields_survive_save_and_reopen() {
    let mut doc = new_document();
    let (table, _, title) = first_table(&mut doc);
    let people = run(&mut doc, "table.add", json!({ "title": "People", "fields": [{ "title": "Name", "type": "text", "primary": true }] }))["id"]
        .as_i64()
        .unwrap();
    run(&mut doc, "table.set", json!({ "tableId": people, "title": "Persons", "description": "Who works on what" }));
    let created = run(&mut doc, "field.add", json!({ "tableId": table, "title": "Score", "type": "number" }));
    let (score, score_name) = (created["id"].as_i64().unwrap(), created["name"].as_str().unwrap().to_owned());
    run(&mut doc, "field.set", json!({ "tableId": table, "fieldId": score, "title": "Points" }));
    let copy = run(&mut doc, "field.duplicate", json!({ "tableId": table, "fieldId": score }))["id"].as_i64().unwrap();
    run(&mut doc, "field.delete", json!({ "tableId": table, "fieldId": copy }));
    run(&mut doc, "field.add", json!({ "tableId": table, "title": "Due", "type": "date" }));
    run(&mut doc, "field.add", json!({ "tableId": table, "title": "Done", "type": "checkbox" }));
    run(&mut doc, "record.add", json!({ "tableId": table, "values": { title: "Write the plan 中文 🚀", score_name: 3 } }));
    let edited = view(&mut doc);
    let names: Vec<_> =
        edited["document"]["tables"].as_array().unwrap().iter().map(|table| table["title"].as_str().unwrap_or_default().to_owned()).collect();
    assert!(names.contains(&"Persons".to_owned()), "{names:?}");
    assert!(edited.to_string().contains("Points") && !edited.to_string().contains("Points copy"), "{edited}");
    round_trip(&mut doc);
}

#[test]
fn views_with_filters_sorts_groups_and_columns_survive_save_and_reopen() {
    let mut doc = new_document();
    let (table, primary, title) = first_table(&mut doc);
    let created = run(&mut doc, "field.add", json!({ "tableId": table, "title": "Score", "type": "number" }));
    let (score, score_name) = (created["id"].as_i64().unwrap(), created["name"].as_str().unwrap().to_owned());
    for (name, points) in [("Ada", 3), ("Grace", 5), ("Linus", 1)] {
        run(&mut doc, "record.add", json!({ "tableId": table, "values": { title.clone(): name, score_name.clone(): points } }));
    }
    let view_id = run(&mut doc, "tableView.add", json!({ "tableId": table, "title": "Board", "type": "grid" }))["id"].as_i64().unwrap();
    run(&mut doc, "tableView.set", json!({ "tableId": table, "viewId": view_id, "title": "Top scores" }));
    run(
        &mut doc,
        "tableView.setFilterTree",
        json!({ "tableId": table, "viewId": view_id, "tree": { "conjunction": "and", "children": [{ "condition": { "field_id": score, "operator": "greater_than", "value": 2 } }] } }),
    );
    run(&mut doc, "tableView.setSorts", json!({ "tableId": table, "viewId": view_id, "items": [{ "field_id": score, "direction": "desc" }] }));
    run(&mut doc, "tableView.setGroups", json!({ "tableId": table, "viewId": view_id, "items": [{ "field_id": primary, "direction": "asc" }] }));
    run(&mut doc, "tableView.setConfig", json!({ "tableId": table, "viewId": view_id, "row_height": "short", "frozen_column_count": 1 }));
    run(&mut doc, "tableView.setField", json!({ "tableId": table, "viewId": view_id, "fieldId": score, "visible": false, "width": 120 }));
    let at = json!({ "tableId": table, "viewId": view_id });
    let sorts = read(&mut doc, "tableView.getSorts", at.clone());
    assert!(sorts.to_string().contains("desc"), "{sorts}");
    let mut back = round_trip(&mut doc);
    assert_eq!(read(&mut back, "tableView.getSorts", at), sorts);
}

#[test]
fn record_links_survive_save_and_reopen() {
    let mut doc = new_document();
    let (table, _, title) = first_table(&mut doc);
    let people = run(&mut doc, "table.add", json!({ "title": "People", "fields": [{ "title": "Name", "type": "text", "primary": true }] }))["id"]
        .as_i64()
        .unwrap();
    let name = primary_name(&mut doc, people);
    let ada = run(&mut doc, "record.add", json!({ "tableId": people, "values": { name.clone(): "Ada" } }));
    let grace = run(&mut doc, "record.add", json!({ "tableId": people, "values": { name: "Grace" } }));
    let owner =
        run(&mut doc, "field.add", json!({ "tableId": table, "title": "Owner", "type": "link_to_record", "options": { "target_table_id": people } }))
            ["id"]
            .as_i64()
            .unwrap();
    let task = run(&mut doc, "record.add", json!({ "tableId": table, "values": { title: "Ship" } }))["id"].as_i64().unwrap();
    let keys = [ada["row_key"].as_str().unwrap(), grace["row_key"].as_str().unwrap()];
    run(&mut doc, "recordLink.replace", json!({ "tableId": table, "recordId": task, "field_id": owner, "target_row_keys": keys }));
    let at = json!({ "tableId": table, "recordId": task, "field_id": owner });
    let links = read(&mut doc, "recordLink.list", at.clone());
    assert!(links.to_string().contains("Ada") && links.to_string().contains("Grace"), "{links}");
    let mut back = round_trip(&mut doc);
    assert_eq!(read(&mut back, "recordLink.list", at), links);
}

#[test]
fn csv_imports_survive_save_and_reopen() {
    let mut doc = new_document();
    let (table, _, title) = first_table(&mut doc);
    run(&mut doc, "file.importCsv", json!({ "tableId": table, "content": format!("{title}\nFrom CSV\n\"Quoted, with a comma\"\n中文\n") }));
    let people = run(&mut doc, "table.add", json!({ "title": "People", "fields": [{ "title": "Name", "type": "text", "primary": true }] }))["id"]
        .as_i64()
        .unwrap();
    let csv = base64::engine::general_purpose::STANDARD.encode("Name,Age\nAda,36\nGrace,45\n");
    run(
        &mut doc,
        "file.runImport",
        json!({ "fileName": "people.csv", "content": csv, "write_mode": "append", "schema_mode": "auto", "target": { "kind": "existing_table", "table_id": people } }),
    );
    let exported = read(&mut doc, "file.exportCsv", json!({ "tableId": table }));
    assert!(exported.to_string().contains("Quoted, with a comma"), "{exported}");
    let people_csv = read(&mut doc, "file.exportCsv", json!({ "tableId": people }));
    assert!(people_csv.to_string().contains("Grace"), "{people_csv}");
    let mut back = round_trip(&mut doc);
    assert_eq!(read(&mut back, "file.exportCsv", json!({ "tableId": table })), exported);
}

#[test]
fn dashboards_survive_save_and_reopen() {
    let mut doc = new_document();
    let (table, _, _) = first_table(&mut doc);
    run(
        &mut doc,
        "dashboard.set",
        json!({ "dashboardId": "d1", "title": "Overview", "order": 1, "source_table_id": table, "layout": [], "widgets": [{ "id": "w1", "type": "metric" }] }),
    );
    run(
        &mut doc,
        "dashboard.set",
        json!({ "dashboardId": "d2", "title": "Spare", "order": 2, "source_table_id": table, "layout": [], "widgets": [] }),
    );
    run(&mut doc, "dashboard.delete", json!({ "dashboardId": "d2" }));
    let dashboards = read(&mut doc, "dashboard.list", json!({}));
    assert!(dashboards.to_string().contains("Overview") && !dashboards.to_string().contains("Spare"), "{dashboards}");
    let mut back = round_trip(&mut doc);
    assert_eq!(read(&mut back, "dashboard.list", json!({})), dashboards);
}

#[test]
fn attachments_survive_save_and_reopen() {
    let mut doc = new_document();
    let (table, _, _) = first_table(&mut doc);
    run(&mut doc, "field.add", json!({ "tableId": table, "name": "files", "title": "Files", "type": "attachment" }));
    let data = base64::engine::general_purpose::STANDARD.encode(b"hello attachment");
    let attachment = run(&mut doc, "attachment.add", json!({ "tableId": table, "name": "note.txt", "mimeType": "text/plain", "data": data }));
    let hash = attachment["url"].as_str().unwrap().trim_start_matches("kition-blob:").to_owned();
    run(&mut doc, "record.add", json!({ "tableId": table, "values": { "title": "With a file", "files": [attachment] } }));
    let loose = base64::engine::general_purpose::STANDARD.encode(b"a blob no record names");
    let unnamed = run(&mut doc, "attachment.add", json!({ "tableId": table, "name": "loose.txt", "mimeType": "text/plain", "data": loose }));
    let unnamed = unnamed["url"].as_str().unwrap().trim_start_matches("kition-blob:").to_owned();
    let stored = read(&mut doc, "attachment.get", json!({ "hash": hash }));
    let mut back = round_trip(&mut doc);
    assert_eq!(read(&mut back, "attachment.get", json!({ "hash": hash })), stored, "the bytes a record names travel in the file");
    assert!(back.file().blob(&unnamed).unwrap().is_none(), "a blob no record names does not");
}
