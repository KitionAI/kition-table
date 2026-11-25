//! Writes the conformance fixtures (`conformance/`, KHP conformance): `basic.ktable` (no
//! diagnostics), `edit.json` (steps that change it), and one `check-<code>.ktable` per code of
//! `agent/checks.json`, on which `table_check` reports that code. Every fixture is made with the
//! engine's own commands from a new document (its starter table "Tasks": `title`, the required
//! primary field, `status`, a single select of "To do", "In progress", "Done", and `notes`).
//!
//! `cargo run -p kition-table-kit --example checks -- conformance [fixture names]` (with names,
//! only those are written; the others keep their bytes).

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

/// The starter table's id: the first table of a new document.
const TASKS: i64 = 1;

fn call(doc: &mut TableDocument, command: &str, params: Value) -> Result<Value, String> {
    doc.execute(command, params).map(|answer| answer["result"].clone())
}

fn id(value: &Value) -> Result<i64, String> {
    value["id"].as_i64().ok_or_else(|| format!("no id in {value}"))
}

fn task(doc: &mut TableDocument, values: Value) -> Result<Value, String> {
    call(doc, "table.records.create", json!({ "tableId": TASKS, "values": values }))
}

fn field(doc: &mut TableDocument, table: i64, spec: Value) -> Result<i64, String> {
    let mut params = json!({ "tableId": table });
    if let (Some(target), Some(spec)) = (params.as_object_mut(), spec.as_object()) {
        target.extend(spec.clone());
    }
    id(&call(doc, "table.fields.create", params)?)
}

/// A second table, "Projects", with one record.
fn projects(doc: &mut TableDocument) -> Result<i64, String> {
    let table = id(&call(doc, "table.tables.create", json!({ "title": "Projects" }))?)?;
    call(doc, "table.records.create", json!({ "tableId": table, "values": { "title": "Launch" } }))?;
    Ok(table)
}

fn table(name: &str, build: impl FnOnce(&mut TableDocument) -> Result<(), String>) -> Result<Vec<u8>, String> {
    let mut doc = TableDocument::create(name, 0, 0)?;
    build(&mut doc)?;
    doc.save(None)
}

fn main() -> Result<(), String> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).ok_or("usage: checks <conformance folder> [fixture names]")?);
    let only: Vec<String> = std::env::args().skip(2).collect();
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    for (name, bytes) in fixtures()? {
        if only.is_empty() || only.iter().any(|wanted| wanted == name) {
            std::fs::write(dir.join(name), bytes).map_err(|error| error.to_string())?;
        }
    }
    if only.is_empty() || only.iter().any(|wanted| wanted == "edit.json") {
        let edit = json!({ "steps": [{ "command": "table.records.create", "params": { "tableId": TASKS, "values": { "title": "Ship the plan", "status": "In progress" } } }] });
        let text = serde_json::to_string_pretty(&edit).map_err(|error| error.to_string())?;
        std::fs::write(dir.join("edit.json"), format!("{text}\n")).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Every `.ktable` fixture, by file name.
pub fn fixtures() -> Result<Vec<(&'static str, Vec<u8>)>, String> {
    Ok(vec![
        (
            "basic.ktable",
            table("Tasks.ktable", |doc| {
                task(doc, json!({ "title": "Write the plan", "status": "To do" }))?;
                task(doc, json!({ "title": "Review the plan", "status": "Done", "notes": "Looks good" }))?;
                Ok(())
            })?,
        ),
        (
            "check-broken-link.ktable",
            table("Links.ktable", |doc| {
                let target = projects(doc)?;
                field(doc, TASKS, json!({ "title": "Project", "type": "link_to_record", "options": { "target_table_id": target } }))?;
                task(doc, json!({ "title": "Write the plan" }))?;
                call(doc, "table.tables.delete", json!({ "tableId": target })).map(|_| ())
            })?,
        ),
        (
            "check-broken-lookup.ktable",
            table("Lookups.ktable", |doc| {
                let target = projects(doc)?;
                let link = field(doc, TASKS, json!({ "title": "Project", "type": "link_to_record", "options": { "target_table_id": target } }))?;
                let target_title = call(doc, "table.fields.list", json!({ "tableId": target }))?;
                let read = target_title
                    .as_array()
                    .or_else(|| target_title["items"].as_array())
                    .and_then(|fields| fields.first())
                    .map(id)
                    .ok_or("no field in Projects")??;
                field(
                    doc,
                    TASKS,
                    json!({ "title": "Project name", "type": "lookup", "options": { "link_field_id": link, "target_field_id": read } }),
                )?;
                task(doc, json!({ "title": "Write the plan" }))?;
                call(doc, "table.fields.delete", json!({ "tableId": TASKS, "fieldId": link })).map(|_| ())
            })?,
        ),
        (
            "check-dashboard-missing-table.ktable",
            table("Dashboards.ktable", |doc| {
                let target = projects(doc)?;
                call(
                    doc,
                    "table.dashboards.upsert",
                    json!({ "dashboardId": "projects", "title": "Projects", "source_table_id": target, "layout": [], "widgets": [] }),
                )?;
                call(doc, "table.tables.delete", json!({ "tableId": target })).map(|_| ())
            })?,
        ),
        (
            "check-formula-error.ktable",
            table("Formulas.ktable", |doc| {
                field(doc, TASKS, json!({ "title": "Score", "type": "formula", "formula": "1 +" }))?;
                task(doc, json!({ "title": "Write the plan" })).map(|_| ())
            })?,
        ),
        (
            "check-formula-unknown-field.ktable",
            table("Formulas.ktable", |doc| {
                field(doc, TASKS, json!({ "title": "Score", "type": "formula", "formula": "{effort} * 2" }))?;
                task(doc, json!({ "title": "Write the plan" })).map(|_| ())
            })?,
        ),
        (
            "check-field-type-mismatch.ktable",
            table("Types.ktable", |doc| {
                field(doc, TASKS, json!({ "title": "Effort", "type": "number" }))?;
                task(doc, json!({ "title": "Write the plan", "effort": "three" })).map(|_| ())
            })?,
        ),
        (
            "check-select-option-missing.ktable",
            table("Options.ktable", |doc| task(doc, json!({ "title": "Write the plan", "status": "Blocked" })).map(|_| ()))?,
        ),
        (
            "check-required-empty.ktable",
            table("Required.ktable", |doc| {
                task(doc, json!({ "title": "Write the plan" }))?;
                // The notes field becomes required after the record was written without notes.
                call(doc, "table.fields.update", json!({ "tableId": TASKS, "fieldId": 3, "required": true })).map(|_| ())
            })?,
        ),
        (
            "check-duplicate-primary.ktable",
            table("Duplicates.ktable", |doc| {
                task(doc, json!({ "title": "Write the plan" }))?;
                task(doc, json!({ "title": "Write the plan" })).map(|_| ())
            })?,
        ),
    ])
}
