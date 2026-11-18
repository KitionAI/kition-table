//! Attachment pictures: PNG, JPEG, WebP, GIF, SVG and HEIC decoded into thumbnails, turned by their
//! orientation, measured from their headers, and refused when they are something else or too large.

use super::image::{MAX_PIXELS, decode, exif_orientation, size};

/// A 32 x 16 JPEG, its left half red and its right half blue, with no EXIF block.
const SPLIT_JPEG: &[u8] = include_bytes!("../testdata/split.jpg");
/// The same picture as a HEIC (made with sips), with no EXIF block.
const SPLIT_HEIC: &[u8] = include_bytes!("../testdata/split.heic");

const RED: [u8; 3] = [220, 30, 30];
const BLUE: [u8; 3] = [30, 30, 220];

/// A `width` x `height` picture, its left half `left` and its right half `right` (straight RGBA).
fn halves(width: u32, height: u32, left: [u8; 4], right: [u8; 4]) -> Vec<u8> {
    (0..height).flat_map(|_| (0..width).flat_map(move |x| if x < width / 2 { left } else { right })).collect()
}

fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("a header");
    writer.write_image_data(rgba).expect("the pixels");
    writer.finish().expect("the end");
    bytes
}

/// The CRC-32 a PNG chunk ends with.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// A TIFF-structured EXIF block holding only an orientation.
fn exif(orientation: u16, big_endian: bool) -> Vec<u8> {
    let u16b = |value: u16| if big_endian { value.to_be_bytes() } else { value.to_le_bytes() };
    let u32b = |value: u32| if big_endian { value.to_be_bytes() } else { value.to_le_bytes() };
    let mut tiff = if big_endian { b"MM\0*".to_vec() } else { b"II*\0".to_vec() };
    tiff.extend_from_slice(&u32b(8));
    tiff.extend_from_slice(&u16b(1));
    // Tag 0x0112 (orientation), type 3 (SHORT), count 1, the value in the first two bytes.
    tiff.extend_from_slice(&u16b(0x0112));
    tiff.extend_from_slice(&u16b(3));
    tiff.extend_from_slice(&u32b(1));
    tiff.extend_from_slice(&u16b(orientation));
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&u32b(0));
    tiff
}

