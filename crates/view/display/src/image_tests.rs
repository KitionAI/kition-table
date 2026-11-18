//! Attachment pictures: PNG, JPEG and WebP decoded into thumbnails, turned by their orientation,
//! measured from their headers, and refused when they are something else or too large.

use super::image::{MAX_PIXELS, decode, exif_orientation, size};

/// A 32 x 16 JPEG, its left half red and its right half blue, with no EXIF block.
const SPLIT_JPEG: &[u8] = include_bytes!("../testdata/split.jpg");

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

#[test]
fn other_formats_broken_bytes_and_huge_pictures_are_refused() {
    assert_eq!(size(b"GIF89a\x01\x00\x01\x00"), None);
    assert!(decode(b"GIF89a\x01\x00\x01\x00", 64).is_none());
    assert!(decode(b"<svg xmlns='http://www.w3.org/2000/svg'/>", 64).is_none());
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
