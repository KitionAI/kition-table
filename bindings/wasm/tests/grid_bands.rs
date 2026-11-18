//! The editor's grid in bands (cases table.editor.grid/bands and table.editor.grid/band-raster):
//! the axes `gridAxes` lays out from the arrangement `gridOpen` answers, and the bands
//! `gridBands` answers for them, each a display list of its own area, clipped to it, with the
//! icons and emoji tiles it uses; `gridRaster` draws a band at the device size.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::PathBuf;

use kition_kit::KitDocument;
use kition_table_display::{DisplayList, Layer, Op, decode};
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn fonts() {
    let dir = std::env::var_os("KITION_FONTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../kition-fonts")));
    let found = kition_table_fonts::register_dir(&dir);
    assert_eq!(found, kition_table_fonts::CHECKOUT_FILES.len(), "the kition-fonts checkout at {}", dir.display());
}

fn call(doc: &mut TableDocument, method: &str, params: Value) -> Value {
    doc.call(method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))["result"].clone()
}

const TITLES: [&str; 6] = [
    "Quarterly planning",
    "季度计划 Q3",
    "서울 지사",
    "Launch \u{1F680}",
    "Ethiopic \u{1230}\u{120B}",
    "A long title that does not fit into its column at all",
];

/// A table of 70 records in six fields, a view grouped by status, and the views' ids.
fn document() -> (TableDocument, i64, i64, i64) {
    fonts();
    let mut doc = TableDocument::create("Bands.ktable", 0, 0).unwrap();
    let table = call(&mut doc, "documents.get", json!({}))["tables"][0].clone();
    let table_id = table["id"].as_i64().unwrap();
    let grid = table["views"][0]["id"].as_i64().unwrap();
    let primary = table["fields"].as_array().unwrap().iter().find(|field| field["is_primary"] == true).unwrap()["name"].as_str().unwrap().to_owned();
    for field in table["fields"].as_array().unwrap().iter().filter(|field| field["is_primary"] != true) {
        call(&mut doc, "fields.delete", json!({ "tableId": table_id, "fieldId": field["id"] }));
    }
    let mut field = |title: &str, kind: &str, options: Value| {
        let made = call(&mut doc, "fields.create", json!({ "tableId": table_id, "title": title, "type": kind, "options": options }));
        if made["field"].is_object() { made["field"].clone() } else { made }
    };
    let status = field("Status", "single_select", json!({ "choices": ["To do", "Done"] }));
    let score = field("Score", "rating", json!({ "max": 5, "icon": "star" }));
    let done = field("Done", "checkbox", Value::Null);
    let amount = field("Amount", "number", Value::Null);
    let notes = field("Notes", "long_text", Value::Null);
    let records: Vec<Value> = (0..70)
        .map(|i| {
            json!({
                primary.clone(): TITLES[i % TITLES.len()],
                status["name"].as_str().unwrap(): if i % 3 == 0 { "Done" } else { "To do" },
                score["name"].as_str().unwrap(): i % 6,
                done["name"].as_str().unwrap(): i % 2 == 0,
                amount["name"].as_str().unwrap(): i as f64 * 12.5,
                notes["name"].as_str().unwrap(): format!("Note {i}"),
            })
        })
        .collect();
    call(&mut doc, "transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "at": null, "records": records }] }));
    let grouped = call(&mut doc, "views.create", json!({ "tableId": table_id, "title": "Grouped", "type": "grid" }));
    let grouped = grouped["id"].as_i64().or_else(|| grouped["view"]["id"].as_i64()).unwrap();
    call(&mut doc, "views.putGroups", json!({ "tableId": table_id, "viewId": grouped, "items": [{ "field_id": status["id"], "direction": "asc" }] }));
    (doc, table_id, grid, grouped)
}

/// Arranges `view` and lays it out; answers the axes.
fn axes(doc: &mut TableDocument, table_id: i64, view: i64, theme: &str) -> Value {
    let axis = doc.grid_open(&json!({ "tableId": table_id, "viewId": view })).unwrap();
    doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": view, "theme": theme })).unwrap()
}

/// A batch's items: (kind, payload).
fn items(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
    assert_eq!(&bytes[..4], b"KTDB");
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 1);
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let mut at = 12;
    let mut out = Vec::new();
    for _ in 0..count {
        let length = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        out.push((bytes[at], bytes[at + 8..at + 8 + length].to_vec()));
        at += 8 + length.next_multiple_of(4);
    }
    assert_eq!(at, bytes.len(), "the batch ends with its last item");
    out
}

