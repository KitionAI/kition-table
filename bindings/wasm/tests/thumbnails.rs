//! The picture draws the pictures a table attaches (grid plan D5): a PNG, JPEG, WebP, GIF, SVG or
//! HEIC held in the file is decoded and drawn as the thumbnail the editor shows, as wide as its
//! proportions and centre-cropped; a file that is not a picture is a chip with its name. A
//! gallery's cards show their cover pictures the same way.

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
    count_near(doc, color, 8)
}

/// How many of the picture's pixels are `color` (within `tolerance` per channel).
fn count_near(doc: &TableDocument, color: [u8; 3], tolerance: u8) -> usize {
    let (width, height) = (doc.width(), doc.height());
    let rgba = doc.render_area(0, 0, width as i32, height as i32, width, height).unwrap();
    rgba.as_chunks::<4>().0.iter().filter(|px| px[3] == 255 && px.iter().zip(color).all(|(a, b)| a.abs_diff(b) <= tolerance)).count()
}

const GREEN: [u8; 3] = [20, 200, 40];
const MAGENTA: [u8; 3] = [200, 20, 200];
const BLUE: [u8; 3] = [20, 40, 210];
/// The HEIC fixture's halves (lossy: compared within 24).
const HEIC_RED: [u8; 3] = [220, 30, 30];

/// A 20 x 10 GIF whose first frame is green and whose second is magenta.
fn gif() -> Vec<u8> {
    let palette = [GREEN, MAGENTA].concat();
    let mut bytes = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut bytes, 20, 10, &palette).unwrap();
        for index in [0u8, 1] {
            let frame = gif::Frame { width: 20, height: 10, delay: 10, buffer: std::borrow::Cow::Owned(vec![index; 200]), ..gif::Frame::default() };
            encoder.write_frame(&frame).unwrap();
        }
    }
    bytes
}

/// A 40 x 20 SVG, all blue.
const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'><rect width='40' height='20' fill='rgb(20,40,210)'/></svg>";

/// A 32 x 16 HEIC, its left half red and its right half blue (made with sips).
const HEIC: &[u8] = include_bytes!("../../../crates/view/display/testdata/split.heic");

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

/// A GIF draws its first frame, an SVG is drawn by resvg and a HEIC is decoded, in the grid and as
/// a gallery card's cover (cover-cropped into the card's art box).
#[test]
fn gif_svg_and_heic_pictures_draw_in_the_grid_and_as_gallery_covers() {
    fonts();
    let mut doc = TableDocument::create("Pictures.ktable", 0, 0).unwrap();
    call(&mut doc, "tables.delete", json!({ "tableId": 1 }));
    let fields = [
        json!({ "name": "title", "title": "Title", "type": "text", "primary": true }),
        json!({ "name": "files", "title": "Files", "type": "attachment" }),
    ];
    let table = call(&mut doc, "tables.create", json!({ "title": "Pictures", "fields": fields }))["id"].as_i64().unwrap();
    let put = |doc: &mut TableDocument, bytes: &[u8], name: &str, mime: &str| {
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        call(doc, "attachments.put", json!({ "tableId": table, "name": name, "mimeType": mime, "data": data }))
    };
    let moving = put(&mut doc, &gif(), "moving.gif", "image/gif");
    let drawn = put(&mut doc, SVG.as_bytes(), "drawn.svg", "image/svg+xml");
    let photo = put(&mut doc, HEIC, "photo.heic", "image/heic");
    let records = json!([{ "title": "GIF", "files": [moving] }, { "title": "SVG", "files": [drawn] }, { "title": "HEIC", "files": [photo] }]);
    call(&mut doc, "transact", json!({ "ops": [{ "op": "records.create", "tableId": table, "records": records }] }));

    // The grid: 2:1 thumbnails 68 x 34 in 40 px rows, a little less for the corners and border.
    call(&mut doc, "preview.show", json!({ "tableId": table }));
    let thumbnail = 60 * 30..=68 * 34;
    let green = count(&doc, GREEN);
    assert!(thumbnail.contains(&green), "the GIF's first frame: {green} green pixels");
    assert_eq!(count(&doc, MAGENTA), 0, "never its second frame");
    let blue = count(&doc, BLUE);
    assert!(thumbnail.contains(&blue), "the SVG: {blue} blue pixels");
    let red = count_near(&doc, HEIC_RED, 24);
    assert!((25 * 30..=34 * 34).contains(&red), "the HEIC's red half: {red} pixels");

    // The gallery: each card's cover is its first picture, cover-cropped into a 216 x 122 box
    // with 12 px corners.
    let gallery = call(&mut doc, "views.create", json!({ "tableId": table, "title": "Covers", "type": "gallery" }))["id"].clone();
    call(&mut doc, "preview.show", json!({ "tableId": table, "viewId": gallery }));
    let cover = 205 * 112..=216 * 122;
    let green = count(&doc, GREEN);
    assert!(cover.contains(&green), "the GIF's cover: {green} green pixels");
    assert_eq!(count(&doc, MAGENTA), 0);
    let blue = count(&doc, BLUE);
    assert!(cover.contains(&blue), "the SVG's cover: {blue} blue pixels");
    let red = count_near(&doc, HEIC_RED, 24);
    assert!((90 * 112..=108 * 122).contains(&red), "the HEIC's cover, its left half red: {red} pixels");

    // A cover the file does not hold keeps the placeholder.
    let url = json!({ "url": "https://example.com/wide.png", "name": "wide.png", "mimeType": "image/png" });
    let id = call(&mut doc, "records.list", json!({ "tableId": table }))["items"][0]["id"].clone();
    call(&mut doc, "records.update", json!({ "tableId": table, "recordId": id, "values": { "files": [url] } }));
    assert_eq!(count(&doc, GREEN), 0);
}

/// `gridPicture` decodes what the page's browser does not (an SVG anywhere, a HEIC outside
/// Safari) as the picture does: width, height, then straight RGBA8; no bytes for a format it does
/// not read either.
#[test]
fn grid_picture_decodes_for_the_page() {
    let doc = TableDocument::create("Pictures.ktable", 0, 0).unwrap();
    let answer = doc.grid_picture(SVG.as_bytes(), 384);
    let (width, height) = (u32::from_le_bytes(answer[..4].try_into().unwrap()), u32::from_le_bytes(answer[4..8].try_into().unwrap()));
    assert_eq!((width, height), (384, 192), "an SVG drawn at the thumbnail's size");
    assert_eq!(answer.len(), 8 + 384 * 192 * 4);
    assert_eq!(answer[8..12], [20, 40, 210, 255]);
    let answer = doc.grid_picture(HEIC, 384);
    assert_eq!(answer[..8], [32, 0, 0, 0, 16, 0, 0, 0]);
    // Half-transparent white comes back straight, not premultiplied.
    let half = "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><rect width='2' height='2' fill='white' fill-opacity='0.5'/></svg>";
    let answer = doc.grid_picture(half.as_bytes(), 2);
    assert!(answer[8..11].iter().all(|channel| *channel >= 254) && (127..=129).contains(&answer[11]), "{:?}", &answer[8..12]);
    assert!(doc.grid_picture(b"BM not a picture", 384).is_empty());
}
