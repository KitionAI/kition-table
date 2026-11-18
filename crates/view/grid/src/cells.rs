//! One cell's value laid out by its field's type, as the editor's cells show it: text (wrapped in
//! tall rows), numbers and dates, choice chips in their tones, checkboxes, rating symbols, people,
//! attachments and buttons. Contents are centred in the cell, as the editor's grid aligns them.

use kition_table_display::{Align, DisplayList, Fit, Hit, HitKind, Op, Resources};
use kition_table_file::{Field, blobs};
use serde_json::Value;

use crate::format::{self, choices, display, tone};
use crate::text::{self, Place, Shaper, baseline_below_middle};
use crate::theme::{BODY, MEDIUM, REGULAR, Rgba, SEMIBOLD, SMALL, TINY, Theme};

/// A cell's inner padding.
pub const PAD: f32 = 8.0;
/// The line height of wrapped text, and the space above a cell's first line when it overflows.
pub const LINE_HEIGHT: f32 = 22.0;
pub const TEXT_TOP: f32 = 10.0;
const CHIP_HEIGHT: f32 = 20.0;
const CHIP_PAD: f32 = 8.0;
const CHIP_GAP: f32 = 6.0;
const CHIP_RADIUS: f32 = 6.0;
const CHECKBOX: f32 = 20.0;
const RATING: f32 = 16.0;
const RATING_GAP: f32 = 3.0;
const PERSON_HEIGHT: f32 = 24.0;
const AVATAR: f32 = 20.0;
const BUTTON_HEIGHT: f32 = 24.0;
const BUTTON_MIN: f32 = 60.0;
const BUTTON_MAX: f32 = 126.0;
const THUMB_GAP: f32 = 4.0;
const THUMB_INSET: f32 = 3.0;
/// A file chip: its paperclip, where its name starts, and its width beyond the name.
const FILE_ICON: f32 = 14.0;
const FILE_TEXT: f32 = 24.0;
const FILE_CHIP_EXTRA: f32 = 34.0;
/// A thumbnail's corner radius, and the "+N" badge's side padding.
const THUMB_RADIUS: f32 = 4.0;
const BADGE_PAD: f32 = 6.0;
/// Tessera's icon stroke, in its 24-unit grid.
const ICON_STROKE: f32 = 1.5;

/// A box in document pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    pub fn middle(self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn right(self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }
}

/// Where layout writes: the list, its resources, the shaper and the theme.
pub struct Out<'a> {
    pub list: &'a mut DisplayList,
    pub resources: &'a mut Resources,
    pub shaper: &'a mut Shaper,
    pub theme: Theme,
    /// Where dates are read (`Zone::default()`, UTC, for a picture).
    pub zone: &'a format::Zone,
}

impl Out<'_> {
    pub fn paint(&mut self, color: Rgba) -> u16 {
        self.list.paint(color)
    }

    pub fn rect(&mut self, area: Area, color: Rgba) {
        let paint = self.paint(color);
        self.list.push(Op::Rect { x: area.x, y: area.y, w: area.w, h: area.h, paint, snap: true });
    }

    pub fn rrect(&mut self, area: Area, r: f32, color: Rgba) {
        let paint = self.paint(color);
        self.list.push(Op::RRect { x: area.x, y: area.y, w: area.w, h: area.h, r, paint });
    }

    pub fn border(&mut self, area: Area, r: f32, width: f32, color: Rgba) {
        let paint = self.paint(color);
        self.list.push(Op::RRectStroke { x: area.x, y: area.y, w: area.w, h: area.h, r, width, paint });
    }

    /// A Tessera icon `size` px square from its top left (nothing for an unknown name).
    pub fn icon(&mut self, name: &str, x: f32, y: f32, size: f32, color: Rgba, solid: bool) {
        let Some(icon) = self.resources.icon(name) else { return };
        let paint = self.paint(color);
        self.list.push(Op::Icon { icon, x, y, size, stroke: ICON_STROKE, paint, solid });
    }

    pub fn hit(&mut self, kind: HitKind, index: u16, col: u16, row: u32, area: Area) {
        self.list.hits.push(Hit { kind, part: 0, index, col, row, x: area.x, y: area.y, w: area.w, h: area.h });
    }

    /// One line of text, cut to `max`, vertically centred on `middle` from the anchor `x`.
    /// Answers its width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, text: &str, x: f32, middle: f32, align: Align, size: f32, weight: u16, color: Rgba, max: f32) -> f32 {
        let line = self.shaper.line(text, weight, size, max);
        if line.is_empty() {
            return 0.0;
        }
        let paint = self.paint(color);
        let baseline = middle + baseline_below_middle(size);
        text::emit(self.list, self.resources, &line, Place { x, baseline, align, size, weight, paint });
        line.width
    }
}

