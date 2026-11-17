//! A dashboard, as the editor lays it out: widgets on a 12-column grid (rows of 80 px, 16 px
//! apart), each a card with its title and its number, bar, line or pie chart, or table.

use kition_table_file::Field;
use kurbo::{BezPath, Point, Rect};
use serde_json::Value;

use crate::data::{Answer, DashboardShown, answer};
use crate::format::{display, number};
use crate::scene::{Rgba, Scene, shape};
use crate::theme::*;

pub const WIDTH: f64 = 1200.0;
const PAD: f64 = 20.0;
const MARGIN: f64 = 16.0;
const COLUMNS: f64 = 12.0;
const ROW: f64 = 80.0;

fn color_of(widget: &Value, fallback: Rgba) -> Rgba {
    let text = widget.get("config").and_then(|config| config.get("color")).and_then(Value::as_str).unwrap_or_default();
    let hex = text.trim_start_matches('#');
    match u32::from_str_radix(hex, 16) {
        Ok(rgb) if hex.len() == 6 => crate::scene::hex(rgb),
        _ => fallback,
    }
}

/// A count as the editor writes it (`toLocaleString`: thousands separated by commas).
fn grouped(value: f64) -> String {
    let text = number(value);
    let (whole, fraction) = text.split_once('.').map_or((text.as_str(), None), |(whole, fraction)| (whole, Some(fraction)));
    let (sign, digits) = whole.strip_prefix('-').map_or(("", whole), |digits| ("-", digits));
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    match fraction {
        Some(fraction) => format!("{sign}{out}.{fraction}"),
        None => format!("{sign}{out}"),
    }
}

/// Lays the dashboard out from `top`; answers its height.
pub fn draw(scene: &mut Scene, shown: &DashboardShown, top: f64) -> f64 {
    let widgets = &shown.dashboard.widgets;
    let layout: Vec<Value> = shown.dashboard.config.get("layout").and_then(Value::as_array).cloned().unwrap_or_default();
    let column = (WIDTH - 2.0 * PAD - (COLUMNS - 1.0) * MARGIN) / COLUMNS;
    let mut placed: Vec<(String, Rect)> = Vec::new();
    for item in &layout {
        let Some(id) = item.get("widget_id").and_then(Value::as_str) else { continue };
        let number = |key: &str, default: f64| item.get(key).and_then(Value::as_f64).unwrap_or(default).max(0.0);
        let (x, y, w, h) = (number("x", 0.0), number("y", 0.0), number("w", 4.0).max(1.0), number("h", 2.0).max(1.0));
        let left = PAD + x * (column + MARGIN);
        let up = top + PAD + y * (ROW + MARGIN);
        let area = Rect::new(left, up, left + w * column + (w - 1.0) * MARGIN, up + h * ROW + (h - 1.0) * MARGIN);
        placed.push((id.to_owned(), area));
    }
    // Widgets the layout leaves out stack below it, half the width each.
    let mut next = placed.iter().map(|(_, area)| area.y1).fold(top + PAD, f64::max);
    for id in widgets.keys() {
        if placed.iter().any(|(placed, _)| placed == id) {
            continue;
        }
        let area = Rect::new(PAD, next + MARGIN, PAD + 6.0 * column + 5.0 * MARGIN, next + MARGIN + 2.0 * ROW + MARGIN);
        next = area.y1;
        placed.push((id.clone(), area));
    }
    let bottom = placed.iter().map(|(_, area)| area.y1).fold(top + PAD, f64::max) + PAD;
    scene.rect(Rect::new(0.0, top, WIDTH, bottom), MUTED_BG);
    for (id, area) in &placed {
        let Some(widget) = widgets.get(id) else { continue };
        widget_card(scene, widget, &shown.fields, &answer(widget, &shown.fields, &shown.records), *area);
    }
    bottom - top
}

