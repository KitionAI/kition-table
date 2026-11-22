//! The display list drawn with vello_cpu: the headless picture's player (the page's is a Canvas
//! 2D one with the same rules). Text is shaped again from each run's string in its one face, at
//! its weight and size, and placed from its anchor and the width the engine measured.

use kition_table_fonts::{FaceId, OpticalSizing, Role};
use kurbo::{Affine, BezPath, Rect, RoundedRect, Shape, Stroke};
use std::sync::Arc;
use vello_cpu::peniko::color::PremulRgba8;
use vello_cpu::peniko::{Color, ImageSampler};

use vello_cpu::{Image, ImageSource, Pixmap, PixmapMut, RenderContext, Resources as VelloResources};

use crate::{Axis, DisplayList, Face, Fit, Op, Resources, line_pixel, snap};

/// A placeholder's color where an image is not decoded.
const IMAGE_PLACEHOLDER: [u8; 4] = [240, 240, 240, 255];

fn paint(rgba: [u8; 4]) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
}

fn role(face: Face) -> Role {
    match face {
        Face::Sans => Role::Sans,
        Face::Fallback => Role::Fallback,
        Face::Hangul => Role::Hangul,
        Face::Mono => Role::Mono,
    }
}

/// Where a canvas is: its origin in document pixels and its device pixels per document pixel.
#[derive(Clone, Copy, Debug)]
struct At {
    x: f64,
    y: f64,
    dpr: f64,
}

impl At {
    fn screen(self) -> Affine {
        Affine::scale(self.dpr) * Affine::translate((-self.x, -self.y))
    }

    /// A box with its edges on device pixel edges, in device pixels.
    fn snapped(self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        let (x, y, w, h) = (f64::from(x), f64::from(y), f64::from(w), f64::from(h));
        Rect::new(snap(x, self.x, self.dpr), snap(y, self.y, self.dpr), snap(x + w, self.x, self.dpr), snap(y + h, self.y, self.dpr))
    }
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(f64::from(x), f64::from(y), f64::from(x + w), f64::from(y + h))
}

