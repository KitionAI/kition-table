//! `kition-table-display`: the Table grid's display list, format v1.
//!
//! A display list says what one part of the grid looks like (a band of rows and columns, a header
//! strip, or a whole view for the picture) as a short list of drawing operations in document CSS
//! pixels, independent of the device pixel ratio. The engine (`kition-table-grid`) makes it; the
//! editor's page plays it on a Canvas 2D and the headless picture rasterizes the same list, so the
//! screen and the picture agree. The layout (text cut, wrapped and placed, chips, icons) is done
//! before the list is made: neither side measures or lays anything out when it draws.
//!
//! # Bytes
//!
//! A list travels as one buffer (a Worker hands it to the page as a transferable `ArrayBuffer`),
//! little endian, every section starting on a 4-byte boundary:
//!
//! ```text
//! header   56 B   magic "KTDL" | version u16 = 1 | flags u16
//!                 epoch u32 | theme u16 | resources u16
//!                 layer u8 | col_band u8 | pad u16 | row_band u32
//!                 origin x, y f32 | size w, h f32
//!                 palette count u16 | pad u16 | op count u32 | string bytes u32 | hit count u32
//! palette  u32 RGBA (straight, R in the lowest byte) x palette count
//! strings  UTF-8, string bytes long, padded with zeros to 4 bytes
//! ops      records, each a multiple of 4 bytes, the first byte its opcode (see [`Op`])
//! hits     28 B each (see [`Hit`])
//! ```
//!
//! The page and the engine ship in one package, so a list of another version is refused
//! ([`DecodeError::Version`]); there is no reader for another version.
//!
//! # Pixel alignment
//!
//! Both players align the same things to device pixels by the same rules, with `dpr` the device
//! pixel ratio and `o` the canvas origin in document pixels (a canvas starts on a device pixel):
//!
//! - a coordinate `v` of a [`Op::Rect`] with `snap`, a [`Op::Tofu`] box, a [`Op::Clip`] and the
//!   ends of a line goes to the device pixel edge `round((v - o) * dpr)` ([`snap`]);
//! - a line of [`Op::Lines`] at `pos` is the one device pixel row (or column) `floor((pos - o) *
//!   dpr)` ([`line_pixel`]), one device pixel thick at every ratio;
//! - an [`Op::Emoji`] is drawn with its pen position snapped as a coordinate is, so its tile
//!   (rasterized once by the engine) lands on whole device pixels.
//!
//! Everything else (text, rounded rectangles, icons, shadows, images) is drawn at its exact
//! position and antialiased.
//!
//! # Resource table
//!
//! A list names icons, emoji and attachment pictures by id ([`Op::Icon`], [`Op::Emoji`],
//! [`Op::Image`]); the header's `resources` is the version of the [`Resources`] those ids refer
//! to. The engine sends the page what it lacks as a resource table, apart from the lists, in one
//! buffer, little endian, 4-byte aligned ([`encode_resources`], [`decode_resources`]):
//!
//! ```text
//! header   12 B   magic "KTRS" | version u16 = 1 | resources u16 | entry count u32
//! entry           kind u8 | 0 u8 | 0 u16 | payload length u32 | payload, padded with zeros to 4 B
//!   1 icon        id u16 | path count u16 | fill count u16 | 0 u16 | grid f32
//!                 | (path count + fill count) x byte length u32 | UTF-8 SVG path data back to back
//!                 (paths are stroked; fills are filled and stroked)
//!   2 emoji       atlas u32 | size f32 | dpr f32 | width u32 | height u32 | left i32 | top i32
//!                 | straight RGBA8, width x height x 4 B (the tile of `emoji_tile`)
//!   3 image       key u32 | width u32 | height u32 | url byte length u32 | UTF-8 url
//! ```
//!
//! `resources` is the version the entries bring the page to. An image's url is the attachment's
//! (`kition-blob:<sha256>` for a file held in the document); the page loads and decodes it, and
//! applies the picture's orientation as the engine does. Its width and height are the picture's
//! as shown (after orientation), or 0 when the engine does not know them; the grid's layout uses
//! them for the thumbnail's proportions. The headless rasterizer reads [`Resources`] directly.
//! A table of another version is refused, as a list is.

mod codec;
pub mod emoji;
mod raster;
mod resources;
mod table;

pub use codec::{DecodeError, decode, encode};
pub use raster::{Tile, draw, emoji_tile, raster};
pub use resources::{Entry, ResourceTable, Resources};
pub use table::{decode_resources, encode_resources};