fn lists(batch: &[(u8, Vec<u8>)]) -> Vec<DisplayList> {
    batch.iter().filter(|(kind, _)| *kind == 1).map(|(_, bytes)| decode(bytes).unwrap()).collect()
}

#[test]
fn the_axes_lay_out_the_arrangement_with_the_editors_geometry() {
    let (mut doc, table_id, grid, grouped) = document();
    let plain = axes(&mut doc, table_id, grid, "light");
    // 70 records of the default 40 px and the add row.
    assert_eq!(plain["rowRuns"], json!([[40.0, 70], [32.0, 1]]));
    assert_eq!(plain["headerHeight"], json!(32.0));
    // No sort and no groups: the row head has its drag handle (three controls of 24 px).
    assert_eq!(plain["rowHeadWidth"], json!(72.0));
    let columns = plain["columns"].as_array().unwrap();
    assert_eq!(columns.len(), 7, "six fields and the add-field column");
    assert_eq!(columns[0], json!({ "x": 72.0, "width": 200.0, "frozen": true, "fieldId": columns[0]["fieldId"] }));
    assert_eq!(columns[6]["width"], json!(100.0));
    assert_eq!(plain["bandRows"], json!(32));
    assert_eq!(plain["skeleton"], json!({ "background": "#ffffff", "line": "#e6e3e0" }));

    // Grouped by status: a 48 px header before each group's records and an add row after them;
    // the records are numbered within their group and the row head has no drag handle.
    let grouped = axes(&mut doc, table_id, grouped, "dark");
    assert_eq!(grouped["rowRuns"], json!([[48.0, 1], [40.0, 24], [32.0, 1], [48.0, 1], [40.0, 46], [32.0, 1]]));
    assert_eq!(grouped["rowHeadWidth"], json!(48.0));
    assert_eq!(grouped["skeleton"]["background"], json!("#1b1d23"));
}

