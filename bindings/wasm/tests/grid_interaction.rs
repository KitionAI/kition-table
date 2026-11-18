//! What the page's interaction reads from the engine (case table.editor.grid/interaction-engine):
//! `gridRows` (what each linear row is), the row head bands' control slots and the header's
//! checkbox (hit records the overlay draws the controls in and hit-tests), and the overlay's
//! colors and icons in `gridAxes`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::PathBuf;

use kition_kit::KitDocument;
use kition_table_arrange::Axis;
use kition_table_display::{DisplayList, HitKind, Layer, Op, decode};
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

/// A table of 9 records (title and status), a plain view and a view grouped by status.
fn document() -> (TableDocument, i64, i64, i64) {
    fonts();
    let mut doc = TableDocument::create("Interaction.ktable", 0, 0).unwrap();
    let table = call(&mut doc, "documents.get", json!({}))["tables"][0].clone();
    let table_id = table["id"].as_i64().unwrap();
    let grid = table["views"][0]["id"].as_i64().unwrap();
    let primary = table["fields"].as_array().unwrap().iter().find(|field| field["is_primary"] == true).unwrap()["name"].as_str().unwrap().to_owned();
    for field in table["fields"].as_array().unwrap().iter().filter(|field| field["is_primary"] != true) {
        call(&mut doc, "fields.delete", json!({ "tableId": table_id, "fieldId": field["id"] }));
    }
    let made = call(
        &mut doc,
        "fields.create",
        json!({ "tableId": table_id, "title": "Status", "type": "single_select", "options": { "choices": ["To do", "Done"] } }),
    );
    let status = if made["field"].is_object() { made["field"].clone() } else { made };
    let records: Vec<Value> = (0..9)
        .map(|i| json!({ primary.clone(): format!("Record {i}"), status["name"].as_str().unwrap(): if i % 3 == 0 { "Done" } else { "To do" } }))
        .collect();
    call(&mut doc, "transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "at": null, "records": records }] }));
    let grouped = call(&mut doc, "views.create", json!({ "tableId": table_id, "title": "Grouped", "type": "grid" }));
    let grouped = grouped["id"].as_i64().or_else(|| grouped["view"]["id"].as_i64()).unwrap();
    call(&mut doc, "views.putGroups", json!({ "tableId": table_id, "viewId": grouped, "items": [{ "field_id": status["id"], "direction": "asc" }] }));
    (doc, table_id, grid, grouped)
}

/// Arranges and lays out `view`; answers the arrangement and the axes.
fn open(doc: &mut TableDocument, table_id: i64, view: i64, params: Value) -> (Axis, Value) {
    let bytes = doc.grid_open(&json!({ "tableId": table_id, "viewId": view })).unwrap();
    let mut params = params;
    params["tableId"] = json!(table_id);
    params["viewId"] = json!(view);
    let axes = doc.grid_axes(&bytes, &params).unwrap();
    (Axis::decode(&bytes).unwrap(), axes)
}

/// `gridRows` read back: (kind, value) per linear row, and the epoch.
fn rows(bytes: &[u8]) -> (u32, Vec<(u8, f64)>) {
    assert_eq!(&bytes[..4], b"KTRW");
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 1);
    let epoch = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let values_at = 16 + count.next_multiple_of(8);
    assert_eq!(bytes.len(), values_at + 8 * count, "kinds padded to 8 bytes, then one f64 per row");
    let rows = (0..count)
        .map(|row| (bytes[16 + row], f64::from_le_bytes(bytes[values_at + 8 * row..values_at + 8 * row + 8].try_into().unwrap())))
        .collect();
    (epoch, rows)
}

/// The batch's display lists and the ids of the icons it brings.
fn batch(bytes: &[u8]) -> (Vec<DisplayList>, Vec<u16>) {
    assert_eq!(&bytes[..4], b"KTDB");
    let count = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    let mut at = 12;
    let (mut lists, mut icons) = (Vec::new(), Vec::new());
    for _ in 0..count {
        let length = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let payload = &bytes[at + 8..at + 8 + length];
        match bytes[at] {
            1 => lists.push(decode(payload).unwrap()),
            2 => icons.push(u16::from_le_bytes([payload[0], payload[1]])),
            _ => {}
        }
        at += 8 + length.next_multiple_of(4);
    }
    (lists, icons)
}

