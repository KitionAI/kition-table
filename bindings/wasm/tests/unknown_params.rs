//! A command with a parameter schema refuses a key the schema does not name, and says which
//! (platform rule E7a, kit requirement AX15): an agent that misspells a parameter is told instead
//! of the command running without it.
#![allow(clippy::unwrap_used, clippy::panic)]

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::json;

fn doc() -> TableDocument {
    TableDocument::create("Table.ktable", 0, 0).unwrap()
}

#[test]
fn every_command_refuses_an_unknown_parameter() {
    if let Err(problems) = kition_kit::testing::check_unknown_params(doc, &[]) {
        panic!("{} problems:\n{}", problems.len(), problems.join("\n"));
    }
}

#[test]
fn a_misspelled_parameter_names_the_one_it_resembles() {
    let error = doc().execute("record.list", json!({ "tabelId": 1 })).unwrap_err();
    assert!(error.contains("`tabelId`") && error.contains("did you mean `tableId`"), "{error}");
    let error = doc().execute("ai.prepare", json!({ "tableId": 1, "fieldId": 2, "recordIds": [3] })).unwrap_err();
    assert!(error.contains("`recordIds`") && error.contains("`record_ids`"), "{error}");
}

#[test]
fn the_undo_methods_take_the_scope_they_read() {
    let mut table = doc();
    table.execute("history.undo", json!({ "scope": "agent" })).unwrap();
    table.execute("history.redo", json!({ "scope": "person" })).unwrap();
}
