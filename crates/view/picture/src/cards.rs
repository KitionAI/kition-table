//! The card views, as the editor lays them out: a kanban board (a column per choice of its field,
//! cards with the record's title and three more fields), a gallery (cards with a cover, the title
//! and four more fields) and a calendar (the records by their date, as a list).

use kition_table_display::{DisplayList, Fit, Header, Op, Resources};
use kition_table_file::{Field, Record, TableFile, blobs};
use kurbo::{Point, Rect};
use serde_json::Value;

use crate::cell;
use crate::data::TableShown;
use crate::format::{self, display, tone};
use crate::scene::{Scene, shape};
use crate::theme::*;

const PAD: f64 = 16.0;
const COLUMN: f64 = 280.0;
const GAP: f64 = 16.0;
const COLUMN_HEAD: f64 = 44.0;
const META_ROW: f64 = 28.0;
/// The most cards a kanban column draws.
const MAX_CARDS: usize = 40;

fn title_of(field: &Field) -> &str {
    if field.title.is_empty() { &field.name } else { &field.title }
}

fn primary(shown: &TableShown) -> Option<&Field> {
    shown.fields.iter().find(|field| field.is_primary).or_else(|| shown.fields.first())
}

/// The record's title: its primary field's text, or "Untitled".
fn record_title(shown: &TableShown, record: &Record) -> String {
    let text = primary(shown).map(|field| format::cell_text(field, record.values.get(&field.name))).unwrap_or_default();
    if text.trim().is_empty() { "Untitled".to_owned() } else { text }
}

/// A card's field rows: the field's title, then its value, from `y`; answers the next row's top.
fn meta_rows(scene: &mut Scene, fields: &[&Field], record: &Record, left: f64, right: f64, mut y: f64) -> f64 {
    for field in fields {
        let middle = y + META_ROW / 2.0;
        let label = shape(title_of(field), SMALL, REGULAR, 84.0);
        scene.text_mid(label, left, middle, MUTED);
        let area = Rect::new(left + 92.0 - cell::PAD, y, right + cell::PAD, y + META_ROW);
        cell::draw(scene, field, record.values.get(&field.name), area);
        y += META_ROW;
    }
    y
}

fn card_box(scene: &mut Scene, area: Rect, radius: f64) {
    scene.shadow(area, radius, 2.0, crate::scene::alpha(FOREGROUND, 14));
    scene.round_rect(area, radius, PAPER);
    scene.border(area, radius, 1.0, BORDER);
}

/// The kanban's columns: the field's choices in order (a checkbox: checked, unchecked), then any
/// other value the records hold, each with its records.
fn board_columns<'r>(shown: &'r TableShown, field: &Field) -> Vec<(String, Vec<&'r Record>)> {
    let mut columns: Vec<(String, Vec<&Record>)> = Vec::new();
    if field.field_type == "single_select" {
        for choice in field.options.get("choices").and_then(Value::as_array).into_iter().flatten() {
            let text = display(Some(choice));
            if !text.is_empty() && !columns.iter().any(|(key, _)| *key == text) {
                columns.push((text, Vec::new()));
            }
        }
    }
    if field.field_type == "checkbox" {
        columns.push(("true".to_owned(), Vec::new()));
        columns.push(("false".to_owned(), Vec::new()));
    }
    for record in &shown.records {
        let value = record.values.get(&field.name);
        let key = if field.field_type == "checkbox" {
            value.is_some_and(format::truthy).to_string()
        } else {
            let text = display(value);
            if text.is_empty() { "Empty".to_owned() } else { text }
        };
        match columns.iter_mut().find(|(known, _)| *known == key) {
            Some((_, records)) => records.push(record),
            None => columns.push((key, vec![record])),
        }
    }
    columns
}

/// A kanban board's width.
pub fn board_width(shown: &TableShown) -> f64 {
    let count = shown.board_field.as_ref().map_or(1, |field| board_columns(shown, field).len().max(1));
    PAD + count as f64 * (COLUMN + GAP)
}

