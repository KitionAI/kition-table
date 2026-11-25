//! A large clear on the grid (case table.editor.grid/clear-engine): `gridClear` sets the cleared
//! fields' empty values on the records of the ids it is given, finding their row keys itself, as
//! one `transact` of one `cells.set`, so one undo brings every cell back; ids that name no record
//! of the table are left out, and a clear that names no field is refused.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn call(doc: &mut TableDocument, method: &str, params: Value) -> Value {
    doc.call(method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))["result"].clone()
}

fn field_name(made: &Value) -> String {
    let field = if made["field"].is_object() { &made["field"] } else { made };
    field["name"].as_str().unwrap().to_owned()
}

#[test]
fn grid_clear_empties_the_cells_of_the_ids_in_one_undo_step() {
    let mut doc = TableDocument::create("Clear.ktable", 0, 0).unwrap();
    let table = call(&mut doc, "file.get", json!({}))["tables"][0].clone();
    let table_id = table["id"].as_i64().unwrap();
    let title = table["fields"].as_array().unwrap().iter().find(|field| field["is_primary"] == true).unwrap()["name"].as_str().unwrap().to_owned();
    let amount = field_name(&call(&mut doc, "field.add", json!({ "tableId": table_id, "title": "Amount", "type": "number" })));
    let notes = field_name(&call(&mut doc, "field.add", json!({ "tableId": table_id, "title": "Notes", "type": "text" })));
    let records: Vec<Value> =
        (1..=5).map(|i| json!({ title.clone(): format!("Record {i}"), amount.clone(): i * 10, notes.clone(): format!("Note {i}") })).collect();
    call(&mut doc, "edit.transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "at": null, "records": records }] }));
    let listed = |doc: &mut TableDocument| call(doc, "record.list", json!({ "tableId": table_id }))["items"].as_array().unwrap().clone();
    let before = listed(&mut doc);
    let ids: Vec<i64> = before.iter().map(|item| item["id"].as_i64().unwrap()).collect();

    // Records 1, 3 and 5 cleared in both fields; an unknown id left out.
    let answer = doc
        .grid_clear(&json!({ "tableId": table_id, "values": { amount.clone(): null, notes.clone(): null } }), &[ids[0], ids[2], 999_999, ids[4]])
        .unwrap();
    assert_eq!(answer["result"]["cleared"], json!(3));
    let after = listed(&mut doc);
    for (index, record) in after.iter().enumerate() {
        let values = &record["values"];
        assert_eq!(values[&title], json!(format!("Record {}", index + 1)), "the title is not cleared");
        if index % 2 == 0 {
            assert!(values[&amount].is_null() && values[&notes].is_null(), "record {} is cleared: {values}", index + 1);
        } else {
            assert_eq!(values[&amount], json!((index + 1) * 10));
            assert_eq!(values[&notes], json!(format!("Note {}", index + 1)));
        }
    }

    // One undo step brings every cleared cell back.
    let undone = call(&mut doc, "history.undo", json!({}));
    assert_eq!(undone["done"], json!(true));
    let restored = listed(&mut doc);
    for (old, now) in before.iter().zip(&restored) {
        assert_eq!(old["values"], now["values"]);
    }

    // Nothing to clear is no write; a clear that names no field is refused.
    let revision = doc.revision();
    assert_eq!(
        doc.grid_clear(&json!({ "tableId": table_id, "values": { amount.clone(): null } }), &[999_999]).unwrap()["result"]["cleared"],
        json!(0)
    );
    assert_eq!(doc.revision(), revision);
    assert!(doc.grid_clear(&json!({ "tableId": table_id, "values": {} }), &ids).unwrap_err().contains("values"));
    assert!(doc.grid_clear(&json!({ "values": { amount: null } }), &ids).unwrap_err().contains("tableId"));
}
