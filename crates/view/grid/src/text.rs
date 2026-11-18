//! Text as the grid lays it out: shaped per character's face into runs of one face each, cut with
//! an ellipsis, wrapped at line break opportunities (UAX #14), and written into a display list as
//! `TEXT` runs, `EMOJI` glyphs and `TOFU` boxes. The players never measure: the engine places
//! every run and cuts it 1 px short of its room, so a browser's slightly different measure stays
//! inside the cell.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use kition_table_display::{Align, DisplayList, Face, Op, Resources};
use kition_table_fonts::{FaceId, Family, Role};

/// The ellipsis a cut line ends with.
pub const ELLIPSIS: &str = "\u{2026}";
/// What a cut leaves free at a line's end, for players that measure a little wider.
pub const SLACK: f32 = 1.0;

/// What a piece of a line is drawn as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A run of one text face.
    Text(Face),
    /// Color glyphs of the emoji face.
    Emoji(FaceId),
    /// Characters no face covers.
    Tofu,
}

/// A run of a shaped line: its bytes of the line's text, where it starts and how wide it is, and
/// its glyphs' ids and pen positions (for emoji and tofu).
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub kind: Kind,
    pub range: Range<usize>,
    pub x: f32,
    pub width: f32,
    pub glyphs: Vec<(u32, f32, f32)>,
}

/// One line of text, shaped and measured.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub text: String,
    pub pieces: Vec<Piece>,
    pub width: f32,
}

impl Line {
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }
}

fn kind(role: Option<Role>, face: FaceId, tofu: bool) -> Kind {
    if tofu {
        return Kind::Tofu;
    }
    match role {
        Some(Role::Emoji) => Kind::Emoji(face),
        Some(Role::Fallback) => Kind::Text(Face::Fallback),
        Some(Role::Hangul) => Kind::Text(Face::Hangul),
        Some(Role::Mono) => Kind::Text(Face::Mono),
        _ => Kind::Text(Face::Sans),
    }
}

/// Shapes and measures text, remembering what it shaped (one table's cells repeat a lot).
#[derive(Default)]
pub struct Shaper {
    lines: HashMap<(String, u16, u32), Arc<Line>>,
}

/// The cache keeps at most this many lines before it starts again.
const CACHE_MAX: usize = 50_000;

impl Shaper {
    /// `text` on one line, as it is (line breaks and tabs as spaces).
    pub fn shape(&mut self, text: &str, weight: u16, size: f32) -> Arc<Line> {
        let key = (text.to_owned(), weight, size.to_bits());
        if let Some(found) = self.lines.get(&key) {
            return found.clone();
        }
        let flat = text.replace(['\n', '\r', '\t'], " ");
        let runs = kition_table_fonts::shape(&flat, Family::Sans, weight, size);
        let mut pieces: Vec<Piece> = Vec::with_capacity(runs.len());
        let mut x = 0.0;
        for (index, run) in runs.iter().enumerate() {
            let start = run.glyphs.iter().map(|glyph| glyph.cluster).min().unwrap_or(0);
            let end = runs.get(index + 1).and_then(|next| next.glyphs.iter().map(|glyph| glyph.cluster).min()).unwrap_or(flat.len());
            let scale = size / run.upem;
            let mut pen = 0.0;
            let mut glyphs = Vec::with_capacity(run.glyphs.len());
            for glyph in &run.glyphs {
                if glyph.advance > 0.0 || run.tofu {
                    glyphs.push((glyph.id, pen + glyph.x_offset * scale, glyph.advance * scale));
                }
                pen += glyph.advance * scale;
            }
            pieces.push(Piece {
                kind: kind(kition_table_fonts::role(run.face), run.face, run.tofu),
                range: start..end.max(start),
                x,
                width: pen,
                glyphs,
            });
            x += pen;
        }
        let line = Arc::new(Line { text: flat, pieces, width: x });
        if self.lines.len() >= CACHE_MAX {
            self.lines.clear();
        }
        self.lines.insert(key, line.clone());
        line
    }

    pub fn width(&mut self, text: &str, weight: u16, size: f32) -> f32 {
        self.shape(text, weight, size).width
    }

