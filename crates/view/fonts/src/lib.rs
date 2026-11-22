//! `kition-table-fonts`: the faces the Table kit's picture draws with, registered at run time.
//!
//! Ported from kition-note's `kition-note-fonts`. A web build embeds no font (the kits' size and
//! asset rules): the editor hands the engine its faces through `addFont` before a table opens
//! (Kition platform E9), from the `kition-fonts` checkout its build copies; the showcase builder
//! and tests register the same files ([`register_dir`]). The headless executable embeds them at
//! build time (feature `embed`, [`register_embedded`]), so its previews have text wherever it runs.
//! Without a face the picture draws no text. Inter sets the table, Droid Sans Fallback the characters Inter lacks (Chinese and
//! Japanese first), Nanum Gothic Korean, chosen per character, so a mixed line shapes as runs of one face each. A character no
//! face covers is a tofu run ([`Run::tofu`]): a box of [`TOFU_ADVANCE`] em that every player draws the same, never a system
//! font's glyph.
//!
//! Shaping keeps the browsers' default features (kerning, ligatures, contextual alternates): a
//! Canvas cannot turn them off, and the grid's runs must measure as the page's `measureText`
//! does. Inter's optical size axis follows the font size (clamped to the axis), as Chromium's Canvas
//! and every browser's CSS text set it ([`OpticalSizing::Auto`]), unless the caller measures for a
//! Canvas that keeps the face's default optical size at every size, as WebKit's does
//! ([`OpticalSizing::FixedDefault`]; the editor finds out which its page's Canvas does).
//!
//! Emoji draw in color from Noto Color Emoji (COLRv1): a character with emoji presentation, or one
//! an emoji variation selector follows, and the joiners and modifiers of an emoji sequence shape
//! in it; [`color_steps`] reads its glyphs' layers and gradients for the picture to paint.
//!
//! Weight comes from a variable face's `wght` axis, optical size from its `opsz` axis. Outlines are
//! cached per face, weight, optical size and glyph.
//!
//! The editor gives the engine Inter before a table opens and the other faces only once text needs
//! them (Kition platform E9, E9a: kit-editor's `lazyFonts`): [`take_wanted`] answers whether text
//! laid out since it last answered had a character no registered face covers (or an emoji without
//! the emoji face), and [`generation`] moves with every face registered, so a layout cached with
//! fewer faces is made again.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

mod color;
pub use color::{Brush, CompositeMode, Extend, Paint, Step};

use harfrust::{Direction, ShapeOptions, Tag, UnicodeBuffer};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Inter.
    Sans,
    /// A monospace face.
    Mono,
    /// Droid Sans Fallback: Chinese and Japanese.
    Fallback,
    /// Nanum Gothic: Korean.
    Hangul,
    /// Noto Color Emoji.
    Emoji,
}

/// A tofu box's advance, in em.
pub const TOFU_ADVANCE: f32 = 0.6;

/// How a face with an optical size axis (`opsz`, Inter's) is set at a font size. Every function
/// that shapes, measures or draws text takes it, so a layout and its drawing agree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OpticalSizing {
    /// The optical size is the font size, clamped to the axis: CSS's `font-optical-sizing: auto`,
    /// and Chromium's Canvas. The headless picture and exports always use it.
    #[default]
    Auto,
    /// The axis's default optical size at every font size: Playwright's WebKit Canvas on Linux
    /// (and, by its engine, WebKitGTK's), which measures and draws Inter at its default `opsz` 14
    /// whatever the size.
    FixedDefault,
}

impl OpticalSizing {
    /// The setting's name in the editor's requests: `auto` or `fixed-default`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::FixedDefault => "fixed-default",
        }
    }

    /// The setting a name ([`OpticalSizing::name`]) names.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "auto" => Some(Self::Auto),
            "fixed-default" => Some(Self::FixedDefault),
            _ => None,
        }
    }
}

/// A face's shaper instance and design-space location at one weight and optical size.
type Instance = (Option<Arc<harfrust::ShaperInstance>>, Arc<Location>);

/// A variable instance: the weight, and the optical size's bits (0 for a face without `opsz`).
type InstanceKey = (u16, u32);