fn band(doc: &mut TableDocument, epoch: u32, layer: u32) -> DisplayList {
    let (lists, _) = batch(&doc.grid_bands(epoch, &[layer, 0, 0], 2.0, 0, &[]).unwrap());
    lists.into_iter().next().unwrap()
}

#[test]
fn grid_rows_name_each_linear_row() {
    let (mut doc, table_id, grid, grouped) = document();
    let (axis, axes) = open(&mut doc, table_id, grid, json!({}));
    let (epoch, plain) = rows(&doc.grid_rows().unwrap());
    assert_eq!(u64::from(epoch), axes["epoch"].as_u64().unwrap());
    // The records in the arrangement's order, then the add row, whose anchor is the last record.
    let mut expected: Vec<(u8, f64)> = axis.ids.iter().map(|id| (0, *id as f64)).collect();
    expected.push((2, *axis.ids.last().unwrap() as f64));
    assert_eq!(plain, expected);

    // Grouped by status (3 "Done", 6 "To do"): per group a header (its index in the
    // arrangement's groups), its records, an add row anchored on the group's last record.
    let (axis, axes) = open(&mut doc, table_id, grouped, json!({}));
    let (_, grouped_rows) = rows(&doc.grid_rows().unwrap());
    // A group header row's value indexes the axes' groups, which carry the editor's group ids.
    let ids: Vec<&str> = axes["groups"].as_array().unwrap().iter().map(|group| group["id"].as_str().unwrap()).collect();
    assert_eq!(ids, axis.groups.iter().map(|group| group.id.as_str()).collect::<Vec<_>>());
    let mut expected = Vec::new();
    for (index, group) in axis.groups.iter().enumerate() {
        expected.push((1u8, index as f64));
        let ids = &axis.ids[group.at as usize..(group.at + group.count) as usize];
        expected.extend(ids.iter().map(|id| (0u8, *id as f64)));
        expected.push((2, *ids.last().unwrap() as f64));
    }
    let counts: Vec<u32> = axis.groups.iter().map(|group| group.count).collect();
    assert!(counts == [3, 6] || counts == [6, 3], "{counts:?}");
    assert_eq!(grouped_rows, expected);
}

#[test]
fn row_head_controls_have_a_slot_each_centred_on_the_row_at_every_height() {
    let (mut doc, table_id, grid, grouped) = document();
    for (name, height) in [("short", 32.0f32), ("medium", 40.0), ("tall", 56.0), ("extra_tall", 88.0)] {
        call(&mut doc, "views.patchConfig", json!({ "tableId": table_id, "viewId": grid, "row_height": name }));
        let (_, axes) = open(&mut doc, table_id, grid, json!({}));
        let epoch = axes["epoch"].as_u64().unwrap() as u32;
        let head = band(&mut doc, epoch, Layer::RowHead as u32);
        let first: Vec<_> = head.hits.iter().filter(|hit| hit.row == 0).collect();
        let kinds: Vec<HitKind> = first.iter().map(|hit| hit.kind).collect();
        assert_eq!(kinds, [HitKind::DragHandle, HitKind::Checkbox, HitKind::Expand], "{name}: no sort, no groups: the drag handle first");
        for (slot, hit) in first.iter().enumerate() {
            assert_eq!((hit.x, hit.w, hit.col), (slot as f32 * 24.0, 24.0, u16::MAX), "{name}: slot {slot}");
            // The slot is the row: a control centred in it is centred on the row.
            assert_eq!((hit.y, hit.h), (32.0, height), "{name}");
        }
        // Every record has its three; the add row has none of them.
        assert_eq!(head.hits.iter().filter(|hit| hit.kind == HitKind::Checkbox).count(), 9, "{name}");
        assert!(head.hits.iter().all(|hit| hit.row < 9 || hit.kind == HitKind::AppendRow), "{name}");
    }
    // Grouped: no drag handle; group headers and add rows have no row controls.
    let (_, axes) = open(&mut doc, table_id, grouped, json!({}));
    let epoch = axes["epoch"].as_u64().unwrap() as u32;
    let head = band(&mut doc, epoch, Layer::RowHead as u32);
    assert!(head.hits.iter().all(|hit| hit.kind != HitKind::DragHandle));
    let (_, linear) = rows(&doc.grid_rows().unwrap());
    let checkboxes: Vec<u32> = head.hits.iter().filter(|hit| hit.kind == HitKind::Checkbox).map(|hit| hit.row).collect();
    let records: Vec<u32> = linear.iter().enumerate().filter(|(_, (kind, _))| *kind == 0).map(|(row, _)| row as u32).collect();
    assert_eq!(checkboxes, records);
    assert_eq!(records.len(), 9);
    let expand = head.hits.iter().find(|hit| hit.kind == HitKind::Expand).unwrap();
    assert_eq!((expand.x, expand.w), (24.0, 24.0));
}