/// A chip of `label` from `x`, centred on `middle`, at most `limit` far right; answers its width
/// (0 when there is no room).
pub fn chip(out: &mut Out<'_>, x: f32, middle: f32, label: &str, (background, color): (Rgba, Rgba), limit: f32) -> f32 {
    let room = limit - x - 2.0 * CHIP_PAD;
    if room < 8.0 {
        return 0.0;
    }
    let width = out.shaper.line(label, REGULAR, SMALL, room).width;
    if width <= 0.0 {
        return 0.0;
    }
    let chip_width = width + 2.0 * CHIP_PAD;
    out.rrect(Area { x, y: middle - CHIP_HEIGHT / 2.0, w: chip_width, h: CHIP_HEIGHT }, CHIP_RADIUS, background);
    out.text(label, x + CHIP_PAD, middle, Align::Left, SMALL, REGULAR, color, room);
    chip_width
}

/// The width of a row of chips for `labels`, at most `limit`.
fn chips_width(out: &mut Out<'_>, labels: &[String], limit: f32) -> f32 {
    let mut width = 0.0;
    for label in labels {
        width += out.shaper.width(label, REGULAR, SMALL) + 2.0 * CHIP_PAD + CHIP_GAP;
        if width > limit {
            return limit;
        }
    }
    (width - CHIP_GAP).max(0.0)
}

/// A checkbox `size` px square from its top left.
pub fn checkbox(out: &mut Out<'_>, x: f32, y: f32, size: f32, checked: bool) {
    let area = Area { x, y, w: size, h: size };
    if checked {
        let (primary, on) = (out.theme.primary, out.theme.on_primary);
        out.rrect(area, 4.0, primary);
        out.icon("check", x + size * 0.15, y + size * 0.15, size * 0.7, on, false);
    } else {
        let muted = out.theme.muted;
        out.border(area, 4.0, 1.0, muted);
    }
}

/// The text a person, attachment or link shows, one per item.
fn names(value: Option<&Value>) -> Vec<String> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(|item| display(Some(item))).filter(|text| !text.is_empty()).collect(),
        Some(other) => {
            let text = display(Some(other));
            if text.is_empty() { Vec::new() } else { vec![text] }
        }
    }
}

/// Whether an attachment item is a picture (its MIME type, else its name's extension).
pub fn is_picture(item: &Value) -> bool {
    let mime = ["mime_type", "mimeType", "type"].iter().find_map(|key| item.get(key).and_then(Value::as_str)).unwrap_or_default();
    if !mime.is_empty() {
        return mime.starts_with("image/");
    }
    let name = display(Some(item)).to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp", ".gif"].iter().any(|ext| name.ends_with(ext))
}

/// The url an attachment's file is loaded from, as the editor finds it: the item itself when it
/// is text, else its `url`, `src` or `href`, else the blob url of its `sha256`.
pub fn attachment_url(item: &Value) -> Option<String> {
    let text = |value: Option<&Value>| value.and_then(Value::as_str).map(str::trim).filter(|url| !url.is_empty()).map(str::to_owned);
    if let Some(url) = text(Some(item)).or_else(|| ["url", "src", "href"].iter().find_map(|key| text(item.get(key)))) {
        return Some(url);
    }
    ["sha256", "hash"].iter().find_map(|key| item.get(key).and_then(Value::as_str)).filter(|hash| blobs::is_blob_hash(hash)).map(blobs::blob_url)
}