/// Lays a kanban board out from `top`; answers its height.
pub fn board(scene: &mut Scene, shown: &TableShown, top: f64) -> f64 {
    let Some(field) = shown.board_field.as_ref() else { return 0.0 };
    let others: Vec<&Field> = shown
        .columns
        .iter()
        .map(|column| &column.field)
        .filter(|other| Some(other.id) != primary(shown).map(|primary| primary.id) && other.id != field.id)
        .take(3)
        .collect();
    let card_height = 12.0 + 24.0 + others.len() as f64 * META_ROW + 10.0;
    let columns = board_columns(shown, field);
    let tallest = columns.iter().map(|(_, records)| records.len().min(MAX_CARDS)).max().unwrap_or(0) as f64;
    let column_height = COLUMN_HEAD + 8.0 + tallest * (card_height + 8.0) + 40.0;
    let mut x = PAD;
    for (key, records) in &columns {
        let area = Rect::new(x, top + PAD, x + COLUMN, top + PAD + column_height);
        scene.round_rect(area, 12.0, COLUMN_BG);
        scene.border(area, 12.0, 1.0, BORDER);
        let middle = area.y0 + COLUMN_HEAD / 2.0;
        let label = match field.field_type.as_str() {
            "checkbox" => if key == "true" { "Checked" } else { "Unchecked" }.to_owned(),
            _ => key.clone(),
        };
        let count = shape(&records.len().to_string(), SMALL, MEDIUM, f64::INFINITY);
        let count_width = count.width + 14.0;
        let count_x = area.x1 - 12.0 - count_width;
        if field.field_type == "single_select" && key != "Empty" {
            cell::chip(scene, area.x0 + 12.0, middle, &label, tone(field, key, &kition_table_grid::theme::Theme::LIGHT), count_x - 8.0);
        } else {
            let line = shape(&label, LARGE, MEDIUM, count_x - area.x0 - 20.0);
            scene.text_mid(line, area.x0 + 12.0, middle, FOREGROUND);
        }
        scene.round_rect(Rect::new(count_x, middle - 10.0, count_x + count_width, middle + 10.0), 10.0, PAPER);
        scene.text_mid(count, count_x + 7.0, middle, MUTED);
        scene.hline(area.x0, area.x1, area.y0 + COLUMN_HEAD - 1.0, BORDER);
        let mut y = area.y0 + COLUMN_HEAD + 8.0;
        for record in records.iter().take(MAX_CARDS) {
            let card = Rect::new(area.x0 + 8.0, y, area.x1 - 8.0, y + card_height);
            card_box(scene, card, 12.0);
            let head = card.y0 + 12.0 + 12.0;
            scene.icon("grip-vertical", Point::new(card.x0 + 8.0, head - 8.0), 16.0, MUTED);
            let title = shape(&record_title(shown, record), LARGE, SEMIBOLD, card.width() - 44.0);
            scene.text_mid(title, card.x0 + 30.0, head, FOREGROUND);
            meta_rows(scene, &others, record, card.x0 + 12.0, card.x1 - 12.0, card.y0 + 36.0);
            y += card_height + 8.0;
        }
        scene.icon("plus", Point::new(area.x0 + 14.0, y + 12.0 - 8.0), 16.0, MUTED);
        let add = shape("New", BODY, MEDIUM, f64::INFINITY);
        scene.text_mid(add, area.x0 + 36.0, y + 12.0, MUTED);
        x += COLUMN + GAP;
    }
    column_height + 2.0 * PAD
}

const GALLERY_CARD: f64 = 240.0;
const GALLERY_COLUMNS: usize = 4;

/// A gallery's width.
pub fn gallery_width(shown: &TableShown) -> f64 {
    let columns = shown.records.len().clamp(1, GALLERY_COLUMNS);
    PAD + columns as f64 * (GALLERY_CARD + GAP)
}

