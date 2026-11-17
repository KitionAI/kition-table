//! One value drawn by its field's type, as the editor's cells draw it: text, numbers and dates,
//! choice chips in their tones, checkboxes, rating stars, links to records, URLs, people,
//! attachments and buttons.

use kition_table_file::Field;
use kurbo::{BezPath, Point, Rect};
use serde_json::Value;

use crate::format::{self, choices, display, tone};
use crate::scene::{Rgba, Scene, shape};
use crate::theme::*;

/// Where a value sits in its box: centred, as the grid's cells, or from the left, as cards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}

/// The inner padding of a cell.
pub const PAD: f64 = 8.0;
const CHIP_HEIGHT: f64 = 20.0;
const CHIP_PAD: f64 = 8.0;
const CHIP_GAP: f64 = 6.0;
const CHIP_RADIUS: f64 = 6.0;

/// A chip of `text` with its left edge at `x`, centred on `middle`; answers its width. Nothing is
/// drawn when it would not start before `limit`.
pub fn chip(scene: &mut Scene, x: f64, middle: f64, text: &str, (background, color): (Rgba, Rgba), limit: f64) -> f64 {
    let room = (limit - x - 2.0 * CHIP_PAD).max(0.0);
    if room < 8.0 {
        return 0.0;
    }
    let line = shape(text, SMALL, MEDIUM, room);
    let width = line.width + 2.0 * CHIP_PAD;
    let top = middle - CHIP_HEIGHT / 2.0;
    scene.round_rect(Rect::new(x, top, x + width, top + CHIP_HEIGHT), CHIP_RADIUS, background);
    scene.text_mid(line, x + CHIP_PAD, middle, color);
    width
}

/// How wide a row of chips for `texts` is.
fn chips_width(texts: &[String], limit: f64) -> f64 {
    let mut width = 0.0;
    for text in texts {
        let line = shape(text, SMALL, MEDIUM, limit);
        width += line.width + 2.0 * CHIP_PAD + CHIP_GAP;
        if width > limit {
            return limit;
        }
    }
    (width - CHIP_GAP).max(0.0)
}

