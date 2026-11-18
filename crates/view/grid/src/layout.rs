//! A grid view's geometry and its whole-view display list: the header row (each field's type
//! icon, title and menu caret, the add-field column), the rows in display order (group headers,
//! records with their row heads, an add row after each group or after the records), the frozen
//! columns' divider, and the focus outline. Sizes are the editor's.

use kition_table_display::{Align, Axis, DisplayList, Header, HitKind, Key, Layer, Line, Op, Resources};
use kition_table_file::{Field, Record};
use serde_json::Value;

use crate::cells::{self, Area, Out, chip};
use crate::format::{choices, display, tone};
use crate::text::Shaper;
use crate::theme::{BODY, Mode, REGULAR, TINY, Theme};

/// The header row's height.
pub const HEADER: f32 = 32.0;
/// A group header row's height.
pub const GROUP_HEADER: f32 = 48.0;
/// The add-record row's height.
pub const APPEND_ROW: f32 = 32.0;
/// The add-field column's width.
pub const APPEND_COLUMN: f32 = 100.0;
/// A column's width when the view sets none.
pub const DEFAULT_WIDTH: f32 = 200.0;
/// One row head control's width (drag handle, checkbox, expand); a row head is at least two wide.
pub const ROW_CONTROL: f32 = 24.0;
/// Row heights by the view's `row_height`.
pub const ROW_HEIGHTS: [(&str, f32); 4] = [("short", 32.0), ("medium", 40.0), ("tall", 56.0), ("extra_tall", 88.0)];

/// The row height a view's `row_height` names (medium by default).
pub fn row_height(name: Option<&str>) -> f32 {
    ROW_HEIGHTS.iter().find(|(key, _)| Some(*key) == name).map_or(40.0, |(_, height)| *height)
}

/// A shown column: a field and its width in the view.
#[derive(Clone, Debug)]
pub struct Column {
    pub field: Field,
    pub width: f32,
}

/// A cell to outline: a field, a record, or both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Focus {
    pub field: Option<i64>,
    pub record: Option<i64>,
}

/// What a grid shows: its columns, its records in order, what they are grouped by, and how.
#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    pub columns: &'a [Column],
    pub records: &'a [Record],
    /// The fields the records are grouped by, outermost first (they are in group order).
    pub groups: &'a [Field],
    /// The number of the first record (records are numbered from 1).
    pub first: usize,
    pub row_height: f32,
    /// How many columns are frozen at the left.
    pub frozen: usize,
    /// Whether rows can be dragged (no sort and no groups): the row head has a drag handle.
    pub reorderable: bool,
    pub theme: Theme,
}

/// A row of the grid in display order.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A group's header: its depth (0 outermost), the field, its key, and how many records.
    Group { depth: usize, field: usize, record: usize, count: usize },
    /// A record (`index` into the view's records) and its number.
    Record { index: usize, number: usize },
    /// An add-record row.
    Append,
}

/// The geometry of a view.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub row_head: f32,
    /// Each column's left edge.
    pub lefts: Vec<f32>,
    /// The right edge of the frozen columns (the row head's when none is frozen).
    pub frozen_right: f32,
    /// The right edge of the last column; the add-field column follows.
    pub columns_right: f32,
    pub rows: Vec<(Row, f32, f32)>,
    /// The grid's own width (with the add-field column) and height (with the header row).
    pub width: f32,
    pub height: f32,
}

fn group_key(field: &Field, record: &Record) -> String {
    let text = display(record.values.get(&field.name));
    if text.is_empty() { "Empty".to_owned() } else { text }
}