/// The first four bytes of every list.
pub const MAGIC: [u8; 4] = *b"KTDL";
/// The format version this crate writes and reads.
pub const VERSION: u16 = 1;
/// The header's size in bytes.
pub const HEADER_BYTES: usize = 56;
/// A hit record's size in bytes.
pub const HIT_BYTES: usize = 28;
/// The first four bytes of every resource table.
pub const RESOURCE_MAGIC: [u8; 4] = *b"KTRS";
/// The resource table format version this crate writes and reads.
pub const RESOURCE_VERSION: u16 = 1;

/// Which part of the grid a list draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Layer {
    /// Cells that scroll both ways.
    #[default]
    Body = 0,
    /// Cells of the frozen columns (scroll vertically only).
    Frozen = 1,
    /// The column headers that scroll horizontally.
    Header = 2,
    /// The column headers of the frozen columns.
    HeaderFrozen = 3,
    /// The row heads (numbers, controls, group toggles).
    RowHead = 4,
    /// A whole view at once, unscrolled: the picture's grid.
    View = 5,
}

impl Layer {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Body,
            1 => Self::Frozen,
            2 => Self::Header,
            3 => Self::HeaderFrozen,
            4 => Self::RowHead,
            5 => Self::View,
            _ => return None,
        })
    }
}

/// Which band a list draws: its layer, its column band, and its row band.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Key {
    pub layer: Layer,
    pub col_band: u8,
    pub row_band: u32,
}

/// A list's header (the counts are the sections' own lengths).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Header {
    /// Reserved, zero in v1.
    pub flags: u16,
    /// The arrangement the list was laid out for.
    pub epoch: u32,
    /// The theme it was drawn in (0 light, 1 dark).
    pub theme: u16,
    /// The version of the resource table its ids refer to.
    pub resources: u16,
    pub key: Key,
    /// Where the band starts and how large it is, in document CSS pixels.
    pub origin: (f32, f32),
    pub size: (f32, f32),
}

/// Where a text op's `x` is: the run's left end, its middle, or its right end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Align {
    #[default]
    Left = 0,
    Center = 1,
    Right = 2,
}

impl Align {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Left,
            1 => Self::Center,
            2 => Self::Right,
            _ => return None,
        })
    }

    /// The run's left end for an anchor at `x` and a run `width` wide.
    pub fn left(self, x: f32, width: f32) -> f32 {
        match self {
            Self::Left => x,
            Self::Center => x - width / 2.0,
            Self::Right => x - width,
        }
    }
}

/// Which way the lines of a [`Op::Lines`] run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Axis {
    /// Horizontal lines: `pos` is a y, `from` and `to` are xs.
    #[default]
    Horizontal = 0,
    /// Vertical lines: `pos` is an x, `from` and `to` are ys.
    Vertical = 1,
}

impl Axis {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Horizontal,
            1 => Self::Vertical,
            _ => return None,
        })
    }
}

/// How an image fills its box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Fit {
    /// Scaled to cover the box, centred, the rest cut off.
    #[default]
    Cover = 0,
    /// Scaled to fit inside the box, centred.
    Contain = 1,
}

impl Fit {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Cover,
            1 => Self::Contain,
            _ => return None,
        })
    }
}

/// The face a text op is set in. Each run of text is in one face: the engine chose it per
/// character (Inter, then Droid Sans Fallback, then Nanum Gothic), and characters none of them
/// covers are [`Op::Tofu`] boxes, so the page never falls back to a system font.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Face {
    /// Inter (variable: weight, and optical size set to the font size).
    #[default]
    Sans = 0,
    /// Droid Sans Fallback: Chinese and Japanese.
    Fallback = 1,
    /// Nanum Gothic: Korean.
    Hangul = 2,
    /// A monospace face.
    Mono = 3,
}

impl Face {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Sans,
            1 => Self::Fallback,
            2 => Self::Hangul,
            3 => Self::Mono,
            _ => return None,
        })
    }
}

/// A string of a list's string section: its byte offset and length.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Str {
    pub offset: u32,
    pub len: u16,
}

/// One line of an [`Op::Lines`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Line {
    pub pos: f32,
    pub from: f32,
    pub to: f32,
}