fn widget_card(scene: &mut Scene, widget: &Value, fields: &[Field], answer: &Answer, area: Rect) {
    scene.shadow(area, 12.0, 2.0, crate::scene::alpha(FOREGROUND, 14));
    scene.round_rect(area, 12.0, PAPER);
    scene.border(area, 12.0, 1.0, BORDER);
    let title = widget.get("title").and_then(Value::as_str).unwrap_or_default();
    let description = widget.get("description").and_then(Value::as_str).unwrap_or_default();
    let head = if description.is_empty() { 44.0 } else { 58.0 };
    scene.text_mid(shape(title, LARGE, SEMIBOLD, area.width() - 32.0), area.x0 + 16.0, area.y0 + 22.0, FOREGROUND);
    if !description.is_empty() {
        scene.text_mid(shape(description, SMALL, REGULAR, area.width() - 32.0), area.x0 + 16.0, area.y0 + 40.0, MUTED);
    }
    scene.hline(area.x0, area.x1, area.y0 + head, BORDER);
    let content = Rect::new(area.x0 + 16.0, area.y0 + head + 16.0, area.x1 - 16.0, area.y1 - 16.0);
    let kind = widget.get("type").and_then(Value::as_str).unwrap_or_default();
    match answer {
        Answer::Metric(value) => {
            let label = widget.get("config").and_then(|config| config.get("value_label")).and_then(Value::as_str).unwrap_or("items");
            let middle = (content.y0 + content.y1) / 2.0;
            scene.text_mid(shape(&grouped(*value), 36.0, SEMIBOLD, content.width()), content.x0, middle - 10.0, color_of(widget, FOREGROUND));
            scene.text_mid(shape(label, LARGE, REGULAR, content.width()), content.x0, middle + 24.0, MUTED);
        }
        Answer::Series(points) if points.is_empty() => {
            let line = shape("No data to display", LARGE, REGULAR, content.width());
            let x = content.x0 + (content.width() - line.width) / 2.0;
            scene.text_mid(line, x, (content.y0 + content.y1) / 2.0, MUTED);
        }
        Answer::Series(points) if kind == "pie" => pie(scene, points, content),
        Answer::Series(points) => bars(scene, points, content, color_of(widget, PRIMARY), kind == "line"),
        Answer::Rows { columns, rows } => table(scene, fields, columns, rows, content),
    }
}

/// A bar chart (or a line chart): values on a scale from 0 with light grid lines, the
/// categories' labels below.
fn bars(scene: &mut Scene, points: &[(String, f64)], area: Rect, color: Rgba, line: bool) {
    let max = points.iter().map(|(_, value)| *value).fold(0.0_f64, f64::max).max(1.0);
    // A whole-number scale of four steps.
    let step = (max / 4.0).ceil().max(1.0);
    let top_value = step * 4.0;
    let axis = 32.0;
    let plot = Rect::new(area.x0 + axis, area.y0 + 6.0, area.x1, area.y1 - 22.0);
    for tick in 0..=4 {
        let value = step * f64::from(tick);
        let y = plot.y1 - plot.height() * value / top_value;
        scene.hline(plot.x0, plot.x1, y.round(), CELL_LINE);
        let label = shape(&grouped(value), 11.0, REGULAR, f64::INFINITY);
        let x = plot.x0 - 6.0 - label.width;
        scene.text_mid(label, x, y, MUTED);
    }
    let slot = plot.width() / points.len().max(1) as f64;
    let mut path = BezPath::new();
    for (index, (label, value)) in points.iter().enumerate() {
        let center = plot.x0 + slot * (index as f64 + 0.5);
        let y = plot.y1 - plot.height() * value / top_value;
        if line {
            if index == 0 {
                path.move_to((center, y));
            } else {
                path.line_to((center, y));
            }
        } else {
            let half = (slot * 0.32).min(28.0);
            scene.round_rect(Rect::new(center - half, y, center + half, plot.y1), 4.0_f64.min((plot.y1 - y) / 2.0), color);
        }
        let text = shape(label, 11.0, REGULAR, slot - 4.0);
        let x = center - text.width / 2.0;
        scene.text_mid(text, x, area.y1 - 9.0, MUTED);
    }
    if line {
        scene.stroke(path, 2.0, color);
        for (index, (_, value)) in points.iter().enumerate() {
            let center = plot.x0 + slot * (index as f64 + 0.5);
            let y = plot.y1 - plot.height() * value / top_value;
            scene.circle(Point::new(center, y), 3.5, color);
        }
    }
}

