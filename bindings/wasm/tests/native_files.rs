//! The in-memory document and the file on disk are one format: a document the kit saves opens with
//! `TableFile::open` on disk, writable, with the kit's edits; and a file written on disk opens in
//! the kit.

#![allow(clippy::unwrap_used)]

use kition_kit::KitDocument;
use kition_table_file::{CreateOptions, FieldSpec, TableFile, TableOp, TableSpec};
use kition_table_kit::TableDocument;
use serde_json::{Map, Value, json};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("native-files");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn a_document_the_kit_saves_opens_natively_with_its_edits() {
    let mut doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
    let document = doc.call("documents.get", &Value::Null).unwrap()["result"].clone();
    let table_id = document["tables"][0]["id"].as_i64().unwrap();
    let rows = doc.execute("table.records.create", json!({ "tableId": table_id, "values": { "title": "From the kit" } })).unwrap();
    let field = rows["result"]["values"].as_object().unwrap().keys().next().unwrap().clone();
    let path = scratch("kit-saved.ktable");
    std::fs::write(&path, doc.sealed_save().unwrap()).unwrap();

    let mut native = TableFile::open(&path).unwrap();
    assert!(native.read_only().is_none(), "the file opens writable");
    let records = native.list_records(table_id).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].values[&field], "From the kit");
    // And it takes writes, as the runtime makes them.
    let mut values = Map::new();
    values.insert(field.clone(), Value::from("From Kition"));
    native.transact(&[TableOp::RecordsCreate { table_id, at: None, records: vec![values] }]).unwrap();
    assert_eq!(native.list_records(table_id).unwrap().len(), 2);
}

#[test]
fn a_file_kition_writes_opens_in_the_kit() {
    let path = scratch("native.ktable");
    let mut native = TableFile::create(&path, &CreateOptions { title: "Native".into(), ..Default::default() }).unwrap();
    let table = native
        .create_table(&TableSpec {
            name: "items".into(),
            title: "Items".into(),
            fields: vec![FieldSpec { name: "name".into(), title: "Name".into(), field_type: "text".into(), primary: true, ..Default::default() }],
            ..Default::default()
        })
        .unwrap();
    let rows = (0..50).map(|index| [("name".to_owned(), Value::from(format!("Item {index}")))].into_iter().collect()).collect();
    native.transact(&[TableOp::RecordsCreate { table_id: table.id, at: None, records: rows }]).unwrap();
    drop(native);

    let mut doc = TableDocument::open("native.ktable", &std::fs::read(&path).unwrap()).unwrap();
    let page = doc.call("records.list", &json!({ "tableId": table.id, "limit": 10, "offset": 40 })).unwrap();
    assert_eq!(page["result"]["total"], 50);
    assert_eq!(page["result"]["items"].as_array().unwrap().len(), 10);
    assert_eq!(page["result"]["items"][0]["values"]["name"], "Item 40");
}