/// One registered font file.
struct Face {
    data: Arc<Vec<u8>>,
    role: Role,
    upem: f32,
    ascent: f32,
    descent: f32,
    /// Whether the face has a `wght` axis.
    variable: bool,
    /// Its `opsz` axis (smallest, default, largest), if it has one.
    opsz: Option<(f32, f32, f32)>,
    shaper: harfrust::ShaperData,
    /// Shaper instances and design-space locations per weight and optical size, made on first use.
    instances: Mutex<HashMap<InstanceKey, Instance>>,
    /// Shape plans by weight, script, and language, made on first use: making one per run took
    /// most of a large table's first picture.
    plans: Mutex<HashMap<PlanKey, Arc<harfrust::ShapePlan>>>,
    /// Which characters of the Basic Multilingual Plane the face maps, one bit each: choosing a
    /// face per character read the character map each time, a quarter of a band's layout.
    bmp: Box<[u64; 1024]>,
}

/// What a shape plan is for (the direction is always left to right, the features the defaults).
type PlanKey = (InstanceKey, Option<harfrust::Script>, Option<harfrust::Language>);

impl Face {
    fn skrifa(&self) -> Option<skrifa::FontRef<'_>> {
        skrifa::FontRef::new(&self.data).ok()
    }

    fn hb(&self) -> Option<harfrust::FontRef<'_>> {
        harfrust::FontRef::new(&self.data).ok()
    }

    fn covers(&self, c: char) -> bool {
        let code = u32::from(c) as usize;
        if code < 0x1_0000 {
            return self.bmp[code / 64] & (1 << (code % 64)) != 0;
        }
        self.skrifa().is_some_and(|font| font.charmap().map(c).is_some())
    }

    /// The instance key for `weight` at `size` px: the optical size is the font size clamped to
    /// the face's `opsz` axis ([`OpticalSizing::Auto`]), or the axis's default
    /// ([`OpticalSizing::FixedDefault`]).
    fn key(&self, weight: u16, size: f32, sizing: OpticalSizing) -> InstanceKey {
        let opsz = self.opsz.map_or(0.0, |(min, default, max)| match sizing {
            OpticalSizing::Auto => size.clamp(min, max),
            OpticalSizing::FixedDefault => default,
        });
        (if self.variable { weight } else { 0 }, opsz.to_bits())
    }

    /// The shaper instance and location for `key` (the default instance for a static face).
    fn at(&self, key: InstanceKey) -> Instance {
        if !self.variable && self.opsz.is_none() {
            return (None, Arc::new(Location::default()));
        }
        let mut cache = self.instances.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(found) = cache.get(&key) {
            return found.clone();
        }
        let mut settings: Vec<(&[u8; 4], f32)> = Vec::new();
        if self.variable {
            settings.push((b"wght", f32::from(key.0)));
        }
        if self.opsz.is_some() {
            settings.push((b"opsz", f32::from_bits(key.1)));
        }
        let location =
            self.skrifa().map(|font| font.axes().location(settings.iter().map(|(tag, value)| (skrifa::Tag::new(tag), *value)))).unwrap_or_default();
        let instance = self.hb().map(|hb| {
            Arc::new(harfrust::ShaperInstance::from_variations(
                &hb,
                settings.iter().map(|(tag, value)| harfrust::Variation { tag: Tag::new(tag), value: *value }),
            ))
        });
        let made = (instance, Arc::new(location));
        cache.insert(key, made.clone());
        made
    }
}

/// Every registered face, and the outline cache.
#[derive(Default)]
struct Store {
    faces: RwLock<Vec<Arc<Face>>>,
    outlines: Mutex<HashMap<(usize, InstanceKey, u32), Arc<BezPath>>>,
}

/// The cache drops everything past this many outlines (a long CJK table reaches thousands).
const OUTLINE_CACHE_MAX: usize = 20_000;

/// Moves with every face registered (see [`generation`]).
static GENERATION: AtomicU32 = AtomicU32::new(0);
/// Whether text needed a face that is not registered since [`take_wanted`] last answered.
static WANTED: AtomicBool = AtomicBool::new(false);