/// `SPLIT_JPEG` with an EXIF APP1 segment naming `orientation`, right after its SOI marker.
fn turned_jpeg(orientation: u16) -> Vec<u8> {
    let tiff = exif(orientation, true);
    let mut segment = vec![0xFF, 0xE1];
    segment.extend_from_slice(&u16::try_from(2 + 6 + tiff.len()).expect("short").to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend_from_slice(&tiff);
    let mut bytes = SPLIT_JPEG[..2].to_vec();
    bytes.extend_from_slice(&segment);
    bytes.extend_from_slice(&SPLIT_JPEG[2..]);
    bytes
}

/// The premultiplied pixel at (`x`, `y`) of a pixmap, as [r, g, b, a].
fn at(pixmap: &vello_cpu::Pixmap, x: u16, y: u16) -> [u8; 4] {
    let px = pixmap.sample(x, y);
    [px.r, px.g, px.b, px.a]
}

/// Whether a pixel is near `color` (lossy formats).
fn near(px: [u8; 4], color: [u8; 3]) -> bool {
    px[3] == 255 && px.iter().zip(color).all(|(a, b)| a.abs_diff(b) <= 24)
}

#[test]
fn a_png_decodes_and_shrinks_by_area() {
    let bytes = png(16, 8, &halves(16, 8, [220, 30, 30, 255], [30, 30, 220, 255]));
    assert_eq!(size(&bytes), Some((16, 8)));
    let full = decode(&bytes, 512).expect("decodes");
    assert_eq!((full.width(), full.height()), (16, 8));
    assert_eq!((at(&full, 0, 0), at(&full, 15, 7)), ([220, 30, 30, 255], [30, 30, 220, 255]));
    let small = decode(&bytes, 4).expect("decodes");
    assert_eq!((small.width(), small.height()), (4, 2), "the longest side is at most 4, proportions kept");
    assert_eq!((at(&small, 0, 1), at(&small, 3, 0)), ([220, 30, 30, 255], [30, 30, 220, 255]));
}

#[test]
fn transparency_is_premultiplied_and_averaged_as_light() {
    // Opaque white beside transparent black: the average is half-transparent white, not grey.
    let bytes = png(2, 1, &[255, 255, 255, 255, 0, 0, 0, 0]);
    let one = decode(&bytes, 1).expect("decodes");
    assert_eq!(at(&one, 0, 0), [128, 128, 128, 128], "premultiplied white at half alpha");
}

#[test]
fn a_jpeg_decodes_and_turns_by_its_orientation() {
    assert_eq!(size(SPLIT_JPEG), Some((32, 16)));
    let upright = decode(SPLIT_JPEG, 512).expect("decodes");
    assert_eq!((upright.width(), upright.height()), (32, 16));
    assert!(near(at(&upright, 2, 8), RED) && near(at(&upright, 29, 8), BLUE), "{:?} {:?}", at(&upright, 2, 8), at(&upright, 29, 8));

    // Orientation 6: shown turned a quarter clockwise, so the left half is on top.
    let turned = turned_jpeg(6);
    assert_eq!(size(&turned), Some((16, 32)));
    let shown = decode(&turned, 512).expect("decodes");
    assert_eq!((shown.width(), shown.height()), (16, 32));
    assert!(near(at(&shown, 8, 2), RED) && near(at(&shown, 8, 29), BLUE), "{:?} {:?}", at(&shown, 8, 2), at(&shown, 8, 29));
    // Orientation 8: a quarter counter-clockwise, the left half at the bottom.
    let shown = decode(&turned_jpeg(8), 512).expect("decodes");
    assert!(near(at(&shown, 8, 29), RED) && near(at(&shown, 8, 2), BLUE));
    // Orientation 3: half a turn, the left half on the right.
    let shown = decode(&turned_jpeg(3), 512).expect("decodes");
    assert_eq!((shown.width(), shown.height()), (32, 16));
    assert!(near(at(&shown, 29, 8), RED) && near(at(&shown, 2, 8), BLUE));
}

#[test]
fn a_webp_decodes() {
    let rgba = halves(8, 4, [220, 30, 30, 255], [30, 30, 220, 255]);
    let mut bytes = Vec::new();
    image_webp::WebPEncoder::new(&mut bytes).encode(&rgba, 8, 4, image_webp::ColorType::Rgba8).expect("encodes");
    assert_eq!(size(&bytes), Some((8, 4)));
    let pixmap = decode(&bytes, 512).expect("decodes");
    assert_eq!((pixmap.width(), pixmap.height()), (8, 4));
    assert_eq!((at(&pixmap, 0, 0), at(&pixmap, 7, 3)), ([220, 30, 30, 255], [30, 30, 220, 255]));
}

#[test]
fn orientation_reads_both_byte_orders() {
    for orientation in 1..=8 {
        assert_eq!(exif_orientation(&exif(orientation, true)), orientation as u8);
        assert_eq!(exif_orientation(&exif(orientation, false)), orientation as u8);
    }
    let mut prefixed = b"Exif\0\0".to_vec();
    prefixed.extend_from_slice(&exif(6, false));
    assert_eq!(exif_orientation(&prefixed), 6);
    assert_eq!(exif_orientation(&exif(9, true)), 1, "not an orientation");
    assert_eq!(exif_orientation(b"MM\0*\0\0\0\x08"), 1, "cut short");
    assert_eq!(exif_orientation(b""), 1);
}

/// `bytes` (a PNG) with an `eXIf` chunk naming `orientation` right after its IHDR chunk.
fn turned_png(bytes: &[u8], orientation: u16) -> Vec<u8> {
    let tiff = exif(orientation, false);
    let mut chunk = u32::try_from(tiff.len()).expect("short").to_be_bytes().to_vec();
    chunk.extend_from_slice(b"eXIf");
    chunk.extend_from_slice(&tiff);
    let crc = crc32(&chunk[4..]);
    chunk.extend_from_slice(&crc.to_be_bytes());
    // The signature (8) and the IHDR chunk (25).
    let mut out = bytes[..33].to_vec();
    out.extend_from_slice(&chunk);
    out.extend_from_slice(&bytes[33..]);
    out
}

/// A PNG's `eXIf` orientation is applied as a JPEG's is (as a browser shows it).
#[test]
fn a_png_turns_by_its_exif_chunk() {
    let bytes = png(16, 8, &halves(16, 8, [220, 30, 30, 255], [30, 30, 220, 255]));
    let turned = turned_png(&bytes, 6);
    assert_eq!(size(&turned), Some((8, 16)));
    let shown = decode(&turned, 512).expect("decodes");
    assert_eq!((shown.width(), shown.height()), (8, 16));
    assert_eq!((at(&shown, 4, 1), at(&shown, 4, 14)), ([220, 30, 30, 255], [30, 30, 220, 255]), "the left half on top");
    let shown = decode(&turned_png(&bytes, 3), 512).expect("decodes");
    assert_eq!((shown.width(), shown.height()), (16, 8));
    assert_eq!((at(&shown, 1, 4), at(&shown, 14, 4)), ([30, 30, 220, 255], [220, 30, 30, 255]), "half a turn");
    assert_eq!(size(&turned_png(&bytes, 1)), Some((16, 8)));
}

/// A GIF of two frames: the first a 4 x 2 red-and-blue picture at (2, 1) on an 8 x 4 screen, the
/// second all green.
fn two_frame_gif() -> Vec<u8> {
    let palette = [220, 30, 30, 30, 30, 220, 30, 200, 30];
    let mut bytes = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut bytes, 8, 4, &palette).expect("a header");
        let mut first = gif::Frame {
            left: 2,
            top: 1,
            width: 4,
            height: 2,
            buffer: std::borrow::Cow::Owned(vec![0, 0, 1, 1, 0, 0, 1, 1]),
            ..gif::Frame::default()
        };
        first.delay = 10;
        encoder.write_frame(&first).expect("the first frame");
        let second = gif::Frame { width: 8, height: 4, buffer: std::borrow::Cow::Owned(vec![2; 32]), ..gif::Frame::default() };
        encoder.write_frame(&second).expect("the second frame");
    }
    bytes
}