/// The cover a gallery card shows: the first attachment of the record's cover field when it is a
/// picture (as the editor's gallery takes it), decoded as the grid's thumbnails are, from the blob
/// the file holds for it. `None` for no picture, a picture at a web address (the picture loads
/// nothing) or one that is not read.
fn cover_picture(file: &TableFile, field: &Field, record: &Record) -> Option<(Resources, u32)> {
    let first = match record.values.get(&field.name)? {
        Value::Array(items) => items.first()?,
        Value::Null => return None,
        item => item,
    };
    if !kition_table_grid::is_picture(first) {
        return None;
    }
    let url = kition_table_grid::attachment_url(first)?;
    let blob = file.blob(blobs::blob_hash_of_url(&url)?).ok()??;
    let pixmap = kition_table_display::image::decode(&blob.bytes, crate::THUMBNAIL)?;
    let mut resources = Resources::default();
    let key = resources.image(&url);
    resources.set_image(key, pixmap);
    Some((resources, key))
}

/// Lays a gallery out from `top`, its covers' pictures read from `file`; answers its height.
pub fn gallery(scene: &mut Scene, file: &TableFile, shown: &TableShown, top: f64) -> f64 {
    let cover = shown.cover_field.as_ref();
    let others: Vec<&Field> = shown
        .columns
        .iter()
        .map(|column| &column.field)
        .filter(|other| Some(other.id) != primary(shown).map(|primary| primary.id) && Some(other.id) != cover.map(|cover| cover.id))
        .take(4)
        .collect();
    let inner = GALLERY_CARD - 24.0;
    let cover_height = (inner * 9.0 / 16.0).round();
    let rows = others.len() + usize::from(cover.is_some());
    let card_height = 12.0 + cover_height + 12.0 + 22.0 + rows as f64 * META_ROW + 10.0;
    let mut bottom = top + PAD;
    for (index, record) in shown.records.iter().enumerate() {
        let (column, row) = (index % GALLERY_COLUMNS, index / GALLERY_COLUMNS);
        let x = PAD + column as f64 * (GALLERY_CARD + GAP);
        let y = top + PAD + row as f64 * (card_height + GAP);
        let card = Rect::new(x, y, x + GALLERY_CARD, y + card_height);
        card_box(scene, card, 12.0);
        let art = Rect::new(card.x0 + 12.0, card.y0 + 12.0, card.x1 - 12.0, card.y0 + 12.0 + cover_height);
        if let Some((resources, key)) = cover.and_then(|field| cover_picture(file, field, record)) {
            // The picture cover-cropped into the art box, centred, with its corners.
            let mut list = DisplayList::new(Header::default());
            let (x, y, w, h) = (art.x0 as f32, art.y0 as f32, art.width() as f32, art.height() as f32);
            list.push(Op::Image { key, x, y, w, h, fit: Fit::Cover, radius: 12.0 });
            scene.list(art, list, resources);
        } else {
            scene.round_rect(art, 12.0, PRIMARY_SOFT);
            let mid = Point::new((art.x0 + art.x1) / 2.0, (art.y0 + art.y1) / 2.0);
            scene.icon("gallery-horizontal-end", Point::new(mid.x - 10.0, mid.y - 10.0), 20.0, PRIMARY);
        }
        let title_middle = art.y1 + 12.0 + 11.0;
        let title = shape(&record_title(shown, record), LARGE, SEMIBOLD, inner);
        scene.text_mid(title, card.x0 + 12.0, title_middle, FOREGROUND);
        let mut row_top = art.y1 + 12.0 + 22.0;
        if let Some(cover) = cover {
            let middle = row_top + META_ROW / 2.0;
            scene.text_mid(shape("Cover", SMALL, REGULAR, 84.0), card.x0 + 12.0, middle, MUTED);
            let set = record.values.get(&cover.name).is_some_and(format::truthy);
            let (text, color) = if set { (title_of(cover).to_owned(), hex_emerald()) } else { ("Empty".to_owned(), MUTED) };
            scene.text_mid(shape(&text, SMALL, REGULAR, inner - 92.0), card.x0 + 104.0, middle, color);
            row_top += META_ROW;
        }
        meta_rows(scene, &others, record, card.x0 + 12.0, card.x1 - 12.0, row_top);
        bottom = bottom.max(card.y1);
    }
    bottom - top + PAD
}

