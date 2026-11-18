//! The editor's grid in bands (grid plan sections 3.4, 3.7, 3.8): the extras `gridAxes`,
//! `gridBands`, `gridRaster` and `gridMeasure`, and the bands' timings in `gridStats`.
//!
//! The page arranges a view with `gridOpen` (`grid.rs`, `kition-table-arrange`) and hands the axis
//! it got to `gridAxes`, which lays out the arrangement's linear rows (group headers, records, add
//! rows) and the view's columns with the grid's geometry (`kition-table-grid`) and answers what the
//! page scrolls and hit-tests with: the rows' heights as runs, the columns, the header and row
//! head, and how the grid is cut into bands. `gridBands` then answers display lists
//! (`kition-table-display`, format v1) for the bands the page asks for, in one batch with the
//! icons and emoji tiles they use that the page does not hold yet:
//!
//! ```text
//! header  12 B  magic "KTDB" | version u16 = 1 | 0 u16 | item count u32
//! item          kind u8 | 0 u8 | 0 u16 | length u32 | payload, padded with zeros to 4 bytes
//!   1 band      a display list (format v1)
//!   2 icon      id u16 | 0 u16 | UTF-8 JSON { "grid", "paths", "fills" } (Tessera path data)
//!   3 emoji     atlas u32 | size f32 | dpr f32 | width u32 | height u32 | left i32 | top i32 | RGBA
//! ```
//!
//! A band is a layer (row heads, frozen columns, scrolling cells, and the header strips above
//! them), a run of `band_rows` rows and, for the scrolling layers, a strip `BAND_WIDTH` pixels wide.
//! Each band's ops are clipped to the band, so a cell across a strip's edge is cut there the same
//! way on both sides. Coordinates are the whole grid's, unscrolled (the picture's): the row heads
//! from x = 0, the header from y = 0. `gridRaster` rasterizes a band as the headless picture does,
//! for the parity test; `gridMeasure` answers how the grid's shaping splits and measures text,
//! for the width test.

use std::collections::{HashMap, HashSet};

use kition_table_arrange::{Arrangement, Axis as Arranged, GroupHeader, Stopwatch};
use kition_table_display::{Align, Axis, DisplayList, Header, HitKind, Key, Layer, Line, Op, Resources};
use kition_table_file::{Field, Record, TableFile};
use kition_table_grid::text::{Kind, Shaper};
use kition_table_grid::theme::{BODY, Mode, REGULAR, Rgba, TINY, Theme};
use kition_table_grid::{
    APPEND_COLUMN, APPEND_ROW, Area, Column, DEFAULT_WIDTH, GROUP_HEADER, HEADER, Out, ROW_CONTROL, cell, chip, field_icon, row_height,
};
use serde_json::{Value, json};

/// A column band's width, in CSS pixels.
pub const BAND_WIDTH: f32 = 2048.0;
/// About how tall a row band is: its row count follows the view's row height.
const BAND_HEIGHT: f32 = 1280.0;
/// The records the bands keep read (they repeat while a person scrolls back and forth).
const RECORD_CACHE: usize = 8192;
/// The batch format's version.
const BATCH_VERSION: u16 = 1;

/// A row of the grid in display order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    /// A group header: its index in the axis's groups.
    Group(usize),
    /// A record and its number.
    Record { id: i64, number: usize },
    /// An add-record row.
    Append,
}

/// An arrangement laid out: what `gridAxes` answered, kept for the bands.
struct Layout {
    epoch: u32,
    table_id: i64,
    theme: Theme,
    columns: Vec<Column>,
    lefts: Vec<f32>,
    frozen: usize,
    row_head: f32,
    frozen_right: f32,
    columns_right: f32,
    width: f32,
    /// The fields the records are grouped by, outermost first.
    group_fields: Vec<Field>,
    groups: Vec<GroupHeader>,
    rows: Vec<Row>,
    /// Each row's top (and the bottom of the last), from the header's bottom.
    tops: Vec<f32>,
    band_rows: usize,
}

/// How many band times `gridStats` keeps (the latest).
const TIMINGS_KEPT: usize = 4096;

/// The bands' timings, for the performance test.
#[derive(Default)]
struct Timings {
    /// Bands made so far, and the time of the latest `TIMINGS_KEPT` of them.
    count: u64,
    made: std::collections::VecDeque<f64>,
    bytes: u64,
}

/// The document the bands are read from, at its revision (the read records are of one revision).
#[derive(Clone, Copy)]
pub(crate) struct Source<'a> {
    pub file: &'a TableFile,
    pub revision: u64,
}

/// The document's grid in bands: its layout, its resources and caches.
#[derive(Default)]
pub(crate) struct Bands {
    layout: Option<Layout>,
    resources: Resources,
    shaper: Shaper,
    records: HashMap<i64, Record>,
    /// The document revision the cached records are of.
    revision: u64,
    timings: Timings,
}

