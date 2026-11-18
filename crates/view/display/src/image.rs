//! Attachment pictures for the headless rasterizer: PNG, JPEG and WebP read into a thumbnail, and
//! their size as shown, read from their headers.
//!
//! A picture is shown as a browser shows it: turned by its EXIF orientation (JPEG and WebP; the
//! page's `createImageBitmap` does the same by default), so its size as shown has width and height
//! swapped for orientations 5 to 8. Other formats (GIF, SVG, HEIC) are not read: the grid draws
//! a placeholder for them, the same on both sides.

use std::io::Cursor;

use vello_cpu::Pixmap;
use vello_cpu::peniko::color::PremulRgba8;

/// The most pixels a picture may have to be decoded (a 48-megapixel photo fits).
pub const MAX_PIXELS: u64 = 50_000_000;

/// The formats read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Png,
    Jpeg,
    Webp,
}

fn format(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else {
        None
    }
}

/// The EXIF orientation (1 to 8) in a TIFF-structured EXIF block, 1 when it has none.
pub fn exif_orientation(exif: &[u8]) -> u8 {
    let exif = exif.strip_prefix(b"Exif\0\0").unwrap_or(exif);
    let big = match exif.get(..4) {
        Some(b"MM\0*") => true,
        Some(b"II*\0") => false,
        _ => return 1,
    };
    let u16_at = |at: usize| exif.get(at..at + 2).map(|b| if big { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) });
    let u32_at = |at: usize| {
        exif.get(at..at + 4).map(|b| if big { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) })
    };
    let Some(ifd) = u32_at(4).map(|offset| offset as usize) else { return 1 };
    let count = u16_at(ifd).unwrap_or(0);
    for index in 0..usize::from(count) {
        let entry = ifd + 2 + 12 * index;
        if u16_at(entry) == Some(0x0112) {
            return u16_at(entry + 8).and_then(|value| u8::try_from(value).ok()).filter(|value| (1..=8).contains(value)).unwrap_or(1);
        }
    }
    1
}

/// A decoded picture: straight RGBA8, its stored size, and its orientation.
struct Raw {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    orientation: u8,
}

fn too_large(width: u32, height: u32) -> bool {
    width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS
}

/// The stored size and orientation, from the headers.
fn header(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    match format(bytes)? {
        Format::Png => {
            let reader = png::Decoder::new(bytes).read_info().ok()?;
            let info = reader.info();
            Some((info.width, info.height, 1))
        }
        Format::Jpeg => {
            let mut decoder = zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(bytes));
            decoder.decode_headers().ok()?;
            let (width, height) = decoder.dimensions()?;
            let orientation = decoder.exif().map_or(1, |exif| exif_orientation(exif));
            Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?, orientation))
        }
        Format::Webp => {
            let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
            let (width, height) = decoder.dimensions();
            let orientation = decoder.exif_metadata().ok().flatten().map_or(1, |exif| exif_orientation(&exif));
            Some((width, height, orientation))
        }
    }
}

fn read(bytes: &[u8]) -> Option<Raw> {
    let (width, height, orientation) = header(bytes)?;
    if too_large(width, height) {
        return None;
    }
    let rgba = match format(bytes)? {
        Format::Png => {
            let mut decoder = png::Decoder::new(bytes);
            decoder.set_transformations(png::Transformations::normalize_to_color8());
            let mut reader = decoder.read_info().ok()?;
            let mut buffer = vec![0; reader.output_buffer_size()];
            let frame = reader.next_frame(&mut buffer).ok()?;
            buffer.truncate(frame.buffer_size());
            match frame.color_type {
                png::ColorType::Rgba => buffer,
                png::ColorType::Rgb => buffer.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect(),
                png::ColorType::GrayscaleAlpha => buffer.as_chunks::<2>().0.iter().flat_map(|[v, a]| [*v, *v, *v, *a]).collect(),
                png::ColorType::Grayscale => buffer.iter().flat_map(|v| [*v, *v, *v, 255]).collect(),
                png::ColorType::Indexed => return None,
            }
        }
        Format::Jpeg => {
            let options = zune_core::options::DecoderOptions::default()
                .jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGB)
                .set_max_width(65_535)
                .set_max_height(65_535);
            let mut decoder = zune_jpeg::JpegDecoder::new_with_options(zune_core::bytestream::ZCursor::new(bytes), options);
            let rgb = decoder.decode().ok()?;
            rgb.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect()
        }
        Format::Webp => {
            let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
            let mut buffer = vec![0; decoder.output_buffer_size()?];
            decoder.read_image(&mut buffer).ok()?;
            if decoder.has_alpha() { buffer } else { buffer.as_chunks::<3>().0.iter().flat_map(|[r, g, b]| [*r, *g, *b, 255]).collect() }
        }
    };
    (rgba.len() as u64 == u64::from(width) * u64::from(height) * 4).then_some(Raw { width, height, rgba, orientation })
}