/// A GIF shows its first frame on its logical screen, transparent where the frame is not.
#[test]
fn a_gif_shows_its_first_frame() {
    let bytes = two_frame_gif();
    assert_eq!(size(&bytes), Some((8, 4)));
    let shown = decode(&bytes, 512).expect("decodes");
    assert_eq!((shown.width(), shown.height()), (8, 4));
    assert_eq!((at(&shown, 2, 1), at(&shown, 5, 2)), ([220, 30, 30, 255], [30, 30, 220, 255]), "the first frame, where it is placed");
    assert_eq!((at(&shown, 0, 0), at(&shown, 7, 3)), ([0, 0, 0, 0], [0, 0, 0, 0]), "nothing outside it, and never the second frame");
    let small = decode(&bytes, 4).expect("decodes");
    assert_eq!((small.width(), small.height()), (4, 2));
}

const SPLIT_SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'>\
    <rect width='20' height='20' fill='rgb(220,30,30)'/><rect x='20' width='20' height='20' fill='rgb(30,30,220)'/></svg>";

/// An SVG is drawn by resvg at the thumbnail's size, larger than its own when that is smaller; its
/// size is its own (from `width`/`height`, else its `viewBox`).
#[test]
fn an_svg_is_drawn_at_the_thumbnail_size() {
    assert_eq!(size(SPLIT_SVG.as_bytes()), Some((40, 20)));
    let shown = decode(SPLIT_SVG.as_bytes(), 80).expect("draws");
    assert_eq!((shown.width(), shown.height()), (80, 40), "drawn larger than its own size");
    assert_eq!((at(&shown, 2, 20), at(&shown, 77, 20)), ([220, 30, 30, 255], [30, 30, 220, 255]));
    let small = decode(SPLIT_SVG.as_bytes(), 10).expect("draws");
    assert_eq!((small.width(), small.height()), (10, 5));
    let boxed = "\u{FEFF}<?xml version='1.0'?>\n<!-- a box -->\n<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 30 60'><rect width='30' height='60' fill='rgb(30,30,220)'/></svg>";
    assert_eq!(size(boxed.as_bytes()), Some((30, 60)), "the viewBox's size");
    let shown = decode(boxed.as_bytes(), 60).expect("draws");
    assert_eq!((shown.width(), shown.height(), at(&shown, 15, 30)), (30, 60, [30, 30, 220, 255]));
    // Nothing is loaded from outside the file: an image by reference draws nothing.
    let linked = "<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink' width='8' height='8'>\
        <image width='8' height='8' xlink:href='https://kition.ai/logo.png'/></svg>";
    let shown = decode(linked.as_bytes(), 8).expect("draws");
    assert_eq!(at(&shown, 4, 4), [0, 0, 0, 0]);
}