fn hex(color: Rgba) -> String {
    if color[3] == 255 {
        format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
    } else {
        format!("rgba({},{},{},{:.4})", color[0], color[1], color[2], f32::from(color[3]) / 255.0)
    }
}

/// `#rrggbb` as a color.
fn parse_hex(text: &str) -> Option<Rgba> {
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8, 255])
}

/// Black or white, whichever reads on `color`.
fn on(color: Rgba) -> Rgba {
    let luminance = 0.2126 * f32::from(color[0]) + 0.7152 * f32::from(color[1]) + 0.0722 * f32::from(color[2]);
    if luminance > 150.0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }
}

/// The view's shown columns with their widths.
fn columns(file: &TableFile, fields: &[Field], view: Option<&kition_table_file::View>) -> Result<Vec<Column>, String> {
    let empty = serde_json::Map::new();
    let config = view.map_or(&empty, |view| &view.config);
    let layouts = match view {
        Some(view) => file.list_view_fields(view.id).map_err(|error| error.to_string())?,
        None => Vec::new(),
    };
    let hidden: Vec<&str> =
        config.get("hidden_field_names").and_then(Value::as_array).map(|names| names.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    Ok(fields
        .iter()
        .filter(|field| !hidden.contains(&field.name.as_str()))
        .filter(|field| layouts.iter().find(|layout| layout.field_id == field.id).is_none_or(|layout| layout.visible))
        .map(|field| {
            let width = layouts.iter().find(|layout| layout.field_id == field.id).map(|layout| layout.width).filter(|width| *width > 0);
            Column { field: field.clone(), width: width.map_or(DEFAULT_WIDTH, |width| width as f32) }
        })
        .collect())
}

/// The linear rows of an arrangement: a header where a group starts (nested ones after their
/// parent), the records of groups that are not collapsed, and an add row after each innermost
/// group (after all records without groups).
fn linear_rows(axis: &Arranged, depth: usize) -> Vec<Row> {
    let mut rows = Vec::with_capacity(axis.ids.len() + 2 * axis.groups.len() + 1);
    if depth == 0 || axis.groups.is_empty() {
        rows.extend(axis.ids.iter().enumerate().map(|(index, id)| Row::Record { id: *id, number: index + 1 }));
        rows.push(Row::Append);
        return rows;
    }
    let mut next_group = 0;
    // Records from here to `hidden_until` belong to a collapsed group.
    let mut hidden_until = 0usize;
    let mut number = 0;
    let mut innermost_end: Option<usize> = None;
    for (index, id) in axis.ids.iter().enumerate() {
        if innermost_end == Some(index) {
            rows.push(Row::Append);
            innermost_end = None;
        }
        while let Some(group) = axis.groups.get(next_group).filter(|group| group.at as usize == index) {
            if index >= hidden_until {
                rows.push(Row::Group(next_group));
                if group.collapsed {
                    hidden_until = index + group.count as usize;
                }
                if group.depth as usize + 1 >= depth {
                    number = 0;
                    if !group.collapsed {
                        innermost_end = Some(index + group.count as usize);
                    }
                }
            }
            next_group += 1;
        }
        if index >= hidden_until {
            number += 1;
            rows.push(Row::Record { id: *id, number });
        }
    }
    if innermost_end.is_some() {
        rows.push(Row::Append);
    }
    rows
}

/// A batch of items.
struct Batch(Vec<u8>, u32);

impl Batch {
    fn new() -> Self {
        let mut bytes = Vec::with_capacity(64 * 1024);
        bytes.extend_from_slice(b"KTDB");
        bytes.extend_from_slice(&BATCH_VERSION.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        Self(bytes, 0)
    }
    fn item(&mut self, kind: u8, parts: &[&[u8]]) {
        let length: usize = parts.iter().map(|part| part.len()).sum();
        self.0.extend_from_slice(&[kind, 0, 0, 0]);
        self.0.extend_from_slice(&u32::try_from(length).unwrap_or(u32::MAX).to_le_bytes());
        for part in parts {
            self.0.extend_from_slice(part);
        }
        self.0.resize(self.0.len().next_multiple_of(4), 0);
        self.1 += 1;
    }
    fn finish(mut self) -> Vec<u8> {
        self.0[8..12].copy_from_slice(&self.1.to_le_bytes());
        self.0
    }
}

impl Layout {
    fn row_top(&self, row: usize) -> f32 {
        HEADER + self.tops[row.min(self.rows.len())]
    }

    fn row_height(&self, row: usize) -> f32 {
        self.tops[row + 1] - self.tops[row]
    }

    fn row_bands(&self) -> usize {
        self.rows.len().div_ceil(self.band_rows)
    }

    /// The rows of a row band.
    fn band_rows(&self, band: u32) -> std::ops::Range<usize> {
        let first = (band as usize).saturating_mul(self.band_rows).min(self.rows.len());
        first..(first + self.band_rows).min(self.rows.len())
    }

    /// The document box of a band, or `None` for a band outside the grid.
    fn area(&self, key: Key) -> Option<Area> {
        let strip = || {
            let x = self.frozen_right + f32::from(key.col_band) * BAND_WIDTH;
            (x < self.width).then(|| (x, BAND_WIDTH.min(self.width - x)))
        };
        let rows = || {
            let range = self.band_rows(key.row_band);
            (!range.is_empty()).then(|| (self.row_top(range.start), self.row_top(range.end) - self.row_top(range.start)))
        };
        Some(match key.layer {
            Layer::Body => {
                let ((x, w), (y, h)) = (strip()?, rows()?);
                Area { x, y, w, h }
            }
            Layer::Frozen => {
                let (y, h) = rows()?;
                Area { x: self.row_head, y, w: self.frozen_right - self.row_head, h }
            }
            Layer::RowHead => {
                let (y, h) = rows()?;
                Area { x: 0.0, y, w: self.row_head, h }
            }
            Layer::Header => {
                let (x, w) = strip()?;
                Area { x, y: 0.0, w, h: HEADER }
            }
            Layer::HeaderFrozen => Area { x: 0.0, y: 0.0, w: self.frozen_right, h: HEADER },
            Layer::View => return None,
        })
    }

    /// The columns whose cells a layer's band at `area` draws (shown indexes).
    fn columns_in(&self, layer: Layer, area: Area) -> Vec<usize> {
        let frozen = matches!(layer, Layer::Frozen | Layer::HeaderFrozen);
        (0..self.columns.len())
            .filter(|index| (*index < self.frozen) == frozen)
            .filter(|index| self.lefts[*index] < area.right() && self.lefts[*index] + self.columns[*index].width > area.x)
            .collect()
    }
}

impl Bands {
    /// `gridAxes`: lays out `axis` (from `gridOpen` or `gridSync`) for the view `params` names
    /// (`{tableId, viewId?, theme?, brand?}`); answers the axes as JSON.
    pub(crate) fn axes(&mut self, file: &TableFile, axis: &[u8], params: &Value) -> Result<Value, String> {
        let axis = Arranged::decode(axis)?;
        let table_id = params.get("tableId").and_then(Value::as_i64).ok_or("invalid params: tableId is a table's id")?;
        let table = file.get_table(table_id).map_err(|_| format!("not found: table {table_id}"))?;
        let view = match params.get("viewId").and_then(Value::as_i64) {
            Some(id) => Some(table.views.iter().find(|view| view.id == id).ok_or_else(|| format!("not found: view {id} of table {table_id}"))?),
            None => None,
        };
        let mode = if params.get("theme").and_then(Value::as_str) == Some("dark") { Mode::Dark } else { Mode::Light };
        let mut theme = Theme::of(mode);
        if let Some(brand) = params.get("brand").and_then(Value::as_str).and_then(parse_hex) {
            theme = theme.with_brand(brand, on(brand));
        }
        let arrangement = Arrangement::of(file, &table.fields, view)?;
        let columns = columns(file, &table.fields, view)?;
        let empty = serde_json::Map::new();
        let config = view.map_or(&empty, |view| &view.config);
        let record_height = row_height(config.get("row_height").and_then(Value::as_str));
        // The editor freezes the first column unless the view says otherwise.
        let frozen = config.get("frozen_column_count").and_then(Value::as_u64).map_or(1, |count| count as usize).min(columns.len());
        let reorderable = arrangement.sorts.is_empty() && arrangement.groups.is_empty();
        let row_head = if reorderable { 3.0 } else { 2.0 } * ROW_CONTROL;
        let mut lefts = Vec::with_capacity(columns.len());
        let mut x = row_head;
        for column in &columns {
            lefts.push(x);
            x += column.width;
        }
        let columns_right = x;
        let frozen_right = row_head + columns.iter().take(frozen).map(|column| column.width).sum::<f32>();
        let group_fields: Vec<Field> = arrangement.groups.iter().map(|(field, _)| field.clone()).collect();
        let rows = linear_rows(&axis, group_fields.len());
        let mut tops = Vec::with_capacity(rows.len() + 1);
        let mut y = 0.0f32;
        let mut runs: Vec<(f32, u32)> = Vec::new();
        for row in &rows {
            tops.push(y);
            let height = match row {
                Row::Group(_) => GROUP_HEADER,
                Row::Record { .. } => record_height,
                Row::Append => APPEND_ROW,
            };
            match runs.last_mut() {
                Some((last, count)) if *last == height => *count += 1,
                _ => runs.push((height, 1)),
            }
            y += height;
        }
        tops.push(y);
        let band_rows = ((BAND_HEIGHT / record_height).round() as usize).clamp(8, 64);
        let mut answer_columns: Vec<Value> = columns
            .iter()
            .zip(&lefts)
            .enumerate()
            .map(|(index, (column, left))| json!({ "x": left, "width": column.width, "frozen": index < frozen, "fieldId": column.field.id }))
            .collect();
        answer_columns.push(json!({ "x": columns_right, "width": APPEND_COLUMN, "frozen": false, "fieldId": null }));
        let answer = json!({
            "epoch": axis.epoch,
            "rowRuns": runs,
            "columns": answer_columns,
            "headerHeight": HEADER,
            "rowHeadWidth": row_head,
            "bandRows": band_rows,
            "bandWidth": BAND_WIDTH,
            "skeleton": { "background": hex(theme.paper), "line": hex(theme.line) },
        });
        self.layout = Some(Layout {
            epoch: axis.epoch,
            table_id,
            theme,
            lefts,
            frozen,
            row_head,
            frozen_right,
            columns_right,
            width: columns_right + APPEND_COLUMN,
            columns,
            group_fields,
            groups: axis.groups,
            rows,
            tops,
            band_rows,
        });
        Ok(answer)
    }

    /// The records of `ids`, read once per document revision.
    fn read(&mut self, source: Source<'_>, table_id: i64, ids: &[i64]) -> Result<(), String> {
        if source.revision != self.revision || self.records.len() > RECORD_CACHE {
            self.records.clear();
            self.revision = source.revision;
        }
        for id in ids {
            if !self.records.contains_key(id) {
                let record = source.file.get_record(table_id, *id).map_err(|error| error.to_string())?;
                self.records.insert(*id, record);
            }
        }
        Ok(())
    }

    /// One band's list.
    fn band(&mut self, source: Source<'_>, key: Key) -> Result<Option<DisplayList>, String> {
        let layout = self.layout.as_ref().ok_or("no grid is laid out: call gridAxes first")?;
        let Some(area) = layout.area(key) else { return Ok(None) };
        let ids: Vec<i64> = match key.layer {
            Layer::Body | Layer::Frozen => layout.rows[layout.band_rows(key.row_band)]
                .iter()
                .filter_map(|row| match row {
                    Row::Record { id, .. } => Some(*id),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let table_id = layout.table_id;
        self.read(source, table_id, &ids)?;
        let Self { layout: Some(layout), resources, shaper, records, .. } = self else { return Ok(None) };
        let theme = layout.theme;
        let mut list = DisplayList::new(Header {
            epoch: layout.epoch,
            theme: match theme.mode {
                Mode::Light => 0,
                Mode::Dark => 1,
            },
            key,
            origin: (area.x, area.y),
            size: (area.w, area.h),
            ..Header::default()
        });
        let mut out = Out { list: &mut list, resources, shaper, theme };
        out.list.push(Op::Clip { x: area.x, y: area.y, w: area.w, h: area.h });
        match key.layer {
            Layer::Header | Layer::HeaderFrozen => header(&mut out, layout, key.layer, area),
            _ => rows(&mut out, layout, records, key, area),
        }
        out.list.push(Op::Unclip);
        list.header.resources = resources.version;
        Ok(Some(list))
    }
}

/// The header strip: each column's type icon, title and menu caret; the corner's checkbox; the
/// add-field column's plus.
fn header(out: &mut Out<'_>, layout: &Layout, layer: Layer, area: Area) {
    let theme = out.theme;
    out.rect(area, theme.header);
    let line = out.paint(theme.line);
    out.list.push(Op::Lines { axis: Axis::Horizontal, paint: line, lines: vec![Line { pos: HEADER - 1.0, from: area.x, to: area.right() }] });
    let mut down = Vec::new();
    if layer == Layer::HeaderFrozen {
        down.push(Line { pos: layout.row_head - 1.0, from: 0.0, to: HEADER });
    }
    let columns = layout.columns_in(layer, area);
    for index in &columns {
        down.push(Line { pos: layout.lefts[*index] + layout.columns[*index].width - 1.0, from: 0.0, to: HEADER });
    }
    out.list.push(Op::Lines { axis: Axis::Vertical, paint: line, lines: down });
    if layer == Layer::HeaderFrozen && layout.frozen > 0 {
        let divider = out.paint(theme.divider);
        out.list.push(Op::Lines {
            axis: Axis::Vertical,
            paint: divider,
            lines: vec![Line { pos: layout.frozen_right - 1.0, from: 0.0, to: HEADER }],
        });
    }
    let middle = HEADER / 2.0;
    if layer == Layer::HeaderFrozen {
        let center = if layout.row_head > 2.0 * ROW_CONTROL { layout.row_head / 2.0 } else { ROW_CONTROL / 2.0 };
        out.border(Area { x: center - 8.0, y: middle - 8.0, w: 16.0, h: 16.0 }, 4.0, 1.0, theme.muted);
    }
    for index in columns {
        let column = &layout.columns[index];
        let left = layout.lefts[index];
        let col = u16::try_from(index).unwrap_or(u16::MAX);
        let mut x = left + 8.0;
        let mut room = column.width - 16.0 - 12.0 - 8.0;
        if column.field.is_primary {
            out.icon("lock", x, middle - 8.0, 16.0, theme.muted, false);
            x += 20.0;
            room -= 24.0;
        }
        if let Some(icon) = field_icon(&column.field) {
            out.icon(icon, x, middle - 8.0, 16.0, theme.muted, false);
        }
        x += 20.0;
        room -= 16.0;
        let title = if column.field.title.is_empty() { &column.field.name } else { &column.field.title };
        out.text(title, x, middle, Align::Left, BODY, REGULAR, theme.text, room.max(0.0));
        let caret = Area { x: left + column.width - 8.0 - 12.0, y: middle - 6.0, w: 12.0, h: 12.0 };
        out.icon("chevron-down", caret.x, caret.y, 12.0, theme.muted, false);
        out.hit(HitKind::ColumnMenu, 0, col, u32::MAX, caret);
        out.hit(HitKind::ColumnEdge, 0, col, u32::MAX, Area { x: left + column.width - 3.0, y: 0.0, w: 5.0, h: HEADER });
    }
    if layer == Layer::Header && layout.columns_right < area.right() {
        let add = Area { x: layout.columns_right, y: 0.0, w: APPEND_COLUMN, h: HEADER };
        out.icon("plus", add.x + (APPEND_COLUMN - 20.0) / 2.0, middle - 10.0, 20.0, theme.muted, false);
        out.hit(HitKind::AppendColumn, 0, u16::MAX, u32::MAX, add);
    }
}

/// A row band of the row heads, the frozen columns or a strip of the scrolling ones.
fn rows(out: &mut Out<'_>, layout: &Layout, records: &HashMap<i64, Record>, key: Key, area: Area) {
    let theme = out.theme;
    let range = layout.band_rows(key.row_band);
    let columns = if key.layer == Layer::RowHead { Vec::new() } else { layout.columns_in(key.layer, area) };
    // Surfaces: the paper, the add-field column, group rows.
    out.rect(area, theme.paper);
    if key.layer == Layer::Body && layout.columns_right < area.right() {
        out.rect(Area { x: layout.columns_right, y: area.y, w: area.right() - layout.columns_right, h: area.h }, theme.header);
    }
    let depth = layout.group_fields.len();
    for row in range.clone() {
        if let Row::Group(group) = layout.rows[row] {
            let shades = &theme.groups[3 - depth.min(3)..];
            let level = layout.groups.get(group).map_or(0, |group| group.depth as usize);
            let color = shades.get(level).copied().unwrap_or(theme.groups[2]);
            let right = area.right().min(layout.columns_right);
            if right > area.x {
                out.rect(Area { x: area.x, y: layout.row_top(row), w: right - area.x, h: layout.row_height(row) }, color);
            }
        }
    }
    // Lines: each row's bottom, each column's right edge beside records and add rows, the row
    // head's right edge, the frozen columns' divider.
    let mut across = Vec::with_capacity(range.len());
    let mut down = Vec::new();
    let mut edges: Vec<f32> = columns.iter().map(|index| layout.lefts[*index] + layout.columns[*index].width).collect();
    if key.layer == Layer::RowHead {
        edges.push(layout.row_head);
    }
    let right = area.right().min(layout.columns_right);
    for row in range.clone() {
        let (y, height) = (layout.row_top(row), layout.row_height(row));
        if right > area.x {
            across.push(Line { pos: y + height - 1.0, from: area.x, to: right });
        }
        if !matches!(layout.rows[row], Row::Group(_)) {
            for edge in &edges {
                down.push(Line { pos: edge - 1.0, from: y, to: y + height });
            }
        }
    }
    let line = out.paint(theme.line);
    out.list.push(Op::Lines { axis: Axis::Horizontal, paint: line, lines: across });
    out.list.push(Op::Lines { axis: Axis::Vertical, paint: line, lines: merge(down) });
    if key.layer == Layer::Frozen && layout.frozen > 0 {
        let divider = out.paint(theme.divider);
        out.list.push(Op::Lines {
            axis: Axis::Vertical,
            paint: divider,
            lines: vec![Line { pos: layout.frozen_right - 1.0, from: area.y, to: area.bottom() }],
        });
    }
    // The group header's title and value sit in the first column.
    let titles = if layout.frozen > 0 { key.layer == Layer::Frozen } else { key.layer == Layer::Body && key.col_band == 0 };
    for row in range {
        let (y, height) = (layout.row_top(row), layout.row_height(row));
        let id = u32::try_from(row).unwrap_or(u32::MAX);
        let middle = y + height / 2.0;
        match layout.rows[row] {
            Row::Group(group) => {
                let Some(header) = layout.groups.get(group) else { continue };
                if key.layer == Layer::RowHead {
                    let toggle =
                        Area { x: (layout.row_head - 20.0) / 2.0 + header.depth as f32 * 8.0, y: y + (GROUP_HEADER - 20.0) / 2.0, w: 20.0, h: 20.0 };
                    out.icon(if header.collapsed { "chevron-right" } else { "chevron-down" }, toggle.x, toggle.y, 20.0, theme.muted, false);
                    out.hit(HitKind::GroupToggle, 0, u16::MAX, id, toggle);
                } else if titles {
                    group_title(out, layout, header, y);
                }
            }
            Row::Record { id: record_id, number } => {
                if key.layer == Layer::RowHead {
                    out.text(&number.to_string(), layout.row_head / 2.0, middle, Align::Center, TINY, REGULAR, theme.muted, layout.row_head);
                    continue;
                }
                let Some(record) = records.get(&record_id) else { continue };
                for index in &columns {
                    let column = &layout.columns[*index];
                    let cell_area = Area { x: layout.lefts[*index], y, w: column.width, h: height };
                    cell(out, &column.field, record.values.get(&column.field.name), cell_area, u16::try_from(*index).unwrap_or(u16::MAX), id);
                }
            }
            Row::Append => {
                if key.layer == Layer::RowHead {
                    out.icon("plus", layout.row_head / 2.0 - 10.0, middle - 10.0, 20.0, theme.muted, false);
                    out.hit(HitKind::AppendRow, 0, u16::MAX, id, Area { x: 0.0, y, w: layout.columns_right, h: height });
                }
            }
        }
    }
}

/// A group header's field title and value (chips for a choice field), in the first column.
fn group_title(out: &mut Out<'_>, layout: &Layout, header: &GroupHeader, y: f32) {
    let theme = out.theme;
    let Some(field) = layout.group_fields.get(header.depth as usize) else { return };
    let left = layout.row_head + 8.0;
    let right = layout.lefts.get(1).copied().unwrap_or(layout.columns_right) - 8.0;
    let title = if field.title.is_empty() { &field.name } else { &field.title };
    out.text(title, left, y + 6.0 + BODY / 2.0, Align::Left, BODY, REGULAR, theme.muted, right - left);
    let middle = y + 18.0 + (GROUP_HEADER - 18.0) / 2.0;
    let items = kition_table_grid::format::choices(Some(&header.value));
    if matches!(field.field_type.as_str(), "single_select" | "multi_select") && !items.is_empty() {
        let mut x = left;
        for item in &items {
            let width = chip(out, x, middle, item, kition_table_grid::format::tone(field, item), right);
            if width == 0.0 {
                break;
            }
            x += width + 6.0;
        }
    } else {
        let text = kition_table_grid::format::cell_text(field, Some(&header.value).filter(|value| !value.is_null()));
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

/// A layer by its byte in a band key.
fn layer(value: u32) -> Option<Layer> {
    Some(match value {
        0 => Layer::Body,
        1 => Layer::Frozen,
        2 => Layer::Header,
        3 => Layer::HeaderFrozen,
        4 => Layer::RowHead,
        _ => return None,
    })
}

/// An emoji tile's key as the page writes it (`atlas:size:dpr`, numbers as JavaScript prints them).
fn tile_key(atlas: u32, size: f32, dpr: f64) -> String {
    format!("{atlas}:{}:{dpr}", f64::from(size))
}

impl Bands {
    /// `gridBands`: the bands `keys` (layer, column band, row band triples) of arrangement
    /// `epoch`, the icons from id `icons` on, and the emoji tiles at `dpr` they use that are not
    /// in `tiles`, as one batch. A band outside the grid is left out.
    pub(crate) fn bands(&mut self, source: Source<'_>, epoch: u32, keys: &[u32], dpr: f64, icons: u32, tiles: &[String]) -> Result<Vec<u8>, String> {
        let laid_out = self.layout.as_ref().ok_or("no grid is laid out: call gridAxes first")?.epoch;
        if laid_out != epoch {
            return Err(format!("the grid is laid out for arrangement {laid_out}, not {epoch}"));
        }
        let mut batch = Batch::new();
        let mut known: HashSet<String> = tiles.iter().cloned().collect();
        let mut wanted: Vec<(u32, f32)> = Vec::new();
        for key in keys.as_chunks::<3>().0 {
            let (Some(layer), Ok(col_band)) = (layer(key[0]), u8::try_from(key[1])) else { return Err(format!("not a band key: {key:?}")) };
            let watch = Stopwatch::start();
            let Some(list) = self.band(source, Key { layer, col_band, row_band: key[2] })? else { continue };
            let bytes = kition_table_display::encode(&list);
            if self.timings.made.len() == TIMINGS_KEPT {
                self.timings.made.pop_front();
            }
            self.timings.made.push_back(watch.elapsed_ms());
            self.timings.count += 1;
            self.timings.bytes += bytes.len() as u64;
            for op in &list.ops {
                if let Op::Emoji { atlas, size, .. } = op
                    && known.insert(tile_key(*atlas, *size, dpr))
                {
                    wanted.push((*atlas, *size));
                }
            }
            batch.item(1, &[&bytes]);
        }
        let mut id = icons;
        while let Some(icon) = u16::try_from(id).ok().and_then(|id| self.resources.icon_of(id)) {
            let data = json!({ "grid": kition_tessera_icons::GRID, "paths": icon.paths, "fills": icon.fills }).to_string();
            batch.item(2, &[&u16::try_from(id).unwrap_or(u16::MAX).to_le_bytes(), &[0, 0], data.as_bytes()]);
            id += 1;
        }
        for (atlas, size) in wanted {
            if let Some(tile) = kition_table_display::emoji_tile(&self.resources, atlas, size, dpr) {
                let mut head = Vec::with_capacity(28);
                head.extend_from_slice(&atlas.to_le_bytes());
                head.extend_from_slice(&size.to_le_bytes());
                head.extend_from_slice(&(dpr as f32).to_le_bytes());
                for value in [tile.width, tile.height] {
                    head.extend_from_slice(&value.to_le_bytes());
                }
                for value in [tile.left, tile.top] {
                    head.extend_from_slice(&value.to_le_bytes());
                }
                batch.item(3, &[&head, &tile.rgba]);
            }
        }
        Ok(batch.finish())
    }

    /// `gridRaster`: one band rasterized as the headless picture draws it, `width` and `height`
    /// (u32 each) and then straight RGBA8, `round(size * dpr)` device pixels.
    pub(crate) fn raster(&mut self, source: Source<'_>, epoch: u32, key: [u32; 3], dpr: f64) -> Result<Vec<u8>, String> {
        let laid_out = self.layout.as_ref().ok_or("no grid is laid out: call gridAxes first")?.epoch;
        if laid_out != epoch {
            return Err(format!("the grid is laid out for arrangement {laid_out}, not {epoch}"));
        }
        let (Some(layer), Ok(col_band)) = (layer(key[0]), u8::try_from(key[1])) else { return Err(format!("not a band key: {key:?}")) };
        let list = self.band(source, Key { layer, col_band, row_band: key[2] })?.ok_or("the band is outside the grid")?;
        let (x, y) = (f64::from(list.header.origin.0), f64::from(list.header.origin.1));
        let width = (f64::from(list.header.size.0) * dpr).round() as u32;
        let height = (f64::from(list.header.size.1) * dpr).round() as u32;
        let rgba = kition_table_display::raster(&list, &self.resources, x, y, dpr, width, height);
        let mut out = Vec::with_capacity(8 + rgba.len());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&rgba);
        Ok(out)
    }

    /// `gridMeasure`: how the grid's shaping splits `[{text, weight, size}]` into runs of one face
    /// and how wide each is: `[[{face, text, width}]]` (emoji and tofu runs left out).
    pub(crate) fn measure(&mut self, asks: &Value) -> Result<Value, String> {
        let asks = asks.as_array().ok_or("invalid params: a list of {text, weight, size}")?;
        let mut answer = Vec::with_capacity(asks.len());
        for ask in asks {
            let text = ask.get("text").and_then(Value::as_str).ok_or("invalid params: text")?;
            let weight = ask.get("weight").and_then(Value::as_u64).and_then(|weight| u16::try_from(weight).ok()).unwrap_or(REGULAR);
            let size = ask.get("size").and_then(Value::as_f64).unwrap_or(f64::from(BODY)) as f32;
            let line = self.shaper.shape(text, weight, size);
            let runs: Vec<Value> = line
                .pieces
                .iter()
                .filter_map(|piece| match piece.kind {
                    Kind::Text(face) => Some(json!({ "face": face as u8, "text": &line.text[piece.range.clone()], "width": piece.width })),
                    _ => None,
                })
                .collect();
            answer.push(Value::Array(runs));
        }
        Ok(Value::Array(answer))
    }

    /// The bands' part of `gridStats`: how many were made, the milliseconds of the latest ones
    /// (oldest first), the bytes answered, and the row bands of the layout.
    pub(crate) fn stats(&self) -> Value {
        json!({
            "count": self.timings.count,
            "ms": self.timings.made,
            "bytes": self.timings.bytes,
            "rowBands": self.layout.as_ref().map_or(0, Layout::row_bands),
        })
    }
}

impl crate::TableDocument {
    /// Lays out the editor's arrangement for its grid (`gridAxes`): `axis` is what `gridOpen` or
    /// `gridSync` answered, `params` `{tableId, viewId?, theme?, brand?}`; answers the axes.
    pub fn grid_axes(&mut self, axis: &[u8], params: &Value) -> Result<Value, String> {
        self.bands.axes(&self.file, axis, params)
    }

    /// The grid's bands as one batch (`gridBands`, see the module's documentation).
    pub fn grid_bands(&mut self, epoch: u32, keys: &[u32], dpr: f64, icons: u32, tiles: &[String]) -> Result<Vec<u8>, String> {
        self.bands.bands(Source { file: &self.file, revision: self.revision }, epoch, keys, dpr, icons, tiles)
    }

    /// One band rasterized (`gridRaster`): width and height (u32 each), then RGBA8.
    pub fn grid_raster(&mut self, epoch: u32, key: [u32; 3], dpr: f64) -> Result<Vec<u8>, String> {
        self.bands.raster(Source { file: &self.file, revision: self.revision }, epoch, key, dpr)
    }

    /// How the grid splits and measures text (`gridMeasure`).
    pub fn grid_measure(&mut self, asks: &Value) -> Result<Value, String> {
        self.bands.measure(asks)
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use kition_kit::wasm_bindgen::prelude::*;

    use crate::__kition_kit_export::WebDocument;

    fn error(message: String) -> JsError {
        JsError::new(&message)
    }

    #[wasm_bindgen(js_class = "TableDocument", wasm_bindgen = kition_kit::wasm_bindgen)]
    impl WebDocument {
        /// Lays out the arrangement `axis` (what `gridOpen` or `gridSync` answered) for the view
        /// `params` (`{tableId, viewId?, theme?, brand?}`); answers the axes as JSON.
        #[wasm_bindgen(js_name = gridAxes)]
        pub fn grid_axes(&mut self, axis: &[u8], params: &str) -> Result<String, JsError> {
            let params = kition_kit::parse_params(params).map_err(error)?;
            self.0.grid_axes(axis, &params).map(|value| value.to_string()).map_err(error)
        }

        /// The bands `keys` (layer, column band, row band triples) of arrangement `epoch` at
        /// `dpr`, with the icons from id `icons` on and the emoji tiles not in `tiles` (a JSON
        /// list of `atlas:size:dpr`), as one batch.
        #[wasm_bindgen(js_name = gridBands)]
        pub fn grid_bands(&mut self, epoch: u32, keys: &[u32], dpr: f64, icons: u32, tiles: &str) -> Result<Vec<u8>, JsError> {
            let tiles: Vec<String> = serde_json::from_str(tiles).map_err(|e| error(e.to_string()))?;
            self.0.grid_bands(epoch, keys, dpr, icons, &tiles).map_err(error)
        }

        /// One band as the engine rasterizes it (for the parity test): width, height, RGBA8.
        #[wasm_bindgen(js_name = gridRaster)]
        pub fn grid_raster(&mut self, epoch: u32, layer: u32, col_band: u32, row_band: u32, dpr: f64) -> Result<Vec<u8>, JsError> {
            self.0.grid_raster(epoch, [layer, col_band, row_band], dpr).map_err(error)
        }

        /// How the grid splits `[{text, weight, size}]` into runs and measures them (JSON).
        #[wasm_bindgen(js_name = gridMeasure)]
        pub fn grid_measure(&mut self, asks: &str) -> Result<String, JsError> {
            let asks: serde_json::Value = serde_json::from_str(asks).map_err(|e| error(e.to_string()))?;
            self.0.grid_measure(&asks).map(|value| value.to_string()).map_err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use kition_table_arrange::{Axis as Arranged, GroupHeader};
    use serde_json::json;

    use super::{Row, linear_rows};

    fn group(at: u32, count: u32, depth: u32, collapsed: bool) -> GroupHeader {
        GroupHeader { at, count, depth, collapsed, id: format!("g{depth}:{at}"), value: json!(at) }
    }

    #[test]
    fn records_alone_end_with_an_add_row() {
        let axis = Arranged { ids: vec![5, 3, 9], ..Arranged::default() };
        let rows = linear_rows(&axis, 0);
        assert_eq!(rows, [Row::Record { id: 5, number: 1 }, Row::Record { id: 3, number: 2 }, Row::Record { id: 9, number: 3 }, Row::Append]);
    }

    #[test]
    fn nested_groups_head_their_records_and_collapsed_ones_hide_them() {
        // Two outer groups; the first holds two inner groups, the second inner one collapsed.
        let axis = Arranged {
            ids: vec![1, 2, 3, 4, 5],
            groups: vec![group(0, 3, 0, false), group(0, 1, 1, false), group(1, 2, 1, true), group(3, 2, 0, false), group(3, 2, 1, false)],
            ..Arranged::default()
        };
        let rows = linear_rows(&axis, 2);
        assert_eq!(
            rows,
            [
                Row::Group(0),
                Row::Group(1),
                Row::Record { id: 1, number: 1 },
                Row::Append,
                Row::Group(2),
                Row::Group(3),
                Row::Group(4),
                Row::Record { id: 4, number: 1 },
                Row::Record { id: 5, number: 2 },
                Row::Append,
            ]
        );
        // A collapsed outer group hides its inner headers too.
        let axis = Arranged { ids: vec![1, 2], groups: vec![group(0, 2, 0, true), group(0, 2, 1, false)], ..Arranged::default() };
        assert_eq!(linear_rows(&axis, 2), [Row::Group(0)]);
    }
}
