//! The agent's preview (`table.file.render`, KHP A39) of `conformance/basic.ktable`: not uniform,
//! framed to the table's fields and records, of a diagnostic's target (`target`), at a scale up to 2 and a
//! size budget, and equal to `conformance/expected/` within `expected/tolerance.json`. The picture's
//! faces come from the kition-fonts checkout beside this repository (or `KITION_FONTS_DIR`).
//! `KITION_TABLE_WRITE_EXPECTED=1 cargo test -p kition-table-kit --test preview` rewrites the expected previews.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn basic() -> TableDocument {
    let fonts = std::env::var_os("KITION_FONTS_DIR").map(PathBuf::from).unwrap_or_else(|| repo().join("../kition-fonts"));
    assert_eq!(
        kition_table_fonts::register_dir(&fonts),
        kition_table_fonts::CHECKOUT_FILES.len(),
        "the kition-fonts checkout at {}",
        fonts.display()
    );
    TableDocument::open("basic.ktable", &std::fs::read(repo().join("conformance/basic.ktable")).unwrap()).unwrap()
}

struct Image {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

fn decode(png_bytes: &[u8]) -> Image {
    let mut reader = png::Decoder::new(png_bytes).read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut rgba).unwrap();
    assert_eq!((info.color_type, info.bit_depth), (png::ColorType::Rgba, png::BitDepth::Eight));
    rgba.truncate(info.buffer_size());
    Image { rgba, width: info.width, height: info.height }
}

/// The preview's answer and its pixels.
fn render(doc: &mut TableDocument, params: Value) -> (Value, Image, Vec<u8>) {
    let answer = doc.execute("table.file.render", params).unwrap();
    assert_eq!(answer["mimeType"], "image/png");
    let bytes = base64::engine::general_purpose::STANDARD.decode(answer["png"].as_str().unwrap()).unwrap();
    let image = decode(&bytes);
    assert_eq!((answer["width"].as_u64().unwrap(), answer["height"].as_u64().unwrap()), (u64::from(image.width), u64::from(image.height)));
    (answer, image, bytes)
}

fn colours(image: &Image) -> BTreeSet<[u8; 4]> {
    image.rgba.as_chunks::<4>().0.iter().copied().collect()
}

/// The basic fixture's ids: its table, its second record, and its `status` field.
fn title(doc: &mut TableDocument) -> String {
    doc.execute("table.tables.list", json!({})).unwrap()["result"]["items"][0]["title"].as_str().unwrap().to_owned()
}

fn ids(doc: &mut TableDocument) -> (i64, i64, i64) {
    let document = doc.execute("table.documents.get", json!({})).unwrap()["result"].clone();
    let table = &document["tables"][0];
    let status = table["fields"].as_array().unwrap().iter().find(|field| field["name"] == "status").unwrap()["id"].as_i64().unwrap();
    let records = doc.execute("table.records.list", json!({ "tableId": table["id"] })).unwrap()["result"]["items"].clone();
    (table["id"].as_i64().unwrap(), records[1]["id"].as_i64().unwrap(), status)
}

#[test]
fn the_preview_is_real_and_framed_to_the_table() {
    let mut doc = basic();
    let (_, image, _) = render(&mut doc, json!({}));
    // Three fields and two records: the row heads (72), three columns of 200 and the add column
    // (100); the tab and view bars (44, 52), the header row (32), two rows of 40, the add row (32)
    // and the footer (36).
    assert_eq!((image.width, image.height), (772, 276));
    assert_eq!((doc.width(), doc.height()), (772, 276));
    assert!(colours(&image).len() > 4, "a uniform preview: {:?}", colours(&image));
    doc.execute("table.records.create", json!({ "tableId": 1, "values": { "title": "Ship the plan" } })).unwrap();
    let (_, grown, _) = render(&mut doc, json!({}));
    assert_eq!((grown.width, grown.height), (772, 316));
}

