//! Color glyphs (the emoji face's COLR v1 and v0): each read once into a short list of drawing
//! steps in font units, y pointing up, which the view paints with its own renderer. Colors come
//! from the face's first palette; the text's own color stands for the palette's foreground entry.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kurbo::BezPath;
use skrifa::color::{Brush as SkrifaBrush, ColorPainter, Transform};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::types::BoundingBox;
use skrifa::{GlyphId, MetadataProvider};

pub use skrifa::color::{CompositeMode, Extend};

/// A color: straight RGBA, or the text's color at an alpha.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Paint {
    Rgba([u8; 4]),
    Foreground(f32),
}

/// What fills an area.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Paint),
    Linear {
        p0: (f32, f32),
        p1: (f32, f32),
        stops: Vec<(f32, Paint)>,
        extend: Extend,
    },
    Radial {
        c0: (f32, f32),
        r0: f32,
        c1: (f32, f32),
        r1: f32,
        stops: Vec<(f32, Paint)>,
        extend: Extend,
    },
    /// Angles in degrees, counterclockwise from the x axis (y up).
    Sweep {
        center: (f32, f32),
        start: f32,
        end: f32,
        stops: Vec<(f32, Paint)>,
        extend: Extend,
    },
}

/// One drawing step. Transforms are `[xx, yx, xy, yy, dx, dy]`.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    PushTransform([f32; 6]),
    PopTransform,
    /// Clips what follows to a glyph's outline, or to a box `[x0, y0, x1, y1]`.
    PushClip(Arc<BezPath>),
    PushClipBox([f32; 4]),
    PopClip,
    /// Fills a glyph's outline, its brush moved by `brush_transform`.
    FillPath {
        path: Arc<BezPath>,
        brush_transform: Option<[f32; 6]>,
        brush: Brush,
    },
    /// Fills the innermost clip (a fill after a clip box or a clip glyph).
    Fill(Brush),
    PushLayer(CompositeMode),
    PopLayer,
}

/// A color glyph's steps, by face data and glyph.
type Painted = HashMap<(usize, u32), Option<Arc<Vec<Step>>>>;

fn painted() -> &'static Mutex<Painted> {
    static PAINTED: std::sync::OnceLock<Mutex<Painted>> = std::sync::OnceLock::new();
    PAINTED.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The steps that draw glyph `id` of the face whose bytes are `data`, or `None` for a glyph
/// without color (drawn as its outline). `key` names the face in the cache.
pub(crate) fn steps(key: usize, data: &[u8], id: u32) -> Option<Arc<Vec<Step>>> {
    if let Some(found) = painted().lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&(key, id)) {
        return found.clone();
    }
    let made = read(data, id).map(Arc::new);
    painted().lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert((key, id), made.clone());
    made
}

fn read(data: &[u8], id: u32) -> Option<Vec<Step>> {
    let font = skrifa::FontRef::new(data).ok()?;
    let glyph = font.color_glyphs().get(GlyphId::new(id))?;
    let palette: Vec<[u8; 4]> = font
        .color_palettes()
        .get(0)
        .map(|palette| palette.colors().iter().map(|color| [color.red, color.green, color.blue, color.alpha]).collect())
        .unwrap_or_default();
    let mut painter = Recorder { font: &font, palette, steps: Vec::new() };
    glyph.paint(LocationRef::default(), &mut painter).ok()?;
    Some(painter.steps)
}

struct Recorder<'a> {
    font: &'a skrifa::FontRef<'a>,
    palette: Vec<[u8; 4]>,
    steps: Vec<Step>,
}

/// The palette's foreground entry (the text's color).
const FOREGROUND: u16 = 0xFFFF;

impl Recorder<'_> {
    fn paint(&self, index: u16, alpha: f32) -> Paint {
        if index == FOREGROUND {
            return Paint::Foreground(alpha);
        }
        let [r, g, b, a] = self.palette.get(usize::from(index)).copied().unwrap_or([0, 0, 0, 255]);
        Paint::Rgba([r, g, b, (f32::from(a) * alpha.clamp(0.0, 1.0)).round() as u8])
    }

    fn stops(&self, stops: &[skrifa::color::ColorStop]) -> Vec<(f32, Paint)> {
        stops.iter().map(|stop| (stop.offset, self.paint(stop.palette_index, stop.alpha))).collect()
    }

    fn brush(&self, brush: SkrifaBrush<'_>) -> Brush {
        match brush {
            SkrifaBrush::Solid { palette_index, alpha } => Brush::Solid(self.paint(palette_index, alpha)),
            SkrifaBrush::LinearGradient { p0, p1, color_stops, extend } => {
                Brush::Linear { p0: (p0.x, p0.y), p1: (p1.x, p1.y), stops: self.stops(color_stops), extend }
            }
            SkrifaBrush::RadialGradient { c0, r0, c1, r1, color_stops, extend } => {
                Brush::Radial { c0: (c0.x, c0.y), r0, c1: (c1.x, c1.y), r1, stops: self.stops(color_stops), extend }
            }
            SkrifaBrush::SweepGradient { c0, start_angle, end_angle, color_stops, extend } => {
                Brush::Sweep { center: (c0.x, c0.y), start: start_angle, end: end_angle, stops: self.stops(color_stops), extend }
            }
        }
    }

    /// Glyph `id`'s outline in font units, y up.
    fn outline(&self, id: GlyphId) -> Arc<BezPath> {
        let mut pen = Pen(BezPath::new());
        if let Some(glyph) = self.font.outline_glyphs().get(id) {
            let _ = glyph.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut pen);
        }
        Arc::new(pen.0)
    }
}

fn matrix(transform: Transform) -> [f32; 6] {
    [transform.xx, transform.yx, transform.xy, transform.yy, transform.dx, transform.dy]
}

impl ColorPainter for Recorder<'_> {
    fn push_transform(&mut self, transform: Transform) {
        self.steps.push(Step::PushTransform(matrix(transform)));
    }

    fn pop_transform(&mut self) {
        self.steps.push(Step::PopTransform);
    }

    fn push_clip_glyph(&mut self, glyph_id: GlyphId) {
        let path = self.outline(glyph_id);
        self.steps.push(Step::PushClip(path));
    }

    fn push_clip_box(&mut self, clip_box: BoundingBox<f32>) {
        self.steps.push(Step::PushClipBox([clip_box.x_min, clip_box.y_min, clip_box.x_max, clip_box.y_max]));
    }

    fn pop_clip(&mut self) {
        self.steps.push(Step::PopClip);
    }

    fn fill(&mut self, brush: SkrifaBrush<'_>) {
        let brush = self.brush(brush);
        self.steps.push(Step::Fill(brush));
    }

    fn fill_glyph(&mut self, glyph_id: GlyphId, brush_transform: Option<Transform>, brush: SkrifaBrush<'_>) {
        let path = self.outline(glyph_id);
        let brush = self.brush(brush);
        self.steps.push(Step::FillPath { path, brush_transform: brush_transform.map(matrix), brush });
    }

    fn push_layer(&mut self, composite_mode: CompositeMode) {
        self.steps.push(Step::PushLayer(composite_mode));
    }

    fn pop_layer(&mut self) {
        self.steps.push(Step::PopLayer);
    }
}

/// Skrifa's outline as is (y up), for the color steps.
struct Pen(BezPath);

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), f64::from(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), f64::from(y)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((f64::from(cx0), f64::from(cy0)), (f64::from(x), f64::from(y)));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((f64::from(cx0), f64::from(cy0)), (f64::from(cx1), f64::from(cy1)), (f64::from(x), f64::from(y)));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}
