//! `kition-table-fonts`: the faces the Table kit's picture draws with, registered at run time.
//!
//! Ported from kition-note's `kition-note-fonts`. A web build embeds no font (the kits' size and
//! asset rules): the editor hands the engine its faces through `addFont` before a table opens
//! (Kition platform E9), from the `kition-fonts` checkout its build copies; the showcase builder
//! and tests register the same files ([`register_dir`]). The headless executable embeds them at
//! build time (feature `embed`, [`register_embedded`]), so its previews have text wherever it runs.
//! Without a face the picture draws no text. Inter sets the table, Droid Sans Fallback the characters Inter lacks (Chinese and
//! Japanese first), chosen per character, so a mixed line shapes as runs of one face each.
//!
//! Emoji draw in color from Noto Color Emoji (COLRv1): a character with emoji presentation, or one
//! an emoji variation selector follows, and the joiners and modifiers of an emoji sequence shape
//! in it; [`color_steps`] reads its glyphs' layers and gradients for the picture to paint.
//!
//! Weight comes from a variable face's `wght` axis. Outlines are cached per face, weight, and glyph.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

mod color;
pub use color::{Brush, CompositeMode, Extend, Paint, Step};

use harfrust::{Direction, Feature, ShapeOptions, Tag, UnicodeBuffer};
use kurbo::BezPath;
use skrifa::instance::{Location, LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::string::StringId;
use skrifa::{GlyphId, MetadataProvider};

/// What a run of text is set in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    /// The table: Inter.
    Sans,
    /// Code: a monospace face, when one is registered.
    Mono,
}

/// The weights the view asks for.
pub const REGULAR: u16 = 400;
pub const SEMIBOLD: u16 = 600;
pub const BOLD: u16 = 700;

/// Which role a registered face plays, from its family name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Sans,
    Mono,
    Fallback,
    Emoji,
}

/// A face's shaper instance and design-space location at one weight.
type Instance = (Option<Arc<harfrust::ShaperInstance>>, Arc<Location>);

/// One registered font file.
struct Face {
    data: Arc<Vec<u8>>,
    role: Role,
    upem: f32,
    ascent: f32,
    descent: f32,
    /// Whether the face has a `wght` axis.
    variable: bool,
    shaper: harfrust::ShaperData,
    /// Shaper instances and design-space locations per weight, made on first use.
    instances: Mutex<HashMap<u16, Instance>>,
    /// Shape plans by weight, script, and language, made on first use: making one per run took
    /// most of a large table's first picture.
    plans: Mutex<HashMap<PlanKey, Arc<harfrust::ShapePlan>>>,
}

/// What a shape plan is for (the direction is always left to right, the features fixed by role).
type PlanKey = (u16, Option<harfrust::Script>, Option<harfrust::Language>);

impl Face {
    fn skrifa(&self) -> Option<skrifa::FontRef<'_>> {
        skrifa::FontRef::new(&self.data).ok()
    }

    fn hb(&self) -> Option<harfrust::FontRef<'_>> {
        harfrust::FontRef::new(&self.data).ok()
    }

    fn covers(&self, c: char) -> bool {
        self.skrifa().is_some_and(|font| font.charmap().map(c).is_some())
    }

    /// The shaper instance and location for `weight` (the default instance for a static face).
    fn at(&self, weight: u16) -> Instance {
        if !self.variable {
            return (None, Arc::new(Location::default()));
        }
        let mut cache = self.instances.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(found) = cache.get(&weight) {
            return found.clone();
        }
        let wght = Tag::new(b"wght");
        let value = f32::from(weight);
        let location = self.skrifa().map(|font| font.axes().location([("wght", value)])).unwrap_or_default();
        let instance = self.hb().map(|hb| Arc::new(harfrust::ShaperInstance::from_variations(&hb, [harfrust::Variation { tag: wght, value }])));
        let made = (instance, Arc::new(location));
        cache.insert(weight, made.clone());
        made
    }
}

/// Every registered face, and the outline cache.
#[derive(Default)]
struct Store {
    faces: RwLock<Vec<Arc<Face>>>,
    outlines: Mutex<HashMap<(usize, u16, u32), Arc<BezPath>>>,
}

/// The cache drops everything past this many outlines (a long CJK table reaches thousands).
const OUTLINE_CACHE_MAX: usize = 20_000;

fn store() -> &'static Store {
    static STORE: std::sync::OnceLock<Store> = std::sync::OnceLock::new();
    STORE.get_or_init(Store::default)
}

