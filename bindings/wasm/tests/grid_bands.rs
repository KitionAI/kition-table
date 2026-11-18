//! The editor's grid in bands (cases table.editor.grid/bands and table.editor.grid/band-raster):
//! the axes `gridAxes` lays out from the arrangement `gridOpen` answers, and the bands
//! `gridBands` answers for them, each a display list of its own area, clipped to it, with the
//! resources it uses (icon and emoji tiles, the pictures' urls and sizes) in one resource table,
//! each once per page session; `gridRaster` draws a band at the device size.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::PathBuf;

use kition_kit::KitDocument;
use kition_table_display::{DisplayList, Entry, IconStyle, Layer, Op, decode, decode_resources};
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
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 2);
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

/// The entries of a batch's resource table (at most one, after the bands).
fn entries(batch: &[(u8, Vec<u8>)]) -> Vec<Entry> {
    let tables: Vec<&Vec<u8>> = batch.iter().filter(|(kind, _)| *kind == 2).map(|(_, bytes)| bytes).collect();
    assert!(tables.len() <= 1, "one resource table");
    assert!(batch.iter().skip_while(|(kind, _)| *kind == 1).all(|(kind, _)| *kind == 2), "the table after the bands");
    tables.first().map(|bytes| decode_resources(bytes).unwrap().entries).unwrap_or_default()
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

/// The editor shows a view's settings before it saves them (and a read-only person's, which it
/// never saves): `gridAxes` lays out with the editor's `config` (case table.editor.grid/editor).
#[test]
fn the_axes_follow_the_editors_config_before_it_is_saved() {
    let (mut doc, table_id, grid, _) = document();
    let axis = doc.grid_open(&json!({ "tableId": table_id, "viewId": grid })).unwrap();
    let config = json!({ "hidden_field_names": ["notes", "amount"], "row_height": "tall", "frozen_column_count": 2 });
    let laid = doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": grid, "config": config })).unwrap();
    assert_eq!(laid["rowRuns"], json!([[56.0, 70], [32.0, 1]]), "tall rows");
    let columns = laid["columns"].as_array().unwrap();
    assert_eq!(columns.len(), 5, "four fields and the add-field column");
    assert_eq!(columns.iter().filter(|column| column["frozen"] == true).count(), 2);
    // The view itself still says otherwise.
    let stored = doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": grid })).unwrap();
    assert_eq!(stored["rowRuns"], json!([[40.0, 70], [32.0, 1]]));
    assert_eq!(stored["columns"].as_array().unwrap().len(), 7);
    // Groups the editor has not saved yet: headers, and no drag handle in the row head.
    let groups = json!({ "group_items": [{ "id": "g", "field_name": "status", "direction": "asc" }] });
    let axis = doc.grid_open(&json!({ "tableId": table_id, "viewId": grid, "config": groups.clone() })).unwrap();
    let grouped = doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": grid, "config": groups })).unwrap();
    assert_eq!(grouped["rowHeadWidth"], json!(48.0));
    assert_eq!(grouped["groups"].as_array().unwrap().len(), 2);
    assert!(
        doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": grid, "config": "tall" })).is_err(),
        "a config that is not an object is refused"
    );
}

/// The editor's bands read dates in the page's time zone (`gridAxes`' `zone`), as the old grid
/// formatted them with the browser's; without one, in UTC as the picture does (case
/// table.editor.grid/editor).
#[test]
fn the_bands_read_dates_in_the_page_s_zone() {
    fonts();
    let mut doc = TableDocument::create("Dates.ktable", 0, 0).unwrap();
    let table = call(&mut doc, "documents.get", json!({}))["tables"][0].clone();
    let (table_id, view) = (table["id"].as_i64().unwrap(), table["views"][0]["id"].as_i64().unwrap());
    let due = call(&mut doc, "fields.create", json!({ "tableId": table_id, "title": "Due", "type": "datetime" }));
    let due = if due["field"].is_object() { due["field"].clone() } else { due };
    let title = table["fields"].as_array().unwrap().iter().find(|field| field["is_primary"] == true).unwrap()["name"].as_str().unwrap().to_owned();
    call(
        &mut doc,
        "records.create",
        json!({ "tableId": table_id, "values": { title: "Launch", due["name"].as_str().unwrap(): "2026-07-23T04:14:47Z" } }),
    );
    let texts = |doc: &mut TableDocument, zone: Value| -> Vec<String> {
        let axis = doc.grid_open(&json!({ "tableId": table_id, "viewId": view })).unwrap();
        let laid = doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": view, "zone": zone })).unwrap();
        let epoch = u32::try_from(laid["epoch"].as_u64().unwrap()).unwrap();
        let batch = items(&doc.grid_bands(epoch, &[0, 0, 0, 1, 0, 0], 1.0, 1, false).unwrap());
        lists(&batch)
            .iter()
            .flat_map(|list| {
                list.ops
                    .iter()
                    .filter_map(|op| if let Op::Text { text, .. } = op { Some(list.text(*text).to_owned()) } else { None })
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    let shanghai = texts(&mut doc, json!([[-8.64e15, 480]]));
    assert!(shanghai.iter().any(|text| text == "2026/07/23 12:14"), "{shanghai:?}");
    let utc = texts(&mut doc, Value::Null);
    assert!(utc.iter().any(|text| text == "2026/07/23 04:14"), "{utc:?}");
    let axis = doc.grid_open(&json!({ "tableId": table_id, "viewId": view })).unwrap();
    assert!(
        doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": view, "zone": [[0, "east"]] })).is_err(),
        "a zone of other things is refused"
    );
}

#[test]
fn every_band_draws_its_own_area_clipped_with_the_resources_it_uses() {
    let (mut doc, table_id, grid, _) = document();
    let answer = axes(&mut doc, table_id, grid, "light");
    let epoch = answer["epoch"].as_u64().unwrap() as u32;
    let keys = [0, 0, 0, 1, 0, 0, 4, 0, 0, 2, 0, 0, 3, 0, 0, 0, 0, 1, 0, 0, 9];
    let batch = items(&doc.grid_bands(epoch, &keys, 2.0, 7, false).unwrap());
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
    let sent = entries(&batch);
    let tiles: Vec<&Entry> = sent.iter().filter(|entry| matches!(entry, Entry::Emoji { .. })).collect();
    assert_eq!(tiles.len(), 1, "one tile for the one emoji at one size");
    let Entry::Emoji { atlas, size, dpr, tile } = tiles[0] else { unreachable!() };
    assert_eq!((*atlas, *size, *dpr), (emoji[0], 13.0, 2.0));
    assert_eq!(tile.rgba.len(), (tile.width * tile.height * 4) as usize);
    // Every icon a band names is in the batch as its tile, in the band's style and color.
    let used: Vec<(u16, IconStyle)> = bands
        .iter()
        .flat_map(|list| {
            list.ops.iter().filter_map(|op| match op {
                Op::Icon { icon, size, stroke, paint, solid, .. } => {
                    Some((*icon, IconStyle { size: *size, stroke: *stroke, solid: *solid, paint: list.color(*paint) }))
                }
                _ => None,
            })
        })
        .collect();
    assert!(!used.is_empty());
    for (id, style) in &used {
        assert!(
            sent.iter().any(|entry| matches!(entry, Entry::Icon { id: sent, style: was, dpr: 2.0, .. } if sent == id && was == style)),
            "icon {id} {style:?} is sent"
        );
    }
    for entry in &sent {
        if let Entry::Icon { id, style, tile, .. } = entry {
            assert_eq!(tile.rgba.len(), (tile.width * tile.height * 4) as usize);
            assert!(tile.rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0), "icon {id} {style:?} has its pixels");
        }
    }

    // Asked again in the same session: no icon and no tile again; a new session gets them again.
    let again = items(&doc.grid_bands(epoch, &keys[3..6], 2.0, 7, false).unwrap());
    assert!(again.iter().all(|(kind, _)| *kind == 1), "{:?}", again.iter().map(|(kind, _)| kind).collect::<Vec<_>>());
    let other = items(&doc.grid_bands(epoch, &keys[3..6], 2.0, 8, false).unwrap());
    assert!(entries(&other).iter().any(|entry| matches!(entry, Entry::Icon { .. })), "a new session holds nothing");
    // At another ratio the same session gets the tiles at that ratio.
    let denser = items(&doc.grid_bands(epoch, &keys[3..6], 1.5, 8, false).unwrap());
    let denser = entries(&denser);
    assert!(!denser.is_empty() && denser.iter().all(|entry| matches!(entry, Entry::Icon { dpr: 1.5, .. } | Entry::Emoji { dpr: 1.5, .. })));
}

/// A PNG of `width` x `height` pixels of one color.
fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&rgba.repeat((width * height) as usize)).unwrap();
    writer.finish().unwrap();
    bytes
}

#[test]
fn pictures_have_their_proportions_and_the_page_gets_their_urls_and_sizes() {
    use base64::Engine as _;
    let (mut doc, table_id, grid, _) = document();
    let files = call(&mut doc, "fields.create", json!({ "tableId": table_id, "title": "Files", "type": "attachment" }));
    let files = if files["field"].is_object() { files["field"].clone() } else { files };
    let data = base64::engine::general_purpose::STANDARD.encode(png(40, 20, [200, 30, 30, 255]));
    let put = call(&mut doc, "attachments.put", json!({ "tableId": table_id, "name": "wide.png", "mimeType": "image/png", "data": data }));
    let attachment = if put["attachment"].is_object() { put["attachment"].clone() } else { put };
    let first = call(&mut doc, "records.list", json!({ "tableId": table_id, "limit": 1 }))["items"][0]["id"].clone();
    call(
        &mut doc,
        "records.update",
        json!({ "tableId": table_id, "recordId": first, "values": { files["name"].as_str().unwrap(): [attachment.clone()] } }),
    );
    let answer = axes(&mut doc, table_id, grid, "light");
    let epoch = answer["epoch"].as_u64().unwrap() as u32;
    let strips = answer["columns"].as_array().unwrap().iter().filter(|column| column["frozen"] == false).count();
    let keys: Vec<u32> = (0..u32::try_from(strips).unwrap()).flat_map(|strip| [0, strip.min(1), 0]).take(6).collect();
    let batch = items(&doc.grid_bands(epoch, &keys, 2.0, 1, false).unwrap());
    let image = lists(&batch)
        .iter()
        .flat_map(|list| list.ops.iter().filter_map(|op| if let Op::Image { key, w, h, .. } = op { Some((*key, *w, *h)) } else { None }))
        .next()
        .expect("the picture's thumbnail");
    // A 2:1 picture is a 2:1 thumbnail, as tall as the row allows.
    assert_eq!(image.1, image.2 * 2.0, "{image:?}");
    let url = attachment["url"].as_str().map(str::to_owned).unwrap_or_else(|| format!("kition-blob:{}", attachment["sha256"].as_str().unwrap()));
    assert!(
        entries(&batch).contains(&Entry::Image { key: image.0, width: 40, height: 20, source: url }),
        "the page gets the picture's url and size: {:?}",
        entries(&batch).iter().filter(|entry| matches!(entry, Entry::Image { .. })).collect::<Vec<_>>()
    );
    // The engine's raster of that band shows the picture, decoded as the headless picture does.
    let band = lists(&batch).into_iter().find(|list| list.ops.iter().any(|op| matches!(op, Op::Image { .. }))).unwrap();
    let key = band.header.key;
    let raster = doc.grid_raster(epoch, [key.layer as u32, u32::from(key.col_band), key.row_band], 1.0).unwrap();
    let width = u32::from_le_bytes(raster[0..4].try_into().unwrap()) as usize;
    let Some(Op::Image { x, y, w, h, .. }) = band.ops.iter().find(|op| matches!(op, Op::Image { .. })) else { unreachable!() };
    let (cx, cy) = ((x + w / 2.0 - band.header.origin.0) as usize, (y + h / 2.0 - band.header.origin.1) as usize);
    assert_eq!(&raster[8 + 4 * (cy * width + cx)..][..4], &[200, 30, 30, 255]);
}

#[test]
fn a_draft_is_the_band_measured_from_advances_and_counted_apart() {
    let (mut doc, table_id, grid, _) = document();
    let answer = axes(&mut doc, table_id, grid, "light");
    let epoch = answer["epoch"].as_u64().unwrap() as u32;
    let keys = [0, 0, 0, 1, 0, 0, 4, 0, 0];
    let exact = lists(&items(&doc.grid_bands(epoch, &keys, 2.0, 1, false).unwrap()));
    let before = doc.grid_stats()["bands"].clone();
    let draft = lists(&items(&doc.grid_bands(epoch, &keys, 2.0, 1, true).unwrap()));
    assert_eq!(exact.len(), draft.len());
    for (exact, draft) in exact.iter().zip(&draft) {
        assert_eq!((exact.header.flags, draft.header.flags), (0, kition_table_display::DRAFT));
        assert_eq!((draft.header.key, draft.header.origin, draft.header.size), (exact.header.key, exact.header.origin, exact.header.size));
        let kinds = |list: &DisplayList| list.hits.iter().map(|hit| (hit.kind, hit.col, hit.row)).collect::<Vec<_>>();
        assert_eq!(kinds(draft), kinds(exact), "{:?}: the same parts", draft.header.key);
        assert_eq!(
            draft.ops.iter().filter(|op| matches!(op, Op::Text { .. })).count() > 0,
            exact.ops.iter().filter(|op| matches!(op, Op::Text { .. })).count() > 0,
            "{:?}: text drawn",
            draft.header.key
        );
        assert!(draft.ops.iter().all(|op| !matches!(op, Op::Emoji { .. })), "{:?}: no emoji in a draft", draft.header.key);
    }
    // Drafts are timed apart from the exact bands.
    let after = doc.grid_stats()["bands"].clone();
    assert_eq!(after["count"], before["count"]);
    assert_eq!(after["drafts"].as_u64().unwrap(), before["drafts"].as_u64().unwrap() + 3);
}

#[test]
fn a_band_of_another_arrangement_is_refused() {
    let (mut doc, table_id, grid, _) = document();
    let answer = axes(&mut doc, table_id, grid, "light");
    let epoch = answer["epoch"].as_u64().unwrap() as u32;
    assert!(doc.grid_bands(epoch + 1, &[0, 0, 0], 1.0, 0, false).unwrap_err().contains("not"));
    assert!(doc.grid_bands(epoch, &[7, 0, 0], 1.0, 0, false).unwrap_err().contains("band key"));
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
