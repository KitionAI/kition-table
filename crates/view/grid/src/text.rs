//! Text as the grid lays it out: shaped per character's face into runs of one face each, cut with
//! an ellipsis, wrapped at line break opportunities (UAX #14), and written into a display list as
//! `TEXT` runs, `EMOJI` glyphs and `TOFU` boxes. The players never measure: the engine places
//! every run and cuts it 1 px short of its room, so a browser's slightly different measure stays
//! inside the cell.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use kition_table_display::{Align, DisplayList, Face, Op, Resources};
use kition_table_fonts::{FaceId, Family, OpticalSizing, Role};

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
/// its glyphs: id, pen position and advance (for emoji and tofu) and the byte of the line's text
/// the glyph's cluster starts at (for cutting).
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub kind: Kind,
    pub range: Range<usize>,
    pub x: f32,
    pub width: f32,
    pub glyphs: Vec<(u32, f32, f32, usize)>,
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

/// A bounded cache that keeps what was used lately: two generations, the current one and the one
/// before it. A hit in the older one moves the entry to the current one; when the current one is
/// full it becomes the older one and the older one goes. So an entry used once in the last
/// `GENERATION` insertions is always kept, and memory stays under three times that. The
/// generation that goes is freed a few entries per insertion, not at once (freeing tens of
/// thousands of lines at once stalled a band by tens of milliseconds).
struct Recent<K, V> {
    current: HashMap<K, V>,
    older: HashMap<K, V>,
    retiring: std::collections::hash_map::IntoIter<K, V>,
}

impl<K, V> Default for Recent<K, V> {
    fn default() -> Self {
        Self { current: HashMap::new(), older: HashMap::new(), retiring: HashMap::new().into_iter() }
    }
}

/// How many entries of a generation that went are freed per insertion.
const RETIRE_STEP: usize = 64;

/// The narrowest a character's advance is taken to be, in em: a text longer than a line could hold
/// at this width is cut without shaping all of it.
const NARROWEST: f32 = 0.15;

/// How far past its room (in em) the advances may put a prefix that still fits once shaped
/// (kerning and ligatures at the cut): a prefix further out is taken not to fit.
const CUT_MARGIN: f32 = 0.25;

/// How many entries a generation holds.
const GENERATION: usize = 64 * 1024;

impl<K: std::hash::Hash + Eq, V: Clone> Recent<K, V> {
    fn get(&mut self, key: &K) -> Option<V> {
        if let Some(found) = self.current.get(key) {
            return Some(found.clone());
        }
        let (key, value) = self.older.remove_entry(key)?;
        self.insert(key, value.clone());
        Some(value)
    }

    fn insert(&mut self, key: K, value: V) {
        for _ in 0..RETIRE_STEP {
            if self.retiring.next().is_none() {
                break;
            }
        }
        if self.current.len() >= GENERATION {
            let older = std::mem::replace(&mut self.older, std::mem::take(&mut self.current));
            // What is left of the generation before (none, at this pace) goes now.
            self.retiring = older.into_iter();
        }
        self.current.insert(key, value);
    }
}

/// Shapes and measures text, remembering what it shaped and how it cut it (one table's cells
/// repeat a lot, and a person scrolls back and forth).
///
/// A draft shaper ([`Shaper::draft`]) measures from the characters' advances only
/// (`kition_table_fonts::advances`: no kerning or ligatures) and leaves emoji out, several times
/// faster: for the grid's low-detail bands while its view jumps, which exact bands replace. It
/// remembers nothing (a jumping view rarely shows a text twice).
///
/// What it remembers was shaped with the faces registered then: a face registered later (the
/// editor's lazy fonts) drops it all, so text drawn as tofu gets its face.
///
/// It measures with one optical sizing ([`OpticalSizing`]; `Auto` unless set): the editor's grid
/// sets the one its page's Canvas draws with ([`Shaper::set_sizing`]), and every width the grid
/// derives from text (cuts with an ellipsis, wrapping, chips, badges, buttons) comes from it.
#[derive(Default)]
pub struct Shaper {
    draft: bool,
    sizing: OpticalSizing,
    lines: Recent<(String, u16, u32), Arc<Line>>,
    /// Lines cut to a width: (text, weight, size, width) to the line [`Shaper::line`] answered.
    cuts: Recent<(String, u16, u32, u32), Arc<Line>>,
    /// The faces' generation (`kition_table_fonts::generation`) what it remembers was shaped in.
    generation: u32,
}