/// A donut chart with its legend beside it.
fn pie(scene: &mut Scene, points: &[(String, f64)], area: Rect) {
    let total: f64 = points.iter().map(|(_, value)| value.max(0.0)).sum();
    let legend = (area.width() * 0.45).min(220.0);
    let radius = ((area.width() - legend - 16.0).min(area.height()) / 2.0).max(8.0);
    let center = Point::new(area.x0 + radius, (area.y0 + area.y1) / 2.0);
    let inner = radius * 0.64;
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for (index, (_, value)) in points.iter().enumerate() {
        if total <= 0.0 {
            break;
        }
        let sweep = std::f64::consts::TAU * value.max(0.0) / total;
        if sweep <= 0.0 {
            continue;
        }
        let at = |radius: f64, angle: f64| Point::new(center.x + radius * angle.cos(), center.y + radius * angle.sin());
        let ring = kurbo::Arc::new(center, (radius, radius), angle, sweep, 0.0);
        let back = kurbo::Arc::new(center, (inner, inner), angle + sweep, -sweep, 0.0);
        let mut path = BezPath::new();
        path.move_to(at(radius, angle));
        path.extend(ring.append_iter(0.1));
        path.line_to(at(inner, angle + sweep));
        path.extend(back.append_iter(0.1));
        path.close_path();
        scene.fill(path, CHART.get(index % CHART.len()).copied().unwrap_or(PRIMARY));
        angle += sweep;
    }
    let mut y = center.y - points.len().min(8) as f64 * 11.0;
    // The legend beside the donut, its counts at most a legend's width away.
    let x = center.x + radius + 32.0;
    let right = (x + legend).min(area.x1);
    for (index, (label, value)) in points.iter().take(8).enumerate() {
        let middle = y + 11.0;
        scene.round_rect(Rect::new(x, middle - 5.0, x + 10.0, middle + 5.0), 3.0, CHART.get(index % CHART.len()).copied().unwrap_or(PRIMARY));
        let count = shape(&grouped(*value), SMALL, MEDIUM, f64::INFINITY);
        let count_width = count.width;
        scene.text_mid(count, right - count_width, middle, FOREGROUND);
        scene.text_mid(shape(label, SMALL, REGULAR, right - x - 28.0 - count_width), x + 16.0, middle, MUTED);
        y += 22.0;
    }
}

/// A table widget: its columns' titles, then its rows.
fn table(scene: &mut Scene, fields: &[Field], columns: &[(String, String)], rows: &[serde_json::Map<String, Value>], area: Rect) {
    if columns.is_empty() {
        return;
    }
    let width = area.width() / columns.len() as f64;
    let row = 30.0;
    scene.round_rect(Rect::new(area.x0, area.y0, area.x1, area.y0 + row), 6.0, MUTED_BG);
    for (index, (_, title)) in columns.iter().enumerate() {
        let x = area.x0 + index as f64 * width + 10.0;
        scene.text_mid(shape(title, SMALL, MEDIUM, width - 20.0), x, area.y0 + row / 2.0, FOREGROUND);
    }
    let mut y = area.y0 + row;
    for values in rows {
        if y + row > area.y1 {
            break;
        }
        for (index, (name, _)) in columns.iter().enumerate() {
            let x = area.x0 + index as f64 * width + 10.0;
            let text = match fields.iter().find(|field| &field.name == name) {
                Some(field) => crate::format::cell_text(field, values.get(name)),
                None => display(values.get(name)),
            };
            let text = if text.is_empty() { "\u{2014}".to_owned() } else { text };
            scene.text_mid(shape(&text, SMALL, REGULAR, width - 20.0), x, y + row / 2.0, MUTED);
        }
        scene.hline(area.x0, area.x1, y + row - 1.0, CELL_LINE);
        y += row;
    }
}

#[cfg(test)]
mod tests {
    use super::grouped;

    #[test]
    fn counts_group_thousands() {
        assert_eq!(grouped(1234567.0), "1,234,567");
        assert_eq!(grouped(-1200.5), "-1,200.5");
        assert_eq!(grouped(12.0), "12");
    }
}
