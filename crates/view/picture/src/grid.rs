//! A grid view: the header row (each field's type icon and title), group header rows, the records'
//! rows with their numbers and typed cells, and the footer with the record count.

use kition_table_file::{Field, Record};
use kurbo::{Point, Rect};
use serde_json::Value;

use crate::cell::{self, Align};
use crate::data::TableShown;
use crate::format::{choices, display, tone};
use crate::scene::{Scene, shape};
use crate::theme::*;

pub const ROW_HEAD: f64 = 56.0;
pub const HEADER: f64 = 36.0;
const GROUP_HEADER: f64 = 40.0;
const ADD_COLUMN: f64 = 44.0;
const FOOTER: f64 = 36.0;

/// A cell the picture outlines: a field, a record, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    pub field: Option<i64>,
    pub record: Option<i64>,
}

/// The grid's width.
pub fn width(shown: &TableShown) -> f64 {
    ROW_HEAD + shown.columns.iter().map(|column| column.width).sum::<f64>() + ADD_COLUMN
}

fn group_key(field: &Field, record: &Record) -> String {
    let text = display(record.values.get(&field.name));
    if text.is_empty() { "Empty".to_owned() } else { text }
}

/// Lays the grid out from `top`, `width` wide; answers its height and the focused cell's area.
pub fn draw(scene: &mut Scene, shown: &TableShown, top: f64, width: f64, focus: Option<Focus>) -> (f64, Option<Rect>) {
    let grid_width = self::width(shown);
    let mut lefts = Vec::with_capacity(shown.columns.len());
    let mut x = ROW_HEAD;
    for column in &shown.columns {
        lefts.push(x);
        x += column.width;
    }
    let mut focused = None;

    // The header row.
    scene.rect(Rect::new(0.0, top, width, top + HEADER), HEADER_BG);
    let middle = top + HEADER / 2.0;
    cell::checkbox(scene, Point::new(ROW_HEAD / 2.0 - 7.0, middle - 7.0), 14.0, false);
    for (column, left) in shown.columns.iter().zip(&lefts) {
        let right = left + column.width;
        cell::draw_field_icon(scene, &column.field, Point::new(left + 8.0, middle - 8.0), ICON);
        let title = if column.field.title.is_empty() { &column.field.name } else { &column.field.title };
        let line = shape(title, BODY, MEDIUM, (column.width - 30.0 - 22.0).max(0.0));
        scene.text_mid(line, left + 30.0, middle, CELL_TEXT);
        // The column menu's caret.
        let mut caret = kurbo::BezPath::new();
        caret.move_to((right - 18.0, middle - 2.0));
        caret.line_to((right - 10.0, middle - 2.0));
        caret.line_to((right - 14.0, middle + 2.5));
        caret.close_path();
        scene.fill(caret, ICON);
        scene.vline(right - 1.0, top, top + HEADER, CELL_LINE);
        if focus.is_some_and(|focus| focus.field == Some(column.field.id) && focus.record.is_none()) {
            focused = Some(Rect::new(*left, top, right, top + HEADER));
        }
    }
    scene.icon("plus", Point::new(x + (ADD_COLUMN - 16.0) / 2.0, middle - 8.0), 16.0, ICON);
    scene.vline(ROW_HEAD - 1.0, top, top + HEADER, CELL_LINE);
    scene.hline(0.0, width, top + HEADER - 1.0, CELL_LINE);

    // The rows, with a header before each group.
    let mut y = top + HEADER;
    let row = shown.row_height;
    let depth = shown.groups.len();
    let mut previous: Vec<String> = Vec::new();
    for (index, record) in shown.records.iter().enumerate() {
        let keys: Vec<String> = shown.groups.iter().map(|field| group_key(field, record)).collect();
        let changed = if previous.is_empty() { Some(0) } else { keys.iter().zip(&previous).position(|(key, before)| key != before) };
        if let Some(level) = changed.filter(|_| depth > 0) {
            for (level, field) in shown.groups.iter().enumerate().skip(level) {
                let count = shown
                    .records
                    .iter()
                    .skip(index)
                    .take_while(|other| shown.groups.iter().take(level + 1).zip(&keys).all(|(field, key)| &group_key(field, other) == key))
                    .count();
                group_header(scene, field, record.values.get(&field.name), level, depth, count, y, width);
                y += GROUP_HEADER;
            }
        }
        previous = keys;

        let bottom = y + row;
        let middle = y + row / 2.0;
        let number = shape(&(shown.first + index).to_string(), SMALL, REGULAR, f64::INFINITY);
        let number_x = (ROW_HEAD - number.width) / 2.0;
        scene.text_mid(number, number_x, middle, ROW_NUMBER);
        for (column, left) in shown.columns.iter().zip(&lefts) {
            let area = Rect::new(*left, y, left + column.width, bottom);
            cell::draw(scene, &column.field, record.values.get(&column.field.name), area, Align::Center);
            scene.vline(area.x1 - 1.0, y, bottom, CELL_LINE);
            if focus.is_some_and(|focus| focus.record == Some(record.id) && focus.field.is_none_or(|field| field == column.field.id))
                && focused.is_none()
            {
                focused = Some(area);
            }
        }
        scene.vline(ROW_HEAD - 1.0, y, bottom, CELL_LINE);
        scene.hline(0.0, grid_width.max(width), bottom - 1.0, CELL_LINE);
        y = bottom;
    }

    // The footer.
    scene.rect(Rect::new(0.0, y, width, y + FOOTER), PAPER);
    scene.icon("plus", Point::new(16.0, y + FOOTER / 2.0 - 8.0), 16.0, ICON);
    let count = if shown.windowed {
        format!("{}–{} of {} records", shown.first, shown.first + shown.records.len().saturating_sub(1), shown.shown)
    } else if shown.records.len() < shown.shown {
        format!("{} of {} records", shown.records.len(), shown.shown)
    } else if shown.shown == 1 {
        "1 record".to_owned()
    } else {
        format!("{} records", shown.shown)
    };
    let line = shape(&count, SMALL, REGULAR, f64::INFINITY);
    scene.text_mid(line, ROW_HEAD, y + FOOTER / 2.0, MUTED);
    y += FOOTER;

    if let Some(area) = focused {
        scene.border(area, 0.0, 2.0, PRIMARY);
    }
    (y - top, focused)
}