/// `text-emerald-600`.
const fn hex_emerald() -> crate::scene::Rgba {
    crate::scene::hex(0x059669)
}

pub const CALENDAR_WIDTH: f64 = 760.0;
const AGENDA_ROW: f64 = 64.0;

/// Lays a calendar view out from `top` (`width` wide): its heading, then each record by its date;
/// answers its height.
pub fn calendar(scene: &mut Scene, shown: &TableShown, top: f64, width: f64) -> f64 {
    let date_field = shown.fields.iter().find(|field| field.field_type == "date" || field.field_type == "datetime");
    let others: Vec<&Field> = shown
        .columns
        .iter()
        .map(|column| &column.field)
        .filter(|other| Some(other.id) != primary(shown).map(|primary| primary.id) && Some(other.id) != date_field.map(|date| date.id))
        .take(2)
        .collect();
    let right = width.min(CALENDAR_WIDTH) - PAD;
    let head = Rect::new(PAD, top + PAD, right, top + PAD + 60.0);
    scene.round_rect(head, 12.0, PRIMARY_SOFT);
    scene.border(head, 12.0, 1.0, PRIMARY_LINE);
    let heading = match date_field {
        Some(field) => format!("Calendar by {}", title_of(field)),
        None => "Add a date field to see a calendar".to_owned(),
    };
    scene.text_mid(shape(&heading, LARGE, SEMIBOLD, head.width() - 32.0), head.x0 + 16.0, head.y0 + 21.0, FOREGROUND);
    let count = shown.records.len();
    let scheduled = if count == 1 { "1 scheduled record".to_owned() } else { format!("{count} scheduled records") };
    scene.text_mid(shape(&scheduled, SMALL, REGULAR, head.width() - 32.0), head.x0 + 16.0, head.y0 + 42.0, MUTED);
    let mut y = head.y1 + 12.0;
    for record in &shown.records {
        let date_box = Rect::new(PAD, y, PAD + 160.0, y + AGENDA_ROW - 8.0);
        scene.round_rect(date_box, 6.0, PRIMARY_SOFT);
        scene.border(date_box, 6.0, 1.0, PRIMARY_LINE);
        let date = date_field
            .map(|field| format::cell_text(field, record.values.get(&field.name)))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "No date".to_owned());
        scene.text_mid(shape(&date, BODY, MEDIUM, 136.0), date_box.x0 + 12.0, (date_box.y0 + date_box.y1) / 2.0, PRIMARY);
        let card = Rect::new(date_box.x1 + 12.0, y, right, y + AGENDA_ROW - 8.0);
        card_box(scene, card, 6.0);
        scene.text_mid(shape(&record_title(shown, record), BODY, SEMIBOLD, card.width() - 24.0), card.x0 + 12.0, card.y0 + 17.0, FOREGROUND);
        let mut x = card.x0 + 12.0;
        for field in &others {
            let value = record.values.get(&field.name);
            if display(value).is_empty() && field.field_type != "checkbox" {
                continue;
            }
            let area = Rect::new(x - cell::PAD, card.y0 + 26.0, (x + 200.0).min(card.x1 - 4.0) + cell::PAD, card.y1 - 2.0);
            cell::draw(scene, field, value, area);
            x += 210.0;
        }
        y += AGENDA_ROW;
    }
    y - top + PAD
}
