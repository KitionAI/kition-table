//! What the picture shows, read from the document: the table and view the editor opens (its first
//! table and that table's first view) or the ones [`Shown`] names, with the view's records
//! filtered, sorted and grouped as the editor does, or a dashboard.
//!
//! The view's filter, sorts and groups are the kit's one arrangement (`kition-table-arrange`), the
//! same the editor and the agent's `read` show; its layout comes from the editor's config keys
//! (`hidden_field_names`, `row_height`, `frozen_column_count`) and the engine's
//! (`views.patchField`).

use std::cmp::Ordering;

use kition_table_arrange::{Arranged, Arrangement};
use kition_table_file::{Dashboard, Field, Record, Table, TableFile, View};
use serde_json::{Map, Value};

use crate::format::{display, natural_cmp};

/// Which table, view or dashboard the picture shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// The table (default: the first).
    pub table: Option<i64>,
    /// The view of that table (default: its first view).
    pub view: Option<i64>,
    /// A dashboard instead of a table.
    pub dashboard: Option<String>,
    /// Every field and record in order, without the view's settings (a diagnostic's preview).
    pub plain: bool,
    /// Only the records at these positions of the table or view, counted from 1, both included.
    pub rows: Option<(usize, usize)>,
}

/// The most records a picture lays out.
pub const MAX_RECORDS: usize = 400;

/// A tab of the tab bar.
#[derive(Clone, Debug)]
pub struct Tab {
    pub title: String,
    pub dashboard: bool,
    pub active: bool,
}

/// A shown column: a field and its width in the view.
#[derive(Clone, Debug)]
pub struct Column {
    pub field: Field,
    pub width: f64,
}

/// A table as the view shows it.
#[derive(Clone, Debug)]
pub struct TableShown {
    pub table: Table,
    /// `None` for a plain picture.
    pub view: Option<View>,
    /// The table's fields, in order.
    pub fields: Vec<Field>,
    /// The visible fields with their widths.
    pub columns: Vec<Column>,
    /// The records the view shows, in its order (at most [`MAX_RECORDS`]).
    pub records: Vec<Record>,
    /// The position of the first of `records` among the records the view shows, from 1.
    pub first: usize,
    /// Whether `records` are a range of rows [`Shown::rows`] asked for.
    pub windowed: bool,
    /// How many records the view shows in all.
    pub shown: usize,
    /// How many records the table has.
    pub total: usize,
    /// The fields the view groups by, outermost first.
    pub groups: Vec<Field>,
    /// The fields it sorts by.
    pub sorts: Vec<Field>,
    /// How many filter conditions apply.
    pub filters: usize,
    /// Rows' height in pixels.
    pub row_height: f64,
    /// How many columns the grid freezes at its left.
    pub frozen: usize,
    /// A kanban view's column field.
    pub board_field: Option<Field>,
    /// A gallery view's cover field.
    pub cover_field: Option<Field>,
}

/// A dashboard and the data it reads.
#[derive(Clone, Debug)]
pub struct DashboardShown {
    pub dashboard: Dashboard,
    pub fields: Vec<Field>,
    pub records: Vec<Record>,
}

/// What the picture shows below its tab bar.
#[derive(Clone, Debug)]
pub enum Content {
    /// A document without tables.
    Empty,
    Table(Box<TableShown>),
    Dashboard(Box<DashboardShown>),
}

/// The tab bar and what is shown.
#[derive(Clone, Debug)]
pub struct Document {
    pub tabs: Vec<Tab>,
    pub content: Content,
}

