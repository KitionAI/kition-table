//! Large writes on a table (case regressions/issue-table-bulk-write): a whole column cleared,
//! pasted (a different value in every record) and filled (one value in every record), each one
//! `transact` of one `cells.set` as the grid sends it, undone and redone as one step, and the same
//! on a second copy of the document that hears of each write.
//!
//! A write costs about the same per record whatever its size. Before, the replica noted each
//! written record in its undo footprint with a search and a sort of everything noted so far, and
//! each record's edit read the table's link fields again, so clearing a column of 100,000 records
//! took minutes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
#![allow(clippy::disallowed_methods, reason = "native timing tests read the clock")]

use std::time::Instant;

use kition_kdm::Origin;
use kition_table_file::{CellEdit, CreateOptions, FieldSpec, TableFile, TableOp, TableSpec};
use serde_json::{Map, Value, json};

/// A document with one table of `count` records: a title, a number (`amount`) and a text
/// (`notes`) in each. Answers it, the table's id, and the records' row keys in order.
fn table(count: usize) -> (TableFile, i64, Vec<String>) {
    let mut file = TableFile::create_in_memory(&CreateOptions { title: "Bulk".into(), ..Default::default() }).unwrap();
    file.set_origin(Origin::person("user:test"));
    let field = |name: &str, field_type: &str, primary: bool| FieldSpec {
        name: name.into(),
        title: name.into(),
        field_type: field_type.into(),
        primary,
        ..Default::default()
    };
    let fields = vec![field("title", "text", true), field("amount", "number", false), field("notes", "text", false)];
    let table = file.create_table(&TableSpec { title: "Rows".into(), fields, ..Default::default() }).unwrap();
    for start in (0..count).step_by(1_000) {
        let records: Vec<Map<String, Value>> = (start..count.min(start + 1_000))
            .map(|row| json!({ "title": format!("Record {row}"), "amount": row, "notes": format!("Note {row}") }).as_object().cloned().unwrap())
            .collect();
        file.transact(&[TableOp::RecordsCreate { table_id: table.id, at: None, records }]).unwrap();
    }
    let keys = file.list_record_values(table.id, &[]).unwrap().into_iter().map(|record| record.row_key).collect();
    (file, table.id, keys)
}

/// Every record's values, in order.
fn values(file: &TableFile, table: i64) -> Vec<Map<String, Value>> {
    file.list_records(table).unwrap().into_iter().map(|record| record.values).collect()
}

/// Every record's `amount` and `notes`, in order.
fn cells(file: &TableFile, table: i64) -> Vec<(Value, Value)> {
    values(file, table)
        .into_iter()
        .map(|values| {
            let cell = |name: &str| values.get(name).cloned().unwrap_or(Value::Null);
            (cell("amount"), cell("notes"))
        })
        .collect()
}

/// One `cells.set` of `values(index)` in every record.
fn set_cells(file: &mut TableFile, table: i64, keys: &[String], values: impl Fn(usize) -> Value) {
    let records =
        keys.iter().enumerate().map(|(index, key)| CellEdit { row_key: key.clone(), values: values(index).as_object().cloned().unwrap() }).collect();
    file.transact(&[TableOp::CellsSet { table_id: table, records }]).unwrap();
}

/// Sends `to` what it lacks of `from`, as the relay does.
fn send(from: &TableFile, to: &mut TableFile) {
    let since = to.replica_version();
    to.import_replica_changes(&from.export_replica_changes(&since).unwrap()).unwrap();
}

fn converged(file: &TableFile, peer: &TableFile, table: i64) {
    assert_eq!(values(file, table), values(peer, table), "the peer shows what the writer shows");
    assert_eq!(file.replica().state_hash().unwrap(), peer.replica().state_hash().unwrap());
}

fn all_cleared(file: &TableFile, table: i64) -> bool {
    cells(file, table).iter().all(|(amount, notes)| amount.is_null() && notes.is_null())
}

#[test]
fn a_column_cleared_pasted_and_filled_undoes_redoes_and_converges() {
    let (mut file, table, keys) = table(3_000);
    let original = cells(&file, table);
    let mut peer = TableFile::open_bytes(&file.to_bytes().unwrap()).unwrap();

    // A clear of both columns in every record, as one undo step.
    set_cells(&mut file, table, &keys, |_| json!({ "amount": null, "notes": null }));
    assert!(all_cleared(&file, table));
    send(&file, &mut peer);
    converged(&file, &peer, table);
    assert!(file.undo().unwrap().0);
    assert_eq!(cells(&file, table), original, "one undo brings every cell back");
    send(&file, &mut peer);
    converged(&file, &peer, table);
    assert!(file.redo().unwrap().0);
    assert!(all_cleared(&file, table));
    send(&file, &mut peer);
    converged(&file, &peer, table);

    // A paste of a different number into every record, then a fill of one number.
    set_cells(&mut file, table, &keys, |index| json!({ "amount": index * 2 }));
    assert!(cells(&file, table).iter().enumerate().all(|(index, (amount, notes))| *amount == json!(index * 2) && notes.is_null()));
    set_cells(&mut file, table, &keys, |_| json!({ "amount": 7 }));
    assert!(cells(&file, table).iter().all(|(amount, _)| *amount == json!(7)));
    send(&file, &mut peer);
    converged(&file, &peer, table);
    assert!(file.undo().unwrap().0);
    assert!(cells(&file, table).iter().enumerate().all(|(index, (amount, _))| *amount == json!(index * 2)), "the fill undone is the paste");
    assert!(file.undo().unwrap().0);
    assert!(all_cleared(&file, table), "the paste undone is the clear");
    send(&file, &mut peer);
    converged(&file, &peer, table);
}

/// Seconds per record of a clear of `amount` in every record and its undo, the fastest of two.
fn seconds_per_record(count: usize) -> f64 {
    let (mut file, table, keys) = table(count);
    (0..2)
        .map(|_| {
            let started = Instant::now();
            set_cells(&mut file, table, &keys, |_| json!({ "amount": null }));
            assert!(file.undo().unwrap().0);
            started.elapsed().as_secs_f64() / count as f64
        })
        .fold(f64::INFINITY, f64::min)
}

/// A clear of 32 times the records costs about 32 times as much: per record, at most
/// three times a small clear's.
#[test]
fn a_large_clear_costs_about_the_same_per_record_as_a_small_one() {
    let small = seconds_per_record(1_000);
    let large = seconds_per_record(32_000);
    println!("per record, 32,000 against 1,000: {:.2}", large / small);
    assert!(large < small * 3.0, "a clear of 32,000 records costs {:.1} times as much per record as one of 1,000", large / small);
}
