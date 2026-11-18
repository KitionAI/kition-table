//! What the picture shows, read from the document: the table and view the editor opens (its first
//! table and that table's first view) or the ones [`Shown`] names, with the view's records
//! filtered, sorted and grouped as the editor does, or a dashboard.
//!
//! A view's settings come from both places they are written: the editor's config keys
//! (`sort_items`, `group_items`, `group_field_name`, `hidden_field_names`, `row_height`) and the
//! engine's (`views.putSorts`, `views.putGroups`, `views.patchField`); filter conditions use the
//! editor's operators (`is`, `isNot`, `isAnyOf`, …) or the engine's (`eq`, `neq`, `gt`, …).

use std::cmp::Ordering;

use kition_table_file::{Dashboard, Field, FilterTree, Record, Table, TableFile, View};
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
    let records = file.list_records(table.id).map_err(failed)?;
    let shown_table = arrange(file, table, view, records, shown.rows)?;
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

/// `[(field, descending)]` from the editor's `[{field_name, direction}]` items.
fn named_items<'f>(config: &Map<String, Value>, key: &str, fields: &'f [Field]) -> Vec<(&'f Field, bool)> {
    config
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let name = item.get("field_name").and_then(Value::as_str)?;
            let field = fields.iter().find(|field| field.name == name)?;
            Some((field, item.get("direction").and_then(Value::as_str) == Some("desc")))
        })
        .collect()
}

/// What a view does to its table's records, as the editor applies it: its filter, then its
/// groups and sorts as one ordering (groups first). Each field comes with whether it runs
/// descending.
#[derive(Clone, Debug, Default)]
pub struct Arrangement {
    pub filter: Option<FilterTree>,
    pub groups: Vec<(Field, bool)>,
    pub sorts: Vec<(Field, bool)>,
}

impl Arrangement {
    /// The settings of `view` of a table with `fields`, read from the editor's config keys and
    /// the engine's (see the module's notes); none without a view.
    pub fn of(file: &TableFile, fields: &[Field], view: Option<&View>) -> Result<Self, String> {
        let Some(view) = view else {
            return Ok(Self::default());
        };
        let config = &view.config;
        let filter = file.load_filter_tree(view.id).map_err(failed)?;
        let by_id = |id: i64| fields.iter().find(|field| field.id == id);
        let mut groups: Vec<(&Field, bool)> = named_items(config, "group_items", fields);
        if groups.is_empty() {
            groups = file
                .list_view_groups(view.id)
                .map_err(failed)?
                .iter()
                .filter_map(|item| Some((by_id(item.field_id)?, item.direction == "desc")))
                .collect();
        }
        if groups.is_empty()
            && let Some(name) = config.get("group_field_name").and_then(Value::as_str)
            && let Some(field) = fields.iter().find(|field| field.name == name)
            && view.view_type == "grid"
        {
            groups.push((field, false));
        }
        let mut sorts: Vec<(&Field, bool)> = named_items(config, "sort_items", fields);
        if sorts.is_empty() {
            sorts = file
                .list_view_sorts(view.id)
                .map_err(failed)?
                .iter()
                .filter_map(|item| Some((by_id(item.field_id)?, item.direction == "desc")))
                .collect();
        }
        let owned = |items: Vec<(&Field, bool)>| items.into_iter().map(|(field, descending)| (field.clone(), descending)).collect();
        Ok(Self { filter, groups: owned(groups), sorts: owned(sorts) })
    }

    /// How many filter conditions apply.
    pub fn filters(&self) -> usize {
        self.filter.as_ref().map_or(0, count_conditions)
    }

    /// `records` as the view shows them: the ones its filter keeps, in its order.
    pub fn apply(&self, fields: &[Field], mut records: Vec<Record>) -> Vec<Record> {
        if let Some(tree) = &self.filter {
            records.retain(|record| matches_tree(tree, record, fields));
        }
        let ordering: Vec<&(Field, bool)> = self.groups.iter().chain(&self.sorts).collect();
        if !ordering.is_empty() {
            records.sort_by(|a, b| {
                for (field, descending) in &ordering {
                    let order = natural_cmp(&display(a.values.get(&field.name)), &display(b.values.get(&field.name)));
                    let order = if *descending { order.reverse() } else { order };
                    if order != Ordering::Equal {
                        return order;
                    }
                }
                Ordering::Equal
            });
        }
        records
    }
}