/// How many faces were registered so far: what text lays out as may change when it moves (a
/// character drawn as tofu gets its face), so a cache of layouts keeps the generation it was made
/// in and starts again when it moves.
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Acquire)
}

/// Whether text laid out since the last call had a character no registered face covers (a tofu
/// run), or an emoji while no emoji face is registered; the call clears it. The editor gives the
/// engine its other faces once this answers `true` (Kition platform E9a, `wantsFonts`).
pub fn take_wanted() -> bool {
    WANTED.swap(false, Ordering::AcqRel)
}

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
    } else if family.contains("nanum") {
        Role::Hangul
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
    let (family, upem, ascent, descent, variable, opsz, bmp) = {
        let font = skrifa::FontRef::new(&bytes).map_err(|error| format!("not a font: {error}"))?;
        let metrics = font.metrics(Size::unscaled(), LocationRef::default());
        let variable = font.axes().iter().any(|axis| axis.tag() == skrifa::Tag::new(b"wght"));
        let opsz = font
            .axes()
            .iter()
            .find(|axis| axis.tag() == skrifa::Tag::new(b"opsz"))
            .map(|axis| (axis.min_value(), axis.default_value(), axis.max_value()));
        let charmap = font.charmap();
        let mut bmp = Box::new([0u64; 1024]);
        for (code, _) in charmap.mappings() {
            let code = code as usize;
            if code < 0x1_0000 && char::from_u32(code as u32).is_some_and(|c| charmap.map(c).is_some()) {
                bmp[code / 64] |= 1 << (code % 64);
            }
        }
        (family_name(&font), f32::from(metrics.units_per_em), metrics.ascent, -metrics.descent, variable, opsz, bmp)
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
            opsz,
            shaper,
            instances: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            bmp,
        }));
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
    Ok(family)
}

/// The families Kition's font checkout holds for a table, relative to its root.
pub const CHECKOUT_FILES: &[&str] = &[
    "fonts/inter/Inter-Variable.ttf",
    "fonts/droid-sans-fallback/DroidSansFallbackFull.ttf",
    "fonts/nanum-gothic/NanumGothic-Regular.ttf",
    "fonts/noto-color-emoji/Noto-COLRv1.ttf",
];

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

/// A run of text shaped in one face, or a tofu run: characters no face covers, one glyph (id 0,
/// [`TOFU_ADVANCE`] em) each, in a run whose `face` is the family's first face.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub face: FaceId,
    pub tofu: bool,
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
fn preference(family: Family) -> [Role; 4] {
    match family {
        Family::Sans => [Role::Sans, Role::Fallback, Role::Hangul, Role::Mono],
        Family::Mono => [Role::Mono, Role::Sans, Role::Fallback, Role::Hangul],
    }
}

/// The face that draws `c` in `family`, if any face covers it (the emoji face last, for a symbol
/// only it has).
fn face_for(faces: &[Arc<Face>], family: Family, c: char) -> Option<usize> {
    let ranked = |role: Role| faces.iter().position(|face| face.role == role);
    preference(family)
        .iter()
        .filter_map(|role| ranked(*role))
        .chain(ranked(Role::Emoji))
        .find(|index| faces.get(*index).is_some_and(|face| face.covers(c)))
}

/// The family's first registered face.
fn first_face(faces: &[Arc<Face>], family: Family) -> Option<usize> {
    preference(family).iter().find_map(|role| faces.iter().position(|face| face.role == *role))
}

/// Whether `c` is a default-ignorable character that shapes to nothing (joiners, variation
/// selectors, zero-width spaces, the byte order mark): it stays in the run it is in.
fn ignorable(c: char) -> bool {
    matches!(u32::from(c), 0x00AD | 0x034F | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFE00..=0xFE0F | 0xFEFF | 0xE0100..=0xE01EF)
}

/// The registered face of `role`, if any.
pub fn face(role: Role) -> Option<FaceId> {
    faces().iter().position(|face| face.role == role).map(FaceId)
}