/// The view's rows in display order: a header where a group starts, the records, and an add row
/// after each innermost group (or after all records without groups).
pub fn rows(view: &View<'_>) -> Vec<Row> {
    let depth = view.groups.len();
    let mut rows = Vec::with_capacity(view.records.len() + 2);
    let mut previous: Vec<String> = Vec::new();
    let mut number = 0;
    for (index, record) in view.records.iter().enumerate() {
        let keys: Vec<String> = view.groups.iter().map(|field| group_key(field, record)).collect();
        let changed = if previous.is_empty() { Some(0) } else { keys.iter().zip(&previous).position(|(key, before)| key != before) };
        if let Some(level) = changed.filter(|_| depth > 0) {
            if !previous.is_empty() {
                rows.push(Row::Append);
            }
            for (level, _) in view.groups.iter().enumerate().skip(level) {
                let count = view.records[index..]
                    .iter()
                    .take_while(|other| view.groups.iter().take(level + 1).zip(&keys).all(|(field, key)| &group_key(field, other) == key))
                    .count();
                rows.push(Row::Group { depth: level, field: level, record: index, count });
            }
            number = 0;
        }
        previous = keys;
        number += 1;
        let shown = if depth > 0 { number } else { view.first + index };
        rows.push(Row::Record { index, number: shown });
    }
    rows.push(Row::Append);
    rows
}

/// Lays the view's geometry out from `top`.
pub fn geometry(view: &View<'_>, top: f32) -> Geometry {
    let row_head = if view.reorderable { 3.0 } else { 2.0 } * ROW_CONTROL;
    let mut lefts = Vec::with_capacity(view.columns.len());
    let mut x = row_head;
    for column in view.columns {
        lefts.push(x);
        x += column.width;
    }
    let frozen_right = view.columns.iter().take(view.frozen).map(|column| column.width).sum::<f32>() + row_head;
    let mut y = top + HEADER;
    let rows = rows(view)
        .into_iter()
        .map(|row| {
            let height = match row {
                Row::Group { .. } => GROUP_HEADER,
                Row::Record { .. } => view.row_height,
                Row::Append => APPEND_ROW,
            };
            let placed = (row, y, height);
            y += height;
            placed
        })
        .collect();
    Geometry { row_head, lefts, frozen_right, columns_right: x, rows, width: x + APPEND_COLUMN, height: y - top }
}

/// A view laid out: its display list and resources, its size, and the focused cell's box.
#[derive(Clone, Debug)]
pub struct Grid {
    pub list: DisplayList,
    pub resources: Resources,
    pub geometry: Geometry,
    /// The width drawn (at least the grid's own).
    pub width: f32,
    pub focus: Option<Area>,
}