    /// `text` on one line, cut with an ellipsis to fit `max` px ([`SLACK`] short of it when cut;
    /// `f32::INFINITY`: no limit).
    pub fn line(&mut self, text: &str, weight: u16, size: f32, max: f32) -> Arc<Line> {
        let whole = self.shape(text, weight, size);
        if whole.width <= max {
            return whole;
        }
        let room = max - SLACK;
        let flat = whole.text.clone();
        let ends: Vec<usize> = flat.char_indices().map(|(at, _)| at).skip(1).chain([flat.len()]).collect();
        // The longest prefix that fits with its ellipsis (widths grow with the prefix).
        let fits = |shaper: &mut Self, end: usize| shaper.width(&cut(&flat, end), weight, size) <= room;
        let (mut low, mut high) = (0usize, ends.len());
        while low < high {
            let mid = (low + high).div_ceil(2);
            if fits(self, ends[mid - 1]) {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        let end = if low == 0 { 0 } else { ends[low - 1] };
        if end == 0 && !fits(self, 0) {
            return Arc::new(Line::default());
        }
        self.shape(&cut(&flat, end), weight, size)
    }

    /// `text` broken into at most `max_lines` lines of at most `max` px each, at line break
    /// opportunities (a word longer than a line breaks between characters); the last line is cut
    /// with an ellipsis when text is left over.
    pub fn lines(&mut self, text: &str, weight: u16, size: f32, max: f32, max_lines: usize) -> Vec<Arc<Line>> {
        let text = text.trim_end_matches(['\n', '\r']);
        if max_lines <= 1 {
            return vec![self.line(text, weight, size, max)];
        }
        let breaks: Vec<(usize, bool)> = unicode_linebreak::linebreaks(text)
            .map(|(at, opportunity)| (at, opportunity == unicode_linebreak::BreakOpportunity::Mandatory))
            .collect();
        let piece = |start: usize, end: usize| text.get(start..end).unwrap_or_default().trim_end_matches([' ', '\n', '\r', '\t']).to_owned();
        let mut out = Vec::new();
        let mut start = 0;
        while start < text.len() {
            if out.len() + 1 == max_lines {
                let rest = text.get(start..).unwrap_or_default();
                out.push(self.line(rest, weight, size, max));
                break;
            }
            // The furthest opportunity whose line fits, stopping at a mandatory break.
            let mut end = None;
            for &(at, mandatory) in breaks.iter().filter(|(at, _)| *at > start) {
                if self.width(&piece(start, at), weight, size) > max {
                    break;
                }
                end = Some(at);
                if mandatory {
                    break;
                }
            }
            let end = match end {
                Some(end) => end,
                None => {
                    // Nothing fits: as many characters as fit, at least one.
                    let chars: Vec<usize> = text[start..].char_indices().map(|(at, _)| start + at).skip(1).chain([text.len()]).collect();
                    let mut end = chars.first().copied().unwrap_or(text.len());
                    for at in chars {
                        if self.width(&piece(start, at), weight, size) > max {
                            break;
                        }
                        end = at;
                    }
                    end
                }
            };
            out.push(self.shape(&piece(start, end), weight, size));
            start = end;
        }
        if out.is_empty() {
            out.push(Arc::new(Line::default()));
        }
        out
    }
}

/// The first `end` bytes of `text` without trailing spaces, and an ellipsis.
fn cut(text: &str, end: usize) -> String {
    let mut out = text.get(..end).unwrap_or_default().trim_end().to_owned();
    out.push_str(ELLIPSIS);
    out
}

/// How a line is drawn: its anchor, its alphabetic baseline, its alignment, size, weight and
/// color (a palette index).
#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub x: f32,
    pub baseline: f32,
    pub align: Align,
    pub size: f32,
    pub weight: u16,
    pub paint: u16,
}

/// Writes `line` into `list`: a line of one text run is one `TEXT` op anchored as asked (the
/// player aligns it with the engine's width); a mixed line is placed run by run from its left.
pub fn emit(list: &mut DisplayList, resources: &mut Resources, line: &Line, place: Place) {
    let Place { x, baseline, align, size, weight, paint } = place;
    if let [piece] = line.pieces.as_slice()
        && let Kind::Text(face) = piece.kind
    {
        let text = list.string(&line.text[piece.range.clone()]);
        list.push(Op::Text { x, y: baseline, align, face, weight, size, paint, text, width: piece.width });
        return;
    }
    let left = align.left(x, line.width);
    for piece in &line.pieces {
        let at = left + piece.x;
        match piece.kind {
            Kind::Text(face) => {
                let text = list.string(&line.text[piece.range.clone()]);
                list.push(Op::Text { x: at, y: baseline, align: Align::Left, face, weight, size, paint, text, width: piece.width });
            }
            Kind::Emoji(face) => {
                for (glyph, gx, _) in &piece.glyphs {
                    let atlas = resources.emoji(face, *glyph);
                    list.push(Op::Emoji { atlas, x: at + gx, y: baseline, size });
                }
            }
            Kind::Tofu => {
                for (_, gx, advance) in &piece.glyphs {
                    let (w, h) = (advance * 0.75, size * 0.72);
                    list.push(Op::Tofu { x: at + gx + (advance - w) / 2.0, y: baseline - h, w, h, paint });
                }
            }
        }
    }
}

/// How far below a row's middle the baseline of `size` px text sits so the text looks centred,
/// as a browser centres a line box: half Inter's ascent minus its descent.
pub fn baseline_below_middle(size: f32) -> f32 {
    let metrics = kition_table_fonts::metrics(Family::Sans);
    (metrics.ascent - metrics.descent) / 2.0 * size / metrics.upem
}