/// The role of a registered face.
pub fn role(face: FaceId) -> Option<Role> {
    faces().get(face.0).map(|face| face.role)
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

/// What a character is set in while a line is split into runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Face(usize),
    Tofu,
}

/// Shapes `text` in `family` at `weight` and `size` px with `sizing`'s optical size: runs of one
/// face each (or tofu runs), in text order, with clusters as byte offsets into `text`. Without a
/// face the result is empty.
pub fn shape(text: &str, family: Family, weight: u16, size: f32, sizing: OpticalSizing) -> Vec<Run> {
    runs(text, family, (weight, size, sizing), shape_run)
}

/// `text` split into runs as [`shape`] splits it, each glyph the character's own (its character
/// map's glyph and its advance at `weight`, `size` and `sizing`), without shaping: no kerning,
/// ligatures or marks placed. Several times cheaper than shaping, for a draft of a layout (the grid
/// while its view jumps); a run's width is near its shaped width, not equal to it.
pub fn advances(text: &str, family: Family, weight: u16, size: f32, sizing: OpticalSizing) -> Vec<Run> {
    runs(text, family, (weight, size, sizing), advance_run)
}

/// The weight, the size in px and the optical sizing a run is set at.
type Setting = (u16, f32, OpticalSizing);

/// What makes one run: the faces, the run's face, the text and the run's range of it, and its
/// setting.
type RunMaker = fn(&[Arc<Face>], usize, &str, std::ops::Range<usize>, Setting) -> Run;

fn runs(text: &str, family: Family, setting: Setting, make: RunMaker) -> Vec<Run> {
    let faces = faces();
    let Some(first) = first_face(&faces, family).or_else(|| emoji_face(&faces)) else { return Vec::new() };
    let mut runs = Vec::new();
    let mut start = 0;
    let mut current: Option<Slot> = None;
    let emoji = emoji_face(&faces);
    let mut chars = text.char_indices().peekable();
    let flush = |runs: &mut Vec<Run>, slot: Option<Slot>, range: std::ops::Range<usize>| match slot {
        Some(Slot::Face(index)) if !range.is_empty() => runs.push(make(&faces, index, text, range, setting)),
        Some(Slot::Tofu) if !range.is_empty() => runs.push(tofu_run(first, text, range)),
        _ => {}
    };
    while let Some((at, c)) = chars.next() {
        let next = chars.peek().map(|(_, next)| *next);
        if emoji.is_none() && is_emoji(c, next) {
            WANTED.store(true, Ordering::Release);
        }
        let in_emoji = emoji.is_some() && current == emoji.map(Slot::Face);
        // Spaces and marks stay in the run they are in, so a run breaks only at a real change; an
        // emoji sequence's joiners and modifiers stay with its emoji, but a space after an emoji is
        // the text's (the emoji face's space is an emoji wide).
        let slot = if in_emoji && joins_emoji(c) {
            current
        } else if let Some(index) = emoji.filter(|index| is_emoji(c, next) && faces.get(*index).is_some_and(|face| face.covers(c))) {
            Some(Slot::Face(index))
        } else if (c.is_whitespace() && !in_emoji && current != Some(Slot::Tofu)) || (ignorable(c) && current.is_some()) {
            current.or_else(|| face_for(&faces, family, c).map(Slot::Face))
        } else {
            Some(face_for(&faces, family, c).map_or(Slot::Tofu, Slot::Face))
        };
        if slot == Some(Slot::Tofu) {
            WANTED.store(true, Ordering::Release);
        }
        if slot != current {
            flush(&mut runs, current, start..at);
            start = at;
            current = slot;
        }
    }
    flush(&mut runs, current, start..text.len());
    runs
}

/// Shapes `text` in one face only (a display list's text run, whose face the engine chose), at
/// `weight` and `size` px with `sizing`'s optical size.
pub fn shape_in(face: FaceId, text: &str, weight: u16, size: f32, sizing: OpticalSizing) -> Run {
    shape_run(&faces(), face.0, text, 0..text.len(), (weight, size, sizing))
}

/// Whether a registered face covers `c`.
pub fn covers(face: FaceId, c: char) -> bool {
    faces().get(face.0).is_some_and(|found| found.covers(c))
}

