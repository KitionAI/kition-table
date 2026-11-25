//! The optical sizing the grid measures and draws Inter with (`gridOpticalSizing`, cases
//! regressions/issue-webkit-optical-size and table.editor.grid/parity): `fixed-default` (WebKit's
//! Canvas) sets Inter at its default optical size (14) at every font size, `auto` (Chromium's
//! Canvas, the default) at the font size. `gridMeasure` answers the fonts crate's widths in the
//! setting, from 14 px up the two differ by what the fonts crate predicts and under it they agree;
//! `gridRaster` and `gridSpecimen` draw with the setting `gridMeasure` measures with; switching it
//! drops what the grid measured before; an unknown name is refused and changes nothing. The faces
//! come from the kition-fonts checkout beside this repository (or `KITION_FONTS_DIR`).

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::PathBuf;

use kition_kit::KitDocument;
use kition_table_display::{DisplayList, Layer, Op, decode};
use kition_table_fonts::{Family, OpticalSizing};
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn fonts() {
    let dir = std::env::var_os("KITION_FONTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../kition-fonts")));
    let found = kition_table_fonts::register_dir(&dir);
    assert_eq!(found, kition_table_fonts::CHECKOUT_FILES.len(), "the kition-fonts checkout at {}", dir.display());
}

/// Latin in Inter only (one run), wide enough that the settings' widths part by pixels at 20 px.
const TEXT: &str = "Quarterly planning review AVATAR WAVE office";

fn document() -> TableDocument {
    fonts();
    TableDocument::create("Sizing.ktable", 0, 0).unwrap()
}

/// `gridMeasure`'s width of `text` (one run of Inter).
fn measured(doc: &mut TableDocument, text: &str, weight: u16, size: f32) -> f32 {
    let answer = doc.grid_measure(&json!([{ "text": text, "weight": weight, "size": size }])).unwrap();
    let runs = answer[0].as_array().unwrap();
    assert_eq!(runs.len(), 1, "one run of Inter: {answer}");
    assert_eq!(runs[0]["text"], json!(text));
    runs[0]["width"].as_f64().unwrap() as f32
}

/// The fonts crate's width of `text` in `sizing`: its shaped advances at the size.
fn predicted(text: &str, weight: u16, size: f32, sizing: OpticalSizing) -> f32 {
    let runs = kition_table_fonts::shape(text, Family::Sans, weight, size, sizing);
    assert_eq!(runs.len(), 1, "one run of Inter");
    runs[0].glyphs.iter().map(|glyph| glyph.advance).sum::<f32>() * size / runs[0].upem
}

fn set(doc: &mut TableDocument, sizing: &str) -> bool {
    doc.grid_optical_sizing(sizing).unwrap_or_else(|error| panic!("gridOpticalSizing {sizing}: {error}"))
}

#[test]
fn fixed_default_measures_from_14_px_up_as_the_fonts_crate_predicts() {
    let mut doc = document();
    for (weight, size) in [(400, 16.0), (400, 20.0), (600, 16.0), (600, 20.0)] {
        let auto = measured(&mut doc, TEXT, weight, size);
        assert!(set(&mut doc, "fixed-default"), "auto -> fixed-default changes the setting");
        let fixed = measured(&mut doc, TEXT, weight, size);
        assert!(set(&mut doc, "auto"));
        let (fonts_auto, fonts_fixed) =
            (predicted(TEXT, weight, size, OpticalSizing::Auto), predicted(TEXT, weight, size, OpticalSizing::FixedDefault));
        assert!((auto - fonts_auto).abs() < 1e-3, "{weight} {size} px auto: gridMeasure {auto}, fonts crate {fonts_auto}");
        assert!((fixed - fonts_fixed).abs() < 1e-3, "{weight} {size} px fixed-default: gridMeasure {fixed}, fonts crate {fonts_fixed}");
        // Above the axis's default the settings part: fixed-default keeps the wider optical size 14.
        assert!(fixed - auto > 0.5, "{weight} {size} px: fixed-default {fixed} is wider than auto {auto} by more than 0.5 px");
        assert!(((fixed - auto) - (fonts_fixed - fonts_auto)).abs() < 1e-3, "{weight} {size} px: the difference is the fonts crate's");
    }
}

#[test]
fn under_the_optical_size_floor_the_settings_agree() {
    let mut doc = document();
    for (weight, size) in [(400, 12.0), (600, 12.0), (400, 13.0)] {
        let auto = measured(&mut doc, TEXT, weight, size);
        set(&mut doc, "fixed-default");
        let fixed = measured(&mut doc, TEXT, weight, size);
        set(&mut doc, "auto");
        assert_eq!(auto, fixed, "{weight} {size} px");
        assert_eq!(auto, predicted(TEXT, weight, size, OpticalSizing::Auto), "{weight} {size} px");
    }
}

#[test]
fn switching_drops_what_was_measured_in_the_other_setting() {
    let mut doc = document();
    let auto = measured(&mut doc, TEXT, 400, 20.0);
    // Measured (and kept) again in auto, so a stale answer would be at hand.
    assert_eq!(measured(&mut doc, TEXT, 400, 20.0), auto);
    assert!(set(&mut doc, "fixed-default"));
    assert!(!set(&mut doc, "fixed-default"), "the same setting again changes nothing");
    let fixed = measured(&mut doc, TEXT, 400, 20.0);
    assert_ne!(fixed, auto, "fixed-default does not answer the width auto measured");
    assert!(set(&mut doc, "auto"));
    assert_eq!(measured(&mut doc, TEXT, 400, 20.0), auto, "back in auto, the auto width again");
}