/// One drawing operation. Coordinates are document CSS pixels; `paint` is an index into the
/// list's palette. The record layouts (after the opcode byte) are the byte format's.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// `0x01` (20 B): op, flags (bit 0 snap), paint u16, x y w h. A filled rectangle; with
    /// `snap` its four edges go to device pixel edges.
    Rect { x: f32, y: f32, w: f32, h: f32, paint: u16, snap: bool },
    /// `0x02` (24 B): op, 0, paint u16, x y w h r. A filled rounded rectangle.
    RRect { x: f32, y: f32, w: f32, h: f32, r: f32, paint: u16 },
    /// `0x03` (28 B): op, 0, paint u16, x y w h r width. A rounded rectangle's border, `width`
    /// wide, inside the box.
    RRectStroke { x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, paint: u16 },
    /// `0x04` (8 + 12 n B): op, axis u8, paint u16, n u32, n x (pos from to). Hairlines one
    /// device pixel thick.
    Lines { axis: Axis, paint: u16, lines: Vec<Line> },
    /// `0x05` (32 B): op, align u8, paint u16, face u8, 0, weight u16, x y size width, string
    /// offset u32, string length u16, 0 u16. A run of text in one face on the alphabetic
    /// baseline `y`, already cut and wrapped; `width` is its advance as the engine measured it.
    Text { x: f32, y: f32, align: Align, face: Face, weight: u16, size: f32, paint: u16, text: Str, width: f32 },
    /// `0x06` (20 B): op, 0, paint u16, x y w h. A character no face covers: a box outline one
    /// device pixel thick, the same on both sides.
    Tofu { x: f32, y: f32, w: f32, h: f32, paint: u16 },
    /// `0x07` (20 B): op, 0, 0 u16, atlas id u32, x y size. An emoji the engine rasterizes: its
    /// pen position on the baseline and its font size.
    Emoji { atlas: u32, x: f32, y: f32, size: f32 },
    /// `0x08` (24 B): op, flags (bit 0 solid), paint u16, icon id u16, 0 u16, x y size stroke. A
    /// Tessera icon, `size` px square from its top left, its strokes `stroke` wide in the icon's
    /// 24-unit grid; a solid icon's stroked shapes are filled too (a rating's stars).
    Icon { icon: u16, x: f32, y: f32, size: f32, stroke: f32, paint: u16, solid: bool },
    /// `0x09` (28 B): op, fit u8, 0 u16, image key u32, x y w h radius. An attachment's picture,
    /// decoded by the player.
    Image { key: u32, x: f32, y: f32, w: f32, h: f32, fit: Fit, radius: f32 },
    /// `0x0A` (28 B): op, 0, paint u16, x y w h r blur. A soft shadow of a rounded box.
    Shadow { x: f32, y: f32, w: f32, h: f32, r: f32, blur: f32, paint: u16 },
    /// `0x0B` (20 B): op, 0, 0 u16, x y w h. Clips what follows to the box (device pixel edges)
    /// until the matching [`Op::Unclip`]. Only at region edges (a band's seam), never per cell.
    Clip { x: f32, y: f32, w: f32, h: f32 },
    /// `0x0C` (4 B): op, 0, 0 u16.
    Unclip,
}

impl Op {
    /// The opcode byte.
    pub fn opcode(&self) -> u8 {
        match self {
            Self::Rect { .. } => 0x01,
            Self::RRect { .. } => 0x02,
            Self::RRectStroke { .. } => 0x03,
            Self::Lines { .. } => 0x04,
            Self::Text { .. } => 0x05,
            Self::Tofu { .. } => 0x06,
            Self::Emoji { .. } => 0x07,
            Self::Icon { .. } => 0x08,
            Self::Image { .. } => 0x09,
            Self::Shadow { .. } => 0x0A,
            Self::Clip { .. } => 0x0B,
            Self::Unclip => 0x0C,
        }
    }

    /// The record's size in bytes.
    pub fn bytes(&self) -> usize {
        match self {
            Self::Rect { .. } | Self::Tofu { .. } | Self::Emoji { .. } | Self::Clip { .. } => 20,
            Self::RRect { .. } | Self::Icon { .. } => 24,
            Self::RRectStroke { .. } | Self::Image { .. } | Self::Shadow { .. } => 28,
            Self::Lines { lines, .. } => 8 + 12 * lines.len(),
            Self::Text { .. } => 32,
            Self::Unclip => 4,
        }
    }
}