fn faces() -> std::sync::RwLockReadGuard<'static, Vec<Arc<Face>>> {
    store().faces.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The face's family name, for its role.
fn family_name(font: &skrifa::FontRef<'_>) -> String {
    [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| font.localized_strings(id).english_or_first().map(|name| name.to_string()))
        .unwrap_or_default()
}

fn role_of(family: &str) -> Role {
    let family = family.to_ascii_lowercase();
    if family.contains("emoji") {
        Role::Emoji
    } else if family.contains("mono") || family.contains("cousine") || family.contains("courier") {
        Role::Mono
    } else if family.contains("fallback") || family.contains("cjk") {
        Role::Fallback
    } else {
        Role::Sans
    }
}

/// Registers a font file (TrueType or OpenType). Answers the family name, or an error for bytes
/// that are not a font. Registering the same family again keeps the first.
pub fn add_font(bytes: Vec<u8>) -> Result<String, String> {
    let (family, upem, ascent, descent, variable) = {
        let font = skrifa::FontRef::new(&bytes).map_err(|error| format!("not a font: {error}"))?;
        let metrics = font.metrics(Size::unscaled(), LocationRef::default());
        let variable = font.axes().iter().any(|axis| axis.tag() == skrifa::Tag::new(b"wght"));
        (family_name(&font), f32::from(metrics.units_per_em), metrics.ascent, -metrics.descent, variable)
    };
    let shaper = {
        let hb = harfrust::FontRef::new(&bytes).map_err(|error| format!("not a font: {error:?}"))?;
        harfrust::ShaperData::new(&hb)
    };
    let mut faces = store().faces.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    let role = role_of(&family);
    // One face per role: the first registered wins, so a host adding fonts twice changes nothing.
    if !faces.iter().any(|face| face.role == role) {
        faces.push(Arc::new(Face {
            data: Arc::new(bytes),
            role,
            upem: upem.max(1.0),
            ascent,
            descent,
            variable,
            shaper,
            instances: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
        }));
    }
    Ok(family)
}

/// The families Kition's font checkout holds for a table, relative to its root.
pub const CHECKOUT_FILES: &[&str] =
    &["fonts/inter/Inter-Variable.ttf", "fonts/droid-sans-fallback/DroidSansFallbackFull.ttf", "fonts/noto-color-emoji/Noto-COLRv1.ttf"];

/// Registers the table's faces from a `kition-fonts` checkout; answers how many it found.
pub fn register_dir(root: &std::path::Path) -> usize {
    CHECKOUT_FILES.iter().filter_map(|file| std::fs::read(root.join(file)).ok()).filter(|bytes| add_font(bytes.clone()).is_ok()).count()
}

#[cfg(feature = "embed")]
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded_fonts.rs"));
}

/// Registers the faces embedded into the build (feature `embed`, the headless executable's): the
/// [`CHECKOUT_FILES`] of the kition-fonts checkout the build read; answers how many.
#[cfg(feature = "embed")]
pub fn register_embedded() -> usize {
    embedded::EMBEDDED.iter().filter(|bytes| add_font(bytes.to_vec()).is_ok()).count()
}

/// Whether any face is registered (without one the view draws no text).
pub fn has_fonts() -> bool {
    !faces().is_empty()
}

/// A face, by its index in the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FaceId(usize);

/// One shaped glyph: advance and offsets in font units, `cluster` a byte offset in the shaped text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub cluster: usize,
    pub advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

/// A run of text shaped in one face.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub face: FaceId,
    /// Font units per em of `face`: a glyph's advance times `size / upem` is its width.
    pub upem: f32,
    pub glyphs: Vec<Glyph>,
}

/// Font units per em of `face` (1000 for an unknown face).
pub fn upem(face: FaceId) -> f32 {
    faces().get(face.0).map_or(1000.0, |face| face.upem)
}

/// A face's vertical metrics in font units, both positive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub upem: f32,
    pub ascent: f32,
    pub descent: f32,
}

/// The order faces are tried in for `family`.
fn preference(family: Family) -> [Role; 3] {
    match family {
        Family::Sans => [Role::Sans, Role::Fallback, Role::Mono],
        Family::Mono => [Role::Mono, Role::Sans, Role::Fallback],
    }
}

/// The face that draws `c` in `family`, if any face covers it (the emoji face last, for a symbol
/// only it has); otherwise the family's first face.
fn face_for(faces: &[Arc<Face>], family: Family, c: char) -> Option<usize> {
    let order = preference(family);
    let ranked = |role: Role| faces.iter().position(|face| face.role == role);
    order
        .iter()
        .filter_map(|role| ranked(*role))
        .chain(ranked(Role::Emoji))
        .find(|index| faces.get(*index).is_some_and(|face| face.covers(c)))
        .or_else(|| order.iter().find_map(|role| ranked(*role)))
}

