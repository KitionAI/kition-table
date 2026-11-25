//! The document's pixels: the editor's view of it ([`kition_table_picture`]), for hosts' renders
//! and thumbnails and the agent's preview (`file.render`, the manifest's `agent.render`: a
//! diagnostic's target, a table, a view or a range of its rows, a scale, and a size budget; KHP
//! A39, Kition platform K21). `preview.show` picks the table, view or dashboard the picture shows.

use base64::Engine as _;
use kition_table_file::TableFile;
use kition_table_picture::{Focus, Picture, Shown};
use serde_json::{Value, json};

/// The largest scale and size `file.render` takes (A39).
pub const MAX_SCALE: f64 = 2.0;
pub const MAX_SIZE: u32 = 4096;
/// The area around a focused cell a preview shows, at most.
const FOCUS_AREA: (f64, f64) = (1280.0, 720.0);

fn id_after<'p>(parts: &[&'p str], key: &str) -> Option<&'p str> {
    parts.iter().position(|part| *part == key).and_then(|at| parts.get(at + 1)).copied()
}

fn parse_id(file: &TableFile, text: &str, what: &str) -> Result<i64, String> {
    let ty = match what {
        "table" => "Table",
        "view" => "View",
        "field" => "Field",
        "record" => "Record",
        _ => return Err(format!("invalid target: {what}")),
    };
    crate::address::local(file, ty, &json!(text)).ok().filter(|id| *id > 0).ok_or_else(|| format!("invalid target: {what} {text} is not an id"))
}

/// The render command (`kit.json` `agent.render`).
pub const RENDER_COMMAND: &str = "file.render";

/// What a `target` shows: a diagnostic's (A21) `path` (`tables/<id>`, `tables/<id>/fields/<id>`,
/// `tables/<id>/records/<id>`, `tables/<id>/records/<id>/fields/<id>`, `dashboards/<id>`), or a
/// view (`tables/<id>/views/<id>`), or `sheet`, a table's title, when it has no `path`; and
/// `rows`, `[from, to]`, the records at those positions of the table or view (counted from 1). A
/// table shows as the editor opens it, a view as it shows its records; a record or field is
/// outlined in the table's every field and record, in order.
pub fn target_of(file: &TableFile, target: &Value) -> Result<(Shown, Option<Focus>), String> {
    let tables = file.list_tables().map_err(|error| error.to_string())?;
    let named = target.get("table").is_some_and(|v| !v.is_null()) || target.get("view").is_some_and(|v| !v.is_null());
    if named && (target.get("path").is_some_and(|v| !v.is_null()) || target.get("sheet").is_some_and(|v| !v.is_null())) {
        return Err("invalid params: name the view either by table and view ids or by a diagnostic's path or sheet, not both".into());
    }
    let rows = match target.get("rows") {
        None | Some(Value::Null) => None,
        Some(Value::Array(pair)) if pair.len() == 2 && pair.iter().all(Value::is_u64) => {
            let at = |i: usize| pair.get(i).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).unwrap_or_default();
            Some((at(0), at(1)))
        }
        Some(_) => return Err("invalid params: target.rows is [from, to], positions counted from 1".into()),
    };
    if let Some(path) = target.get("path").and_then(Value::as_str) {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.first() == Some(&"dashboards") {
            let id = parts.get(1).copied().unwrap_or_default();
            return Ok((Shown { dashboard: Some(id.to_owned()), ..Shown::default() }, None));
        }
        let table = parse_id(file, id_after(&parts, "tables").ok_or_else(|| format!("invalid target: path {path}"))?, "table")?;
        let found = tables.iter().find(|t| t.id == table).ok_or_else(|| format!("not found: table {table}"))?;
        let view = id_after(&parts, "views").map(|view| parse_id(file, view, "view")).transpose()?;
        if let Some(view) = view
            && !found.views.iter().any(|v| v.id == view)
        {
            return Err(format!("not found: view {view} of table {table}"));
        }
        let field = id_after(&parts, "fields").map(|field| parse_id(file, field, "field")).transpose()?;
        if let Some(field) = field
            && !found.fields.iter().any(|f| f.id == field)
        {
            return Err(format!("not found: field {field}"));
        }
        let record = id_after(&parts, "records").map(|record| parse_id(file, record, "record")).transpose()?;
        if let Some(record) = record {
            file.get_record(table, record).map_err(|_| format!("not found: record {record}"))?;
        }
        // An outlined cell or field shows the table plain, so no filter of a view hides it.
        let focus = (field.is_some() || record.is_some()).then_some(Focus { field, record });
        return Ok((Shown { table: Some(table), view, plain: focus.is_some(), rows, ..Shown::default() }, focus));
    }
    if named {
        let fields = target.as_object().cloned().unwrap_or_default();
        let (table, view) = crate::context::part_of(file, &fields, "target").map_err(|error| match error.strip_prefix("not found: no ") {
            Some(rest) => format!("not found: {rest}"),
            None => format!("invalid params: {error}"),
        })?;
        return Ok((Shown { table, view, rows, ..Shown::default() }, None));
    }
    if let Some(sheet) = target.get("sheet").and_then(Value::as_str) {
        let table = tables.iter().find(|table| table.title == sheet).ok_or_else(|| format!("not found: table {sheet}"))?;
        return Ok((Shown { table: Some(table.id), rows, ..Shown::default() }, None));
    }
    Ok((Shown { rows, ..Shown::default() }, None))
}

