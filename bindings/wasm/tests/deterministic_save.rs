//! A document built by the same commands saves the same bytes in a reproducible build: with the
//! `reproducible-ids` feature (the dev-dependencies enable it) and `KITION_REPRODUCIBLE_IDS` set,
//! its ids (document, records, tables, fields, views, the replica's actor, filter nodes) come from
//! a counter instead of randomness, and with `SOURCE_DATE_EPOCH` set its times are the epoch. Each
//! build runs in a process of its own, as a sample build does: this test starts itself twice.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

/// 2026-11-09T00:00:00Z.
const EPOCH: i64 = 1_794_182_400;
const OUT: &str = "KITION_TABLE_DETERMINISM_OUT";

/// A small table document with every kind of object a sample makes.
fn build() -> Vec<u8> {
    let mut doc = TableDocument::create("Deals.ktable", 0, 0).unwrap();
    doc.call("tables.delete", &json!({ "tableId": 1 })).unwrap();
    let table = doc
        .call(
            "tables.create",
            &json!({ "title": "Deals", "fields": [
                { "name": "deal", "title": "Deal", "type": "text", "primary": true },
                { "name": "stage", "title": "Stage", "type": "single_select", "options": { "choices": ["Lead", "Won"] } },
                { "name": "value", "title": "Value", "type": "number" },
            ] }),
        )
        .unwrap()["result"]
        .clone();
    let table_id = table["id"].as_i64().unwrap();
    let stage = table["fields"][1]["id"].as_i64().unwrap();
    doc.call(
        "transact",
        &json!({ "ops": [{ "op": "records.create", "tableId": table_id, "records": [
            { "deal": "Fleet tracking", "stage": "Lead", "value": 48000 },
            { "deal": "Parent portal", "stage": "Won", "value": 12400 },
        ] }] }),
    )
    .unwrap();
    let view = doc.call("views.create", &json!({ "tableId": table_id, "title": "Open", "type": "grid" })).unwrap()["result"]["id"].as_i64().unwrap();
    doc.call(
        "views.putFilterTree",
        &json!({ "tableId": table_id, "viewId": view, "tree": { "conjunction": "and", "children": [{ "condition": { "field_id": stage, "operator": "neq", "value": "Won" } }] } }),
    )
    .unwrap();
    doc.call(
        "dashboards.upsert",
        &json!({ "dashboardId": "overview", "title": "Overview", "order": 0, "source_table_id": table_id, "layout": [], "widgets": [] }),
    )
    .unwrap();
    doc.save(None).unwrap()
}

/// The child's half: builds the document into the file `OUT` names (nothing without it).
#[test]
fn child_builds_a_document() {
    if let Ok(path) = std::env::var(OUT) {
        std::fs::write(path, build()).unwrap();
    }
}

fn build_in_a_process(path: &PathBuf) -> Vec<u8> {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_builds_a_document", "--test-threads=1", "--quiet"])
        .env("SOURCE_DATE_EPOCH", EPOCH.to_string())
        .env("KITION_REPRODUCIBLE_IDS", EPOCH.to_string())
        .env(OUT, path)
        .status()
        .unwrap();
    assert!(status.success(), "the child build failed");
    std::fs::read(path).unwrap()
}

#[test]
fn the_same_commands_save_the_same_bytes() {
    let dir = std::env::temp_dir().join(format!("kition-table-determinism-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Several builds: a hash map's order differs only now and then.
    let first = build_in_a_process(&dir.join("0.ktable"));
    for build in 1..6 {
        let again = build_in_a_process(&dir.join(format!("{build}.ktable")));
        assert!(first == again, "build {build} saved other bytes than build 0 ({} and {} bytes)", again.len(), first.len());
    }
    std::fs::remove_dir_all(&dir).ok();

    // Every time the document holds is the epoch.
    let mut doc = TableDocument::open("Deals.ktable", &first).unwrap();
    let tables = doc.call("tables.list", &Value::Null).unwrap();
    let table = tables["result"]["items"].as_array().unwrap().iter().find(|table| table["title"] == "Deals").unwrap();
    let records = doc.call("records.list", &json!({ "tableId": table["id"] })).unwrap();
    for record in records["result"]["items"].as_array().unwrap() {
        assert_eq!(record["created_at"], json!(EPOCH * 1000), "{record}");
        assert_eq!(record["updated_at"], json!(EPOCH * 1000), "{record}");
    }
    assert_eq!(table["created_at"], json!(EPOCH * 1000));
}