/// Whether `c` shows as an emoji: emoji presentation by default (Unicode's Emoji_Presentation,
/// in its main blocks), or any character an emoji variation selector (U+FE0F) or a keycap mark
/// follows. A text variation selector (U+FE0E) keeps it text.
fn is_emoji(c: char, next: Option<char>) -> bool {
    match next {
        Some('\u{FE0F}' | '\u{20E3}') => return true,
        Some('\u{FE0E}') => return false,
        _ => {}
    }
    matches!(
        u32::from(c),
        0x1F1E6..=0x1F1FF
            | 0x1F300..=0x1F64F
            | 0x1F680..=0x1F6FF
            | 0x1F7E0..=0x1F7F0
            | 0x1F900..=0x1F9FF
            | 0x1FA70..=0x1FAFF
            | 0x1F004
            | 0x1F0CF
            | 0x1F18E
            | 0x1F191..=0x1F19A
            | 0x1F201
            | 0x1F21A
            | 0x1F22F
            | 0x1F232..=0x1F236
            | 0x1F238..=0x1F23A
            | 0x1F250
            | 0x1F251
            | 0x231A
            | 0x231B
            | 0x23E9..=0x23EC
            | 0x23F0
            | 0x23F3
            | 0x25FD
            | 0x25FE
            | 0x2614
            | 0x2615
            | 0x2648..=0x2653
            | 0x267F
            | 0x2693
            | 0x26A1
            | 0x26AA
            | 0x26AB
            | 0x26BD
            | 0x26BE
            | 0x26C4
            | 0x26C5
            | 0x26CE
            | 0x26D4
            | 0x26EA
            | 0x26F2
            | 0x26F3
            | 0x26F5
            | 0x26FA
            | 0x26FD
            | 0x2705
            | 0x270A
            | 0x270B
            | 0x2728
            | 0x274C
            | 0x274E
            | 0x2753..=0x2755
            | 0x2757
            | 0x2795..=0x2797
            | 0x27B0
            | 0x27BF
            | 0x2B1B
            | 0x2B1C
            | 0x2B50
            | 0x2B55
    )
}

/// Whether `c` joins or modifies the emoji before it (it stays in the emoji's run): the zero
/// width joiner, variation selectors, the keycap mark, skin tones, and tag characters.
fn joins_emoji(c: char) -> bool {
    matches!(u32::from(c), 0x200D | 0xFE0E | 0xFE0F | 0x20E3 | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F)
}

/// The emoji face, if one is registered.
fn emoji_face(faces: &[Arc<Face>]) -> Option<usize> {
    faces.iter().position(|face| face.role == Role::Emoji)
}

/// The color steps of glyph `id` in `face` (an emoji), or `None` for a glyph drawn as its outline.
pub fn color_steps(face: FaceId, id: u32) -> Option<Arc<Vec<Step>>> {
    let face_data = faces().get(face.0).filter(|found| found.role == Role::Emoji).map(|found| found.data.clone())?;
    color::steps(face.0, &face_data, id)
}

/// The metrics of the face prose or code is mainly set in (line height and baseline).
pub fn metrics(family: Family) -> Metrics {
    let faces = faces();
    preference(family).iter().find_map(|role| faces.iter().find(|face| face.role == *role)).map_or(
        Metrics { upem: 1000.0, ascent: 900.0, descent: 250.0 },
        |face| Metrics { upem: face.upem, ascent: face.ascent, descent: face.descent },
    )
}

/// Shapes `text` in `family` at `weight`: runs of one face each, in text order, with clusters as
/// byte offsets into `text`. Without a face the result is empty.
pub fn shape(text: &str, family: Family, weight: u16) -> Vec<Run> {
    let faces = faces();
    let mut runs = Vec::new();
    let mut start = 0;
    let mut current: Option<usize> = None;
    let emoji = emoji_face(&faces);
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let next = chars.peek().map(|(_, next)| *next);
        let in_emoji = emoji.is_some() && current == emoji;
        // Spaces and marks stay in the run they are in, so a run breaks only at a real change; an
        // emoji sequence's joiners and modifiers stay with its emoji, but a space after an emoji is
        // the text's (the emoji face's space is an emoji wide).
        let face = if in_emoji && joins_emoji(c) {
            current
        } else if let Some(index) = emoji.filter(|index| is_emoji(c, next) && faces.get(*index).is_some_and(|face| face.covers(c))) {
            Some(index)
        } else if c.is_whitespace() && !in_emoji {
            current.or_else(|| face_for(&faces, family, c))
        } else {
            face_for(&faces, family, c)
        };
        if face != current {
            if let Some(index) = current
                && at > start
            {
                runs.push(shape_run(&faces, index, text, start..at, weight));
            }
            start = at;
            current = face;
        }
    }
    if let Some(index) = current
        && text.len() > start
    {
        runs.push(shape_run(&faces, index, text, start..text.len(), weight));
    }
    runs
}