fn failed(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Reads what `shown` names. A table, view or dashboard it names that does not exist is an error.
pub fn read(file: &TableFile, shown: &Shown) -> Result<Document, String> {
    let tables = file.list_tables().map_err(failed)?;
    let dashboards = file.list_dashboards().map_err(failed)?;
    if let Some(id) = &shown.dashboard {
        let dashboard = dashboards.iter().find(|dashboard| &dashboard.id == id).cloned().ok_or_else(|| format!("not found: dashboard {id}"))?;
        let tabs = tabs(&tables, &dashboards, None, Some(id));
        let source = dashboard.config.get("source_table_id").and_then(Value::as_i64);
        let (fields, records) = match source.and_then(|id| tables.iter().find(|table| table.id == id)) {
            Some(table) => (table.fields.clone(), file.list_records(table.id).map_err(failed)?),
            None => (Vec::new(), Vec::new()),
        };
        return Ok(Document { tabs, content: Content::Dashboard(Box::new(DashboardShown { dashboard, fields, records })) });
    }
    let table = match shown.table {
        Some(id) => Some(tables.iter().find(|table| table.id == id).ok_or_else(|| format!("not found: table {id}"))?),
        None => tables.first(),
    };
    let Some(table) = table else {
        return Ok(Document { tabs: tabs(&tables, &dashboards, None, None), content: Content::Empty });
    };
    let view = if shown.plain {
        None
    } else {
        match shown.view {
            Some(id) => {
                Some(table.views.iter().find(|view| view.id == id).ok_or_else(|| format!("not found: view {id} of table {}", table.id))?.clone())
            }
            None => table.views.first().cloned(),
        }
    };
    let shown_table = arrange(file, table, view, shown.rows)?;
    Ok(Document { tabs: tabs(&tables, &dashboards, Some(table.id), None), content: Content::Table(Box::new(shown_table)) })
}

fn tabs(tables: &[Table], dashboards: &[Dashboard], table: Option<i64>, dashboard: Option<&String>) -> Vec<Tab> {
    let mut tabs: Vec<Tab> = tables.iter().map(|t| Tab { title: t.title.clone(), dashboard: false, active: Some(t.id) == table }).collect();
    tabs.extend(dashboards.iter().map(|d| Tab { title: d.name.clone(), dashboard: true, active: Some(&d.id) == dashboard }));
    tabs
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default()
}

/// The view's settings applied to the table's records. The records are arranged reading only
/// the cells the view's filter and order need ([`Arranged`]); only the ones the picture lays out
/// are read whole, so a picture of a large table does not parse every record.
fn arrange(file: &TableFile, table: &Table, view: Option<View>, rows: Option<(usize, usize)>) -> Result<TableShown, String> {
    let fields = table.fields.clone();
    let empty = Map::new();
    let config = view.as_ref().map_or(&empty, |view| &view.config);
    let view_fields = match &view {
        Some(view) => file.list_view_fields(view.id).map_err(failed)?,
        None => Vec::new(),
    };
    let hidden = strings(config.get("hidden_field_names"));
    let columns: Vec<Column> = fields
        .iter()
        .filter(|field| !hidden.contains(&field.name))
        .filter(|field| view_fields.iter().find(|layout| layout.field_id == field.id).is_none_or(|layout| layout.visible))
        .map(|field| {
            let width = view_fields.iter().find(|layout| layout.field_id == field.id).map(|layout| layout.width).filter(|width| *width > 0);
            Column { field: field.clone(), width: width.map_or(DEFAULT_WIDTH, |width| width as f64) }
        })
        .collect();

    let arrangement = Arrangement::of(file, &fields, view.as_ref())?;
    let filters = arrangement.filters();
    let arranged = Arranged::new(file, table.id, &fields, &arrangement, "")?;
    let total = arranged.total();
    let groups: Vec<&Field> = arrangement.groups.iter().map(|(field, _)| field).collect();
    let sorts: Vec<&Field> = arrangement.sorts.iter().map(|(field, _)| field).collect();
    let ids: Vec<i64> = arranged.ids().collect();
    let shown = ids.len();
    let (first, count) = match rows {
        Some((from, to)) if from == 0 || to < from || from > shown.max(1) => {
            return Err(format!("not found: rows {from} to {to}: the rows are counted from 1, and {shown} records are shown"));
        }
        Some((from, to)) => (from, to + 1 - from),
        None => (1, shown),
    };
    let records = ids
        .iter()
        .skip(first - 1)
        .take(count.min(MAX_RECORDS))
        .map(|id| file.get_record(table.id, *id).map_err(failed))
        .collect::<Result<Vec<Record>, String>>()?;

    let row_height = f64::from(kition_table_grid::row_height(config.get("row_height").and_then(Value::as_str)));
    // The editor freezes the first column unless the view says otherwise.
    let frozen = config.get("frozen_column_count").and_then(Value::as_u64).map_or(1, |count| count as usize);
    let primary = fields.iter().find(|field| field.is_primary).or_else(|| fields.first());
    let board_field = groups.first().copied().or_else(|| fields.iter().find(|field| field.field_type == "single_select")).or(primary).cloned();
    let cover_field = config
        .get("cover_field_name")
        .and_then(Value::as_str)
        .and_then(|name| fields.iter().find(|field| field.name == name))
        .or_else(|| fields.iter().find(|field| field.field_type == "attachment"))
        .or_else(|| fields.iter().find(|field| field.field_type == "url"))
        .cloned();
    Ok(TableShown {
        table: table.clone(),
        view,
        columns,
        records,
        first,
        windowed: rows.is_some(),
        shown,
        total,
        groups: groups.into_iter().cloned().collect(),
        sorts: sorts.into_iter().cloned().collect(),
        filters,
        row_height,
        frozen,
        board_field,
        cover_field,
        fields,
    })
}

/// A column's width when the view sets none, as the editor's grid draws it.
pub const DEFAULT_WIDTH: f64 = kition_table_grid::DEFAULT_WIDTH as f64;

/// A dashboard widget's answer, as the editor's dashboard computes it.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Metric(f64),
    Series(Vec<(String, f64)>),
    Rows { columns: Vec<(String, String)>, rows: Vec<Map<String, Value>> },
}