/// A HEIC is decoded by heic-rs (a pure-Rust decoder).
#[test]
fn a_heic_decodes() {
    assert_eq!(size(SPLIT_HEIC), Some((32, 16)));
    let shown = decode(SPLIT_HEIC, 512).expect("decodes");
    assert_eq!((shown.width(), shown.height()), (32, 16));
    assert!(near(at(&shown, 2, 8), RED) && near(at(&shown, 29, 8), BLUE), "{:?} {:?}", at(&shown, 2, 8), at(&shown, 29, 8));
    let small = decode(SPLIT_HEIC, 8).expect("decodes");
    assert_eq!((small.width(), small.height()), (8, 4));
}

#[test]
fn other_formats_broken_bytes_and_huge_pictures_are_refused() {
    // BMP, TIFF and AVIF are not read.
    let bmp = b"BM\x3a\0\0\0\0\0\0\0\x36\0\0\0\x28\0\0\0\x01\0\0\0\x01\0\0\0\x01\0\x18\0";
    assert_eq!(size(bmp), None);
    assert!(decode(bmp, 64).is_none());
    assert!(decode(b"II*\0\x08\0\0\0", 64).is_none(), "a TIFF");
    assert!(decode(b"\0\0\0\x1cftypavif\0\0\0\0avifmif1miaf", 64).is_none(), "an AVIF");
    assert!(decode(b"GIF89a\x01\x00\x01\x00", 64).is_none(), "a GIF cut short");
    assert!(decode(b"<svg xmlns='http://www.w3.org/2000/svg'", 64).is_none(), "an SVG cut short");
    assert!(decode(b"<html><body><svg/></body></html>", 64).is_none(), "an SVG inside HTML");
    assert!(decode(&SPLIT_HEIC[..SPLIT_HEIC.len() / 2], 64).is_none(), "a HEIC cut short");
    let bytes = png(4, 4, &[9; 64]);
    assert!(decode(&bytes[..bytes.len() - 20], 64).is_none(), "cut short");
    assert!(decode(&SPLIT_JPEG[..SPLIT_JPEG.len() / 2], 64).is_none(), "a JPEG cut short");
    // A PNG whose header claims more pixels than are decoded.
    let side = (MAX_PIXELS as f64).sqrt() as u32 + 1;
    let mut huge = png(1, 1, &[9; 4]);
    // The IHDR chunk's data (width, height) and its CRC.
    huge[16..20].copy_from_slice(&side.to_be_bytes());
    huge[20..24].copy_from_slice(&side.to_be_bytes());
    let crc = crc32(&huge[12..29]);
    huge[29..33].copy_from_slice(&crc.to_be_bytes());
    assert_eq!(size(&huge), Some((side, side)), "its size is read from the header");
    assert!(decode(&huge, 64).is_none(), "but it is not decoded");
}