impl Shaper {
    /// A shaper of drafts (see the type's documentation).
    pub fn draft() -> Self {
        Self { draft: true, ..Self::default() }
    }

    /// The optical sizing it measures with.
    pub fn sizing(&self) -> OpticalSizing {
        self.sizing
    }

    /// Measures with `sizing` from now on (what it remembers goes when it changes); answers
    /// whether it changed.
    pub fn set_sizing(&mut self, sizing: OpticalSizing) -> bool {
        if sizing == self.sizing {
            return false;
        }
        self.sizing = sizing;
        self.lines = Recent::default();
        self.cuts = Recent::default();
        true
    }

    /// `text` on one line, measured as this shaper measures (shaped, or from advances).
    fn measure(&self, text: &str, weight: u16, size: f32) -> Line {
        let mut line = shaped(text, weight, size, self.draft, self.sizing);
        if self.draft {
            for piece in line.pieces.iter_mut().filter(|piece| matches!(piece.kind, Kind::Emoji(_))) {
                piece.glyphs.clear();
            }
        }
        line
    }

    /// Forgets what was shaped with fewer faces than are registered now.
    fn follow_faces(&mut self) {
        let now = kition_table_fonts::generation();
        if now != self.generation {
            self.generation = now;
            self.lines = Recent::default();
            self.cuts = Recent::default();
        }
    }

    /// `text` on one line, as it is (line breaks and tabs as spaces).
    pub fn shape(&mut self, text: &str, weight: u16, size: f32) -> Arc<Line> {
        self.follow_faces();
        if self.draft {
            return Arc::new(self.measure(text, weight, size));
        }
        let key = (text.to_owned(), weight, size.to_bits());
        if let Some(found) = self.lines.get(&key) {
            return found;
        }
        let line = Arc::new(self.measure(text, weight, size));
        self.lines.insert(key, line.clone());
        line
    }

    pub fn width(&mut self, text: &str, weight: u16, size: f32) -> f32 {
        self.shape(text, weight, size).width
    }

    /// `text` on one line, cut with an ellipsis to fit `max` px ([`SLACK`] short of it when cut;
    /// `f32::INFINITY`: no limit).
    pub fn line(&mut self, text: &str, weight: u16, size: f32, max: f32) -> Arc<Line> {
        self.follow_faces();
        // A text longer than any line `max` wide can hold is cut: only its head is shaped.
        let most = if max.is_finite() { (max / (size * NARROWEST)).ceil() as usize + 8 } else { usize::MAX };
        if let Some((at, _)) = text.char_indices().nth(most) {
            let key = (text.to_owned(), weight, size.to_bits(), max.to_bits());
            if let Some(found) = self.cuts.get(&key) {
                return found;
            }
            let head = self.measure(&text[..at], weight, size);
            if head.width > max {
                let line = self.cut_to(&head, weight, size, max);
                self.remember(key, &line);
                return line;
            }
        }
        let whole = self.shape(text, weight, size);
        if whole.width <= max {
            return whole;
        }
        let key = (text.to_owned(), weight, size.to_bits(), max.to_bits());
        if let Some(found) = self.cuts.get(&key) {
            return found;
        }
        let line = self.cut_to(&whole, weight, size, max);
        self.remember(key, &line);
        line
    }

    /// Keeps a cut (a draft shaper keeps nothing).
    fn remember(&mut self, key: (String, u16, u32, u32), line: &Arc<Line>) {
        if !self.draft {
            self.lines.insert((line.text.clone(), key.1, key.2), line.clone());
            self.cuts.insert(key, line.clone());
        }
    }