/// The records of table `table_id` as its view `view_id` shows them (filtered and in its order),
/// or every record in order without a view; all of them, not only the [`MAX_RECORDS`] a picture
/// lays out. A view that is not the table's is an error.
pub fn view_records(file: &TableFile, table_id: i64, view_id: Option<i64>) -> Result<Vec<Record>, String> {
    let table = file.get_table(table_id).map_err(|_| format!("not found: table {table_id}"))?;
    let records = file.list_records(table_id).map_err(failed)?;
    let view = match view_id {
        Some(id) => Some(table.views.iter().find(|view| view.id == id).ok_or_else(|| format!("not found: view {id} of table {table_id}"))?),
        None => None,
    };
    Ok(Arrangement::of(file, &table.fields, view)?.apply(&table.fields, records))
}

/// The view's settings applied to the table's records.
fn arrange(file: &TableFile, table: &Table, view: Option<View>, records: Vec<Record>, rows: Option<(usize, usize)>) -> Result<TableShown, String> {
    let fields = table.fields.clone();
    let total = records.len();
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
    let mut records = arrangement.apply(&fields, records);
    let groups: Vec<&Field> = arrangement.groups.iter().map(|(field, _)| field).collect();
    let sorts: Vec<&Field> = arrangement.sorts.iter().map(|(field, _)| field).collect();
    let shown = records.len();
    let first = match rows {
        Some((from, to)) if from == 0 || to < from || from > shown.max(1) => {
            return Err(format!("not found: rows {from} to {to}: the rows are counted from 1, and {shown} records are shown"));
        }
        Some((from, to)) => {
            records = records.into_iter().skip(from - 1).take(to + 1 - from).collect();
            from
        }
        None => 1,
    };
    records.truncate(MAX_RECORDS);

    let row_height = match config.get("row_height").and_then(Value::as_str) {
        Some("short") => 32.0,
        Some("tall") => 56.0,
        Some("extra_tall") => 88.0,
        _ => 40.0,
    };
    let primary = fields.iter().find(|field| field.is_primary).or_else(|| fields.first());
    let board_field = groups
        .first()
        .copied()
        .or_else(|| config.get("group_field_name").and_then(Value::as_str).and_then(|name| fields.iter().find(|field| field.name == name)))
        .or_else(|| fields.iter().find(|field| field.field_type == "single_select"))
        .or(primary)
        .cloned();
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
        board_field,
        cover_field,
        fields,
    })
}

/// A column's width when the view sets none, as the editor's grid draws it.
pub const DEFAULT_WIDTH: f64 = 200.0;

fn count_conditions(tree: &FilterTree) -> usize {
    tree.children
        .iter()
        .map(|child| match (&child.group, &child.condition) {
            (Some(group), _) => count_conditions(group),
            (None, Some(_)) => 1,
            _ => 0,
        })
        .sum()
}

fn matches_tree(tree: &FilterTree, record: &Record, fields: &[Field]) -> bool {
    let results = tree.children.iter().filter_map(|child| match (&child.group, &child.condition) {
        (Some(group), _) => Some(matches_tree(group, record, fields)),
        (None, Some(rule)) => {
            let field = fields.iter().find(|field| field.id == rule.field_id)?;
            condition(field, &rule.operator, &rule.value, record.values.get(&field.name))
        }
        _ => None,
    });
    let results: Vec<bool> = results.collect();
    if results.is_empty() {
        return true;
    }
    if tree.conjunction == "or" { results.iter().any(|ok| *ok) } else { results.iter().all(|ok| *ok) }
}

/// A cell's value as filters compare it.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Empty,
    Number(f64),
    Day(i64),
    Flag(bool),
    Text(String),
    List(Vec<String>),
}

const DATE_TYPES: [&str; 4] = ["date", "datetime", "created_time", "last_modified_time"];
const NUMBER_TYPES: [&str; 3] = ["number", "rating", "auto_number"];

fn day_of(value: &Value) -> Option<i64> {
    let millis = match value {
        Value::Number(number) => number.as_f64()? as i64,
        Value::String(text) => crate::format::parse_time(text)?,
        _ => return None,
    };
    Some(millis.div_euclid(86_400_000))
}

fn cell_of(field: &Field, value: Option<&Value>) -> Cell {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return if field.field_type == "checkbox" { Cell::Flag(false) } else { Cell::Empty };
    };
    let kind = field.field_type.as_str();
    if DATE_TYPES.contains(&kind) {
        return day_of(value).map_or(Cell::Empty, Cell::Day);
    }
    if NUMBER_TYPES.contains(&kind) {
        return match value {
            Value::Number(number) => number.as_f64().map_or(Cell::Empty, Cell::Number),
            Value::String(text) => text.trim().parse::<f64>().map_or(Cell::Empty, Cell::Number),
            _ => Cell::Empty,
        };
    }
    match kind {
        "checkbox" => Cell::Flag(crate::format::truthy(value)),
        "multi_select" | "user" | "created_by" | "last_modified_by" | "link_to_record" => {
            let items: Vec<String> = match value {
                Value::Array(items) => items.iter().map(|item| display(Some(item))).filter(|text| !text.is_empty()).collect(),
                other => vec![display(Some(other))],
            };
            if items.is_empty() { Cell::Empty } else { Cell::List(items) }
        }
        _ => {
            let text = display(Some(value));
            if text.trim().is_empty() { Cell::Empty } else { Cell::Text(text.trim().to_owned()) }
        }
    }
}