/// What a widget shows of `records`.
pub fn answer(widget: &Value, fields: &[Field], records: &[Record]) -> Answer {
    let query = widget.get("query").cloned().unwrap_or(Value::Null);
    let filtered: Vec<&Record> = records
        .iter()
        .filter(|record| {
            query.get("filters").and_then(Value::as_array).into_iter().flatten().all(|filter| {
                let name = filter.get("field_name").and_then(Value::as_str).unwrap_or_default();
                let value = record.values.get(name).unwrap_or(&Value::Null);
                let wanted = filter.get("value").unwrap_or(&Value::Null);
                match filter.get("operator").and_then(Value::as_str).unwrap_or_default() {
                    "equals" => value == wanted,
                    "not_equals" => value != wanted,
                    "truthy" => crate::format::truthy(value),
                    "falsy" => !crate::format::truthy(value),
                    _ => true,
                }
            })
        })
        .collect();
    let kind = widget.get("type").and_then(Value::as_str).unwrap_or_default();
    if kind == "table" {
        let wanted = strings(query.get("columns"));
        let columns = fields.iter().filter(|field| wanted.contains(&field.name)).map(|field| (field.name.clone(), field.title.clone())).collect();
        let limit = query.get("limit").and_then(Value::as_u64).filter(|limit| *limit > 0).unwrap_or(10) as usize;
        return Answer::Rows { columns, rows: filtered.iter().take(limit).map(|record| record.values.clone()).collect() };
    }
    let counted = query.get("aggregation").and_then(Value::as_str) == Some("count_true");
    let field_name = query.get("field_name").and_then(Value::as_str).unwrap_or_default();
    let weight = |record: &Record| {
        if counted { f64::from(u8::from(crate::format::truthy(record.values.get(field_name).unwrap_or(&Value::Null)))) } else { 1.0 }
    };
    let Some(group) = query.get("group_by_field_name").and_then(Value::as_str).filter(|name| !name.is_empty()) else {
        return Answer::Metric(filtered.iter().map(|record| weight(record)).sum());
    };
    let mut counts: Vec<(String, f64)> = Vec::new();
    for record in &filtered {
        let label = match record.values.get(group) {
            None | Some(Value::Null) => "Empty".to_owned(),
            Some(Value::String(text)) if text.is_empty() => "Empty".to_owned(),
            Some(Value::Array(items)) => {
                let joined = items.iter().map(|item| display(Some(item))).collect::<Vec<_>>().join(", ");
                if joined.is_empty() { "Empty".to_owned() } else { joined }
            }
            Some(Value::Object(_)) => "Attachment".to_owned(),
            Some(other) => display(Some(other)),
        };
        match counts.iter_mut().find(|(known, _)| *known == label) {
            Some((_, count)) => *count += weight(record),
            None => counts.push((label, weight(record))),
        }
    }
    let order = strings(widget.get("config").and_then(|config| config.get("category_order")));
    let line = kind == "line";
    counts.sort_by(|(a, va), (b, vb)| {
        let (ia, ib) = (order.iter().position(|label| label == a), order.iter().position(|label| label == b));
        if ia.is_some() || ib.is_some() {
            return ia.unwrap_or(usize::MAX).cmp(&ib.unwrap_or(usize::MAX));
        }
        if line {
            return natural_cmp(a, b);
        }
        vb.partial_cmp(va).unwrap_or(Ordering::Equal).then_with(|| a.cmp(b))
    });
    Answer::Series(counts)
}
