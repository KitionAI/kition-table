//! `document.check` (KHP A20, A38): what is wrong with a table document, as diagnostics with a
//! target and a fix command. The codes are catalogued in `agent/checks.json`, each with a fixture
//! in `conformance/` that reports it.
//!
//! Targets name a table by its title (`sheet`) and the object by `path`:
//! `tables/<tableId>/fields/<fieldId>`, `tables/<tableId>/records/<recordId>/fields/<fieldId>`, or
//! `dashboards/<dashboardId>`. Fixes are engine commands as `<prefix>_run` takes them; where the
//! right value is the author's to choose, the params name the object and the message says which
//! key to set.

use std::collections::{BTreeMap, BTreeSet};

use kition_kit::diagnostics::{Diagnostic, Severity, Target};
use kition_table_file::{Field, Record, Table, TableFile, formula_references};
use serde_json::{Map, Value, json};

const FORMULA_ERROR: &str = "#ERROR:";

/// Field types whose values the engine derives; their values are never the author's.
const DERIVED: [&str; 8] = ["formula", "lookup", "rollup", "link_to_record", "created_time", "last_modified_time", "created_by", "last_modified_by"];

fn field_target(table: &Table, field: &Field) -> Target {
    Target { sheet: Some(table.title.clone()), path: Some(format!("tables/{}/fields/{}", table.id, field.id)), ..Target::default() }
}

fn cell_target(table: &Table, record: &Record, field: &Field) -> Target {
    Target {
        sheet: Some(table.title.clone()),
        path: Some(format!("tables/{}/records/{}/fields/{}", table.id, record.id, field.id)),
        ..Target::default()
    }
}

fn label(field: &Field) -> &str {
    if field.title.is_empty() { &field.name } else { &field.title }
}

fn blank(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(text)) => text.trim().is_empty(),
        Some(Value::Array(items)) => items.is_empty(),
        Some(_) => false,
    }
}

fn option_id(options: &Map<String, Value>, key: &str) -> Option<i64> {
    match options.get(key)? {
        Value::Number(number) => number.as_i64().filter(|id| *id > 0),
        Value::String(text) => text.parse().ok().filter(|id: &i64| *id > 0),
        _ => None,
    }
}

/// `YYYY-MM-DD`, optionally followed by a time.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 10
        && bytes.iter().take(10).enumerate().all(|(at, byte)| if at == 4 || at == 7 { *byte == b'-' } else { byte.is_ascii_digit() })
        && (bytes.len() == 10 || matches!(bytes.get(10), Some(b'T' | b' ')))
}

/// The value the field's type would store for `value`, when it does not already hold one:
/// `Some(fixed)` (a number written as text becomes the number, anything else `null`), or `None`
/// when the value fits the type.
fn mismatch(field_type: &str, value: &Value) -> Option<Value> {
    if value.is_null() {
        return None;
    }
    let fits = match field_type {
        "number" | "rating" => value.is_number(),
        "checkbox" => value.is_boolean(),
        "date" | "datetime" => value.as_str().is_some_and(is_date),
        "multi_select" => value.as_array().is_some_and(|items| items.iter().all(Value::is_string)),
        "single_select" | "text" | "long_text" | "url" => value.is_string(),
        _ => true,
    };
    if fits {
        return None;
    }
    let number = || value.as_str().and_then(|text| text.trim().parse::<f64>().ok()).and_then(serde_json::Number::from_f64).map(Value::Number);
    Some(match field_type {
        "number" | "rating" => number().unwrap_or(Value::Null),
        "text" | "long_text" | "url" | "single_select" if value.is_number() || value.is_boolean() => Value::String(value.to_string()),
        _ => Value::Null,
    })
}

fn choices(field: &Field) -> Vec<String> {
    field
        .options
        .get("choices")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

struct Checker<'f> {
    tables: &'f [Table],
    fields: &'f BTreeMap<i64, Vec<Field>>,
    found: Vec<Diagnostic>,
}

