//! Stable addresses also reach reads and diagnostic previews.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_table_kit::{KitDocument, TableDocument};
use serde_json::json;

#[test]
fn published_schemas_accept_stable_and_local_addresses() {
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    doc.execute("table.records.create", json!({"tableId":1,"values":{"title":"One"}})).unwrap();
    let overview = doc.inspect().unwrap();
    let table = overview["tables"][0]["tool_id"].clone();
    let field = overview["tables"][0]["fields"][0]["tool_id"].clone();
    let record = doc.file().get_record(1, 1).unwrap().row_key;
    for (command, local, stable) in [
        (
            "table.records.update",
            json!({"tableId":1,"recordId":1,"values":{"title":"Local"}}),
            json!({"tableId":table,"recordId":record,"values":{"title":"Stable"}}),
        ),
        (
            "table.fields.create",
            json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}),
            json!({"tableId":table,"name":"friends","type":"link_to_record","options":{"target_table_id":table}}),
        ),
        (
            "table.transact",
            json!({"ops":[{"op":"fields.move","tableId":1,"fieldIds":[1],"toIndex":1}]}),
            json!({"ops":[{"op":"fields.move","tableId":table,"fieldIds":[field],"toIndex":1}]}),
        ),
    ] {
        let schema = kition_table_kit::agent::input_json_schema(command).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        for params in [local, stable] {
            assert!(validator.is_valid(&params), "{command}: {params}");
        }
    }
}

#[test]
fn document_and_history_ids_are_not_mislabelled_as_child_ids() {
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    doc.execute("table.records.create", json!({"tableId":1,"values":{"title":"One"}})).unwrap();
    doc.execute("table.records.update", json!({"tableId":1,"recordId":1,"values":{"title":"Two"}})).unwrap();
    let base = doc.file().replica().objects_of_type("Base")[0].to_string();
    let answer = doc.execute("table.documents.get", json!({})).unwrap();
    assert_eq!(answer["result"]["tool_id"], base);
    let answer = doc.execute("table.records.history", json!({"tableId":1,"recordId":1})).unwrap();
    assert!(!answer["result"]["items"].as_array().unwrap().is_empty());
    assert!(answer["result"]["items"].as_array().unwrap().iter().all(|item| item.get("tool_id").is_none()));
}

#[test]
fn preview_diagnostic_paths_accept_stable_ids() {
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    let overview = doc.inspect().unwrap();
    let table = overview["tables"][0]["tool_id"].as_str().unwrap();
    let field = overview["tables"][0]["fields"][0]["tool_id"].as_str().unwrap();
    let numeric = doc.execute("table.file.render", json!({"target":{"path":"tables/1/fields/1"},"maxSize":64})).unwrap();
    let stable = doc.execute("table.file.render", json!({"target":{"path":format!("tables/{table}/fields/{field}")},"maxSize":64})).unwrap();
    assert_eq!(stable, numeric);
}

#[test]
fn changed_layout_preimages_refuse_the_replaced_entry() {
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    let overview = doc.inspect().unwrap();
    let table = &overview["tables"][0];
    let view = table["views"][0]["tool_id"].clone();
    let field = table["fields"][2]["tool_id"].clone();
    let params = json!({"tableId":table["tool_id"],"viewId":view,"fieldId":field,"position":2});
    doc.execute("table.views.patchField", json!({"tableId":1,"viewId":1,"fieldId":3,"position":7})).unwrap();
    let under = doc.under("table.views.patchField", &params).unwrap().unwrap();
    assert!(under.covers.is_some(), "{under:?}");
    let answer = doc
        .covered(
            "table.views.patchField",
            &params,
            &[kition_kit::CoveredAsk {
                id: view,
                before: serde_json::Map::from_iter([("config".to_owned(), serde_json::Value::Null)]),
                ..Default::default()
            }],
        )
        .unwrap()
        .unwrap();
    assert_eq!(answer.covered[0].covered, None, "{answer:?}");
}
