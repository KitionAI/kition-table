//! Tables that work are clean: a count rollup needs no field to read (it counts
//! the linked records), and `file.importCsv` stores each cell as its field's type (numbers and ratings
//! as numbers, checkboxes as booleans, multiple choices as a list), so neither is reported.

#![allow(clippy::unwrap_used)]

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn call(doc: &mut TableDocument, method: &str, params: Value) -> Value {
    doc.call(method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))["result"].clone()
}

fn codes(doc: &TableDocument) -> Vec<String> {
    doc.check().into_iter().map(|diagnostic| format!("{} {}", diagnostic.code, diagnostic.message)).collect()
}

#[test]
fn a_count_rollup_without_a_target_field_is_clean() {
    let mut doc = TableDocument::create("Crm.ktable", 0, 0).unwrap();
    call(&mut doc, "table.delete", json!({ "tableId": 1 }));
    let deals = call(
        &mut doc,
        "table.add",
        json!({ "title": "Deals", "fields": [{ "name": "deal", "title": "Deal", "type": "text", "primary": true }] }),
    )["id"]
        .clone();
    let companies = call(
        &mut doc,
        "table.add",
        json!({ "title": "Companies", "fields": [{ "name": "company", "title": "Company", "type": "text", "primary": true }] }),
    )["id"]
        .clone();
    let link = call(
        &mut doc,
        "field.add",
        json!({ "tableId": companies, "name": "deals", "title": "Deals", "type": "link_to_record", "options": { "target_table_id": deals, "allow_multiple": true } }),
    )["id"]
        .clone();
    call(
        &mut doc,
        "field.add",
        json!({ "tableId": companies, "name": "deal_count", "title": "Deal count", "type": "rollup", "options": { "link_field_id": link, "aggregation": "count" } }),
    );
    let created = call(
        &mut doc,
        "edit.transact",
        json!({ "ops": [{ "op": "records.create", "tableId": deals, "records": [{ "deal": "A" }, { "deal": "B" }] }] }),
    );
    call(&mut doc, "record.add", json!({ "tableId": companies, "values": { "company": "Harbour", "deals": created["created"] } }));
    let records = call(&mut doc, "record.list", json!({ "tableId": companies }));
    assert_eq!(records["items"][0]["values"]["deal_count"], json!(2), "the rollup counts the linked records");
    assert_eq!(codes(&doc), Vec::<String>::new());

    // A sum still needs the field it adds up.
    call(
        &mut doc,
        "field.add",
        json!({ "tableId": companies, "name": "total", "title": "Total", "type": "rollup", "options": { "link_field_id": link, "aggregation": "sum" } }),
    );
    assert!(codes(&doc).iter().any(|code| code.starts_with("table.broken-lookup")), "{:?}", codes(&doc));
}

#[test]
fn a_csv_import_stores_each_cell_as_its_type() {
    let mut doc = TableDocument::create("Stock.ktable", 0, 0).unwrap();
    call(&mut doc, "table.delete", json!({ "tableId": 1 }));
    let table = call(
        &mut doc,
        "table.add",
        json!({ "title": "Parts", "fields": [
            { "name": "part", "title": "Part", "type": "text", "primary": true },
            { "name": "stock", "title": "Stock", "type": "number" },
            { "name": "price", "title": "Price", "type": "number" },
            { "name": "quality", "title": "Quality", "type": "rating", "options": { "max": 5 } },
            { "name": "active", "title": "Active", "type": "checkbox" },
            { "name": "tags", "title": "Tags", "type": "multi_select", "options": { "choices": ["Road", "Gravel", "Kids"] } },
            { "name": "code", "title": "Code", "type": "text" },
        ] }),
    )["id"]
        .clone();
    let csv = "Part,Stock,Price,Quality,Active,Tags,Code\nChain,12,24.50,4,yes,\"Road, Gravel\",007\nBell,,3,5,false,Kids,12\n";
    call(&mut doc, "file.importCsv", json!({ "tableId": table, "content": csv }));
    let records = call(&mut doc, "record.list", json!({ "tableId": table }));
    assert_eq!(
        records["items"][0]["values"],
        json!({ "part": "Chain", "stock": 12, "price": 24.5, "quality": 4, "active": true, "tags": ["Road", "Gravel"], "code": "007" })
    );
    assert_eq!(records["items"][1]["values"], json!({ "part": "Bell", "price": 3, "quality": 5, "active": false, "tags": ["Kids"], "code": "12" }));
    assert_eq!(codes(&doc), Vec::<String>::new());
}