/// The proportions (width over height) a field gives its pictures: an AI field's
/// `aspect_ratio` ("16:9"), as the editor reads it.
fn aspect_ratio(field: &Field) -> Option<f32> {
    let text = field.options.get("ai_config")?.get("aspect_ratio")?.as_str()?;
    let (width, height) = text.split_once(':')?;
    let (width, height) = (width.trim().parse::<f32>().ok()?, height.trim().parse::<f32>().ok()?);
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0).then_some(width / height)
}

/// One attachment of a cell: a picture's thumbnail (by its image key, when it has a url) or a
/// file's chip, and its width.
enum Piece<'v> {
    Picture { key: Option<u32>, width: f32 },
    File { item: &'v Value, width: f32 },
}

impl Piece<'_> {
    fn width(&self) -> f32 {
        match self {
            Self::Picture { width, .. } | Self::File { width, .. } => *width,
        }
    }
}

/// The "+N" badge's width.
fn badge_width(out: &mut Out<'_>, count: usize) -> f32 {
    out.shaper.width(&format!("+{count}"), MEDIUM, SMALL) + 2.0 * BADGE_PAD
}

/// An attachment cell, as the editor's: each picture a thumbnail as tall as the row allows (less
/// an inset), as wide as its proportions (the field's aspect ratio, else the picture's own, else
/// square) and filled centre-cropped; each other file a chip with its name. They are centred
/// when they fit; otherwise they start at the left and a "+N" badge counts the ones left out.
fn attachments(out: &mut Out<'_>, field: &Field, items: &[&Value], area: Area, inner: Area, col: u16, row: u32) {
    let theme = out.theme;
    let side = area.h - 2.0 * THUMB_INSET;
    let middle = area.middle();
    let fixed = aspect_ratio(field);
    let pieces: Vec<Piece<'_>> = items
        .iter()
        .map(|item| {
            if is_picture(item) {
                let key = attachment_url(item).map(|url| out.resources.image(&url));
                let own = key.and_then(|key| out.resources.image_size(key)).map(|(width, height)| width as f32 / height as f32);
                let ratio = fixed.or(own).unwrap_or(1.0);
                Piece::Picture { key, width: (side * ratio).min(inner.w) }
            } else {
                let name = display(Some(item));
                Piece::File { item, width: (out.shaper.width(&name, REGULAR, SMALL) + FILE_CHIP_EXTRA).min(inner.w) }
            }
        })
        .collect();
    let total = pieces.iter().map(|piece| piece.width() + THUMB_GAP).sum::<f32>() - THUMB_GAP;
    let fits = total <= inner.w;
    let mut x = if fits { inner.x + (inner.w - total) / 2.0 } else { inner.x };
    for (index, piece) in pieces.iter().enumerate() {
        let after = pieces.len() - index - 1;
        let mut room = inner.right() - x;
        if !fits && after > 0 {
            room -= THUMB_GAP + badge_width(out, after);
        }
        if index > 0 && piece.width() > room {
            badge(out, x, middle, pieces.len() - index);
            break;
        }
        let width = piece.width().min(room.max(side)).min(inner.right() - x);
        let at = match piece {
            Piece::Picture { key, .. } => {
                let at = Area { x, y: area.y + THUMB_INSET, w: width, h: side };
                match key {
                    Some(key) => out.list.push(Op::Image { key: *key, x: at.x, y: at.y, w: at.w, h: at.h, fit: Fit::Cover, radius: THUMB_RADIUS }),
                    None => out.rrect(at, THUMB_RADIUS, theme.chip),
                }
                out.border(at, THUMB_RADIUS, 1.0, theme.line);
                at
            }
            Piece::File { item, .. } => {
                let at = Area { x, y: middle - CHIP_HEIGHT / 2.0, w: width, h: CHIP_HEIGHT };
                if !file_chip(out, item, at) {
                    break;
                }
                at
            }
        };
        out.hit(HitKind::Attachment, u16::try_from(index).unwrap_or(u16::MAX), col, row, at);
        x = at.right() + THUMB_GAP;
    }
}

/// A "+`count`" badge from `x`, centred on `middle`.
fn badge(out: &mut Out<'_>, x: f32, middle: f32, count: usize) {
    let width = badge_width(out, count);
    let theme = out.theme;
    out.rrect(Area { x, y: middle - CHIP_HEIGHT / 2.0, w: width, h: CHIP_HEIGHT }, CHIP_RADIUS, theme.chip);
    out.text(&format!("+{count}"), x + width / 2.0, middle, Align::Center, SMALL, MEDIUM, theme.muted, width);
}

