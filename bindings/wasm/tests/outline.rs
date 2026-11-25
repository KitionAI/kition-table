//! `file.outline`, the document's outline (`kit.json` `outline.command`, Kition platform
//! K24): the file's tables, then its dashboards, each `{ id, title, kind, view }` at the top level
//! (`items`), for a host to list under the file and open the file at. `view` is the place the
//! editor takes (`document.view`, `document/reveal`). It reads only: the document stays as it was.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

/// A file with the tables "People" and "Second", and the dashboard "Overview".
fn file() -> (TableDocument, i64, i64) {
    let mut doc = TableDocument::create("Team.ktable", 640, 420).unwrap();
    let first = doc.execute("file.get", json!({})).unwrap()["result"]["tables"][0]["id"].as_i64().unwrap();
    doc.execute("table.set", json!({ "tableId": first, "title": "People" })).unwrap();
    let created = doc.execute("table.add", json!({ "title": "Second" })).unwrap()["result"].clone();
    let second = created["id"].as_i64().unwrap();
    doc.execute("dashboard.set", json!({ "dashboardId": "overview", "title": "Overview", "source_table_id": first, "layout": [], "widgets": [] }))
        .unwrap();
    doc.mark_saved();
    (doc, first, second)
}

#[test]
fn lists_the_tables_then_the_dashboards_with_the_view_the_editor_opens() {
    let (mut doc, first, second) = file();
    let answer = doc.execute("file.outline", json!({})).unwrap();
    assert_eq!(
        answer,
        json!({ "items": [
            { "id": first.to_string(), "title": "People", "kind": "table", "view": { "path": format!("tables/{first}") } },
            { "id": second.to_string(), "title": "Second", "kind": "table", "view": { "path": format!("tables/{second}") } },
            { "id": "overview", "title": "Overview", "kind": "dashboard", "view": { "path": "dashboards/overview" } },
        ] }),
        "the outline is at the top level, in the document's order"
    );
}

#[test]
fn reads_only_and_takes_no_parameters() {
    let (mut doc, ..) = file();
    let (revision, saved) = (doc.revision(), doc.saved_bytes().unwrap());
    doc.execute("file.outline", json!({})).unwrap();
    assert_eq!((doc.revision(), doc.dirty()), (revision, false));
    assert_eq!(doc.saved_bytes().unwrap(), saved);
    let command = doc.commands().into_iter().find(|command| command.id == "file.outline").unwrap();
    assert!(!command.mutates, "a read-only session runs it");
    let error = doc.execute("file.outline", json!({ "tableId": 1 })).unwrap_err();
    assert!(error.contains("tableId"), "a parameter it does not take is refused by name: {error}");
}

/// kit.json names the command, so a host finds the outline without knowing the kit.
#[test]
fn the_manifest_names_the_outline_command() {
    let manifest: Value = serde_json::from_str(include_str!("../../../kit.json")).unwrap();
    assert_eq!(manifest["outline"], json!({ "command": "file.outline" }));
}

/// The view of every item is one the editor reads (`web/src/shown.ts`: `tables/<integer>` or
/// `dashboards/<id>`).
#[test]
fn every_view_is_a_path_the_editor_reads() {
    let (mut doc, ..) = file();
    let answer = doc.execute("file.outline", json!({})).unwrap();
    for item in answer["items"].as_array().unwrap() {
        let path = item["view"]["path"].as_str().unwrap();
        let (kind, id) = path.split_once('/').unwrap();
        match item["kind"].as_str().unwrap() {
            "table" => assert!(kind == "tables" && id.parse::<i64>().is_ok(), "{path}"),
            "dashboard" => assert!(kind == "dashboards" && !id.is_empty() && !id.contains('/'), "{path}"),
            other => panic!("an item is a table or a dashboard, not {other}"),
        }
        assert_eq!(item["id"].as_str(), Some(id));
    }
}
