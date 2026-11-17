//! `kition-table-picture`: the Table kit's picture, what its editor shows, for thumbnails, the
//! host's renders and the agent's previews.
//!
//! The picture is the editor's view of the document: the tab bar (its tables and dashboards),
//! the shown table's view bar (its views, and what the shown view filters, sorts and groups by),
//! then the view itself: a grid (each field's type icon and title, typed cells, group headers), a
//! kanban board, a gallery or a calendar list; or a dashboard's widgets. It shows what the editor
//! opens (the first table and its first view) unless [`Shown`] names another. Text is drawn with
//! the faces the host registers (`kition-table-fonts`); without them the picture has no text.

mod cards;
mod cell;
mod chrome;
mod dashboard;
mod data;
mod emoji;
mod format;
mod grid;
mod scene;
mod theme;

use kition_table_file::TableFile;
use kurbo::Rect;
use serde_json::{Value, json};

pub use data::{Arrangement, MAX_RECORDS, Shown, view_records};
pub use grid::Focus;
pub use scene::Scene;

use data::Content;

/// What a picture shows and where its parts are.
#[derive(Clone, Debug)]
pub struct Picture {
    scene: Scene,
    /// The focused cell's area, when a focus was asked for and is shown.
    pub focus: Option<Rect>,
    /// What is shown, as `preview.show` answers it.
    pub shown: Value,
}

/// The picture of `file` showing `shown`, with `focus` outlined in a grid. A table, view or
/// dashboard `shown` names that does not exist is an error.
pub fn draw(file: &TableFile, shown: &Shown, focus: Option<Focus>) -> Result<Picture, String> {
    let document = data::read(file, shown)?;
    let mut content = Scene::default();
    let mut focused = None;
    let top = chrome::TAB_BAR;
    let (width, height, about) = match &document.content {
        Content::Empty => {
            let line = scene::shape("No tables", theme::LARGE, theme::REGULAR, f64::INFINITY);
            content.text_mid(line, 24.0, top + 40.0, theme::MUTED);
            (480.0, top + 80.0, json!({ "tableId": null, "viewId": null, "dashboardId": null }))
        }
        Content::Table(table) => {
            let bar = chrome::view_bar(None, table, 0.0).width;
            let view_type = table.view.as_ref().map_or("grid", |view| view.view_type.as_str());
            let body_top = top + chrome::VIEW_BAR;
            let body_width = match view_type {
                "kanban" => cards::board_width(table),
                "gallery" => cards::gallery_width(table),
                "calendar" => cards::CALENDAR_WIDTH,
                _ => grid::width(table),
            };
            let width = bar.max(body_width).max(480.0);
            chrome::view_bar(Some(&mut content), table, width);
            let body = match view_type {
                "kanban" => cards::board(&mut content, table, body_top),
                "gallery" => cards::gallery(&mut content, table, body_top),
                "calendar" => cards::calendar(&mut content, table, body_top, width),
                _ => {
                    let (height, area) = grid::draw(&mut content, table, body_top, width, focus);
                    focused = area;
                    height
                }
            };
            let about = json!({
                "tableId": table.table.id,
                "table": table.table.title,
                "viewId": table.view.as_ref().map(|view| view.id),
                "view": table.view.as_ref().map(|view| view.title.clone()),
                "viewType": table.view.as_ref().map(|view| view.view_type.clone()),
                "dashboardId": null,
                "records": table.shown,
            });
            (width, body_top + body, about)
        }
        Content::Dashboard(shown) => {
            let body = dashboard::draw(&mut content, shown, top);
            let about = json!({ "tableId": null, "viewId": null, "dashboardId": shown.dashboard.id, "dashboard": shown.dashboard.name });
            (dashboard::WIDTH, top + body, about)
        }
    };
    let mut scene = Scene::default();
    let tabs = chrome::tab_bar(&mut Scene::default(), &document.tabs, 0.0);
    let width = width.max(tabs).ceil();
    scene.rect(Rect::new(0.0, 0.0, width, height.ceil()), theme::PAPER);
    chrome::tab_bar(&mut scene, &document.tabs, width);
    scene.items.extend(content.items);
    scene.width = width;
    scene.height = height.ceil();
    Ok(Picture { scene, focus: focused, shown: about })
}

impl Picture {
    pub fn width(&self) -> u32 {
        self.scene.width.ceil().max(1.0) as u32
    }

    pub fn height(&self) -> u32 {
        self.scene.height.ceil().max(1.0) as u32
    }

    /// The pixels from (`x`, `y`) at `scale` device pixels per picture pixel, `width` x `height`
    /// device pixels, straight RGBA8.
    pub fn render(&self, x: f64, y: f64, scale: f64, width: u32, height: u32) -> Vec<u8> {
        self.scene.render(x, y, scale, width, height)
    }

    /// The pixels of the picture's area (`x0`, `y0`) to (`x1`, `y1`) fitted into `width` x
    /// `height` device pixels.
    pub fn render_area(&self, x0: i32, y0: i32, x1: i32, y1: i32, width: u32, height: u32) -> Vec<u8> {
        let (area_width, area_height) = (f64::from((x1 - x0).max(1)), f64::from((y1 - y0).max(1)));
        let scale = (f64::from(width) / area_width).min(f64::from(height) / area_height);
        self.scene.render(f64::from(x0), f64::from(y0), scale, width, height)
    }
}
