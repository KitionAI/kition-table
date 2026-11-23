//! TEMPORARY (schema freeze): makes the conformance fixtures (`examples/checks.rs`) and the
//! agent cases' `trip.ktable` in the document's final shape (views' filters and sorts, dashboards'
//! widgets as objects), as content files, and prints them as base64 for the change that commits
//! them. Deleted in that change.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[allow(dead_code)]
#[path = "../examples/checks.rs"]
mod checks;

use base64::Engine;
use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn call(doc: &mut TableDocument, command: &str, params: Value) {
    doc.execute(command, params.clone()).unwrap_or_else(|err| panic!("{command} {params}: {err}"));
}

/// `cases/table.agent.context/two-tables-one-run/input/trip.ktable`: the starter table titled
/// Notes with two records, and a table Packing (Item, Packed) with three.
fn trip() -> Vec<u8> {
    let mut doc = TableDocument::create("trip.ktable", 0, 0).unwrap();
    call(&mut doc, "table.tables.update", json!({ "tableId": 1, "title": "Notes" }));
    for title in ["Ask about visas", "Find a sitter"] {
        call(&mut doc, "table.records.create", json!({ "tableId": 1, "values": { "title": title } }));
    }
    call(
        &mut doc,
        "table.tables.create",
        json!({ "title": "Packing", "fields": [{ "title": "Item", "type": "text", "primary": true }, { "title": "Packed", "type": "checkbox" }] }),
    );
    for item in ["Passport", "Charger", "Boots"] {
        call(&mut doc, "table.records.create", json!({ "tableId": 2, "values": { "item": item } }));
    }
    doc.prepare_save().unwrap();
    doc.save(None).unwrap()
}

#[test]
fn print_regenerated_fixtures() {
    let mut out = checks::fixtures().unwrap();
    out.push(("trip.ktable", trip()));
    let mut lines = Vec::new();
    for (name, bytes) in out {
        lines.push(format!("FIXTURE {name} {}", base64::engine::general_purpose::STANDARD.encode(&bytes)));
    }
    panic!("regenerated fixtures (temporary test):\n{}\nEND FIXTURES", lines.join("\n"));
}