/// A file that is not a picture, in `at`: a chip with a paperclip and the file's name, cut to the
/// chip. Answers false when there is no room for it.
fn file_chip(out: &mut Out<'_>, item: &Value, at: Area) -> bool {
    if at.w < FILE_CHIP_EXTRA + 8.0 {
        return false;
    }
    let theme = out.theme;
    out.rrect(at, CHIP_RADIUS, theme.chip);
    out.icon("paperclip", at.x + 6.0, at.middle() - FILE_ICON / 2.0, FILE_ICON, theme.muted, false);
    out.text(&display(Some(item)), at.x + FILE_TEXT, at.middle(), Align::Left, SMALL, REGULAR, theme.text, at.w - FILE_CHIP_EXTRA);
    true
}

/// How many lines of text a row of `height` shows.
pub fn max_lines(height: f32) -> usize {
    (((height - TEXT_TOP) / LINE_HEIGHT).floor() as usize).max(1)
}

/// Lays `value` of `field` out in `area` (`col`, `row` for its hit parts).
pub fn cell(out: &mut Out<'_>, field: &Field, value: Option<&Value>, area: Area, col: u16, row: u32) {
    let inner = Area { x: area.x + PAD, y: area.y, w: area.w - 2.0 * PAD, h: area.h };
    if inner.w <= 4.0 {
        return;
    }
    let middle = area.middle();
    let start = |width: f32| inner.x + ((inner.w - width) / 2.0).max(0.0);
    let theme = out.theme;
    match field.field_type.as_str() {
        "checkbox" => {
            let at = Area { x: start(CHECKBOX), y: middle - CHECKBOX / 2.0, w: CHECKBOX, h: CHECKBOX };
            checkbox(out, at.x, at.y, CHECKBOX, value.is_some_and(format::truthy));
            out.hit(HitKind::Checkbox, 0, col, row, at);
        }
        "single_select" | "multi_select" => {
            let mut items = choices(value);
            if field.field_type == "single_select" {
                items.truncate(1);
            }
            let mut x = start(chips_width(out, &items, inner.w));
            for item in &items {
                let width = chip(out, x, middle, item, tone(field, item, &theme), inner.right());
                if width == 0.0 {
                    break;
                }
                x += width + CHIP_GAP;
            }
        }
        "rating" => {
            let max = field.options.get("max").and_then(Value::as_f64).filter(|max| *max > 0.0).map_or(5, |max| max.min(10.0) as usize);
            let rated = value.and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, max as f64).round() as usize;
            let symbol = match field.options.get("icon").and_then(Value::as_str) {
                Some("heart") => "heart",
                Some("thumbs" | "thumb" | "thumbs-up" | "thumbsUp") => "thumbs-up",
                _ => "star",
            };
            let width = max as f32 * RATING + (max as f32 - 1.0) * RATING_GAP;
            let x = start(width);
            for index in 0..max {
                let at = Area { x: x + index as f32 * (RATING + RATING_GAP), y: middle - RATING / 2.0, w: RATING, h: RATING };
                let color = if index < rated { theme.rating } else { theme.line };
                out.icon(symbol, at.x, at.y, RATING, color, true);
                out.hit(HitKind::Rating, u16::try_from(index).unwrap_or(u16::MAX), col, row, at);
            }
        }
        "user" | "created_by" | "last_modified_by" => {
            let people = names(value);
            let widths: Vec<f32> = people.iter().map(|name| out.shaper.width(name, REGULAR, BODY) + AVATAR + 12.0).collect();
            let total = widths.iter().map(|width| width + CHIP_GAP).sum::<f32>() - CHIP_GAP;
            let mut x = start(total.min(inner.w));
            for (name, width) in people.iter().zip(widths) {
                let width = width.min(inner.right() - x);
                if width < AVATAR + 16.0 {
                    break;
                }
                out.rrect(Area { x, y: middle - PERSON_HEIGHT / 2.0, w: width, h: PERSON_HEIGHT }, PERSON_HEIGHT / 2.0, theme.chip);
                out.rrect(Area { x: x + 2.0, y: middle - AVATAR / 2.0, w: AVATAR, h: AVATAR }, AVATAR / 2.0, theme.avatar);
                let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                out.text(&initial, x + 2.0 + AVATAR / 2.0, middle, Align::Center, TINY, SEMIBOLD, theme.text, AVATAR);
                out.text(name, x + AVATAR + 6.0, middle, Align::Left, BODY, REGULAR, theme.text, width - AVATAR - 12.0);
                x += width + CHIP_GAP;
            }
        }
        "attachment" => {
            let items: Vec<&Value> = match value {
                Some(Value::Array(items)) => items.iter().collect(),
                Some(Value::Null) | None => Vec::new(),
                Some(other) => vec![other],
            };
            attachments(out, field, &items, area, inner, col, row);
        }
        "button" => {
            let label = field.options.get("label").and_then(Value::as_str).filter(|label| !label.is_empty()).unwrap_or(&field.title);
            let width = (out.shaper.width(label, MEDIUM, SMALL) + 20.0).clamp(BUTTON_MIN, BUTTON_MAX).min(inner.w);
            let at = Area { x: start(width), y: middle - BUTTON_HEIGHT / 2.0, w: width, h: BUTTON_HEIGHT };
            out.rrect(at, 4.0, theme.accent);
            out.text(label, at.x + width / 2.0, middle, Align::Center, SMALL, MEDIUM, theme.primary, width - 16.0);
            out.hit(HitKind::Button, 0, col, row, at);
        }
        kind => {
            let text = format::cell_text_in(field, value, out.zone);
            if text.is_empty() {
                return;
            }
            let color = match kind {
                "link_to_record" | "document_link" => theme.primary,
                "url" => theme.url,
                _ => theme.text,
            };
            paragraph(out, &text, area, color);
        }
    }
}

