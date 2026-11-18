//! What the grid reads to edit (case table.editor.grid/editing-engine): `gridRecords` answers the
//! records of some ids in the order asked, with their row keys (what writes name), only the fields
//! asked for when it is given some, and leaves out ids that name no record of the table.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn call(doc: &mut TableDocument, method: &str, params: Value) -> Value {
    doc.call(method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))["result"].clone()
}

#[test]
fn grid_records_answers_the_records_asked_for_in_order_with_their_row_keys() {
    let mut doc = TableDocument::create("Records.ktable", 0, 0).unwrap();
    let table = call(&mut doc, "documents.get", json!({}))["tables"][0].clone();
    let table_id = table["id"].as_i64().unwrap();
    let title = table["fields"].as_array().unwrap().iter().find(|field| field["is_primary"] == true).unwrap()["name"].as_str().unwrap().to_owned();
    let made = call(&mut doc, "fields.create", json!({ "tableId": table_id, "title": "Amount", "type": "number" }));
    let amount = if made["field"].is_object() { made["field"]["name"].clone() } else { made["name"].clone() };
    let amount = amount.as_str().unwrap().to_owned();
    let records: Vec<Value> = (1..=3).map(|i| json!({ title.clone(): format!("Record {i}"), amount.clone(): i * 10 })).collect();
    call(&mut doc, "transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "at": null, "records": records }] }));
    let listed = call(&mut doc, "records.list", json!({ "tableId": table_id }));
    let items = listed["items"].as_array().unwrap();
    let ids: Vec<i64> = items.iter().map(|item| item["id"].as_i64().unwrap()).collect();

    // In the order asked, an unknown id left out.
    let answer = doc.grid_records(&json!({ "tableId": table_id, "ids": [ids[2], 999_999, ids[0]] })).unwrap();
    let answer = answer.as_array().unwrap();
    assert_eq!(answer.len(), 2);
    assert_eq!(answer[0]["id"], json!(ids[2]));
    assert_eq!(answer[1]["id"], json!(ids[0]));
    assert_eq!(answer[0]["row_key"], items[2]["row_key"]);
    assert!(answer[0]["row_key"].as_str().is_some_and(|key| !key.is_empty()));
    assert_eq!(answer[0]["values"][&title], json!("Record 3"));
    assert_eq!(answer[0]["values"][&amount], json!(30));

    // Only the fields asked for; none at all for the row keys alone.
    let only = doc.grid_records(&json!({ "tableId": table_id, "ids": [ids[1]], "fields": [amount] })).unwrap();
    assert_eq!(only[0]["values"], json!({ amount.clone(): 20 }));
    let keys = doc.grid_records(&json!({ "tableId": table_id, "ids": [ids[1]], "fields": [] })).unwrap();
    assert_eq!(keys[0]["values"], json!({}));
    assert_eq!(keys[0]["row_key"], items[1]["row_key"]);

    // Records of another table are not this table's.
    let other = call(&mut doc, "tables.create", json!({ "title": "Other" }));
    let other_id = other["id"].as_i64().or_else(|| other["table"]["id"].as_i64()).unwrap();
    assert_eq!(doc.grid_records(&json!({ "tableId": other_id, "ids": [ids[0]] })).unwrap(), json!([]));

    // Malformed asks are refused.
    assert!(doc.grid_records(&json!({ "ids": [1] })).unwrap_err().contains("tableId"));
    assert!(doc.grid_records(&json!({ "tableId": table_id })).unwrap_err().contains("ids"));
}

/// Many ids (a paste or a fill over a whole column, case regressions/issue-table-bulk-transact) are
/// read in one pass; the answer is what reading them a few at a time answers: the order asked,
/// repeated ids repeated, unknown and deleted ids left out, a record's missing cell left out, and
/// each value as stored.
#[test]
fn grid_records_of_many_ids_answers_what_a_few_at_a_time_answer() {
    let mut doc = TableDocument::create("Many.ktable", 0, 0).unwrap();
    let table = call(&mut doc, "documents.get", json!({}))["tables"][0].clone();
    let table_id = table["id"].as_i64().unwrap();
    let title = table["fields"].as_array().unwrap().iter().find(|field| field["is_primary"] == true).unwrap()["name"].as_str().unwrap().to_owned();
    let made = call(&mut doc, "fields.create", json!({ "tableId": table_id, "title": "Amount", "type": "number" }));
    let amount = if made["field"].is_object() { made["field"]["name"].clone() } else { made["name"].clone() };
    let amount = amount.as_str().unwrap().to_owned();
    let records: Vec<Value> = (0..700)
        .map(|i| match i % 4 {
            0 => json!({ title.clone(): format!("Record {i} \u{e9}\u{1f600} \"quoted\"") }),
            1 => json!({ title.clone(): format!("Record {i}"), amount.clone(): f64::from(i) + 0.5 }),
            2 => json!({ title.clone(): null, amount.clone(): i * 1000 }),
            _ => json!({ amount.clone(): -i }),
        })
        .collect();
    call(&mut doc, "transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "at": null, "records": records }] }));
    let listed = call(&mut doc, "records.list", json!({ "tableId": table_id, "limit": 1000 }));
    let mut ids: Vec<i64> = listed["items"].as_array().unwrap().iter().map(|item| item["id"].as_i64().unwrap()).collect();
    assert_eq!(ids.len(), 700);
    call(&mut doc, "records.delete", json!({ "tableId": table_id, "recordId": ids[5] }));
    ids.reverse();
    let again = [ids[3], 999_999, ids[3], ids[10]];
    ids.extend(again);
    for fields in [json!([]), json!([amount]), json!([title, amount, "no such field"])] {
        let all = doc.grid_records(&json!({ "tableId": table_id, "ids": ids, "fields": fields })).unwrap();
        let few: Vec<Value> = ids
            .chunks(100)
            .flat_map(|chunk| doc.grid_records(&json!({ "tableId": table_id, "ids": chunk, "fields": fields })).unwrap().as_array().unwrap().clone())
            .collect();
        assert_eq!(all, Value::Array(few), "fields {fields}");
        assert_eq!(all.as_array().unwrap().len(), ids.len() - 2);
    }
}