#[test]
fn every_band_draws_its_own_area_clipped_with_the_resources_it_uses() {
    let (mut doc, table_id, grid, _) = document();
    let answer = axes(&mut doc, table_id, grid, "light");
    let epoch = answer["epoch"].as_u64().unwrap() as u32;
    let keys = [0, 0, 0, 1, 0, 0, 4, 0, 0, 2, 0, 0, 3, 0, 0, 0, 0, 1, 0, 0, 9];
    let batch = items(&doc.grid_bands(epoch, &keys, 2.0, 0, &[]).unwrap());
    let bands = lists(&batch);
    // Row band 9 is past the 71 rows: left out.
    assert_eq!(bands.len(), 6);
    let layers: Vec<Layer> = bands.iter().map(|list| list.header.key.layer).collect();
    assert_eq!(layers, [Layer::Body, Layer::Frozen, Layer::RowHead, Layer::Header, Layer::HeaderFrozen, Layer::Body]);
    for list in &bands {
        let (x, y) = list.header.origin;
        let (w, h) = list.header.size;
        assert_eq!(list.header.epoch, epoch);
        assert!(w > 0.0 && h > 0.0);
        assert_eq!(list.ops.first(), Some(&Op::Clip { x, y, w, h }), "{:?} starts with its clip", list.header.key);
        assert_eq!(list.ops.last(), Some(&Op::Unclip));
        for op in &list.ops {
            if let Op::Text { x: tx, width, .. } = op {
                assert!(*tx + width >= x - 1.0 && *tx - width <= x + w + 1.0, "text of {:?} near its band", list.header.key);
            }
        }
    }
    // The first body band: rows 1-32 (40 px from y 32), the scrolling columns from the frozen one.
    let body = &bands[0];
    assert_eq!(body.header.origin, (272.0, 32.0));
    assert_eq!(body.header.size.1, 32.0 * 40.0);
    // The row heads number the rows; the header strips name the fields.
    let texts = |list: &DisplayList| {
        list.ops.iter().filter_map(|op| if let Op::Text { text, .. } = op { Some(list.text(*text).to_owned()) } else { None }).collect::<Vec<_>>()
    };
    assert!(texts(&bands[2]).iter().any(|text| text == "1") && texts(&bands[2]).iter().any(|text| text == "32"));
    assert!(texts(&bands[3]).iter().any(|text| text == "Status"));
    // CJK and Hangul in their own faces, an emoji as a tile, a character no face covers as tofu.
    let frozen = &bands[1];
    assert!(frozen.ops.iter().any(|op| matches!(op, Op::Text { face: kition_table_display::Face::Fallback, .. })));
    assert!(frozen.ops.iter().any(|op| matches!(op, Op::Text { face: kition_table_display::Face::Hangul, .. })));
    assert!(frozen.ops.iter().any(|op| matches!(op, Op::Tofu { .. })));
    let emoji: Vec<u32> = frozen.ops.iter().filter_map(|op| if let Op::Emoji { atlas, .. } = op { Some(*atlas) } else { None }).collect();
    assert!(!emoji.is_empty());
    let tiles: Vec<&Vec<u8>> = batch.iter().filter(|(kind, _)| *kind == 3).map(|(_, bytes)| bytes).collect();
    assert_eq!(tiles.len(), 1, "one tile for the one emoji at one size");
    let (width, height) = (u32::from_le_bytes(tiles[0][12..16].try_into().unwrap()), u32::from_le_bytes(tiles[0][16..20].try_into().unwrap()));
    assert_eq!(tiles[0].len(), 28 + (width * height * 4) as usize);
    // Every icon a band names is in the batch, from id 0 on.
    let icons: Vec<u16> = batch.iter().filter(|(kind, _)| *kind == 2).map(|(_, bytes)| u16::from_le_bytes([bytes[0], bytes[1]])).collect();
    let used: Vec<u16> =
        bands.iter().flat_map(|list| list.ops.iter().filter_map(|op| if let Op::Icon { icon, .. } = op { Some(*icon) } else { None })).collect();
    assert!(!used.is_empty() && used.iter().all(|id| icons.contains(id)));
    let data: Value = serde_json::from_slice(&batch.iter().find(|(kind, _)| *kind == 2).unwrap().1[4..]).unwrap();
    assert_eq!(data["grid"], json!(24.0));
    assert!(data["paths"].is_array() && data["fills"].is_array());

    // Asked again with what the page holds: no icon and no tile again.
    let known = vec![format!("{}:13:2", emoji[0])];
    let again = items(&doc.grid_bands(epoch, &keys[3..6], 2.0, u32::try_from(icons.len()).unwrap(), &known).unwrap());
    assert!(again.iter().all(|(kind, _)| *kind == 1), "{:?}", again.iter().map(|(kind, _)| kind).collect::<Vec<_>>());
}

#[test]
fn a_band_of_another_arrangement_is_refused() {
    let (mut doc, table_id, grid, _) = document();
    let answer = axes(&mut doc, table_id, grid, "light");
    let epoch = answer["epoch"].as_u64().unwrap() as u32;
    assert!(doc.grid_bands(epoch + 1, &[0, 0, 0], 1.0, 0, &[]).unwrap_err().contains("not"));
    assert!(doc.grid_bands(epoch, &[7, 0, 0], 1.0, 0, &[]).unwrap_err().contains("band key"));
}

#[test]
fn the_raster_is_the_band_at_the_device_size() {
    let (mut doc, table_id, grid, _) = document();
    let answer = axes(&mut doc, table_id, grid, "light");
    let epoch = answer["epoch"].as_u64().unwrap() as u32;
    let raster = doc.grid_raster(epoch, [4, 0, 0], 1.5).unwrap();
    let (width, height) = (u32::from_le_bytes(raster[0..4].try_into().unwrap()), u32::from_le_bytes(raster[4..8].try_into().unwrap()));
    assert_eq!((width, height), (108, 1920));
    assert_eq!(raster.len(), 8 + (width * height * 4) as usize);
    // The paper is opaque white under the row heads.
    assert_eq!(&raster[8 + 4 * (width as usize * 10 + 5)..][..4], &[255, 255, 255, 255]);
}

#[test]
fn the_grid_measures_runs_of_one_face() {
    let (mut doc, _, _, _) = document();
    let answer = doc.grid_measure(&json!([{ "text": "Plan 计划 계획", "weight": 600, "size": 13 }])).unwrap();
    let runs = answer[0].as_array().unwrap();
    let faces: Vec<u64> = runs.iter().map(|run| run["face"].as_u64().unwrap()).collect();
    assert_eq!(faces, [0, 1, 2]);
    assert!(runs.iter().all(|run| run["width"].as_f64().unwrap() > 0.0));
}
