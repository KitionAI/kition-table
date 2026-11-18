//! The bars above the records, as the editor draws them: the tab bar (tables and dashboards, the
//! shown one highlighted) and the view bar (the table's views with their type icons, then what the
//! shown view filters, sorts and groups by, and how many records it shows).

use kurbo::{Point, Rect};

use crate::data::{Tab, TableShown};
use crate::scene::{Scene, shape};
use crate::theme::*;

pub const TAB_BAR: f64 = 44.0;
pub const VIEW_BAR: f64 = 52.0;
/// The footer below a grid.
pub const FOOTER: f64 = 36.0;
const TAB_HEIGHT: f64 = 28.0;
const TAB_MAX: f64 = 192.0;
const VIEW_TAB_HEIGHT: f64 = 32.0;
const VIEW_TAB_MAX: f64 = 176.0;

/// The icon of a view type.
pub fn view_icon(view_type: &str) -> &'static str {
    match view_type {
        "kanban" => "square-kanban",
        "calendar" => "calendar-days",
        "gallery" => "gallery-horizontal-end",
        _ => "table-2",
    }
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The tab bar across `width`; answers the width its tabs need.
pub fn tab_bar(scene: &mut Scene, tabs: &[Tab], width: f64) -> f64 {
    scene.rect(Rect::new(0.0, 0.0, width, TAB_BAR), PAPER);
    let mut x = 12.0;
    let top = (TAB_BAR - TAB_HEIGHT) / 2.0;
    let middle = TAB_BAR / 2.0;
    for tab in tabs {
        let icon = if tab.dashboard { 20.0 } else { 0.0 };
        let line = shape(&tab.title, BODY, MEDIUM, TAB_MAX - 20.0 - icon);
        let tab_width = line.width + 20.0 + icon;
        if tab.active {
            scene.round_rect(Rect::new(x, top, x + tab_width, top + TAB_HEIGHT), 8.0, ACCENT);
        }
        let color = if tab.active { FOREGROUND } else { MUTED };
        if tab.dashboard {
            scene.icon("layout-dashboard", Point::new(x + 10.0, middle - 7.0), 14.0, color);
        }
        scene.text_mid(line, x + 10.0 + icon, middle, color);
        x += tab_width + 4.0;
    }
    scene.icon("plus", Point::new(x + 6.0, middle - 8.0), 16.0, MUTED);
    x += 34.0;
    scene.hline(0.0, width, TAB_BAR - 1.0, BORDER);
    x
}

/// What the view bar shows, measured before the picture's width is known.
pub struct ViewBar {
    pub width: f64,
}

/// The view bar of a table, below the tab bar, across `width` (`None`: measure only); answers
/// the width it needs.
pub fn view_bar(scene: Option<&mut Scene>, shown: &TableShown, width: f64) -> ViewBar {
    let mut ignored = Scene::default();
    let scene = scene.unwrap_or(&mut ignored);
    let top = TAB_BAR;
    scene.rect(Rect::new(0.0, top, width, top + VIEW_BAR), PAPER);
    let middle = top + VIEW_BAR / 2.0;
    let tab_top = middle - VIEW_TAB_HEIGHT / 2.0;
    let mut x = 12.0;
    let active = shown.view.as_ref().map(|view| view.id);
    for view in &shown.table.views {
        let line = shape(&view.title, LARGE, MEDIUM, VIEW_TAB_MAX - 44.0);
        let tab_width = line.width + 44.0;
        let is_active = Some(view.id) == active;
        let area = Rect::new(x, tab_top, x + tab_width, tab_top + VIEW_TAB_HEIGHT);
        if is_active {
            scene.round_rect(area, 6.0, PRIMARY_SOFT);
            scene.border(area, 6.0, 1.0, PRIMARY_LINE);
        }
        let color = if is_active { PRIMARY } else { MUTED };
        scene.icon(view_icon(&view.view_type), Point::new(x + 10.0, middle - 8.0), 16.0, color);
        scene.text_mid(line, x + 34.0, middle, color);
        x += tab_width + 4.0;
    }
    let add = Rect::new(x + 4.0, tab_top, x + 4.0 + VIEW_TAB_HEIGHT, tab_top + VIEW_TAB_HEIGHT);
    scene.round_rect(add, 6.0, hex_soft());
    scene.border(add, 6.0, 1.0, BORDER);
    scene.icon("plus", Point::new(add.x0 + 8.0, middle - 8.0), 16.0, FOREGROUND);
    x = add.x1 + 16.0;

    // What the view does to its records.
    let field_title = |field: &kition_table_file::Field| if field.title.is_empty() { field.name.clone() } else { field.title.clone() };
    let mut settings: Vec<(&str, String)> = Vec::new();
    if shown.filters > 0 {
        settings.push(("list-filter", plural(shown.filters, "filter", "filters")));
    }
    if let Some(first) = shown.sorts.first() {
        settings.push(("arrow-up-down", format!("Sorted by {}", field_title(first))));
    }
    if let Some(first) = shown.groups.first() {
        settings.push(("group", format!("Grouped by {}", field_title(first))));
    }
    for (icon, text) in settings {
        let line = shape(&text, BODY, MEDIUM, 220.0);
        let chip_width = line.width + 38.0;
        let area = Rect::new(x, middle - 14.0, x + chip_width, middle + 14.0);
        scene.round_rect(area, 8.0, ACCENT);
        scene.icon(icon, Point::new(x + 10.0, middle - 7.0), 14.0, PRIMARY);
        scene.text_mid(line, x + 28.0, middle, FOREGROUND);
        x += chip_width + 8.0;
    }
    let count = if shown.shown == shown.total {
        plural(shown.total, "record", "records")
    } else {
        format!("{} of {}", shown.shown, plural(shown.total, "record", "records"))
    };
    let line = shape(&count, SMALL, REGULAR, f64::INFINITY);
    let needed = x + 8.0 + line.width + 16.0;
    let count_x = (width - 16.0 - line.width).max(x + 8.0);
    scene.text_mid(line, count_x, middle, MUTED);
    scene.hline(0.0, width, top + VIEW_BAR - 1.0, BORDER);
    ViewBar { width: needed }
}

/// The editor's footer below a grid: how many records the view shows (and which, for a range of
/// rows), at `top` across `width`; answers its height.
pub fn footer(scene: &mut Scene, shown: &TableShown, top: f64, width: f64) -> f64 {
    scene.rect(Rect::new(0.0, top, width, top + FOOTER), PAPER);
    let count = if shown.windowed {
        format!("{}\u{2013}{} of {}", shown.first, shown.first + shown.records.len().saturating_sub(1), plural(shown.shown, "record", "records"))
    } else if shown.records.len() < shown.shown {
        format!("{} of {}", shown.records.len(), plural(shown.shown, "record", "records"))
    } else {
        plural(shown.shown, "record", "records")
    };
    let line = shape(&count, SMALL, REGULAR, f64::INFINITY);
    scene.text_mid(line, 16.0, top + FOOTER / 2.0, MUTED);
    FOOTER
}

/// `--surface-soft`: the add-view button.
const fn hex_soft() -> crate::scene::Rgba {
    crate::scene::hex(0xFBFAF9)
}
