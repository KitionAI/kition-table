//! Times the editor's arrangement (`gridOpen`, `gridSync`) of a large table natively and prints
//! `gridStats` with the time of each arrangement: the table's order, a sort by text, the view
//! `Grouped` as stored, a filter, a search, and an edit moved into a sorted view.
//!
//! `cargo run --release -p kition-table-kit --example grid_arrange -- <table.ktable>`, with the
//! grid's performance fixture (`examples/perf_fixture.rs`, 100,000 records in 50 fields).

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).ok_or("usage: grid_arrange <table.ktable>")?;
    let bytes = std::fs::read(&path).map_err(|error| format!("{path}: {error}"))?;
    let mut doc = TableDocument::open("Perf.ktable", &bytes)?;
    let table = doc.call("documents.get", &json!({}))?["result"]["tables"][0].clone();
    let table_id = table["id"].as_i64().ok_or("no table")?;
    let views = table["views"].as_array().cloned().unwrap_or_default();
    let view = |title: &str| views.iter().find(|view| view["title"] == title).and_then(|view| view["id"].as_i64());
    let all = view("All rows").or_else(|| views.first().and_then(|view| view["id"].as_i64())).ok_or("no view")?;
    let fields = table["fields"].as_array().cloned().unwrap_or_default();
    let field = |title: &str| fields.iter().find(|field| field["title"] == title).and_then(|field| field["name"].as_str()).map(str::to_owned);
    let text = field("Text 1").ok_or("no field Text 1")?;
    let number = field("Number 1").ok_or("no field Number 1")?;

    let scenarios: Vec<(&str, Value)> = vec![
        ("table order", json!({ "tableId": table_id, "viewId": all, "config": {} })),
        (
            "sort by Text 1",
            json!({ "tableId": table_id, "viewId": all, "config": { "sort_items": [{ "id": "s", "field_name": text, "direction": "asc" }] } }),
        ),
        ("view Grouped as stored", json!({ "tableId": table_id, "viewId": view("Grouped").unwrap_or(all) })),
        (
            "filter Number 1 > 500",
            json!({ "tableId": table_id, "viewId": all, "config": { "filter_tree": { "id": "g", "kind": "group", "conjunction": "and", "children": [
                { "id": "c", "kind": "condition", "field_name": number, "operator": "isGreater", "value": 500 },
            ] } } }),
        ),
        ("search \"review\"", json!({ "tableId": table_id, "viewId": all, "config": {}, "query": "review" })),
        (
            "sort by Text 1 again",
            json!({ "tableId": table_id, "viewId": all, "config": { "sort_items": [{ "id": "s", "field_name": text, "direction": "asc" }] } }),
        ),
    ];
    for (label, params) in &scenarios {
        let bytes = doc.grid_open(params)?;
        let stats = doc.grid_stats();
        println!("{label}: {:.1} ms, {} shown, axis {} bytes", stats["arrange"]["lastMs"].as_f64().unwrap_or_default(), stats["shown"], bytes.len());
    }
    let first = doc.call("records.list", &json!({ "tableId": table_id, "limit": 1 }))?["result"]["items"][0]["id"].clone();
    for value in ["zzz moved last", "aaa moved first", "middle"] {
        doc.call("records.update", &json!({ "tableId": table_id, "recordId": first, "values": { text.clone(): value } }))?;
        let bytes = doc.grid_sync()?;
        let stats = doc.grid_stats();
        println!("edit to {value:?}, synced: {:.2} ms, axis {} bytes", stats["relocate"]["lastMs"].as_f64().unwrap_or_default(), bytes.len());
    }
    println!("{}", serde_json::to_string_pretty(&doc.grid_stats()).map_err(|error| error.to_string())?);
    Ok(())
}