/// A group's header row: its field, its value and how many records it holds.
#[allow(clippy::too_many_arguments)]
fn group_header(scene: &mut Scene, field: &Field, value: Option<&Value>, level: usize, depth: usize, count: usize, y: f64, width: f64) {
    let background = GROUP_BG.get(depth.saturating_sub(level + 1)).copied().unwrap_or(GROUP_BG[0]);
    scene.rect(Rect::new(0.0, y, width, y + GROUP_HEADER), background);
    scene.hline(0.0, width, y + GROUP_HEADER - 1.0, CELL_LINE);
    let middle = y + GROUP_HEADER / 2.0;
    let mut x = 16.0 + level as f64 * 20.0;
    scene.icon("chevron-down", Point::new(x, middle - 7.0), 14.0, MUTED);
    x += 22.0;
    let title = if field.title.is_empty() { &field.name } else { &field.title };
    let label = shape(title, SMALL, REGULAR, 160.0);
    let label_width = label.width;
    scene.text_mid(label, x, middle, MUTED);
    x += label_width + 10.0;
    let items = choices(value);
    if matches!(field.field_type.as_str(), "single_select" | "multi_select") && !items.is_empty() {
        for item in &items {
            x += cell::chip(scene, x, middle, item, tone(field, item), width - 80.0) + 6.0;
        }
    } else {
        let text = crate::format::cell_text(field, value);
        let text = if text.is_empty() { "Empty".to_owned() } else { text };
        let line = shape(&text, BODY, SEMIBOLD, 320.0);
        let line_width = line.width;
        scene.text_mid(line, x, middle, CELL_TEXT);
        x += line_width + 10.0;
    }
    let count = shape(&count.to_string(), SMALL, MEDIUM, f64::INFINITY);
    let count_width = count.width + 14.0;
    scene.round_rect(Rect::new(x, middle - 10.0, x + count_width, middle + 10.0), 10.0, MUTED_BG);
    scene.text_mid(count, x + 7.0, middle, MUTED);
}