/// What a clickable part of a cell or header is (the cell itself is hit through the axes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HitKind {
    #[default]
    Checkbox = 0,
    /// A rating's `index`-th symbol.
    Rating = 1,
    /// An attachment's `index`-th picture, or its add button (`part` 1).
    Attachment = 2,
    Button = 3,
    /// A link chip (`index`-th).
    Link = 4,
    /// A row head's expand control.
    Expand = 5,
    /// A row head's drag handle.
    DragHandle = 6,
    /// A group header's collapse toggle.
    GroupToggle = 7,
    /// A column header's menu caret.
    ColumnMenu = 8,
    /// A column's right edge (resize).
    ColumnEdge = 9,
    /// The frozen columns' handle.
    FreezeHandle = 10,
    /// The add-record row, the add-field column.
    AppendRow = 11,
    AppendColumn = 12,
}

impl HitKind {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Checkbox,
            1 => Self::Rating,
            2 => Self::Attachment,
            3 => Self::Button,
            4 => Self::Link,
            5 => Self::Expand,
            6 => Self::DragHandle,
            7 => Self::GroupToggle,
            8 => Self::ColumnMenu,
            9 => Self::ColumnEdge,
            10 => Self::FreezeHandle,
            11 => Self::AppendRow,
            12 => Self::AppendColumn,
            _ => return None,
        })
    }
}

/// A clickable part (28 B): kind u8, part u8, index u16, col u16, 0 u16, row u32, x y w h.
/// `row` is the linear row (records, group headers and add rows in display order) and `col` the
/// shown column; `u32::MAX` and `u16::MAX` stand for none.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    pub kind: HitKind,
    pub part: u8,
    pub index: u16,
    pub col: u16,
    pub row: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// A display list: a header, a palette, strings, operations in drawing order, and hit parts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayList {
    pub header: Header,
    /// Straight RGBA colors the ops' `paint` indexes.
    pub palette: Vec<[u8; 4]>,
    /// The text ops' strings, back to back.
    pub strings: String,
    pub ops: Vec<Op>,
    pub hits: Vec<Hit>,
}

impl DisplayList {
    pub fn new(header: Header) -> Self {
        Self { header, ..Self::default() }
    }

    /// The palette index of `rgba`, added on first use.
    pub fn paint(&mut self, rgba: [u8; 4]) -> u16 {
        if let Some(index) = self.palette.iter().position(|color| *color == rgba) {
            return u16::try_from(index).unwrap_or(u16::MAX);
        }
        self.palette.push(rgba);
        u16::try_from(self.palette.len() - 1).unwrap_or(u16::MAX)
    }

    /// Adds `text` to the string section (at most `u16::MAX` bytes, cut on a character boundary).
    pub fn string(&mut self, text: &str) -> Str {
        let mut end = text.len().min(usize::from(u16::MAX));
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let offset = u32::try_from(self.strings.len()).unwrap_or(u32::MAX);
        self.strings.push_str(&text[..end]);
        Str { offset, len: u16::try_from(end).unwrap_or(u16::MAX) }
    }

    /// The text of a string reference ("" for one outside the section).
    pub fn text(&self, text: Str) -> &str {
        let start = text.offset as usize;
        self.strings.get(start..start + usize::from(text.len)).unwrap_or_default()
    }

    /// The color of a paint index (transparent for an unknown one).
    pub fn color(&self, paint: u16) -> [u8; 4] {
        self.palette.get(usize::from(paint)).copied().unwrap_or([0, 0, 0, 0])
    }

    pub fn push(&mut self, op: Op) {
        self.ops.push(op);
    }

    /// The encoded size in bytes.
    pub fn bytes(&self) -> usize {
        HEADER_BYTES
            + 4 * self.palette.len()
            + self.strings.len().next_multiple_of(4)
            + self.ops.iter().map(Op::bytes).sum::<usize>()
            + HIT_BYTES * self.hits.len()
    }
}

/// The device pixel edge of the document coordinate `v`, for a canvas whose origin is `origin`
/// at `dpr` device pixels per CSS pixel: `round((v - origin) * dpr)`.
pub fn snap(v: f64, origin: f64, dpr: f64) -> f64 {
    ((v - origin) * dpr).round()
}

/// The device pixel row (or column) a line at `pos` covers: `floor((pos - origin) * dpr)`, with
/// a tolerance of 1/10000 device pixel for coordinates that are whole in exact arithmetic.
pub fn line_pixel(pos: f64, origin: f64, dpr: f64) -> f64 {
    ((pos - origin) * dpr + 1e-4).floor()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod raster_tests;

#[cfg(test)]
mod table_tests;
