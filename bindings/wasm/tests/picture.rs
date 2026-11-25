//! The document's picture is what the editor shows: the tab bar, the shown view's
//! bar, and the view itself with every field and record (fields' titles and values drawn as text
//! in the faces the host registers), or a dashboard. It shows the first table's first view, as the
//! editor opens, until `preview.show` picks a table, a view or a dashboard.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

/// Registers the picture's faces from the kition-fonts checkout beside this repository (or
/// `KITION_FONTS_DIR`), as the editor and the showcase do.
fn fonts() {
    let dir = std::env::var_os("KITION_FONTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../kition-fonts")));
    let found = kition_table_fonts::register_dir(&dir);
    assert_eq!(found, kition_table_fonts::CHECKOUT_FILES.len(), "the kition-fonts checkout at {}", dir.display());
}

fn call(doc: &mut TableDocument, method: &str, params: Value) -> Value {
    doc.execute(method, params).unwrap_or_else(|error| panic!("{method}: {error}"))["result"].clone()
}

const STAGES: [&str; 4] = ["Lead", "Proposal", "Won", "Lost"];

/// A Deals table of ten fields and 25 records; answers the document, the table and its fields.
fn deals(first_title: &str) -> (TableDocument, i64, Vec<Value>) {
    fonts();
    let mut doc = TableDocument::create("Deals.ktable", 0, 0).unwrap();
    call(&mut doc, "table.delete", json!({ "tableId": 1 }));
    let mut fields = vec![
        json!({ "name": "deal", "title": first_title, "type": "text", "primary": true }),
        json!({ "name": "stage", "title": "Stage", "type": "single_select", "options": { "choices": STAGES } }),
        json!({ "name": "value", "title": "Value", "type": "number" }),
        json!({ "name": "close", "title": "Close", "type": "date" }),
        json!({ "name": "done", "title": "Done", "type": "checkbox" }),
        json!({ "name": "stars", "title": "Stars", "type": "rating", "options": { "max": 5 } }),
    ];
    for extra in 0..4 {
        fields.push(json!({ "name": format!("note_{extra}"), "title": format!("Note {extra}"), "type": "text" }));
    }
    let table = call(&mut doc, "table.add", json!({ "title": "Deals", "fields": fields }));
    let table_id = table["id"].as_i64().unwrap();
    let records: Vec<Value> = (0..25)
        .map(|index| {
            json!({ "deal": format!("Deal {index}"), "stage": STAGES[index % 4], "value": 1000 * (index + 1), "close": "2026-12-01",
                    "done": index % 2 == 0, "stars": index % 6, "note_0": "Call back" })
        })
        .collect();
    call(&mut doc, "edit.transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "records": records }] }));
    let fields = table["fields"].as_array().unwrap().clone();
    (doc, table_id, fields)
}

fn pixels(doc: &TableDocument, area: [i32; 4]) -> Vec<u8> {
    let [x0, y0, x1, y1] = area;
    doc.render_area(x0, y0, x1, y1, (x1 - x0) as u32, (y1 - y0) as u32).unwrap()
}

#[test]
fn the_picture_shows_the_first_view_with_every_field_and_record() {
    let (mut doc, table, fields) = deals("Deal");
    let shown = call(&mut doc, "preview.show", json!({}));
    let view = call(&mut doc, "tableView.list", json!({ "tableId": table }))["items"][0].clone();
    assert_eq!(shown["tableId"], json!(table));
    assert_eq!(shown["viewId"], view["id"]);
    assert_eq!(shown["viewType"], json!("grid"));
    assert_eq!(shown["records"], json!(25));
    // Ten columns of 200 px beside the row heads, and 25 rows of 40 px below the bars and the
    // header row.
    let (width, height) = (doc.width(), doc.height());
    assert!(width >= 72 + 10 * 200, "{width}");
    assert!(height >= 44 + 52 + 32 + 25 * 40, "{height}");
    // A column's width in the view is the picture's.
    call(&mut doc, "tableView.setField", json!({ "tableId": table, "viewId": view["id"], "fieldId": fields[0]["id"], "width": 320 }));
    assert_eq!(doc.width(), width + 120);
}

#[test]
fn titles_and_values_are_drawn_as_text() {
    // Two titles of one length: bars as long as the text could not tell them apart.
    let (alpha, _, _) = deals("Alpha");
    let (omega, _, _) = deals("Omega");
    let header = [72, 96, 272, 128];
    assert_ne!(pixels(&alpha, header), pixels(&omega, header), "the title's letters are drawn");
    // The rest of the picture is the same.
    assert_eq!(pixels(&alpha, [272, 96, 1200, 400]), pixels(&omega, [272, 96, 1200, 400]));
    // A cell's text: its value's letters, in the cell.
    let (mut doc, table, _) = deals("Deal");
    let before = pixels(&doc, [72, 128, 272, 168]);
    let first = call(&mut doc, "record.list", json!({ "tableId": table, "limit": 1 }))["items"][0]["id"].clone();
    call(&mut doc, "record.set", json!({ "tableId": table, "recordId": first, "values": { "deal": "Deal 9" } }));
    assert_ne!(pixels(&doc, [72, 128, 272, 168]), before);
}

#[test]
fn a_view_shows_its_records_as_it_filters_sorts_and_lays_them_out() {
    let (mut doc, table, fields) = deals("Deal");
    let open = call(&mut doc, "tableView.add", json!({ "tableId": table, "title": "Open", "type": "grid" }))["id"].clone();
    let stage = fields[1]["id"].clone();
    call(
        &mut doc,
        "tableView.setFilterTree",
        json!({ "tableId": table, "viewId": open, "tree": { "conjunction": "and", "children": [{ "condition": { "field_id": stage, "operator": "neq", "value": "Won" } }] } }),
    );
    let shown = call(&mut doc, "preview.show", json!({ "viewId": open }));
    assert_eq!((shown["tableId"].clone(), shown["view"].clone(), shown["records"].clone()), (json!(table), json!("Open"), json!(19)));
    let board = call(&mut doc, "tableView.add", json!({ "tableId": table, "title": "Board", "type": "kanban" }))["id"].clone();
    let shown = call(&mut doc, "preview.show", json!({ "viewId": board }));
    assert_eq!(shown["viewType"], json!("kanban"));
    // A column per choice of the stage field: 280 px wide, 16 px apart, after 16 px.
    assert!(doc.width() >= 16 + 4 * 296, "{}", doc.width());
    for bad in
        [json!({ "viewId": 999 }), json!({ "tableId": 999 }), json!({ "dashboardId": "none" }), json!({ "tableId": table, "dashboardId": "x" })]
    {
        assert!(doc.execute("preview.show", bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn a_dashboard_is_drawn_with_its_widgets_data() {
    let (mut doc, table, _) = deals("Deal");
    call(
        &mut doc,
        "dashboard.set",
        json!({ "dashboardId": "pipeline", "title": "Pipeline", "order": 0, "source_table_id": table,
                "layout": [{ "widget_id": "won", "x": 0, "y": 0, "w": 4, "h": 2 }],
                "widgets": [{ "id": "won", "title": "Won", "type": "metric", "query": { "aggregation": "count", "filters": [{ "field_name": "stage", "operator": "equals", "value": "Won" }] } }] }),
    );
    let shown = call(&mut doc, "preview.show", json!({ "dashboardId": "pipeline" }));
    assert_eq!(shown["dashboardId"], json!("pipeline"));
    assert_eq!(doc.width(), 1200);
    let metric = [20, 64, 400, 240];
    let before = pixels(&doc, metric);
    call(&mut doc, "record.add", json!({ "tableId": table, "values": { "deal": "One more", "stage": "Won" } }));
    assert_ne!(pixels(&doc, metric), before, "the count is drawn from the records");
    // Back to what the editor opens.
    assert_eq!(call(&mut doc, "preview.show", json!({}))["tableId"], json!(table));
}

/// regressions/issue-picture-filter-as-editor: the picture shows the records the editor shows.
/// An editor condition the editor does not apply (one still being written, with no value yet, or
/// one whose operator the editor does not offer for the field's type) is left out of the picture
/// too. Before, the picture had its own copy of the arrangement and applied them: `is` with no
/// value kept only the records with an empty cell, so the picture showed none of the 25.
#[test]
fn the_picture_leaves_out_a_filter_condition_the_editor_does_not_apply() {
    let (mut doc, table, fields) = deals("Deal");
    let view = call(&mut doc, "tableView.add", json!({ "tableId": table, "title": "Half written", "type": "grid" }))["id"].clone();
    let (deal, value) = (fields[0]["id"].clone(), fields[2]["id"].clone());
    let tree = json!({ "conjunction": "and", "children": [
        { "condition": { "field_id": deal, "operator": "is", "value": null } },
        { "condition": { "field_id": value, "operator": "hasAnyOf", "value": ["1000"] } },
        { "condition": { "field_id": deal, "operator": "isNot", "value": "  " } },
    ] });
    call(&mut doc, "tableView.setFilterTree", json!({ "tableId": table, "viewId": view, "tree": tree }));
    let shown = call(&mut doc, "preview.show", json!({ "viewId": view }));
    let axis = doc.grid_open(&json!({ "tableId": table, "viewId": view })).unwrap();
    let editor = u32::from_le_bytes(axis[16..20].try_into().unwrap());
    assert_eq!((shown["records"].clone(), editor), (json!(25), 25));
}
