//! The picture as a list of things to draw, in document pixels (CSS pixels of the editor), and
//! their drawing with vello_cpu at any scale: rectangles, borders, soft shadows, lines, paths,
//! Tessera icons, shaped text, and display lists (a grid view, which `kition-table-display`
//! draws).

use std::sync::Arc;

use kition_table_display::{DisplayList, Resources};
use kition_table_fonts::{FaceId, Family};
use kurbo::{Affine, BezPath, Point, Rect, RoundedRect, Shape, Stroke};
use vello_cpu::peniko::Color;
use vello_cpu::{PixmapMut, RenderContext};

pub use kition_table_grid::theme::{Rgba, alpha, hex};

fn paint(rgba: Rgba) -> Color {
    Color::from_rgba8(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// One glyph of a line of text, placed relative to the line's origin (its baseline's start).
#[derive(Clone, Debug)]
pub struct Glyph {
    pub face: FaceId,
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub advance: f64,
    /// Font units to pixels.
    pub scale: f64,
    /// A character no face covers: a box.
    pub tofu: bool,
}

/// A shaped line of text.
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub glyphs: Vec<Glyph>,
    pub width: f64,
    pub weight: u16,
    pub size: f64,
}

/// The ellipsis a cut line ends with.
const ELLIPSIS: &str = "\u{2026}";

/// `text` shaped at `size` px and `weight`, cut with an ellipsis to at most `max` px wide
/// (`f64::INFINITY`: no limit). Line breaks and tabs show as spaces. Without fonts the line is
/// empty.
pub fn shape(text: &str, size: f64, weight: u16, max: f64) -> Line {
    let text = text.replace(['\n', '\r', '\t'], " ");
    let mut line = shape_all(&text, size, weight);
    if line.width <= max {
        return line;
    }
    let ellipsis = shape_all(ELLIPSIS, size, weight);
    let room = (max - ellipsis.width).max(0.0);
    let mut kept = Vec::new();
    let mut width = 0.0;
    for glyph in line.glyphs {
        if glyph.x + glyph.advance > room {
            break;
        }
        width = glyph.x + glyph.advance;
        kept.push(glyph);
    }
    for mut glyph in ellipsis.glyphs {
        glyph.x += width;
        kept.push(glyph);
    }
    line.glyphs = kept;
    line.width = width + ellipsis.width;
    line
}

fn shape_all(text: &str, size: f64, weight: u16) -> Line {
    let mut glyphs = Vec::new();
    let mut x = 0.0;
    for run in kition_table_fonts::shape(text, Family::Sans, weight, size as f32) {
        let scale = size / f64::from(run.upem);
        for glyph in &run.glyphs {
            let advance = f64::from(glyph.advance) * scale;
            glyphs.push(Glyph {
                face: run.face,
                id: glyph.id,
                x: x + f64::from(glyph.x_offset) * scale,
                y: -f64::from(glyph.y_offset) * scale,
                advance,
                scale,
                tofu: run.tofu,
            });
            x += advance;
        }
    }
    Line { glyphs, width: x, weight, size }
}

/// How far below a row's middle the baseline of `size` px text sits so the text looks centred,
/// as a browser centres a line box: half the face's ascent minus its descent.
pub fn baseline_below_middle(size: f64) -> f64 {
    let metrics = kition_table_fonts::metrics(Family::Sans);
    (f64::from(metrics.ascent) - f64::from(metrics.descent)) / 2.0 * size / f64::from(metrics.upem)
}

/// One thing to draw.
#[derive(Clone, Debug)]
pub enum Item {
    Fill {
        shape: BezPath,
        color: Rgba,
    },
    Stroke {
        shape: BezPath,
        width: f64,
        color: Rgba,
    },
    Shadow {
        area: Rect,
        radius: f64,
        blur: f64,
        color: Rgba,
    },
    Icon {
        icon: &'static kition_tessera_icons::Icon,
        at: Point,
        size: f64,
        color: Rgba,
        stroke: f64,
    },
    Text {
        line: Arc<Line>,
        origin: Point,
        color: Rgba,
    },
    /// A display list and its resources.
    List {
        list: Arc<(DisplayList, Resources)>,
    },
    Clip {
        area: Rect,
    },
    Unclip,
}

/// What a picture draws, in order, and its size.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub items: Vec<(Rect, Item)>,
    pub width: f64,
    pub height: f64,
}