/// A filter value in the cell's terms.
fn value_as(cell: &Cell, value: &Value) -> Cell {
    match cell {
        Cell::Day(_) => day_of(value).map_or(Cell::Empty, Cell::Day),
        Cell::Number(_) => match value {
            Value::Number(number) => number.as_f64().map_or(Cell::Empty, Cell::Number),
            Value::String(text) => text.trim().parse::<f64>().map_or_else(|_| Cell::Text(text.clone()), Cell::Number),
            _ => Cell::Empty,
        },
        Cell::Flag(_) => Cell::Flag(crate::format::truthy(value)),
        _ => match value {
            Value::Null => Cell::Empty,
            Value::Array(items) => Cell::List(items.iter().map(|item| display(Some(item))).collect()),
            other => Cell::Text(display(Some(other))),
        },
    }
}

fn texts(cell: &Cell) -> Vec<String> {
    match cell {
        Cell::Empty => Vec::new(),
        Cell::List(items) => items.clone(),
        Cell::Text(text) => vec![text.clone()],
        Cell::Number(number) => vec![crate::format::number(*number)],
        Cell::Day(day) => vec![day.to_string()],
        Cell::Flag(flag) => vec![flag.to_string()],
    }
}

fn same(a: &Cell, b: &Cell) -> bool {
    match (a, b) {
        (Cell::Number(a), Cell::Number(b)) => (a - b).abs() < 1e-9,
        (Cell::List(a), Cell::Text(b)) | (Cell::Text(b), Cell::List(a)) => a.len() == 1 && a.iter().all(|item| item == b),
        _ => a == b,
    }
}

/// Whether a condition holds; `None` when its operator is unknown (it is ignored, as the editor
/// leaves out a condition it cannot apply).
fn condition(field: &Field, operator: &str, value: &Value, raw: Option<&Value>) -> Option<bool> {
    let cell = cell_of(field, raw);
    let wanted = value_as(&cell, value);
    let lower = |text: &str| text.to_lowercase();
    let contains = || {
        let needle = lower(&display(Some(value)));
        texts(&cell).iter().any(|text| lower(text).contains(&needle))
    };
    let empty = matches!(cell, Cell::Empty) || matches!(&cell, Cell::List(items) if items.is_empty());
    let compare = || match (&cell, &wanted) {
        (Cell::Number(a), Cell::Number(b)) => a.partial_cmp(b),
        (Cell::Day(a), Cell::Day(b)) => Some(a.cmp(b)),
        _ => None,
    };
    let wanted_list = texts(&wanted);
    let have = texts(&cell);
    let any_of = || have.iter().any(|item| wanted_list.iter().any(|want| want.eq_ignore_ascii_case(item)));
    let all_of = || wanted_list.iter().all(|want| have.iter().any(|item| want.eq_ignore_ascii_case(item)));
    Some(match operator {
        "is" | "eq" | "equals" | "isWithIn" => same(&cell, &wanted),
        "isNot" | "neq" | "ne" | "not_equals" => !same(&cell, &wanted),
        "contains" => contains(),
        "doesNotContain" | "not_contains" | "notContains" => !contains(),
        "isEmpty" | "is_empty" | "empty" | "falsy" => empty || cell == Cell::Flag(false),
        "isNotEmpty" | "is_not_empty" | "not_empty" | "truthy" => !(empty || cell == Cell::Flag(false)),
        "isGreater" | "gt" | "greater_than" | "isAfter" => compare() == Some(Ordering::Greater),
        "isGreaterEqual" | "gte" | "isOnOrAfter" => matches!(compare(), Some(Ordering::Greater | Ordering::Equal)),
        "isLess" | "lt" | "less_than" | "isBefore" => compare() == Some(Ordering::Less),
        "isLessEqual" | "lte" | "isOnOrBefore" => matches!(compare(), Some(Ordering::Less | Ordering::Equal)),
        "isAnyOf" | "hasAnyOf" | "in" => any_of(),
        "isNoneOf" | "hasNoneOf" | "not_in" => !any_of(),
        "hasAllOf" => all_of(),
        "isExactly" => all_of() && have.len() == wanted_list.len(),
        "isNotExactly" => !(all_of() && have.len() == wanted_list.len()),
        _ => return None,
    })
}

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