fn shape_run(faces: &[Arc<Face>], index: usize, text: &str, range: std::ops::Range<usize>, weight: u16) -> Run {
    let Some(face) = faces.get(index) else { return Run { face: FaceId(index), upem: 1000.0, glyphs: Vec::new() } };
    let mut glyphs = Vec::new();
    if let (Some(hb), Some(part)) = (face.hb(), text.get(range.clone())) {
        let (instance, _) = face.at(weight);
        let shaper = face.shaper.shaper(&hb).instance(instance.as_deref()).build();
        let mut buffer = UnicodeBuffer::new();
        for (at, c) in part.char_indices() {
            buffer.add(c, u32::try_from(at).unwrap_or(u32::MAX));
        }
        buffer.guess_segment_properties();
        buffer.set_direction(Direction::LeftToRight);
        // No ligatures: an editor keeps every character drawn as itself (`-->` in code, `fi` in prose),
        // so the caret never falls inside a glyph. The emoji face keeps them: its sequences (a family,
        // a flag, a skin tone) are ligatures of one grapheme, which the caret never splits.
        let features = [
            Feature::new(Tag::new(b"liga"), 0, ..),
            Feature::new(Tag::new(b"clig"), 0, ..),
            Feature::new(Tag::new(b"calt"), 0, ..),
            Feature::new(Tag::new(b"dlig"), 0, ..),
        ];
        let features: &[Feature] = if face.role == Role::Emoji { &[] } else { &features };
        // The script the buffer guessed (none for text of common characters only).
        let script = Some(buffer.script()).filter(|script| *script != harfrust::script::UNKNOWN);
        let key: PlanKey = (weight, script, buffer.language());
        let plan = {
            let mut plans = face.plans.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            plans
                .entry(key)
                .or_insert_with_key(|(_, script, language)| {
                    Arc::new(harfrust::ShapePlan::new(&shaper, Direction::LeftToRight, *script, language.as_ref(), features))
                })
                .clone()
        };
        let shaped = shaper.shape(buffer, ShapeOptions::new().features(features).plan(Some(&plan)));
        for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            glyphs.push(Glyph {
                id: info.glyph_id,
                cluster: range.start + info.cluster as usize,
                advance: position.x_advance as f32,
                x_offset: position.x_offset as f32,
                y_offset: position.y_offset as f32,
            });
        }
    }
    Run { face: FaceId(index), upem: face.upem, glyphs }
}

/// The outline of glyph `id` in `face` at `weight`, in font units with y pointing down.
pub fn outline(face: FaceId, weight: u16, id: u32) -> Arc<BezPath> {
    let key = (face.0, weight, id);
    let cache = &store().outlines;
    if let Some(found) = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
        return found.clone();
    }
    let mut pen = FlipPen(BezPath::new());
    if let Some(face) = faces().get(face.0).cloned()
        && let Some(font) = face.skrifa()
        && let Some(glyph) = font.outline_glyphs().get(GlyphId::new(id))
    {
        let (_, location) = face.at(weight);
        let location: LocationRef<'_> = location.as_ref().into();
        let _ = glyph.draw(DrawSettings::unhinted(Size::unscaled(), location), &mut pen);
    }
    let path = Arc::new(pen.0);
    let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if cache.len() >= OUTLINE_CACHE_MAX {
        cache.clear();
    }
    cache.insert(key, path.clone());
    path
}

/// Skrifa's y-up outline as kurbo's y-down path.
struct FlipPen(BezPath);

impl OutlinePen for FlipPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), -f64::from(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), -f64::from(y)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((f64::from(cx0), -f64::from(cy0)), (f64::from(x), -f64::from(y)));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((f64::from(cx0), -f64::from(cy0)), (f64::from(cx1), -f64::from(cy1)), (f64::from(x), -f64::from(y)));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

#[cfg(test)]
mod tests;