fn tofu_run(face: usize, text: &str, range: std::ops::Range<usize>) -> Run {
    let glyphs = text
        .get(range.clone())
        .unwrap_or_default()
        .char_indices()
        .map(|(at, _)| Glyph { id: 0, cluster: range.start + at, advance: TOFU_ADVANCE * 1000.0, x_offset: 0.0, y_offset: 0.0 })
        .collect();
    Run { face: FaceId(face), tofu: true, upem: 1000.0, glyphs }
}

fn advance_run(faces: &[Arc<Face>], index: usize, text: &str, range: std::ops::Range<usize>, (weight, size, sizing): Setting) -> Run {
    let Some(face) = faces.get(index) else { return Run { face: FaceId(index), tofu: false, upem: 1000.0, glyphs: Vec::new() } };
    let mut glyphs = Vec::new();
    if let (Some(font), Some(part)) = (face.skrifa(), text.get(range.clone())) {
        let (_, location) = face.at(face.key(weight, size, sizing));
        let location: LocationRef<'_> = location.as_ref().into();
        let metrics = font.glyph_metrics(Size::unscaled(), location);
        let charmap = font.charmap();
        for (at, c) in part.char_indices() {
            let id = charmap.map(c).unwrap_or_default();
            glyphs.push(Glyph {
                id: id.to_u32(),
                cluster: range.start + at,
                advance: metrics.advance_width(id).unwrap_or_default(),
                x_offset: 0.0,
                y_offset: 0.0,
            });
        }
    }
    Run { face: FaceId(index), tofu: false, upem: face.upem, glyphs }
}

fn shape_run(faces: &[Arc<Face>], index: usize, text: &str, range: std::ops::Range<usize>, (weight, size, sizing): Setting) -> Run {
    let Some(face) = faces.get(index) else { return Run { face: FaceId(index), tofu: false, upem: 1000.0, glyphs: Vec::new() } };
    let mut glyphs = Vec::new();
    if let (Some(hb), Some(part)) = (face.hb(), text.get(range.clone())) {
        let instance_key = face.key(weight, size, sizing);
        let (instance, _) = face.at(instance_key);
        let shaper = face.shaper.shaper(&hb).instance(instance.as_deref()).build();
        let mut buffer = UnicodeBuffer::new();
        for (at, c) in part.char_indices() {
            buffer.add(c, u32::try_from(at).unwrap_or(u32::MAX));
        }
        buffer.guess_segment_properties();
        buffer.set_direction(Direction::LeftToRight);
        // The default features, as a browser shapes (a Canvas cannot turn ligatures or kerning off).
        // The script the buffer guessed (none for text of common characters only).
        let script = Some(buffer.script()).filter(|script| *script != harfrust::script::UNKNOWN);
        let key: PlanKey = (instance_key, script, buffer.language());
        let plan = {
            let mut plans = face.plans.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            plans
                .entry(key)
                .or_insert_with_key(|(_, script, language)| {
                    Arc::new(harfrust::ShapePlan::new(&shaper, Direction::LeftToRight, *script, language.as_ref(), &[]))
                })
                .clone()
        };
        let shaped = shaper.shape(buffer, ShapeOptions::new().plan(Some(&plan)));
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
    Run { face: FaceId(index), tofu: false, upem: face.upem, glyphs }
}

/// The outline of glyph `id` in `face` at `weight` and `size` px with `sizing`'s optical size, in
/// font units with y pointing down.
pub fn outline(face: FaceId, weight: u16, size: f32, sizing: OpticalSizing, id: u32) -> Arc<BezPath> {
    let instance_key = faces().get(face.0).map_or((weight, 0), |found| found.key(weight, size, sizing));
    let key = (face.0, instance_key, id);
    let cache = &store().outlines;
    if let Some(found) = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
        return found.clone();
    }
    let mut pen = FlipPen(BezPath::new());
    if let Some(face) = faces().get(face.0).cloned()
        && let Some(font) = face.skrifa()
        && let Some(glyph) = font.outline_glyphs().get(GlyphId::new(id))
    {
        let (_, location) = face.at(instance_key);
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