#[test]
fn an_unknown_setting_is_refused_and_changes_nothing() {
    let mut doc = document();
    set(&mut doc, "fixed-default");
    let fixed = measured(&mut doc, TEXT, 400, 20.0);
    for name in ["none", "Auto", "fixed", ""] {
        let error = doc.grid_optical_sizing(name).unwrap_err();
        assert!(error.contains("optical sizing is auto or fixed-default"), "{name:?}: {error}");
        assert!(error.contains(&format!("{name:?}")), "the error names what it was given: {error}");
        assert_eq!(measured(&mut doc, TEXT, 400, 20.0), fixed, "after {name:?} the grid still measures in fixed-default");
    }
}

/// `gridSpecimen`'s list and raster: (the list, width, height, straight RGBA8).
fn specimen(doc: &mut TableDocument, asks: Value, dpr: f64) -> (DisplayList, usize, usize, Vec<u8>) {
    let bytes = doc.grid_specimen(&asks, dpr).unwrap();
    let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
    let list = decode(&bytes[4..4 + length]).unwrap();
    let at = 4 + length.next_multiple_of(4);
    let width = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let height = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
    let rgba = bytes[at + 8..].to_vec();
    assert_eq!(rgba.len(), width * height * 4);
    (list, width, height, rgba)
}

/// The leftmost and rightmost device pixel columns of ink (a pixel well apart from the paper).
fn ink_columns(rgba: &[u8], width: usize, paper: [u8; 4]) -> (usize, usize) {
    let mut columns = (usize::MAX, 0);
    for (index, px) in rgba.as_chunks::<4>().0.iter().enumerate() {
        if (0..3).any(|c| px[c].abs_diff(paper[c]) > 96) {
            columns = (columns.0.min(index % width), columns.1.max(index % width));
        }
    }
    assert!(columns.0 != usize::MAX, "the specimen has ink");
    columns
}

#[test]
fn the_raster_draws_with_the_setting_the_grid_measures_with() {
    let mut doc = document();
    let dpr = 2.0;
    let mut inks = Vec::new();
    for sizing in ["auto", "fixed-default"] {
        set(&mut doc, sizing);
        let width = measured(&mut doc, TEXT, 400, 20.0);
        let (list, w, h, rgba) = specimen(&mut doc, json!([{ "text": TEXT, "weight": 400, "size": 20 }]), dpr);
        assert_eq!(list.header.key.layer, Layer::View);
        assert_eq!((w, h), ((list.header.size.0 as f64 * dpr).round() as usize, (list.header.size.1 as f64 * dpr).round() as usize));
        let texts: Vec<(f32, f32)> = list
            .ops
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, width, .. } => Some((*x, *width)),
                _ => None,
            })
            .collect();
        assert_eq!(texts.len(), 1, "{sizing}: one text op");
        // The list's run is as wide as gridMeasure measures it.
        assert_eq!(texts[0].1, width, "{sizing}: the text op's width is gridMeasure's");
        let (left, right) = ink_columns(&rgba, w, [255, 255, 255, 255]);
        let ink = (right + 1 - left) as f32 / dpr as f32;
        // The drawn advance: the ink spans the measured run, short of its first and last glyphs'
        // side bearings only (under 0.15 em together).
        assert!(ink <= width + 0.5 && ink >= width - 0.15 * 20.0, "{sizing}: ink {ink} px, measured {width} px");
        assert!((left as f32 / dpr as f32 - texts[0].0).abs() <= 0.1 * 20.0, "{sizing}: the ink starts at the run's x");
        inks.push((ink, width));
    }
    let ((ink_auto, auto), (ink_fixed, fixed)) = (inks[0], inks[1]);
    // The settings' widths part by pixels, and the drawn text parts by as much: the raster draws
    // in the setting gridMeasure measures in, not in the other one.
    assert!(fixed - auto > 1.0, "measured: fixed-default {fixed}, auto {auto}");
    assert!(((ink_fixed - ink_auto) - (fixed - auto)).abs() <= 0.75, "drawn {ink_auto} -> {ink_fixed} px, measured {auto} -> {fixed} px");
}

/// A table of one record whose primary field holds `TEXT`; its id and its grid view's.
fn table(doc: &mut TableDocument) -> (i64, i64) {
    let call = |doc: &mut TableDocument, method: &str, params: Value| doc.call(method, &params).unwrap()["result"].clone();
    let table = call(doc, "file.get", json!({}))["tables"][0].clone();
    let table_id = table["id"].as_i64().unwrap();
    let view = table["views"][0]["id"].as_i64().unwrap();
    let primary = table["fields"].as_array().unwrap().iter().find(|field| field["is_primary"] == true).unwrap()["name"].as_str().unwrap().to_owned();
    call(doc, "edit.transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "at": null, "records": [{ primary: TEXT }] }] }));
    (table_id, view)
}

#[test]
fn the_grids_own_text_rasterizes_alike_in_both_settings() {
    // The grid sets 10 to 13 px, under the axis's floor: its bands are the same pixels either way.
    let mut doc = document();
    let (table_id, view) = table(&mut doc);
    let axis = doc.grid_open(&json!({ "tableId": table_id, "viewId": view })).unwrap();
    let axes = doc.grid_axes(&axis, &json!({ "tableId": table_id, "viewId": view })).unwrap();
    let epoch = u32::try_from(axes["epoch"].as_u64().unwrap()).unwrap();
    let raster = |doc: &mut TableDocument| -> Vec<Vec<u8>> {
        [Layer::Header as u32, Layer::Body as u32, Layer::RowHead as u32]
            .iter()
            .map(|layer| doc.grid_raster(epoch, [*layer, 0, 0], 2.0).unwrap_or_else(|error| panic!("gridRaster layer {layer}: {error}")))
            .collect()
    };
    let auto = raster(&mut doc);
    set(&mut doc, "fixed-default");
    assert_eq!(raster(&mut doc), auto);
}
