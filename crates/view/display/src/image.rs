//! Attachment pictures: PNG, JPEG, WebP, GIF, SVG and HEIC read into a thumbnail, and their
//! size as shown, read from their headers (an SVG's from its tree).
//!
//! A picture is shown as a browser shows it: turned by its EXIF orientation (JPEG, WebP and PNG's
//! `eXIf` chunk; the page's `createImageBitmap` does the same by default), so its size as shown has
//! width and height swapped for orientations 5 to 8. A GIF shows its first frame on its logical
//! screen; an SVG is drawn by resvg at the thumbnail's size (no text, no embedded pictures); a HEIC
//! or HEIF is decoded by heic-rs with its container's rotation, mirror and crop applied. Other
//! formats (AVIF, TIFF, BMP) are not read: the grid draws a placeholder for them.

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
    Gif,
    Svg,
    Heif,
}

fn format(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Format::Jpeg)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else if is_heif(bytes) {
        Some(Format::Heif)
    } else if is_svg(bytes) {
        Some(Format::Svg)
    } else {
        None
    }
}

/// An ISO-BMFF file whose `ftyp` box names an HEVC-coded HEIF brand (`heic`, `heix`, `hevc`…), or
/// the generic `mif1`/`msf1` without naming AVIF.
fn is_heif(bytes: &[u8]) -> bool {
    if bytes.get(4..8) != Some(b"ftyp") {
        return false;
    }
    let size = bytes.get(..4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize).min(bytes.len());
    // The major brand, then (after the minor version) the compatible ones.
    let compatible = bytes.get(16..size).unwrap_or_default().as_chunks::<4>().0;
    let brands: Vec<&[u8]> = bytes.get(8..12).into_iter().chain(compatible.iter().map(<[u8; 4]>::as_slice)).collect();
    let named = |list: &[&[u8; 4]]| brands.iter().any(|brand| list.iter().any(|name| *brand == name.as_slice()));
    named(&[b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"hevm", b"hevs"]) || (named(&[b"mif1", b"msf1"]) && !named(&[b"avif", b"avis"]))
}

/// Text that opens with an `<svg` element (after an XML declaration, comments or a doctype).
fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    let head = head.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(head);
    let start = head.iter().position(|byte| !byte.is_ascii_whitespace()).unwrap_or(head.len());
    head.get(start) == Some(&b'<') && head.windows(4).any(|window| window == b"<svg")
}

/// A PNG's `eXIf` chunk, when one comes before its pixels (as browsers read it; png 0.17 does not
/// read it).
fn png_exif(bytes: &[u8]) -> Option<&[u8]> {
    let mut at = 8;
    while let Some(head) = bytes.get(at..at + 8) {
        let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        match &head[4..] {
            b"eXIf" => return bytes.get(at + 8..at + 8 + length),
            b"IDAT" | b"IEND" => return None,
            _ => at = at.checked_add(12)?.checked_add(length)?,
        }
    }
    None
}

/// Runs a decoder, a panic in it (a bug its fuzzing has not found yet) being no picture. A
/// WebAssembly build aborts on a panic instead; the decoders are pure Rust without `unsafe`
/// (resvg's tiny-skia aside), and heic-rs is pinned to a fuzzed release.
fn guarded<T>(decoder: impl FnOnce() -> Option<T>) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(decoder)).ok().flatten()
}

/// An SVG's tree: no fonts (its text is not drawn) and nothing loaded from outside it.
fn svg_tree(bytes: &[u8]) -> Option<resvg::usvg::Tree> {
    resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).ok()
}

/// The HEIF decoder's options: straight RGBA8, the container's transforms applied, at most
/// [`MAX_PIXELS`].
fn heif_options() -> heic_rs::DecodeOptions {
    heic_rs::DecodeOptions::default()
        .with_layout(heic_rs::PixelLayout::Rgba8)
        .with_max_pixels(Some(MAX_PIXELS))
        .with_transforms(true)
        .with_alpha(true)
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
            Some((info.width, info.height, png_exif(bytes).map_or(1, exif_orientation)))
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
        // The logical screen the frames are drawn on.
        Format::Gif => {
            let size = |at: usize| bytes.get(at..at + 2).map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])));
            Some((size(6)?, size(8)?, 1))
        }
        Format::Svg => {
            let size = svg_tree(bytes)?.size();
            let side = |value: f32| (value.is_finite() && value > 0.0).then(|| value.ceil().min(f32::from(u16::MAX)) as u32);
            Some((side(size.width())?, side(size.height())?, 1))
        }
        // The size as shown: the container's rotation and crop are already in it.
        Format::Heif => {
            let info = heic_rs::probe(bytes).ok()?;
            Some((info.width, info.height, 1))
        }
    }
}

