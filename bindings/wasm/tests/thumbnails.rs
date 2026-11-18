//! The picture draws the pictures a table attaches (grid plan D5): a PNG, JPEG or WebP held in
//! the file is decoded and drawn as the thumbnail the editor shows, as wide as its proportions
//! and centre-cropped; a file that is not a picture is a chip with its name.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;

use base64::Engine as _;
use kition_kit::KitDocument;
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
    doc.execute(&format!("table.{method}"), params).unwrap_or_else(|error| panic!("{method}: {error}"))["result"].clone()
}

/// A `width` x `height` PNG of one color.
fn png(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    let pixels: Vec<u8> = (0..width * height).flat_map(|_| color).collect();
    writer.write_image_data(&pixels).unwrap();
    writer.finish().unwrap();
    bytes
}

/// How many of the picture's pixels are `color` (within 8 per channel).
fn count(doc: &TableDocument, color: [u8; 3]) -> usize {
    let (width, height) = (doc.width(), doc.height());
    let rgba = doc.render_area(0, 0, width as i32, height as i32, width, height).unwrap();
    rgba.as_chunks::<4>().0.iter().filter(|px| px[3] == 255 && px.iter().zip(color).all(|(a, b)| a.abs_diff(b) <= 8)).count()
}

#[test]
fn the_picture_draws_attached_pictures_as_thumbnails() {
    fonts();
    let mut doc = TableDocument::create("Files.ktable", 0, 0).unwrap();
    call(&mut doc, "tables.delete", json!({ "tableId": 1 }));
    let fields = [
        json!({ "name": "title", "title": "Title", "type": "text", "primary": true }),
        json!({ "name": "files", "title": "Files", "type": "attachment" }),
    ];
    let table = call(&mut doc, "tables.create", json!({ "title": "Files", "fields": fields }))["id"].as_i64().unwrap();
    let put = |doc: &mut TableDocument, bytes: &[u8], name: &str, mime: &str| {
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        call(doc, "attachments.put", json!({ "tableId": table, "name": name, "mimeType": mime, "data": data }))
    };
    // A wide red picture (2:1), and a PDF.
    let red = put(&mut doc, &png(200, 100, [230, 20, 20]), "wide.png", "image/png");
    let brief = put(&mut doc, b"%PDF-1.4 brief", "brief.pdf", "application/pdf");
    let records = json!([{ "title": "Plan", "files": [red, brief] }]);
    call(&mut doc, "transact", json!({ "ops": [{ "op": "records.create", "tableId": table, "records": records }] }));
    call(&mut doc, "preview.show", json!({ "tableId": table }));

    // A 40 px row: a thumbnail 34 px tall and 68 px wide, its corners rounded and its edge under
    // a hairline border, so a little less than 68 x 34 pixels show the picture's red.
    let shown = count(&doc, [230, 20, 20]);
    assert!((60 * 30..=68 * 34).contains(&shown), "a 68 x 34 thumbnail of the picture: {shown} red pixels");

    // A picture the file does not hold is a placeholder: no red.
    let url = json!({ "url": "https://example.com/wide.png", "name": "wide.png", "mimeType": "image/png" });
    let records = call(&mut doc, "records.list", json!({ "tableId": table }));
    let id = records["items"][0]["id"].clone();
    call(&mut doc, "records.update", json!({ "tableId": table, "recordId": id, "values": { "files": [url] } }));
    assert_eq!(count(&doc, [230, 20, 20]), 0);
}