/// PNG bytes of RGBA8 pixels.
pub fn png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer.write_image_data(rgba).map_err(|error| error.to_string())?;
    writer.finish().map_err(|error| error.to_string())?;
    Ok(out)
}

/// The area of `picture` a preview shows: all of it, or the part around its focused cell.
fn area_of(picture: &Picture) -> (f64, f64, f64, f64) {
    let (width, height) = (f64::from(picture.width()), f64::from(picture.height()));
    match picture.focus {
        Some(focus) if width > FOCUS_AREA.0 || height > FOCUS_AREA.1 => {
            let (w, h) = (width.min(FOCUS_AREA.0), height.min(FOCUS_AREA.1));
            let x = (focus.x0 - (w - focus.width()) / 2.0).clamp(0.0, width - w);
            let y = (focus.y0 - (h - focus.height()) / 2.0).clamp(0.0, height - h);
            (x, y, w, h)
        }
        _ => (0.0, 0.0, width, height),
    }
}

/// `file.render`: `{ target?, slide?, scale?, maxSize? }` answers the picture of `target`
/// ([`target_of`]), else of the table at position `slide` (counted from 0, a host's part
/// preview), else what the editor opens (the first table's first view), never what the
/// document's picture shows (`preview.show`, editor state), at `scale` device pixels per document pixel (default 1, up to
/// 2), shrunk so its longer side is at most `maxSize` (default and at most 4096), as
/// `{ png (base64), mimeType, width, height, scale, tableId, viewId }` at the top of the answer,
/// where the kits server reads it (K21). It is never an edit.
pub fn render(file: &TableFile, params: &Value) -> Result<Value, String> {
    let scale = match params.get("scale") {
        None | Some(Value::Null) => 1.0,
        Some(value) => {
            value.as_f64().filter(|scale| *scale > 0.0 && *scale <= MAX_SCALE).ok_or("invalid params: scale is a number above 0, up to 2")?
        }
    };
    let max_size = match params.get("maxSize") {
        None | Some(Value::Null) => MAX_SIZE,
        Some(value) => value
            .as_u64()
            .and_then(|size| u32::try_from(size).ok())
            .filter(|size| (1..=MAX_SIZE).contains(size))
            .ok_or("invalid params: maxSize is an integer from 1 to 4096")?,
    };
    let (shown, focus) = match (params.get("target"), params.get("slide")) {
        (Some(target), _) if target.is_object() => target_of(file, target)?,
        (Some(target), _) if !target.is_null() => return Err("invalid params: target is an object, such as {\"path\": \"tables/1\"}".into()),
        (_, Some(slide)) if !slide.is_null() => {
            let tables = file.list_tables().map_err(|error| error.to_string())?;
            let table = slide
                .as_u64()
                .and_then(|at| usize::try_from(at).ok())
                .and_then(|at| tables.get(at))
                .ok_or_else(|| format!("invalid params: slide {slide} is no table's position (counted from 0, {} tables)", tables.len()))?;
            (Shown { table: Some(table.id), ..Shown::default() }, None)
        }
        _ => (Shown::default(), None),
    };
    let picture = kition_table_picture::draw(file, &shown, focus)?;
    let (x, y, area_width, area_height) = area_of(&picture);
    let scale = scale.min(f64::from(max_size) / area_width.max(area_height));
    let width = ((area_width * scale).round() as u32).max(1);
    let height = ((area_height * scale).round() as u32).max(1);
    let rgba = picture.render(x, y, scale, width, height);
    let bytes = png(&rgba, width, height)?;
    Ok(json!({
        "png": base64::engine::general_purpose::STANDARD.encode(bytes),
        "mimeType": "image/png",
        "width": width,
        "height": height,
        "scale": scale,
        "tableId": picture.shown["tableId"],
        "viewId": picture.shown["viewId"],
    }))
}