#[test]
fn the_header_checkbox_is_a_hit_over_the_box_it_draws() {
    let (mut doc, table_id, grid, grouped) = document();
    for view in [grid, grouped] {
        let (_, axes) = open(&mut doc, table_id, view, json!({}));
        let epoch = axes["epoch"].as_u64().unwrap() as u32;
        let corner = band(&mut doc, epoch, Layer::HeaderFrozen as u32);
        let hit = corner.hits.iter().find(|hit| hit.kind == HitKind::Checkbox).unwrap();
        assert_eq!((hit.row, hit.col), (u32::MAX, u16::MAX));
        let drawn = corner
            .ops
            .iter()
            .find_map(|op| match op {
                Op::RRectStroke { x, y, w, h, .. } if *w == 16.0 => Some((x + w / 2.0, y + h / 2.0)),
                _ => None,
            })
            .unwrap();
        assert_eq!((hit.x + hit.w / 2.0, hit.y + hit.h / 2.0), drawn, "the box sits in the middle of its hit");
        // Over the record checkboxes' slot.
        let head = band(&mut doc, epoch, Layer::RowHead as u32);
        let record = head.hits.iter().find(|hit| hit.kind == HitKind::Checkbox).unwrap();
        assert_eq!((hit.x, hit.w), (record.x, record.w));
    }
}

#[test]
fn the_overlay_draws_with_the_brand_and_gets_its_icons() {
    let (mut doc, table_id, grid, _) = document();
    for (theme, brand, on) in [("light", "#0c8375", "#ffffff"), ("dark", "#3fd1bf", "#000000")] {
        let (_, axes) = open(&mut doc, table_id, grid, json!({ "theme": theme }));
        let overlay = &axes["overlay"];
        // Checked controls and the outlines use the brand in both themes; the mark is drawn on it.
        assert_eq!(
            (overlay["check"].as_str(), overlay["border"].as_str(), overlay["checkMark"].as_str()),
            (Some(brand), Some(brand), Some(on)),
            "{theme}"
        );
        assert_eq!(overlay["controlSize"], json!(16.0));
        assert_eq!(overlay["activeWidth"], json!(2.0));
    }
    let (_, axes) = open(&mut doc, table_id, grid, json!({ "brand": "#5645d4" }));
    assert_eq!(axes["overlay"]["check"], json!("#5645d4"));
    assert_eq!(axes["overlay"]["checkMark"], json!("#ffffff"));
    // The controls' icons are known by id and come with the next batch (the page holds none yet).
    let icons: Vec<u16> =
        ["drag", "expand", "check"].iter().map(|name| u16::try_from(axes["overlay"]["icons"][name].as_u64().unwrap()).unwrap()).collect();
    let epoch = axes["epoch"].as_u64().unwrap() as u32;
    let (_, sent) = batch(&doc.grid_bands(epoch, &[Layer::Body as u32, 0, 0], 2.0, 0, &[]).unwrap());
    for icon in icons {
        assert!(sent.contains(&icon), "icon {icon} is sent");
    }
}