#[test]
fn scale_and_size_budget() {
    let mut doc = basic();
    let (answer, image, _) = render(&mut doc, json!({ "scale": 2 }));
    assert_eq!((image.width, image.height, answer["scale"].as_f64().unwrap()), (1544, 552, 2.0));
    let (_, small, _) = render(&mut doc, json!({ "scale": 2, "maxSize": 120 }));
    assert_eq!((small.width, small.height), (120, 43));
    for bad in [json!({ "scale": 3 }), json!({ "scale": 0 }), json!({ "maxSize": 5000 }), json!({ "target": "tables/1" })] {
        assert!(doc.execute("table.file.render", bad.clone()).is_err(), "{bad}");
    }
}

#[test]
fn a_target_moves_the_preview_to_it() {
    let mut doc = basic();
    let (table, record, status) = ids(&mut doc);
    let (_, whole, _) = render(&mut doc, json!({}));
    let (_, cell, _) = render(&mut doc, json!({ "target": { "path": format!("tables/{table}/records/{record}/fields/{status}") } }));
    assert_ne!(whole.rgba, cell.rgba);
    // The cell is outlined in the primary colour: column 2 (after the row heads and one column
    // of 200), row 2 (below the bars, the header row and the first record): its left edge.
    let at = |image: &Image, x: u32, y: u32| image.rgba[((y * image.width + x) * 4) as usize..][..4].to_vec();
    assert_eq!(at(&cell, 273, 188), vec![12, 131, 117, 255]);
    let sheet = title(&mut doc);
    let (_, by_title, _) = render(&mut doc, json!({ "target": { "sheet": sheet } }));
    assert_eq!(by_title.rgba, whole.rgba);
    for missing in ["tables/99", format!("tables/{table}/records/99/fields/{status}").as_str(), "dashboards/none"] {
        let error = doc.execute("table.file.render", json!({ "target": { "path": missing } })).unwrap_err();
        assert!(error.contains("not found"), "{missing}: {error}");
    }
    // A diagnostic's target previews as it is: the fixture's own check targets.
    let fixture = std::fs::read(repo().join("conformance/check-field-type-mismatch.ktable")).unwrap();
    let mut broken = TableDocument::open("check-field-type-mismatch.ktable", &fixture).unwrap();
    let target = serde_json::to_value(&broken.check()[0].target).unwrap();
    render(&mut broken, json!({ "target": target }));
}

fn expected(name: &str) -> PathBuf {
    repo().join("conformance/expected").join(name)
}

#[test]
fn the_previews_equal_the_expected_previews() {
    let tolerance: Value = serde_json::from_str(&std::fs::read_to_string(expected("tolerance.json")).unwrap()).unwrap();
    let (max_delta, max_pixels) = (tolerance["maxChannelDelta"].as_u64().unwrap(), tolerance["maxDifferentPixels"].as_u64().unwrap());
    let mut doc = basic();
    let (table, record, status) = ids(&mut doc);
    let cases = [
        ("basic.png", json!({})),
        ("basic-scale-2.png", json!({ "scale": 2 })),
        ("basic-target.png", json!({ "target": { "path": format!("tables/{table}/records/{record}/fields/{status}") } })),
    ];
    let write = std::env::var_os("KITION_TABLE_WRITE_EXPECTED").is_some();
    for (name, params) in cases {
        let (_, image, bytes) = render(&mut doc, params);
        if write {
            std::fs::write(expected(name), &bytes).unwrap();
            continue;
        }
        let expected = decode(&std::fs::read(expected(name)).unwrap_or_else(|error| panic!("{name}: {error}")));
        assert_eq!((image.width, image.height), (expected.width, expected.height), "{name}");
        let different = image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.rgba.as_chunks::<4>().0)
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| u64::from(x.abs_diff(*y)) > max_delta))
            .count() as u64;
        assert!(different <= max_pixels, "{name}: {different} pixels differ from expected/{name}");
    }
}