/// Draws `list` into `ctx`, whose pixel (0, 0) is the document point (`x`, `y`), at `dpr` device
/// pixels per document pixel, its text at `sizing`'s optical size (the one the list was laid out
/// with). Leaves the context's transform and paint changed.
pub fn draw(ctx: &mut RenderContext, list: &DisplayList, resources: &Resources, x: f64, y: f64, dpr: f64, sizing: OpticalSizing) {
    let at = At { x, y, dpr };
    let screen = at.screen();
    let mut clips = 0usize;
    // The icons' tiles this list draws, made once each.
    let mut tiles: std::collections::HashMap<IconTileKey, (Arc<Pixmap>, i32)> = std::collections::HashMap::new();
    for op in &list.ops {
        match op {
            Op::Rect { x, y, w, h, paint: index, snap } => {
                ctx.set_paint(paint(list.color(*index)));
                if *snap {
                    let area = at.snapped(*x, *y, *w, *h);
                    if area.width() > 0.0 && area.height() > 0.0 {
                        ctx.set_transform(Affine::IDENTITY);
                        ctx.fill_rect(&area);
                    }
                } else {
                    ctx.set_transform(screen);
                    ctx.fill_rect(&rect(*x, *y, *w, *h));
                }
            }
            Op::RRect { x, y, w, h, r, paint: index } => {
                ctx.set_transform(screen);
                ctx.set_paint(paint(list.color(*index)));
                ctx.fill_path(&RoundedRect::from_rect(rect(*x, *y, *w, *h), f64::from(*r)).to_path(0.05));
            }
            Op::RRectStroke { x, y, w, h, r, width, paint: index } => {
                let half = f64::from(*width) / 2.0;
                let inner = rect(*x, *y, *w, *h).inset(-half);
                ctx.set_transform(screen);
                ctx.set_paint(paint(list.color(*index)));
                ctx.set_stroke(Stroke::new(f64::from(*width)));
                ctx.stroke_path(&RoundedRect::from_rect(inner, (f64::from(*r) - half).max(0.0)).to_path(0.05));
            }
            Op::Lines { axis, paint: index, lines } => {
                ctx.set_transform(Affine::IDENTITY);
                ctx.set_paint(paint(list.color(*index)));
                for line in lines {
                    let (pos, from, to) = (f64::from(line.pos), f64::from(line.from), f64::from(line.to));
                    let area = match axis {
                        Axis::Horizontal => {
                            let row = line_pixel(pos, at.y, dpr);
                            Rect::new(snap(from, at.x, dpr), row, snap(to, at.x, dpr), row + 1.0)
                        }
                        Axis::Vertical => {
                            let column = line_pixel(pos, at.x, dpr);
                            Rect::new(column, snap(from, at.y, dpr), column + 1.0, snap(to, at.y, dpr))
                        }
                    };
                    if area.width() > 0.0 && area.height() > 0.0 {
                        ctx.fill_rect(&area);
                    }
                }
            }
            Op::Text { x, y, align, face, weight, size, paint: index, text, width } => {
                let Some(face) = kition_table_fonts::face(role(*face)) else { continue };
                let run = kition_table_fonts::shape_in(face, list.text(*text), *weight, *size, sizing);
                let scale = f64::from(*size) / f64::from(run.upem);
                let mut pen = f64::from(align.left(*x, *width));
                // The baseline on a device pixel row, as browsers put it.
                let baseline = at.y + snap(f64::from(*y), at.y, dpr) / dpr;
                ctx.set_paint(paint(list.color(*index)));
                for glyph in &run.glyphs {
                    let origin = (pen + f64::from(glyph.x_offset) * scale, baseline - f64::from(glyph.y_offset) * scale);
                    let outline = kition_table_fonts::outline(face, *weight, *size, sizing, glyph.id);
                    ctx.set_transform(screen * Affine::translate(origin) * Affine::scale(scale));
                    ctx.fill_path(&outline);
                    pen += f64::from(glyph.advance) * scale;
                }
            }
            Op::Tofu { x, y, w, h, paint: index } => {
                ctx.set_transform(Affine::IDENTITY);
                ctx.set_paint(paint(list.color(*index)));
                tofu(ctx, at.snapped(*x, *y, *w, *h));
            }
            Op::Emoji { atlas, x, y, size } => {
                let Some((face, glyph)) = resources.emoji_of(*atlas) else { continue };
                let pen = (snap(f64::from(*x), at.x, dpr), snap(f64::from(*y), at.y, dpr));
                emoji(ctx, face, glyph, Affine::translate(pen), f64::from(*size) * dpr);
            }
            Op::Icon { icon, x, y, size, stroke, paint: index, solid } => {
                // The icon's tile, as the page draws it, put at its box's snapped top left.
                let style = IconStyle { size: *size, stroke: *stroke, solid: *solid, paint: list.color(*index) };
                let key = (*icon, size.to_bits(), stroke.to_bits(), *solid, style.paint);
                let tile = match tiles.get(&key) {
                    Some(tile) => tile.clone(),
                    None => {
                        let Some(made) = icon_pixmap(resources, *icon, style, dpr) else { continue };
                        tiles.entry(key).or_insert(made).clone()
                    }
                };
                let (left, top) = (snap(f64::from(*x), at.x, dpr) - f64::from(tile.1), snap(f64::from(*y), at.y, dpr) - f64::from(tile.1));
                let area = Rect::new(left, top, left + f64::from(tile.0.width()), top + f64::from(tile.0.height()));
                ctx.set_transform(Affine::IDENTITY);
                ctx.set_paint(Image { image: ImageSource::Pixmap(tile.0), sampler: ImageSampler::default() });
                ctx.set_paint_transform(Affine::translate((left, top)));
                ctx.fill_rect(&area);
                ctx.reset_paint_transform();
            }
            Op::Image { key, x, y, w, h, fit, radius } => {
                let area = rect(*x, *y, *w, *h);
                let shape = RoundedRect::from_rect(area, f64::from(*radius)).to_path(0.05);
                ctx.set_transform(screen);
                match resources.pixels(*key) {
                    Some(pixmap) if pixmap.width() > 0 && pixmap.height() > 0 => {
                        let (iw, ih) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
                        let scale = match fit {
                            Fit::Cover => (area.width() / iw).max(area.height() / ih),
                            Fit::Contain => (area.width() / iw).min(area.height() / ih),
                        };
                        let offset = (area.x0 + (area.width() - iw * scale) / 2.0, area.y0 + (area.height() - ih * scale) / 2.0);
                        ctx.set_paint(Image { image: ImageSource::Pixmap(pixmap.clone()), sampler: ImageSampler::default() });
                        ctx.set_paint_transform(Affine::translate(offset) * Affine::scale(scale));
                        if *fit == Fit::Contain {
                            let shown = Rect::from_origin_size(offset, (iw * scale, ih * scale));
                            ctx.push_clip_layer(&shown.to_path(0.05));
                            ctx.fill_path(&shape);
                            ctx.pop_layer();
                        } else {
                            ctx.fill_path(&shape);
                        }
                        ctx.reset_paint_transform();
                    }
                    _ => {
                        ctx.set_paint(paint(IMAGE_PLACEHOLDER));
                        ctx.fill_path(&shape);
                    }
                }
            }
            Op::Shadow { x, y, w, h, r, blur, paint: index } => {
                ctx.set_transform(screen);
                ctx.set_paint(paint(list.color(*index)));
                ctx.fill_blurred_rounded_rect(&rect(*x, *y, *w, *h), *r, *blur, false);
            }
            Op::Clip { x, y, w, h } => {
                ctx.set_transform(Affine::IDENTITY);
                ctx.push_clip_layer(&at.snapped(*x, *y, *w, *h).to_path(0.05));
                clips += 1;
            }
            Op::Unclip => {
                if clips > 0 {
                    ctx.pop_layer();
                    clips -= 1;
                }
            }
        }
    }
    while clips > 0 {
        ctx.pop_layer();
        clips -= 1;
    }
}