/// The size `bytes` (a PNG, JPEG or WebP) are shown at, after their orientation, read from the
/// headers alone; `None` for another format or broken bytes.
pub fn size(bytes: &[u8]) -> Option<(u32, u32)> {
    let (width, height, orientation) = header(bytes)?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(if orientation >= 5 { (height, width) } else { (width, height) })
}

/// `bytes` (a PNG, JPEG or WebP) decoded, turned by their orientation and scaled down (an area
/// average) so that neither side is longer than `max_side`: premultiplied pixels for
/// [`crate::Resources::set_image`]. `None` for another format, broken bytes, or a picture of more
/// than [`MAX_PIXELS`].
pub fn decode(bytes: &[u8], max_side: u32) -> Option<Pixmap> {
    let raw = read(bytes)?;
    let longest = raw.width.max(raw.height);
    let max_side = max_side.clamp(1, u32::from(u16::MAX));
    let (width, height) = if longest > max_side {
        let scale = f64::from(max_side) / f64::from(longest);
        (((f64::from(raw.width) * scale).round() as u32).max(1), ((f64::from(raw.height) * scale).round() as u32).max(1))
    } else {
        (raw.width, raw.height)
    };
    let pixels = shrink(&raw, width, height);
    let (pixels, width, height) = orient(pixels, width, height, raw.orientation);
    Some(Pixmap::from_parts(pixels, u16::try_from(width).ok()?, u16::try_from(height).ok()?))
}

/// The picture premultiplied and scaled to `width` x `height` by averaging the source pixels
/// each target pixel covers.
fn shrink(raw: &Raw, width: u32, height: u32) -> Vec<PremulRgba8> {
    let (sw, sh) = (raw.width as usize, raw.height as usize);
    let (tw, th) = (width as usize, height as usize);
    let span = |target: usize, of: usize, source: usize| {
        let start = target * source / of;
        (start, ((target + 1) * source / of).max(start + 1).min(source))
    };
    let mut out = Vec::with_capacity(tw * th);
    for ty in 0..th {
        let (y0, y1) = span(ty, th, sh);
        for tx in 0..tw {
            let (x0, x1) = span(tx, tw, sw);
            let mut sum = [0u64; 4];
            for y in y0..y1 {
                for px in raw.rgba[(y * sw + x0) * 4..(y * sw + x1) * 4].as_chunks::<4>().0 {
                    let alpha = u64::from(px[3]);
                    for (channel, value) in sum.iter_mut().zip(px.iter().take(3)) {
                        *channel += u64::from(*value) * alpha;
                    }
                    sum[3] += alpha;
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u64;
            // Premultiplied channels: sum(c * a) / 255 / n, rounded.
            let premul = |value: u64| ((value + 255 * n / 2) / (255 * n)).min(255) as u8;
            let a = ((sum[3] + n / 2) / n).min(255) as u8;
            out.push(PremulRgba8 { r: premul(sum[0]).min(a), g: premul(sum[1]).min(a), b: premul(sum[2]).min(a), a });
        }
    }
    out
}

/// The pixels turned by EXIF `orientation`: (pixels, width, height) as shown.
fn orient(pixels: Vec<PremulRgba8>, width: u32, height: u32, orientation: u8) -> (Vec<PremulRgba8>, u32, u32) {
    if !(2..=8).contains(&orientation) {
        return (pixels, width, height);
    }
    let (w, h) = (width as usize, height as usize);
    let (ow, oh) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut out = Vec::with_capacity(pixels.len());
    for y in 0..oh {
        for x in 0..ow {
            let (sx, sy) = match orientation {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            out.push(pixels[sy * w + sx]);
        }
    }
    (out, ow as u32, oh as u32)
}