/// Its parameters (a `ParameterSchemaSpec`, KHP A5).
pub fn input_schema() -> Value {
    json!({
        "target": {
            "type": "object",
            "additionalProperties": false,
            "description": "What to draw: a table or a view by id (table, view), or a diagnostic's target as the check answered it. table is a table's id (its first view), view a view's id (the records it shows, filtered and in its order). path is tables/<id> (the table as the editor opens it), tables/<id>/views/<id> (a view, the records it shows), tables/<id>/fields/<id>, tables/<id>/records/<id> or tables/<id>/records/<id>/fields/<id> (every field and record of the table, that one outlined and the picture framed around it), or dashboards/<id>; sheet, a table's title, when there is no path. rows [from, to] draws only the records at those positions of the table or view, counted from 1 as the grid numbers them. Default: the first table's first view, whatever the editor shows.",
            "properties": {
                "table": { "type": "integer", "description": "A table's id, as table_inspect lists it: the table as its first view shows it." },
                "view": { "type": "integer", "description": "A view's id, as table_inspect lists it: the records it shows, in its order." },
                "path": { "type": "string", "description": "tables/<id>[/views/<id> | /fields/<id> | /records/<id>[/fields/<id>]] or dashboards/<id>, with the ids table_inspect answers." },
                "sheet": { "type": "string", "description": "A table's title (a diagnostic gives it beside path)." },
                "rows": { "type": "array", "items": { "type": "integer" }, "description": "[from, to]: the records at these positions, counted from 1, both included." },
            },
        },
        "slide": { "type": "integer", "description": "Without target: the table at this position among the tables, counted from 0 (a host's part preview)." },
        "scale": { "type": "number", "description": "Device pixels per document pixel, above 0 and up to 2; default 1." },
        "maxSize": { "type": "integer", "description": "The longest side in pixels, 1 to 4096; default 4096." },
    })
}

/// Its answer (a `ValueSchemaSpec`, KHP A1), or `{ error }` (A18).
pub fn output_schema(error_spec: Value) -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "png": { "type": "string", "description": "Base64 of the PNG." },
            "mimeType": { "type": "string", "description": "image/png." },
            "width": { "type": "integer", "description": "Its width in pixels." },
            "height": { "type": "integer", "description": "Its height in pixels." },
            "scale": { "type": "number", "description": "The scale it was drawn at." },
            "tableId": { "oneOf": [{ "type": "integer" }, { "type": "null" }], "description": "The table drawn; null for a dashboard." },
            "viewId": { "oneOf": [{ "type": "integer" }, { "type": "null" }], "description": "The view drawn; null without one." },
            "error": error_spec,
        },
    })
}

/// `preview.show`: `{ tableId?, viewId?, dashboardId? }` makes the picture show a table (its
/// first view without `viewId`), a view (of its table), or a dashboard; `{}` shows what the editor
/// opens. Answers what is shown: `{ tableId, table, viewId, view, viewType, dashboardId,
/// records }` (`dashboard` instead of the table's keys for a dashboard).
pub fn show(file: &TableFile, params: &Value) -> Result<(Shown, Value), String> {
    let id = |key: &str| -> Result<Option<i64>, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value.as_i64().filter(|id| *id > 0).map(Some).ok_or_else(|| format!("invalid params: {key} is a positive integer")),
        }
    };
    let dashboard = match params.get("dashboardId") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if !id.is_empty() => Some(id.clone()),
        Some(_) => return Err("invalid params: dashboardId is a dashboard's id".into()),
    };
    let (table, view) = (id("tableId")?, id("viewId")?);
    if dashboard.is_some() && (table.is_some() || view.is_some()) {
        return Err("invalid params: name a dashboard, or a table and a view, not both".into());
    }
    // A view alone names its table.
    let table = match (table, view) {
        (None, Some(view_id)) => {
            let tables = file.list_tables().map_err(|error| error.to_string())?;
            let owner =
                tables.iter().find(|table| table.views.iter().any(|view| view.id == view_id)).ok_or_else(|| format!("not found: view {view_id}"))?;
            Some(owner.id)
        }
        (table, _) => table,
    };
    let shown = Shown { table, view, dashboard, ..Shown::default() };
    let picture = kition_table_picture::draw(file, &shown, None)?;
    Ok((shown, picture.shown))
}