impl Scene {
    fn push(&mut self, bounds: Rect, item: Item) {
        self.items.push((bounds, item));
    }

    pub fn rect(&mut self, area: Rect, color: Rgba) {
        self.push(area, Item::Fill { shape: area.to_path(0.1), color });
    }

    pub fn round_rect(&mut self, area: Rect, radius: f64, color: Rgba) {
        self.push(area, Item::Fill { shape: RoundedRect::from_rect(area, radius).to_path(0.1), color });
    }

    /// A border inside `area`, `width` px wide.
    pub fn border(&mut self, area: Rect, radius: f64, width: f64, color: Rgba) {
        let half = width / 2.0;
        let inner = area.inset(-half);
        self.push(area, Item::Stroke { shape: RoundedRect::from_rect(inner, (radius - half).max(0.0)).to_path(0.1), width, color });
    }

    pub fn shadow(&mut self, area: Rect, radius: f64, blur: f64, color: Rgba) {
        self.push(area.inflate(blur * 2.5, blur * 2.5), Item::Shadow { area, radius, blur, color });
    }

    /// A horizontal hairline at `y` (its top), from `x0` to `x1`.
    pub fn hline(&mut self, x0: f64, x1: f64, y: f64, color: Rgba) {
        self.rect(Rect::new(x0, y, x1, y + 1.0), color);
    }

    /// A vertical hairline at `x` (its left), from `y0` to `y1`.
    pub fn vline(&mut self, x: f64, y0: f64, y1: f64, color: Rgba) {
        self.rect(Rect::new(x, y0, x + 1.0, y1), color);
    }

    pub fn fill(&mut self, shape: BezPath, color: Rgba) {
        self.push(shape.bounding_box(), Item::Fill { shape, color });
    }

    pub fn stroke(&mut self, shape: BezPath, width: f64, color: Rgba) {
        self.push(shape.bounding_box().inflate(width, width), Item::Stroke { shape, width, color });
    }

    pub fn circle(&mut self, center: Point, radius: f64, color: Rgba) {
        self.fill(kurbo::Circle::new(center, radius).to_path(0.1), color);
    }

    /// A Tessera icon `size` px square with its top left at `at`.
    pub fn icon(&mut self, name: &str, at: Point, size: f64, color: Rgba) {
        if let Some(icon) = kition_tessera_icons::icon(name) {
            let area = Rect::from_origin_size(at, (size, size));
            self.push(area.inflate(1.0, 1.0), Item::Icon { icon, at, size, color, stroke: f64::from(kition_tessera_icons::STROKE) });
        }
    }

    /// A line of text with its baseline starting at `origin`.
    pub fn text(&mut self, line: Line, origin: Point, color: Rgba) {
        if line.glyphs.is_empty() {
            return;
        }
        let bounds = Rect::new(origin.x - 2.0, origin.y - line.size * 1.2, origin.x + line.width + 2.0, origin.y + line.size * 0.4);
        self.push(bounds, Item::Text { line: Arc::new(line), origin, color });
    }

    /// Text vertically centred on `middle`, starting at `x`.
    pub fn text_mid(&mut self, line: Line, x: f64, middle: f64, color: Rgba) {
        let below = baseline_below_middle(line.size);
        self.text(line, Point::new(x, middle + below), color);
    }

    /// A display list covering `area`.
    pub fn list(&mut self, area: Rect, list: DisplayList, resources: Resources) {
        self.push(area, Item::List { list: Arc::new((list, resources)) });
    }

    /// Clips what follows to `area` until [`Scene::unclip`].
    pub fn clip(&mut self, area: Rect) {
        self.push(area, Item::Clip { area });
    }

    pub fn unclip(&mut self) {
        self.push(Rect::ZERO, Item::Unclip);
    }