impl Checker<'_> {
    fn field_by_id(&self, field_id: i64) -> Option<&Field> {
        self.fields.values().flatten().find(|field| field.id == field_id)
    }

    fn table_exists(&self, table_id: i64) -> bool {
        self.tables.iter().any(|table| table.id == table_id)
    }

    /// Whether a link field reaches a live table.
    fn link_resolves(&self, field: &Field) -> bool {
        option_id(&field.options, "target_table_id").is_some_and(|table| self.table_exists(table))
    }

    fn fields(&mut self, table: &Table, fields: &[Field]) {
        let names: BTreeSet<&str> = fields.iter().map(|field| field.name.as_str()).collect();
        for field in fields {
            match field.field_type.as_str() {
                "link_to_record" if !self.link_resolves(field) => {
                    let fallback = self.tables.first().map_or(table.id, |first| first.id);
                    self.found.push(
                        Diagnostic::new(
                            "table.broken-link",
                            Severity::Error,
                            format!(
                                "The link field \"{}\" of \"{}\" points at a table that does not exist; set options.target_table_id to a table.",
                                label(field),
                                table.title
                            ),
                        )
                        .on(field_target(table, field))
                        .fix(
                            "table.fields.update",
                            Some(json!({ "tableId": table.id, "fieldId": field.id, "options": { "target_table_id": fallback } })),
                        ),
                    );
                }
                "lookup" | "rollup" => self.lookup(table, field),
                "formula" => {
                    let unknown: Vec<String> = formula_references(&field.formula).into_iter().filter(|name| !names.contains(name.as_str())).collect();
                    if !unknown.is_empty() {
                        self.found.push(
                            Diagnostic::new(
                                "table.formula-unknown-field",
                                Severity::Error,
                                format!(
                                    "The formula of \"{}\" in \"{}\" names fields that do not exist ({}); rewrite its formula.",
                                    label(field),
                                    table.title,
                                    unknown.join(", ")
                                ),
                            )
                            .on(field_target(table, field))
                            .fix("table.fields.update", Some(json!({ "tableId": table.id, "fieldId": field.id, "formula": field.formula }))),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn lookup(&mut self, table: &Table, field: &Field) {
        let link = option_id(&field.options, "link_field_id").and_then(|id| self.field_by_id(id));
        let problem = match link {
            None => Some("its link field does not exist"),
            Some(link) if link.field_type != "link_to_record" || link.table_id != table.id => {
                Some("its link field is not a link field of this table")
            }
            // A broken link is reported once, on the link field (table.broken-link).
            Some(link) if !self.link_resolves(link) => None,
            Some(link) => {
                let target_table = option_id(&link.options, "target_table_id");
                let target = option_id(&field.options, "target_field_id");
                // A count rollup counts the linked records: it reads no field.
                let counts = field.field_type == "rollup" && field.options.get("aggregation").and_then(Value::as_str) == Some("count");
                match target.and_then(|id| self.field_by_id(id)) {
                    Some(target) if Some(target.table_id) == target_table => None,
                    None if target.is_none() && counts => None,
                    _ => Some("the field it reads does not exist in the linked table"),
                }
            }
        };
        if let Some(problem) = problem {
            self.found.push(
                Diagnostic::new(
                    "table.broken-lookup",
                    Severity::Error,
                    format!(
                        "The {} field \"{}\" of \"{}\" shows nothing: {problem}; set options.link_field_id and options.target_field_id, or delete the field.",
                        field.field_type,
                        label(field),
                        table.title
                    ),
                )
                .on(field_target(table, field))
                .fix("table.fields.delete", Some(json!({ "tableId": table.id, "fieldId": field.id }))),
            );
        }
    }

    fn records(&mut self, table: &Table, fields: &[Field], records: &[Record]) {
        let mut primaries: BTreeMap<String, i64> = BTreeMap::new();
        let primary = fields.iter().find(|field| field.is_primary);
        for record in records {
            for field in fields {
                let value = record.values.get(&field.name);
                let derived = DERIVED.contains(&field.field_type.as_str());
                if field.field_type == "formula" {
                    if let Some(message) = value.and_then(Value::as_str).and_then(|text| text.strip_prefix(FORMULA_ERROR)) {
                        self.found.push(
                            Diagnostic::new(
                                "table.formula-error",
                                Severity::Error,
                                format!(
                                    "The formula of \"{}\" in \"{}\" fails on record {}:{message}; rewrite its formula.",
                                    label(field),
                                    table.title,
                                    record.id
                                ),
                            )
                            .on(cell_target(table, record, field))
                            .fix("table.fields.update", Some(json!({ "tableId": table.id, "fieldId": field.id, "formula": field.formula }))),
                        );
                    }
                    continue;
                }
                if derived {
                    continue;
                }
                if let Some(fixed) = value.and_then(|value| mismatch(&field.field_type, value)) {
                    self.found.push(
                        Diagnostic::new(
                            "table.field-type-mismatch",
                            Severity::Error,
                            format!(
                                "Record {} of \"{}\" holds {} in the {} field \"{}\".",
                                record.id,
                                table.title,
                                value.map(Value::to_string).unwrap_or_default(),
                                field.field_type,
                                label(field)
                            ),
                        )
                        .on(cell_target(table, record, field))
                        .fix(
                            "table.records.update",
                            Some(json!({ "tableId": table.id, "recordId": record.id, "values": { field.name.clone(): fixed } })),
                        ),
                    );
                    continue;
                }
                if field.field_type == "single_select" || field.field_type == "multi_select" {
                    let known = choices(field);
                    let values: Vec<&str> = match value {
                        Some(Value::String(text)) if !text.is_empty() => vec![text.as_str()],
                        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).collect(),
                        _ => Vec::new(),
                    };
                    let missing: Vec<&str> = values.into_iter().filter(|value| !known.iter().any(|choice| choice == value)).collect();
                    if !missing.is_empty() {
                        let mut all = known.clone();
                        all.extend(missing.iter().map(|value| (*value).to_owned()));
                        let mut options = field.options.clone();
                        options.insert("choices".into(), json!(all));
                        self.found.push(
                            Diagnostic::new(
                                "table.select-option-missing",
                                Severity::Warning,
                                format!(
                                    "Record {} of \"{}\" selects {} in \"{}\", which is not one of its options.",
                                    record.id,
                                    table.title,
                                    missing.join(", "),
                                    label(field)
                                ),
                            )
                            .on(cell_target(table, record, field))
                            .fix("table.fields.update", Some(json!({ "tableId": table.id, "fieldId": field.id, "options": options }))),
                        );
                    }
                }
                if field.required && blank(value) {
                    self.found.push(
                        Diagnostic::new(
                            "table.required-empty",
                            Severity::Warning,
                            format!(
                                "Record {} of \"{}\" leaves the required field \"{}\" empty; set values.{}.",
                                record.id,
                                table.title,
                                label(field),
                                field.name
                            ),
                        )
                        .on(cell_target(table, record, field))
                        .fix("table.records.update", Some(json!({ "tableId": table.id, "recordId": record.id }))),
                    );
                }
            }
            if let Some(primary) = primary {
                let value = record.values.get(&primary.name);
                if !blank(value) {
                    let key = value.map(Value::to_string).unwrap_or_default();
                    if let Some(first) = primaries.get(&key) {
                        self.found.push(
                            Diagnostic::new(
                                "table.duplicate-primary",
                                Severity::Warning,
                                format!(
                                    "Records {first} and {} of \"{}\" have the same {} {key}, so links and lookups cannot tell them apart; set values.{} of one.",
                                    record.id,
                                    table.title,
                                    label(primary),
                                    primary.name
                                ),
                            )
                            .on(cell_target(table, record, primary))
                            .fix("table.records.update", Some(json!({ "tableId": table.id, "recordId": record.id }))),
                        );
                    } else {
                        primaries.insert(key, record.id);
                    }
                }
            }
        }
    }
}

/// The document's diagnostics, errors first ([`kition_kit::diagnostics::report`] sorts the same).
pub fn check(file: &TableFile) -> Vec<Diagnostic> {
    let Ok(tables) = file.list_tables() else {
        return vec![Diagnostic::new("table.unreadable", Severity::Error, "The document's tables cannot be read.")];
    };
    let fields: BTreeMap<i64, Vec<Field>> = tables.iter().map(|table| (table.id, file.list_fields(table.id).unwrap_or_default())).collect();
    let mut checker = Checker { tables: &tables, fields: &fields, found: Vec::new() };
    for table in &tables {
        let table_fields = fields.get(&table.id).map(Vec::as_slice).unwrap_or_default();
        checker.fields(table, table_fields);
        let records = file.list_records(table.id).unwrap_or_default();
        checker.records(table, table_fields, &records);
    }
    for dashboard in file.list_dashboards().unwrap_or_default() {
        let Some(config) = dashboard.config.as_object() else { continue };
        if !config.contains_key("source_table_id") || option_id(config, "source_table_id").is_some_and(|table| checker.table_exists(table)) {
            continue;
        }
        let mut found = Diagnostic::new(
            "table.dashboard-missing-table",
            Severity::Error,
            format!("The dashboard \"{}\" reads a table that does not exist; set its source_table_id to a table.", dashboard.name),
        )
        .on(Target { path: Some(format!("dashboards/{}", dashboard.id)), ..Target::default() });
        if let Some(first) = tables.first() {
            let mut body = kition_table_api::dashboards::dashboard_json(&dashboard);
            if let Some(body) = body.as_object_mut() {
                body.remove("id");
                body.insert("dashboardId".into(), json!(dashboard.id));
                body.insert("source_table_id".into(), json!(first.id));
            }
            found = found.fix("table.dashboards.upsert", Some(body));
        }
        checker.found.push(found);
    }
    let mut found = checker.found;
    found.sort_by_key(|diagnostic| diagnostic.severity);
    found
}
