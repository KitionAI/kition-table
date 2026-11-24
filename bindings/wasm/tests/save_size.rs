//! Opt-in measurement (`#[ignore]`): how much of a document is free pages before a save, and
//! what the save (which vacuums a sparse database first) costs and writes. Run in release:
//!
//! ```sh
//! cargo test --release -p kition-table-kit --test save_size -- --ignored --nocapture
//! ```

#![allow(clippy::unwrap_used)]
// A native measurement: the clock is read off wasm only.
#![allow(clippy::disallowed_methods)]

use std::time::Instant;

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn pages(doc: &TableDocument) -> (i64, i64) {
    let conn = doc.file().conn();
    let count: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0)).unwrap();
    let free: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0)).unwrap();
    (count, free)
}

fn report(label: &str, doc: &mut TableDocument) {
    let (count, free) = pages(doc);
    let started = Instant::now();
    let bytes = doc.sealed_save().unwrap();
    let save = started.elapsed();
    println!(
        "{label}: {count} pages, {free} free ({:.0}%); save {save:?} writes {:.1} MB",
        free as f64 * 100.0 / count as f64,
        bytes.len() as f64 / 1e6,
    );
}

fn build(count: usize) -> (TableDocument, i64, String) {
    let mut doc = TableDocument::create("Large.ktable", 0, 0).unwrap();
    let document = doc.call("documents.get", &Value::Null).unwrap()["result"].clone();
    let table = &document["tables"][0];
    let table_id = table["id"].as_i64().unwrap();
    let primary = table["primary_field_id"].as_i64().unwrap();
    let field =
        table["fields"].as_array().unwrap().iter().find(|field| field["id"].as_i64() == Some(primary)).unwrap()["name"].as_str().unwrap().to_owned();
    for chunk in 0..count / 1000 {
        let records: Vec<Value> = (0..1000).map(|index| json!({ field.clone(): format!("Item {}", chunk * 1000 + index) })).collect();
        doc.call("transact", &json!({ "ops": [{ "op": "records.create", "tableId": table_id, "at": null, "records": records }] })).unwrap();
    }
    (doc, table_id, field)
}

#[test]
#[ignore = "measurement; run in release"]
fn save_size() {
    for count in [10_000, 50_000] {
        let (mut doc, table_id, field) = build(count);
        report(&format!("{count} records, as built"), &mut doc);
        let page = doc.call("records.list", &json!({ "tableId": table_id, "limit": 2000 })).unwrap();
        let keys: Vec<String> = page["result"]["items"].as_array().unwrap().iter().map(|r| r["row_key"].as_str().unwrap().to_owned()).collect();
        for (round, key) in keys.iter().enumerate().take(600) {
            doc.call("transact", &json!({ "ops": [{ "op": "cells.set", "tableId": table_id, "records": [{ "rowKey": key, "values": { field.clone(): format!("Edit {round}") } }] }] })).unwrap();
        }
        report(&format!("{count} records, after 600 edits"), &mut doc);
        let doomed: Vec<String> = keys.iter().take(2000).cloned().collect();
        doc.call("transact", &json!({ "ops": [{ "op": "records.delete", "tableId": table_id, "rowKeys": doomed }] })).unwrap();
        report(&format!("{count} records, after deleting 2,000"), &mut doc);
    }
}