    /// The pixels of the picture's area from (`x`, `y`) at `scale` device pixels per document
    /// pixel, `width` x `height` device pixels, as straight RGBA8; outside the picture is white.
    pub fn render(&self, x: f64, y: f64, scale: f64, width: u32, height: u32) -> Vec<u8> {
        let (width, height) = (u16::try_from(width.max(1)).unwrap_or(u16::MAX), u16::try_from(height.max(1)).unwrap_or(u16::MAX));
        let mut ctx = RenderContext::new(width, height);
        let to_screen = Affine::scale(scale) * Affine::translate((-x, -y));
        let visible = Rect::new(x, y, x + f64::from(width) / scale, y + f64::from(height) / scale).inflate(4.0, 4.0);
        ctx.set_transform(Affine::IDENTITY);
        ctx.set_paint(paint(hex(0xFFFFFF)));
        ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
        let mut clips = 0usize;
        for (bounds, item) in &self.items {
            match item {
                Item::Clip { area } => {
                    ctx.set_transform(to_screen);
                    ctx.push_clip_layer(&area.to_path(0.1));
                    clips += 1;
                    continue;
                }
                Item::Unclip => {
                    if clips > 0 {
                        ctx.pop_layer();
                        clips -= 1;
                    }
                    continue;
                }
                _ => {}
            }
            if !bounds.overlaps(visible) {
                continue;
            }
            if let Item::List { list } = item {
                kition_table_display::draw(&mut ctx, &list.0, &list.1, x, y, scale);
                continue;
            }
            draw(&mut ctx, item, to_screen);
        }
        while clips > 0 {
            ctx.pop_layer();
            clips -= 1;
        }
        ctx.flush();
        let mut pixels = vec![0u8; usize::from(width) * usize::from(height) * 4];
        let mut resources = vello_cpu::Resources::new();
        if let Some(target) = PixmapMut::new(width, height, &mut pixels) {
            ctx.render(target, &mut resources);
        }
        straight(&mut pixels);
        pixels
    }
}

fn draw(ctx: &mut RenderContext, item: &Item, to_screen: Affine) {
    match item {
        Item::Fill { shape, color } => {
            ctx.set_transform(to_screen);
            ctx.set_paint(paint(*color));
            ctx.fill_path(shape);
        }
        Item::Stroke { shape, width, color } => {
            ctx.set_transform(to_screen);
            ctx.set_paint(paint(*color));
            ctx.set_stroke(Stroke::new(*width));
            ctx.stroke_path(shape);
        }
        Item::Shadow { area, radius, blur, color } => {
            ctx.set_transform(to_screen);
            ctx.set_paint(paint(*color));
            ctx.fill_blurred_rounded_rect(area, *radius as f32, *blur as f32, false);
        }
        Item::Icon { icon, at, size, color, stroke } => {
            let grid = f64::from(kition_tessera_icons::GRID);
            ctx.set_transform(to_screen * Affine::translate(at.to_vec2()) * Affine::scale(size / grid));
            ctx.set_paint(paint(*color));
            ctx.set_stroke(Stroke::new(*stroke).with_caps(kurbo::Cap::Round).with_join(kurbo::Join::Round));
            for data in icon.paths {
                if let Ok(path) = BezPath::from_svg(data) {
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
        Item::Text { line, origin, color } => {
            ctx.set_paint(paint(*color));
            for glyph in &line.glyphs {
                let at = to_screen * Affine::translate((origin.x + glyph.x, origin.y + glyph.y));
                if glyph.tofu {
                    // The box a grid draws for a character no face covers, in outline.
                    let size = line.size;
                    let (w, h) = (glyph.advance * 0.75, size * 0.72);
                    let area = Rect::new((glyph.advance - w) / 2.0, -h, (glyph.advance + w) / 2.0, 0.0);
                    ctx.set_transform(at);
                    ctx.set_stroke(Stroke::new(1.0));
                    ctx.stroke_path(&area.inset(-0.5).to_path(0.1));
                    continue;
                }
                if let Some(steps) = kition_table_fonts::color_steps(glyph.face, glyph.id) {
                    kition_table_display::emoji::paint(ctx, at * Affine::scale_non_uniform(glyph.scale, -glyph.scale), &steps, *color);
                    ctx.set_paint(paint(*color));
                    continue;
                }
                let path = kition_table_fonts::outline(glyph.face, line.weight, line.size as f32, glyph.id);
                ctx.set_transform(at * Affine::scale(glyph.scale));
                ctx.fill_path(&path);
            }
        }
        Item::Clip { .. } | Item::Unclip | Item::List { .. } => {}
    }
}

/// Premultiplied pixels to straight alpha.
fn straight(pixels: &mut [u8]) {
    for px in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(px[3]);
        if alpha != 0 && alpha != 255 {
            for channel in px.iter_mut().take(3) {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}