/// A box outline one device pixel thick, inside `area` (device pixels).
fn tofu(ctx: &mut RenderContext, area: Rect) {
    if area.width() < 1.0 || area.height() < 1.0 {
        return;
    }
    for side in [
        Rect::new(area.x0, area.y0, area.x1, area.y0 + 1.0),
        Rect::new(area.x0, area.y1 - 1.0, area.x1, area.y1),
        Rect::new(area.x0, area.y0 + 1.0, area.x0 + 1.0, area.y1 - 1.0),
        Rect::new(area.x1 - 1.0, area.y0 + 1.0, area.x1, area.y1 - 1.0),
    ] {
        if side.width() > 0.0 && side.height() > 0.0 {
            ctx.fill_rect(&side);
        }
    }
}

/// An icon tile's identity in one list: the icon, its size and stroke (as bits), whether it is
/// solid, and its color.
type IconTileKey = (u16, u32, u32, bool, [u8; 4]);

/// How an icon is drawn: its box's size (document pixels), its stroke width (on the icon's grid),
/// whether its outline is filled too, and its color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IconStyle {
    pub size: f32,
    pub stroke: f32,
    pub solid: bool,
    pub paint: [u8; 4],
}

/// A Tessera icon with its box's top left at `origin` (device pixels).
fn draw_icon(ctx: &mut RenderContext, icon: &kition_tessera_icons::Icon, origin: Affine, style: IconStyle, dpr: f64) {
    let grid = f64::from(kition_tessera_icons::GRID);
    ctx.set_transform(origin * Affine::scale(f64::from(style.size) * dpr / grid));
    ctx.set_paint(paint(style.paint));
    ctx.set_stroke(Stroke::new(f64::from(style.stroke)).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round));
    for data in icon.paths {
        if let Ok(path) = BezPath::from_svg(data) {
            if style.solid {
                ctx.fill_path(&path);
            }
            ctx.stroke_path(&path);
        }
    }
    for data in icon.fills {
        if let Ok(path) = BezPath::from_svg(data) {
            ctx.fill_path(&path);
            ctx.stroke_path(&path);
        }
    }
}

/// Icon `id` drawn with `style` at `dpr` on its own square (premultiplied), and how far the square
/// reaches past the icon's box on every side: the stroke's half width and a pixel of antialiasing.
fn icon_pixmap(resources: &Resources, id: u16, style: IconStyle, dpr: f64) -> Option<(Arc<Pixmap>, i32)> {
    let icon = resources.icon_of(id)?;
    let device = f64::from(style.size) * dpr;
    let margin = (f64::from(style.stroke) * device / f64::from(kition_tessera_icons::GRID) / 2.0).ceil() as i32 + 1;
    let side = u16::try_from(device.ceil() as i32 + 2 * margin).ok()?.max(1);
    let mut ctx = RenderContext::new(side, side);
    draw_icon(&mut ctx, icon, Affine::translate((f64::from(margin), f64::from(margin))), style, dpr);
    let pixels = premultiplied(ctx, u32::from(side), u32::from(side));
    let premul = pixels.as_chunks::<4>().0.iter().map(|px| PremulRgba8 { r: px[0], g: px[1], b: px[2], a: px[3] }).collect();
    Some((Arc::new(Pixmap::from_parts(premul, side, side)), margin))
}