fn read(bytes: &[u8]) -> Option<Raw> {
    let (width, height, orientation) = header(bytes)?;
    if too_large(width, height) {
        return None;
    }
    let rgba = match format(bytes)? {
        Format::Svg => return None,
        Format::Gif => first_frame(bytes, width, height)?,
        Format::Heif => {
            let image = heic_rs::decode(bytes, &heif_options()).ok()?;
            if (image.width, image.height) != (width, height) {
                return None;
            }
            image.data
        }
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

/// A GIF's first frame on its logical screen (`width` x `height`, transparent where the frame
/// does not cover it), straight RGBA8, as a browser shows a GIF that does not move.
fn first_frame(bytes: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(Cursor::new(bytes)).ok()?;
    let frame = decoder.read_next_frame().ok()??;
    let (screen_width, screen_height) = (width as usize, height as usize);
    let (left, top, frame_width) = (usize::from(frame.left), usize::from(frame.top), usize::from(frame.width));
    let mut rgba = vec![0u8; screen_width * screen_height * 4];
    for (row, line) in frame.buffer.chunks_exact(frame_width.max(1) * 4).take(usize::from(frame.height)).enumerate() {
        let y = top + row;
        if y >= screen_height || left >= screen_width {
            continue;
        }
        let shown = frame_width.min(screen_width - left);
        rgba[(y * screen_width + left) * 4..(y * screen_width + left + shown) * 4].copy_from_slice(&line[..shown * 4]);
    }
    Some(rgba)
}

/// The size `bytes` (a PNG, JPEG, WebP, GIF, SVG or HEIC) are shown at, after their orientation,
/// read from the headers alone (an SVG's from its tree); `None` for another format or broken
/// bytes.
pub fn size(bytes: &[u8]) -> Option<(u32, u32)> {
    let (width, height, orientation) = guarded(|| header(bytes))?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(if orientation >= 5 { (height, width) } else { (width, height) })
}

/// `bytes` (a PNG, JPEG, WebP, GIF, SVG or HEIC) decoded, turned by their orientation and scaled
/// down (an area average) so that neither side is longer than `max_side`: premultiplied pixels
/// for [`crate::Resources::set_image`]. An SVG is drawn with its longest side `max_side`, larger
/// than its own size when that is smaller. `None` for another format, broken bytes, or a picture
/// of more than [`MAX_PIXELS`].
pub fn decode(bytes: &[u8], max_side: u32) -> Option<Pixmap> {
    guarded(|| if format(bytes)? == Format::Svg { draw_svg(bytes, max_side) } else { decode_raster(bytes, max_side) })
}

/// An SVG drawn at `max_side` on its longest side.
fn draw_svg(bytes: &[u8], max_side: u32) -> Option<Pixmap> {
    let tree = svg_tree(bytes)?;
    let size = tree.size();
    let longest = size.width().max(size.height());
    if !(longest.is_finite() && longest > 0.0) {
        return None;
    }
    let scale = max_side.clamp(1, u32::from(u16::MAX)) as f32 / longest;
    let width = (size.width() * scale).round().clamp(1.0, f32::from(u16::MAX)) as u32;
    let height = (size.height() * scale).round().clamp(1.0, f32::from(u16::MAX)) as u32;
    let mut canvas = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(width as f32 / size.width(), height as f32 / size.height()), &mut canvas.as_mut());
    // tiny-skia's pixels are premultiplied RGBA8 already.
    let pixels = canvas.data().as_chunks::<4>().0.iter().map(|[r, g, b, a]| PremulRgba8 { r: *r, g: *g, b: *b, a: *a }).collect();
    Some(Pixmap::from_parts(pixels, u16::try_from(width).ok()?, u16::try_from(height).ok()?))
}

/// A raster picture decoded, turned and scaled down (see [`decode`]).
fn decode_raster(bytes: &[u8], max_side: u32) -> Option<Pixmap> {
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