/// The whole view as one display list, from `top`, at least `width` wide, with `focus` outlined.
pub fn lay_out(view: &View<'_>, top: f32, width: f32, focus: Option<Focus>) -> Grid {
    let geometry = geometry(view, top);
    let width = width.max(geometry.width);
    let theme = view.theme;
    let mut list = DisplayList::new(Header {
        theme: match theme.mode {
            Mode::Light => 0,
            Mode::Dark => 1,
        },
        key: Key { layer: Layer::View, col_band: 0, row_band: 0 },
        origin: (0.0, top),
        size: (width, geometry.height),
        ..Header::default()
    });
    let mut resources = Resources::default();
    let mut shaper = Shaper::default();
    let mut out = Out { list: &mut list, resources: &mut resources, shaper: &mut shaper, theme };
    let g = &geometry;
    let bottom = top + g.height;
    let row_ids: Vec<u32> = (0..g.rows.len()).map(|index| u32::try_from(index).unwrap_or(u32::MAX)).collect();

    // Surfaces: the paper, the header row, the add-field column, group rows.
    out.rect(Area { x: 0.0, y: top, w: width, h: g.height }, theme.paper);
    out.rect(Area { x: 0.0, y: top, w: g.columns_right, h: HEADER }, theme.header);
    out.rect(Area { x: g.columns_right, y: top, w: APPEND_COLUMN, h: g.height }, theme.header);
    let groups = view.groups.len();
    for (row, y, height) in &g.rows {
        if let Row::Group { depth, .. } = row {
            let shades = &theme.groups[3 - groups.min(3)..];
            let color = shades.get(*depth).copied().unwrap_or(theme.groups[2]);
            out.rect(Area { x: 0.0, y: *y, w: g.columns_right, h: *height }, color);
        }
    }

    // Lines: each row's bottom, the header's, each column's right edge beside records and add
    // rows, the row head's, and the add-field column's left edge below the header.
    let mut across = vec![Line { pos: top + HEADER - 1.0, from: 0.0, to: g.width }];
    let mut down: Vec<Line> = Vec::new();
    let edges: Vec<f32> = std::iter::once(g.row_head).chain(g.lefts.iter().zip(view.columns).map(|(left, column)| left + column.width)).collect();
    for edge in &edges {
        down.push(Line { pos: edge - 1.0, from: top, to: top + HEADER });
    }
    for (row, y, height) in &g.rows {
        across.push(Line { pos: y + height - 1.0, from: 0.0, to: g.columns_right });
        match row {
            Row::Group { .. } => {}
            Row::Record { .. } | Row::Append => {
                for edge in &edges {
                    down.push(Line { pos: edge - 1.0, from: *y, to: y + height });
                }
            }
        }
    }
    let line = out.paint(theme.line);
    out.list.push(Op::Lines { axis: Axis::Horizontal, paint: line, lines: across });
    out.list.push(Op::Lines { axis: Axis::Vertical, paint: line, lines: merge(down) });
    if view.frozen > 0 && !view.columns.is_empty() {
        let divider = out.paint(theme.divider);
        out.list.push(Op::Lines { axis: Axis::Vertical, paint: divider, lines: vec![Line { pos: g.frozen_right - 1.0, from: top, to: bottom }] });
    }

    // The header row.
    let middle = top + HEADER / 2.0;
    let checkbox_center = if view.reorderable { g.row_head / 2.0 } else { ROW_CONTROL / 2.0 };
    cells::checkbox(&mut out, checkbox_center - 8.0, middle - 8.0, 16.0, false);
    for (index, (column, left)) in view.columns.iter().zip(&g.lefts).enumerate() {
        let col = u16::try_from(index).unwrap_or(u16::MAX);
        let mut x = left + 8.0;
        let mut room = column.width - 16.0 - 12.0 - 8.0;
        if column.field.is_primary {
            out.icon("lock", x, middle - 8.0, 16.0, theme.muted, false);
            x += 20.0;
            room -= 24.0;
        }
        cells::draw_field_icon(&mut out, &column.field, x, middle - 8.0, theme.muted);
        x += 20.0;
        room -= 16.0;
        let title = if column.field.title.is_empty() { &column.field.name } else { &column.field.title };
        out.text(title, x, middle, Align::Left, BODY, REGULAR, theme.text, room.max(0.0));
        let caret = Area { x: left + column.width - 8.0 - 12.0, y: middle - 6.0, w: 12.0, h: 12.0 };
        out.icon("chevron-down", caret.x, caret.y, 12.0, theme.muted, false);
        out.hit(HitKind::ColumnMenu, 0, col, u32::MAX, caret);
        out.hit(HitKind::ColumnEdge, 0, col, u32::MAX, Area { x: left + column.width - 3.0, y: top, w: 5.0, h: HEADER });
    }
    let add = Area { x: g.columns_right, y: top, w: APPEND_COLUMN, h: HEADER };
    out.icon("plus", add.x + (APPEND_COLUMN - 20.0) / 2.0, middle - 10.0, 20.0, theme.muted, false);
    out.hit(HitKind::AppendColumn, 0, u16::MAX, u32::MAX, add);

    // The rows.
    let mut focused = None;
    for ((row, y, height), id) in g.rows.iter().zip(row_ids) {
        let middle = y + height / 2.0;
        match row {
            Row::Group { depth, field, record, .. } => {
                group_header(&mut out, view, g, (*depth, *field, *record), *y, id);
            }
            Row::Record { index, number } => {
                let Some(record) = view.records.get(*index) else { continue };
                out.text(&number.to_string(), g.row_head / 2.0, middle, Align::Center, TINY, REGULAR, theme.muted, g.row_head);
                for (col, (column, left)) in view.columns.iter().zip(&g.lefts).enumerate() {
                    let area = Area { x: *left, y: *y, w: column.width, h: *height };
                    cells::cell(&mut out, &column.field, record.values.get(&column.field.name), area, u16::try_from(col).unwrap_or(u16::MAX), id);
                    if focused.is_none()
                        && focus.is_some_and(|focus| focus.record == Some(record.id) && focus.field.is_none_or(|field| field == column.field.id))
                    {
                        focused = Some(area);
                    }
                }
            }
            Row::Append => {
                out.icon("plus", g.row_head / 2.0 - 10.0, middle - 10.0, 20.0, theme.muted, false);
                out.hit(HitKind::AppendRow, 0, u16::MAX, id, Area { x: 0.0, y: *y, w: g.columns_right, h: *height });
            }
        }
    }
    if focused.is_none()
        && let Some(Focus { field: Some(field), record: None }) = focus
        && let Some((column, left)) = view.columns.iter().zip(&g.lefts).find(|(column, _)| column.field.id == field)
    {
        focused = Some(Area { x: *left, y: top, w: column.width, h: HEADER });
    }
    if let Some(area) = focused {
        out.border(area, 0.0, 2.0, theme.primary);
    }
    Grid { list, resources, geometry, width, focus: focused }
}