/// The tile of icon `id` drawn with `style` at `dpr` (straight RGBA): the pixels the rasterizer
/// puts for an [`Op::Icon`], its top left `left`, `top` device pixels from the box's snapped top
/// left.
pub fn icon_tile(resources: &Resources, id: u16, style: IconStyle, dpr: f64) -> Option<Tile> {
    let (pixmap, margin) = icon_pixmap(resources, id, style, dpr)?;
    let mut rgba: Vec<u8> = pixmap.data().iter().flat_map(|px| [px.r, px.g, px.b, px.a]).collect();
    straight(&mut rgba);
    Some(Tile { width: u32::from(pixmap.width()), height: u32::from(pixmap.height()), left: -margin, top: -margin, rgba })
}

/// An emoji glyph with its pen position at `pen` (device pixels) and `size` device pixels its
/// font size.
fn emoji(ctx: &mut RenderContext, face: FaceId, glyph: u32, pen: Affine, size: f64) {
    let scale = size / f64::from(kition_table_fonts::upem(face));
    match kition_table_fonts::color_steps(face, glyph) {
        Some(steps) => crate::emoji::paint(ctx, pen * Affine::scale_non_uniform(scale, -scale), &steps, [0, 0, 0, 255]),
        None => {
            ctx.set_transform(pen * Affine::scale(scale));
            ctx.set_paint(paint([0, 0, 0, 255]));
            ctx.fill_path(&kition_table_fonts::outline(face, 400, 0.0, OpticalSizing::Auto, glyph));
        }
    }
}

/// An emoji's pixels as the page draws them: straight RGBA, `width` x `height` device pixels,
/// placed with its top left `left`, `top` device pixels from the snapped pen position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    pub rgba: Vec<u8>,
}

/// The tile of emoji `atlas` at a font size of `size` document pixels and `dpr`: the same pixels
/// the rasterizer draws for an [`Op::Emoji`] at a snapped pen position.
pub fn emoji_tile(resources: &Resources, atlas: u32, size: f32, dpr: f64) -> Option<Tile> {
    let (face, glyph) = resources.emoji_of(atlas)?;
    let device = f64::from(size) * dpr;
    // The em box with room for the emoji face's ascent, descent and advance.
    let (left, top) = (0i32, -(device * 1.05).ceil() as i32);
    let (width, bottom) = ((device * 1.35).ceil() as u32, (device * 0.35).ceil() as i32);
    let height = u32::try_from(bottom - top).ok()?;
    let mut ctx = RenderContext::new(u16::try_from(width.max(1)).ok()?, u16::try_from(height.max(1)).ok()?);
    emoji(&mut ctx, face, glyph, Affine::translate((f64::from(-left), f64::from(-top))), device);
    Some(Tile { width, height, left, top, rgba: finish(ctx, width, height) })
}

/// What `ctx` drew, `width` x `height` premultiplied RGBA8.
fn premultiplied(mut ctx: RenderContext, width: u32, height: u32) -> Vec<u8> {
    ctx.flush();
    let (w, h) = (u16::try_from(width).unwrap_or(u16::MAX), u16::try_from(height).unwrap_or(u16::MAX));
    let mut pixels = vec![0u8; usize::from(w) * usize::from(h) * 4];
    let mut resources = VelloResources::new();
    if let Some(target) = PixmapMut::new(w, h, &mut pixels) {
        ctx.render(target, &mut resources);
    }
    pixels
}

fn finish(ctx: RenderContext, width: u32, height: u32) -> Vec<u8> {
    let mut pixels = premultiplied(ctx, width, height);
    straight(&mut pixels);
    pixels
}

/// `list` drawn on a transparent canvas of `width` x `height` device pixels whose top left is the
/// document point (`x`, `y`), at `dpr`, its text at `sizing`'s optical size: straight RGBA8.
pub fn raster(
    list: &DisplayList,
    resources: &Resources,
    (x, y, dpr): (f64, f64, f64),
    (width, height): (u32, u32),
    sizing: OpticalSizing,
) -> Vec<u8> {
    let (w, h) = (u16::try_from(width.max(1)).unwrap_or(u16::MAX), u16::try_from(height.max(1)).unwrap_or(u16::MAX));
    let mut ctx = RenderContext::new(w, h);
    draw(&mut ctx, list, resources, x, y, dpr, sizing);
    finish(ctx, u32::from(w), u32::from(h))
}

/// Premultiplied pixels to straight alpha.
pub(crate) fn straight(pixels: &mut [u8]) {
    for px in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(px[3]);
        if alpha != 0 && alpha != 255 {
            for channel in px.iter_mut().take(3) {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}