/// Text in a cell: as many lines as the row shows, centred in it (from [`TEXT_TOP`] when they
/// overflow), each line centred across the cell.
fn paragraph(out: &mut Out<'_>, text: &str, area: Area, color: Rgba) {
    let lines = out.shaper.lines(text, REGULAR, BODY, area.w - 2.0 * PAD, max_lines(area.h));
    let block = BODY + (lines.len() as f32 - 1.0) * LINE_HEIGHT;
    let top = if block > area.h { TEXT_TOP } else { ((area.h - block) / 2.0).max(0.0) };
    let paint = out.paint(color);
    for (index, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let middle = area.y + top + BODY / 2.0 + index as f32 * LINE_HEIGHT;
        let baseline = middle + baseline_below_middle(BODY);
        let place = Place { x: area.x + area.w / 2.0, baseline, align: Align::Center, size: BODY, weight: REGULAR, paint };
        text::emit(out.list, out.resources, line, place);
    }
}

/// The Tessera icon of a field type, as the editor's `getFieldIcon` (`None`: text, which it
/// writes as "Aa").
pub fn field_icon(field: &Field) -> Option<&'static str> {
    Some(match field.field_type.as_str() {
        "number" | "auto_number" => "hash",
        "date" | "datetime" | "created_time" | "last_modified_time" => "calendar-days",
        "url" | "document_link" | "link_to_record" => "link-2",
        "checkbox" => "check",
        "long_text" => "file-text",
        "rating" => "star",
        "user" | "created_by" | "last_modified_by" => "user",
        "button" => "circle-dot",
        "lookup" | "rollup" => "workflow",
        "single_select" => "list",
        "multi_select" => "list-checks",
        "attachment" => "paperclip",
        "formula" => "sigma",
        "ai" => "sparkles",
        _ => return None,
    })
}

/// A field type's icon, 16 px from (`x`, `y`), or "Aa" for text.
pub fn draw_field_icon(out: &mut Out<'_>, field: &Field, x: f32, y: f32, color: Rgba) {
    match field_icon(field) {
        Some(name) => out.icon(name, x, y, 16.0, color, false),
        None => {
            out.text("Aa", x + 8.0, y + 8.0, Align::Center, 11.0, SEMIBOLD, color, 16.0);
        }
    }
}