/// A group's header row: the toggle in the row head, then in the first column the field's title
/// and the group's value (chips for a choice field).
fn group_header(out: &mut Out<'_>, view: &View<'_>, g: &Geometry, (depth, field, record): (usize, usize, usize), y: f32, id: u32) {
    let theme = out.theme;
    let (Some(field), Some(record)) = (view.groups.get(field), view.records.get(record)) else { return };
    let toggle = Area { x: (g.row_head - 20.0) / 2.0 + depth as f32 * 8.0, y: y + (GROUP_HEADER - 20.0) / 2.0, w: 20.0, h: 20.0 };
    out.icon("chevron-down", toggle.x, toggle.y, 20.0, theme.muted, false);
    out.hit(HitKind::GroupToggle, 0, u16::MAX, id, toggle);
    let left = g.row_head + 8.0;
    let right = g.lefts.get(1).copied().unwrap_or(g.columns_right) - 8.0;
    let title = if field.title.is_empty() { &field.name } else { &field.title };
    out.text(title, left, y + 6.0 + BODY / 2.0, Align::Left, BODY, REGULAR, theme.muted, right - left);
    let middle = y + 18.0 + (GROUP_HEADER - 18.0) / 2.0;
    let value = record.values.get(&field.name);
    let items = choices(value);
    if matches!(field.field_type.as_str(), "single_select" | "multi_select") && !items.is_empty() {
        let mut x = left;
        for item in &items {
            let width = chip(out, x, middle, item, tone(field, item), right);
            if width == 0.0 {
                break;
            }
            x += width + 6.0;
        }
    } else {
        let text = crate::format::cell_text(field, value.filter(|value| !matches!(value, Value::Null)));
        let text = if text.is_empty() { "Empty".to_owned() } else { text };
        out.text(&text, left, middle, Align::Left, BODY, REGULAR, theme.text, right - left);
    }
}

/// Vertical lines at one position that meet end to end, as one line.
fn merge(mut lines: Vec<Line>) -> Vec<Line> {
    lines.sort_by(|a, b| a.pos.total_cmp(&b.pos).then(a.from.total_cmp(&b.from)));
    let mut out: Vec<Line> = Vec::with_capacity(lines.len());
    for line in lines {
        match out.last_mut() {
            Some(last) if last.pos == line.pos && line.from <= last.to => last.to = last.to.max(line.to),
            _ => out.push(line),
        }
    }
    out
}

/// The grid's width for `columns` (the row head, the columns and the add-field column).
pub fn width(columns: &[Column], reorderable: bool) -> f32 {
    (if reorderable { 3.0 } else { 2.0 }) * ROW_CONTROL + columns.iter().map(|column| column.width).sum::<f32>() + APPEND_COLUMN
}

/// The grid's height without laying it out.
pub fn height(view: &View<'_>) -> f32 {
    geometry(view, 0.0).height
}