    /// `whole` (wider than `max`; a whole line, or the head of a longer one) cut with an ellipsis
    /// to fit: the longest prefix that fits with its ellipsis. The glyphs' advances say about
    /// where the cut is; the candidates around it are shaped as they are drawn, and a candidate
    /// the advances put clearly past the room ([`CUT_MARGIN`]) is not shaped at all.
    fn cut_to(&mut self, whole: &Line, weight: u16, size: f32, max: f32) -> Arc<Line> {
        let room = max - SLACK;
        let flat = whole.text.clone();
        let chars: Vec<(usize, char)> = flat.char_indices().collect();
        let end = |count: usize| chars.get(count).map_or(flat.len(), |(at, _)| *at);
        let ellipsis = self.width(ELLIPSIS, weight, size);
        // How wide each prefix is with its ellipsis, near enough: the glyphs whose clusters start
        // in it (a ligature across the cut counts for neither side), trailing spaces left out as
        // the cut leaves them out.
        let mut stops: Vec<(usize, f32)> = whole
            .pieces
            .iter()
            .flat_map(|piece| piece.glyphs.iter().map(move |(_, x, advance, cluster)| (*cluster, piece.x + x + advance)))
            .collect();
        // Each glyph's cluster's end: where the next cluster starts (glyphs of one cluster share it).
        let (mut cluster_end, mut following) = (flat.len(), flat.len());
        for stop in stops.iter_mut().rev() {
            if stop.0 < following {
                cluster_end = following;
                following = stop.0;
            }
            stop.0 = cluster_end;
        }
        let mut wide = vec![ellipsis; chars.len() + 1];
        let (mut glyph, mut reach) = (0usize, 0.0f32);
        for count in 1..=chars.len() {
            while let Some(&(cluster_end, right)) = stops.get(glyph)
                && cluster_end <= end(count)
            {
                reach = right;
                glyph += 1;
            }
            wide[count] = if chars[count - 1].1.is_whitespace() { wide[count - 1] } else { reach + ellipsis };
        }
        let margin = CUT_MARGIN * size;
        let mut count = wide.iter().skip(1).take_while(|width| **width <= room).count();
        let mut kept: Option<Line> = None;
        // Longer while the next prefix may fit.
        while count < chars.len() && wide[count + 1] <= room + margin {
            let line = self.measure(&cut(&flat, end(count + 1)), weight, size);
            if line.width > room {
                break;
            }
            count += 1;
            kept = Some(line);
        }
        // Shorter until the prefix fits.
        while kept.is_none() && count > 0 {
            let line = self.measure(&cut(&flat, end(count)), weight, size);
            if line.width <= room {
                kept = Some(line);
            } else {
                count -= 1;
            }
        }
        let line = match kept {
            Some(line) => line,
            None => {
                let alone = self.measure(ELLIPSIS, weight, size);
                if alone.width > room {
                    return Arc::new(Line::default());
                }
                alone
            }
        };
        Arc::new(line)
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

/// `text` on one line (line breaks and tabs as spaces), shaped run by run in the face each
/// character falls to (or, for a draft, measured from the characters' advances), at `sizing`'s
/// optical size.
fn shaped(text: &str, weight: u16, size: f32, draft: bool, sizing: OpticalSizing) -> Line {
    let flat = text.replace(['\n', '\r', '\t'], " ");
    let runs = if draft {
        kition_table_fonts::advances(&flat, Family::Sans, weight, size, sizing)
    } else {
        kition_table_fonts::shape(&flat, Family::Sans, weight, size, sizing)
    };
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
                glyphs.push((glyph.id, pen + glyph.x_offset * scale, glyph.advance * scale, glyph.cluster));
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
    Line { text: flat, pieces, width: x }
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
                for (glyph, gx, _, _) in &piece.glyphs {
                    let atlas = resources.emoji(face, *glyph);
                    list.push(Op::Emoji { atlas, x: at + gx, y: baseline, size });
                }
            }
            Kind::Tofu => {
                for (_, gx, advance, _) in &piece.glyphs {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The longest prefix that fits, found by halving (what the cut searched before it started
    /// from the measured prefix).
    fn halving(shaper: &mut Shaper, text: &str, weight: u16, size: f32, max: f32) -> Arc<Line> {
        let whole = shaper.shape(text, weight, size);
        if whole.width <= max {
            return whole;
        }
        let (room, flat) = (max - SLACK, whole.text.clone());
        let ends: Vec<usize> = flat.char_indices().map(|(at, _)| at).skip(1).chain([flat.len()]).collect();
        let sizing = shaper.sizing();
        let fits = |end: usize| shaped(&cut(&flat, end), weight, size, false, sizing).width <= room;
        let (mut low, mut high) = (0usize, ends.len());
        while low < high {
            let mid = (low + high).div_ceil(2);
            if fits(ends[mid - 1]) {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        let end = if low == 0 { 0 } else { ends[low - 1] };
        if end == 0 && !fits(0) {
            return Arc::new(Line::default());
        }
        shaper.shape(&cut(&flat, end), weight, size)
    }

    #[test]
    fn a_cut_is_the_longest_prefix_that_fits() {
        crate::tests::fonts();
        let texts = [
            "A long title that does not fit into its column at all",
            "Typography -> => != 1/2 x2 office WAVE AVATAR",
            "季度计划 Q3 客户反馈 and more words after it",
            "서울 지사 회의록 with Latin after",
            "Launch day \u{1F680} \u{2705} with emoji in the middle of it",
            "iiiiiiiiiiiiiiiiiiii WWWWWWWWWWWWWWWWWWWW",
            "    spaces    between    words    ",
            "A note long enough to run far past any column: alpha beta gamma delta review shipping invoice customer launch budget design roadmap meeting draft final update report sprint backlog release vendor contract quarter region pipeline.\nSecond line.",
            "\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301}\u{0301} marks, then words that are wide",
        ];
        let mut shaper = Shaper::default();
        let mut reference = Shaper::default();
        for text in texts.iter().chain(&[
            "The office's fluffy affidavit, finally efficient",
            "Family \u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} and \u{1F44D}\u{1F3FD} together again",
            "1234567.89 + 98765.43 = 1333333.32 (USD)",
            "Mixed 中文 English 한국어 text, Ethiopic \u{1230}\u{120B}\u{121D} too",
        ]) {
            for (weight, size) in [(400, 13.0), (600, 13.0), (500, 12.0), (400, 14.0)] {
                for max in (4..260).step_by(3) {
                    let max = max as f32 + 0.25;
                    assert_eq!(
                        shaper.line(text, weight, size, max),
                        halving(&mut reference, text, weight, size, max),
                        "{text:?} {weight} {size} at {max} px"
                    );
                }
            }
        }
    }

    #[test]
    fn a_draft_cuts_to_its_room_and_leaves_emoji_out() {
        crate::tests::fonts();
        let mut draft = Shaper::draft();
        let line = draft.line("Launch day \u{1F680} with a long tail of words after it", 400, 13.0, 80.0);
        assert!(line.width <= 80.0 - SLACK && line.text.ends_with(ELLIPSIS), "{line:?}");
        let emoji = draft.line("\u{1F680} go", 400, 13.0, f32::INFINITY);
        assert!(emoji.pieces.iter().any(|piece| matches!(piece.kind, Kind::Emoji(_)) && piece.glyphs.is_empty() && piece.width > 0.0), "{emoji:?}");
        let mut list = DisplayList::default();
        emit(&mut list, &mut Resources::default(), &emoji, Place { x: 0.0, baseline: 10.0, align: Align::Left, size: 13.0, weight: 400, paint: 0 });
        assert!(list.ops.iter().all(|op| !matches!(op, Op::Emoji { .. })), "no emoji in a draft");
    }

    #[test]
    fn the_cache_keeps_what_was_used_lately() {
        let mut recent: Recent<u32, u32> = Recent::default();
        for key in 0..GENERATION as u32 {
            recent.insert(key, key);
        }
        // Used now, so it survives the next generation's turn.
        assert_eq!(recent.get(&7), Some(7));
        for key in 0..GENERATION as u32 {
            recent.insert(1_000_000 + key, key);
        }
        assert_eq!(recent.get(&7), Some(7), "used lately");
        assert_eq!(recent.get(&8), None, "not used for a generation");
        assert!(recent.current.len() + recent.older.len() <= 2 * GENERATION);
        // The generation that went is freed a step per insertion, not at once.
        let retiring = recent.retiring.len();
        assert!(retiring > RETIRE_STEP, "{retiring}");
        recent.insert(2_000_000, 0);
        assert_eq!(recent.retiring.len(), retiring - RETIRE_STEP);
    }
}
