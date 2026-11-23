//! `table.file.outline` at its edges: every item has a title a host can show (a table without a
//! title goes by its name, a dashboard without a name by its id), a deleted table is not listed,
//! and a file with no tables lists its dashboards alone (or nothing).
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn outline(doc: &mut TableDocument) -> Vec<Value> {
    doc.execute("table.file.outline", json!({})).unwrap()["items"].as_array().unwrap().clone()
}

fn tables(doc: &mut TableDocument) -> Vec<i64> {
    doc.execute("table.documents.get", json!({})).unwrap()["result"]["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|table| table["id"].as_i64().unwrap())
        .collect()
}

#[test]
fn an_unnamed_dashboard_goes_by_its_id() {
    let mut doc = TableDocument::create("Team.ktable", 640, 420).unwrap();
    let first = tables(&mut doc)[0];
    doc.execute("table.dashboards.upsert", json!({ "dashboardId": "unnamed", "title": "", "source_table_id": first, "layout": [], "widgets": [] }))
        .unwrap();
    doc.execute("table.dashboards.upsert", json!({ "dashboardId": "blank", "title": "   ", "source_table_id": first, "layout": [], "widgets": [] }))
        .unwrap();
    let dashboards: Vec<Value> = outline(&mut doc).into_iter().filter(|item| item["kind"] == "dashboard").collect();
    assert_eq!(
        dashboards,
        vec![
            json!({ "id": "unnamed", "title": "unnamed", "kind": "dashboard", "view": { "path": "dashboards/unnamed" } }),
            json!({ "id": "blank", "title": "blank", "kind": "dashboard", "view": { "path": "dashboards/blank" } }),
        ],
        "a dashboard without a name is listed by its id"
    );
}

#[test]
fn every_title_is_one_a_host_can_show() {
    let mut doc = TableDocument::create("Team.ktable", 640, 420).unwrap();
    doc.execute("table.tables.create", json!({ "title": "" })).unwrap();
    doc.execute("table.tables.create", json!({ "title": "  " })).unwrap();
    let first = tables(&mut doc)[0];
    doc.execute("table.dashboards.upsert", json!({ "dashboardId": "d", "title": "", "source_table_id": first, "layout": [], "widgets": [] }))
        .unwrap();
    let items = outline(&mut doc);
    assert_eq!(items.len(), 4);
    for item in &items {
        let title = item["title"].as_str().unwrap();
        assert!(!title.trim().is_empty(), "an empty title: {item}");
    }
}

#[test]
fn a_deleted_table_is_not_listed() {
    let mut doc = TableDocument::create("Team.ktable", 640, 420).unwrap();
    let second = doc.execute("table.tables.create", json!({ "title": "Second" })).unwrap()["result"]["id"].as_i64().unwrap();
    doc.execute("table.tables.delete", json!({ "tableId": second })).unwrap();
    let ids: Vec<String> = outline(&mut doc).iter().map(|item| item["id"].as_str().unwrap().to_owned()).collect();
    assert_eq!(ids, tables(&mut doc).iter().map(i64::to_string).collect::<Vec<_>>());
    assert!(!ids.contains(&second.to_string()));
}

#[test]
fn a_file_with_no_tables_lists_its_dashboards_alone() {
    let mut doc = TableDocument::create("Team.ktable", 640, 420).unwrap();
    let first = tables(&mut doc)[0];
    doc.execute(
        "table.dashboards.upsert",
        json!({ "dashboardId": "overview", "title": "Overview", "source_table_id": first, "layout": [], "widgets": [] }),
    )
    .unwrap();
    doc.execute("table.tables.delete", json!({ "tableId": first })).unwrap();
    assert_eq!(tables(&mut doc), Vec::<i64>::new());
    assert_eq!(
        outline(&mut doc),
        vec![json!({ "id": "overview", "title": "Overview", "kind": "dashboard", "view": { "path": "dashboards/overview" } })]
    );
    doc.execute("table.dashboards.delete", json!({ "dashboardId": "overview" })).unwrap();
    assert_eq!(outline(&mut doc), Vec::<Value>::new(), "an empty file has an empty outline");
}