/// A five-pointed star `size` px across with its top left at `at`.
fn star(at: Point, size: f64) -> BezPath {
    let center = Point::new(at.x + size / 2.0, at.y + size / 2.0 + size * 0.04);
    let (outer, inner) = (size / 2.0, size / 2.0 * 0.45);
    let mut path = BezPath::new();
    for point in 0..10 {
        let radius = if point % 2 == 0 { outer } else { inner };
        let angle = -std::f64::consts::FRAC_PI_2 + f64::from(point) * std::f64::consts::PI / 5.0;
        let p = Point::new(center.x + radius * angle.cos(), center.y + radius * angle.sin());
        if point == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    path.close_path();
    path
}

/// A checkbox `size` px square at `at`.
pub fn checkbox(scene: &mut Scene, at: Point, size: f64, checked: bool) {
    let area = Rect::from_origin_size(at, (size, size));
    if checked {
        scene.round_rect(area, 4.0, PRIMARY);
        scene.icon("check", Point::new(at.x + 2.0, at.y + 2.0), size - 4.0, PAPER);
    } else {
        scene.border(area, 4.0, 1.5, ROW_NUMBER);
    }
}

/// The text a person, attachment or link shows.
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

/// Draws `value` of `field` in `area`.
pub fn draw(scene: &mut Scene, field: &Field, value: Option<&Value>, area: Rect, align: Align) {
    let middle = (area.y0 + area.y1) / 2.0;
    let inner = Rect::new(area.x0 + PAD, area.y0, area.x1 - PAD, area.y1);
    if inner.width() <= 4.0 {
        return;
    }
    let start = |width: f64| match align {
        Align::Left => inner.x0,
        Align::Center => inner.x0 + ((inner.width() - width) / 2.0).max(0.0),
    };
    match field.field_type.as_str() {
        "checkbox" => {
            let size = 18.0;
            checkbox(scene, Point::new(start(size), middle - size / 2.0), size, value.is_some_and(format::truthy));
        }
        "single_select" | "multi_select" => {
            let mut items = choices(value);
            if field.field_type == "single_select" {
                items.truncate(1);
            }
            let mut x = start(chips_width(&items, inner.width()));
            for item in &items {
                let width = chip(scene, x, middle, item, tone(field, item), inner.x1);
                if width == 0.0 {
                    break;
                }
                x += width + CHIP_GAP;
            }
        }
        "rating" => {
            let max = field.options.get("max").and_then(Value::as_f64).filter(|max| *max > 0.0).map_or(5, |max| max.min(10.0) as usize);
            let rated = value.and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, max as f64);
            let size = 15.0;
            let gap = 2.0;
            let width = max as f64 * size + (max as f64 - 1.0) * gap;
            let x = start(width);
            for index in 0..max {
                let color = if (index as f64) < rated.round() { STAR } else { STAR_OFF };
                scene.fill(star(Point::new(x + index as f64 * (size + gap), middle - size / 2.0), size), color);
            }
        }
        "user" | "created_by" | "last_modified_by" => {
            // Each person: a pill with an avatar (the name's initial) and the name.
            let people: Vec<(String, crate::scene::Line)> =
                names(value).into_iter().map(|name| (name.clone(), shape(&name, SMALL, REGULAR, (inner.width() - 36.0).max(0.0)))).collect();
            let total = people.iter().map(|(_, line)| line.width + 34.0 + CHIP_GAP).sum::<f64>() - CHIP_GAP;
            let mut x = start(total.min(inner.width()));
            for (name, line) in people {
                let width = line.width + 34.0;
                if line.glyphs.is_empty() || x + width > inner.x1 + 1.0 {
                    break;
                }
                let top = middle - 11.0;
                scene.round_rect(Rect::new(x, top, x + width, top + 22.0), 11.0, OPTION_BG);
                scene.circle(Point::new(x + 12.0, middle), 8.0, AVATAR_BG);
                let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
                let letter = shape(&initial, 10.0, SEMIBOLD, f64::INFINITY);
                let letter_x = x + 12.0 - letter.width / 2.0;
                scene.text_mid(letter, letter_x, middle, CELL_TEXT);
                scene.text_mid(line, x + 26.0, middle, CELL_TEXT);
                x += width + CHIP_GAP;
            }
        }
        "attachment" => {
            // Each file: a chip with a paperclip and the file's name.
            let files: Vec<crate::scene::Line> =
                names(value).iter().map(|name| shape(name, SMALL, REGULAR, (inner.width() - 2.0 * CHIP_PAD - 18.0).max(0.0))).collect();
            let total = files.iter().map(|line| line.width + 2.0 * CHIP_PAD + 18.0 + CHIP_GAP).sum::<f64>() - CHIP_GAP;
            let mut x = start(total.min(inner.width()));
            for line in files {
                let width = line.width + 2.0 * CHIP_PAD + 18.0;
                if line.glyphs.is_empty() || x + width > inner.x1 + 1.0 {
                    break;
                }
                let top = middle - CHIP_HEIGHT / 2.0;
                scene.round_rect(Rect::new(x, top, x + width, top + CHIP_HEIGHT), CHIP_RADIUS, MUTED_BG);
                scene.icon("paperclip", Point::new(x + 6.0, middle - 7.0), 14.0, ICON);
                scene.text_mid(line, x + CHIP_PAD + 16.0, middle, CELL_TEXT);
                x += width + CHIP_GAP;
            }
        }
        "button" => {
            let label = field.options.get("label").and_then(Value::as_str).filter(|label| !label.is_empty()).unwrap_or(&field.title);
            let line = shape(label, SMALL, MEDIUM, inner.width() - 20.0);
            let width = line.width + 20.0;
            let x = start(width);
            scene.round_rect(Rect::new(x, middle - 12.0, x + width, middle + 12.0), 6.0, PRIMARY_SOFT);
            scene.text_mid(line, x + 10.0, middle, PRIMARY);
        }
        kind => {
            let text = format::cell_text(field, value);
            if text.is_empty() {
                return;
            }
            let color = match kind {
                "link_to_record" | "document_link" => LINK,
                "url" => URL,
                _ => CELL_TEXT,
            };
            let line = shape(&text, BODY, REGULAR, inner.width());
            let x = start(line.width);
            scene.text_mid(line, x, middle, color);
        }
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
        // The editor writes "Aa" for these; the picture names them.
        "single_select" => "list",
        "multi_select" => "list-checks",
        "attachment" => "paperclip",
        "formula" => "sigma",
        "ai" => "sparkles",
        _ => return None,
    })
}

/// A field type's icon at `at` (16 px), or the editor's "Aa" for text.
pub fn draw_field_icon(scene: &mut Scene, field: &Field, at: Point, color: Rgba) {
    match field_icon(field) {
        Some(name) => scene.icon(name, at, 16.0, color),
        None => {
            let line = shape("Aa", 11.0, SEMIBOLD, f64::INFINITY);
            let x = at.x + (16.0 - line.width) / 2.0;
            scene.text_mid(line, x, at.y + 8.0, color);
        }
    }
}
