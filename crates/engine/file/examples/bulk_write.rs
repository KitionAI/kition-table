//! Times a table's large writes natively, as the grid sends them (one `transact` of one
//! `cells.set`): a number column cleared in every record, its undo and redo, a paste of a
//! different value into every record, and a fill of one value into every record. Steps that are
//! not cell writes mix the history: `create` adds a record at the end and `delete` deletes the
//! first one, each one transaction.
//!
//! `cargo run --release -p kition-table-file --example bulk_write -- [<file.ktable>]`
//!
//! With a file (the grid's performance fixture, `kition-table-kit`'s `perf_fixture` output) it
//! opens a copy of it in memory; without one it builds a table of `BULK_ROWS` records (default
//! 20,000) in 12 fields. `BULK_PHASES` picks the steps (default `clear,undo,redo,paste,fill`); a
//! step repeated writes what is already there (a second `paste` changes nothing), and
//! `create,clear,undo,undo` undoes a column clear and then the record created before it. Each step
//! prints its time, the CPU time it took, and the process's resident memory after it.

#![allow(clippy::disallowed_methods, reason = "a native timing tool reads the clock")]

use std::time::Instant;

use kition_kdm::Origin;
use kition_table_file::{CellEdit, CreateOptions, FieldSpec, TableFile, TableOp, TableSpec};
use serde_json::{Map, Value, json};

fn timed<R>(label: &str, run: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
    let (started, cpu) = (Instant::now(), process("cputime"));
    let out = run()?;
    let seconds = started.elapsed().as_secs_f64();
    let cpu = process("cputime").zip(cpu).map(|(after, before)| clock_seconds(&after) - clock_seconds(&before));
    let resident = process("rss").and_then(|kb| kb.parse::<u64>().ok()).map(|kb| kb / 1024);
    match (cpu, resident) {
        (Some(cpu), Some(mb)) => eprintln!("{label}: {seconds:.2} s, cpu {cpu:.2} s, resident {mb} MB"),
        _ => eprintln!("{label}: {seconds:.2} s"),
    }
    Ok(out)
}

/// One `ps` column of this process (`None` where there is no `ps`).
fn process(column: &str) -> Option<String> {
    let out = std::process::Command::new("ps").args(["-o", &format!("{column}="), "-p", &std::process::id().to_string()]).output().ok()?;
    Some(String::from_utf8(out.stdout).ok()?.trim().to_owned())
}

/// `[[hh:]mm:]ss.cc` in seconds.
fn clock_seconds(clock: &str) -> f64 {
    clock.split(':').fold(0.0, |total, part| total * 60.0 + part.trim().parse::<f64>().unwrap_or(0.0))
}

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// A document of one table of `rows` records: 8 text and 4 number fields.
fn built(rows: usize) -> Result<TableFile, String> {
    let mut file = TableFile::create_in_memory(&CreateOptions { title: "Bulk".into(), ..Default::default() }).map_err(text)?;
    let field = |index: usize, field_type: &str| FieldSpec {
        name: format!("{}{index}", if field_type == "text" { "t" } else { "n" }),
        title: format!("{field_type} {index}"),
        field_type: field_type.into(),
        primary: index == 0 && field_type == "text",
        ..Default::default()
    };
    let mut fields: Vec<FieldSpec> = (0..8).map(|index| field(index, "text")).collect();
    fields.extend((0..4).map(|index| field(index, "number")));
    let table = file.create_table(&TableSpec { title: "Rows".into(), fields, ..Default::default() }).map_err(text)?;
    for start in (0..rows).step_by(2_000) {
        let records: Vec<Map<String, Value>> = (start..rows.min(start + 2_000))
            .map(|row| {
                let mut values: Map<String, Value> =
                    (0..8).map(|column| (format!("t{column}"), json!(format!("row {row} words {column}")))).collect();
                values.extend((0..4).map(|column| (format!("n{column}"), json!(row * 10 + column))));
                values
            })
            .collect();
        file.transact(&[TableOp::RecordsCreate { table_id: table.id, at: None, records }]).map_err(text)?;
    }
    TableFile::open_bytes(&file.to_bytes().map_err(text)?).map_err(text)
}

fn main() -> Result<(), String> {
    let mut file = match std::env::args().nth(1) {
        Some(path) => timed("open", || TableFile::open_bytes(&std::fs::read(&path).map_err(text)?).map_err(text))?,
        None => {
            let rows = std::env::var("BULK_ROWS").ok().and_then(|rows| rows.parse().ok()).unwrap_or(20_000);
            timed("build", || built(rows))?
        }
    };
    file.set_origin(Origin::person("user:bench"));
    let table = file.list_tables().map_err(text)?.into_iter().next().ok_or("no table")?;
    let number = table.fields.iter().find(|field| field.field_type == "number").map(|field| field.name.clone()).ok_or("no number field")?;
    let keys: Vec<String> = file.list_record_values(table.id, &[]).map_err(text)?.into_iter().map(|record| record.row_key).collect();
    eprintln!("{} records; writing {number}", keys.len());
    let cells_set = |value: &dyn Fn(usize) -> Value| TableOp::CellsSet {
        table_id: table.id,
        records: keys
            .iter()
            .enumerate()
            .map(|(index, key)| CellEdit { row_key: key.clone(), values: Map::from_iter([(number.clone(), value(index))]) })
            .collect(),
    };
    let phases = std::env::var("BULK_PHASES").unwrap_or_else(|_| "clear,undo,redo,paste,fill".into());
    for phase in phases.split(',').filter(|phase| !phase.is_empty()) {
        match phase {
            "clear" => {
                let op = cells_set(&|_| Value::Null);
                timed("clear", || file.transact(&[op]).map_err(text))?;
            }
            "undo" => {
                timed("undo", || file.undo().map_err(text))?;
            }
            "redo" => {
                timed("redo", || file.redo().map_err(text))?;
            }
            "paste" => {
                let op = cells_set(&|index| json!(index));
                timed("paste", || file.transact(&[op]).map_err(text))?;
            }
            "fill" => {
                let op = cells_set(&|_| json!(7));
                timed("fill", || file.transact(&[op]).map_err(text))?;
            }
            "create" => {
                let op = TableOp::RecordsCreate { table_id: table.id, at: None, records: vec![Map::from_iter([(number.clone(), json!(1))])] };
                timed("create", || file.transact(&[op]).map_err(text))?;
            }
            "delete" => {
                let op = TableOp::RecordsDelete { table_id: table.id, row_keys: keys.first().cloned().into_iter().collect() };
                timed("delete", || file.transact(&[op]).map_err(text))?;
            }
            other => return Err(format!("unknown phase {other}")),
        }
    }
    Ok(())
}
