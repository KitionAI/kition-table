//! A large `transact` through the request layer (case regressions/issue-table-bulk-transact): a
//! `cells.set` over every record of a table, as the grid sends a paste or a fill, costs about what
//! the file's own write of the same op costs. Before, the check that every `values` key names a
//! field read the table's fields again for each record, which on the page took longer than the
//! write itself.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
#![allow(clippy::disallowed_methods, reason = "native timing tests read the clock")]

use std::time::Instant;

use kition_kdm::Origin;
use kition_table_api::{DocRef, handle};
use kition_table_file::{CellEdit, CreateOptions, FieldSpec, TableFile, TableOp, TableSpec};
use serde_json::{Map, Value, json};

const FIELDS: usize = 40;
const RECORDS: usize = 4_000;

/// A table of `RECORDS` records in `FIELDS` text fields, saved and opened again (as the page opens
/// a file); answers the bytes, the table's id and the records' row keys.
fn document() -> (Vec<u8>, i64, Vec<String>) {
    let mut file = TableFile::create_in_memory(&CreateOptions { title: "Bulk".into(), ..Default::default() }).unwrap();
    file.set_origin(Origin::person("user:test"));
    let fields = (0..FIELDS)
        .map(|index| FieldSpec {
            name: format!("f{index}"),
            title: format!("Field {index}"),
            field_type: "text".into(),
            primary: index == 0,
            ..Default::default()
        })
        .collect();
    let table = file.create_table(&TableSpec { title: "Rows".into(), fields, ..Default::default() }).unwrap();
    for start in (0..RECORDS).step_by(1_000) {
        let records: Vec<Map<String, Value>> = (start..RECORDS.min(start + 1_000))
            .map(|row| (0..FIELDS).map(|index| (format!("f{index}"), json!(format!("row {row} cell {index}")))).collect())
            .collect();
        file.transact(&[TableOp::RecordsCreate { table_id: table.id, at: None, records }]).unwrap();
    }
    let keys = file.list_record_values(table.id, &[]).unwrap().into_iter().map(|record| record.row_key).collect();
    (file.to_bytes().unwrap(), table.id, keys)
}

fn open(bytes: &[u8]) -> TableFile {
    let mut file = TableFile::open_bytes(bytes).unwrap();
    file.set_origin(Origin::person("user:test"));
    file
}

#[test]
fn a_large_transact_costs_about_what_its_write_costs() {
    let (bytes, table_id, keys) = document();
    let value = |index: usize| json!(format!("pasted {index}"));
    let doc = DocRef { document_id: 1, user_id: 1 };
    let mut best = f64::MAX;
    // The better of three tries, each on fresh copies, so a busy machine does not decide it.
    for _ in 0..3 {
        let mut direct = open(&bytes);
        let op = TableOp::CellsSet {
            table_id,
            records: keys
                .iter()
                .enumerate()
                .map(|(index, key)| CellEdit { row_key: key.clone(), values: Map::from_iter([("f1".to_owned(), value(index))]) })
                .collect(),
        };
        let started = Instant::now();
        direct.transact(&[op]).unwrap();
        let write = started.elapsed().as_secs_f64();

        let mut through = open(&bytes);
        let params = json!({ "ops": [{
            "op": "cells.set",
            "tableId": table_id,
            "records": keys.iter().enumerate().map(|(index, key)| json!({ "rowKey": key, "values": { "f1": value(index) } })).collect::<Vec<_>>(),
        }] });
        let started = Instant::now();
        handle(&mut through, &doc, "edit.transact", &params).unwrap();
        let request = started.elapsed().as_secs_f64();

        let values =
            |file: &TableFile| file.list_records(table_id).unwrap().into_iter().map(|record| (record.row_key, record.values)).collect::<Vec<_>>();
        assert!(values(&through) == values(&direct), "the request wrote other values than the write");
        best = best.min(request / write);
    }
    assert!(best <= 1.5, "a transact of {RECORDS} records cost {best:.2} times its write");
}

#[test]
fn an_unknown_key_in_a_large_transact_is_refused_once_per_record() {
    let (bytes, table_id, keys) = document();
    let mut file = open(&bytes);
    let params = json!({ "ops": [{
        "op": "cells.set",
        "tableId": table_id,
        "records": keys.iter().take(3).map(|key| json!({ "rowKey": key, "values": { "f1": "x", "Field 2": "y" } })).collect::<Vec<_>>(),
    }] });
    let error = handle(&mut file, &DocRef { document_id: 1, user_id: 1 }, "edit.transact", &params).unwrap_err();
    let kition_table_api::ApiError::InvalidParams(issues) = error else { panic!("{error:?}") };
    assert_eq!(
        issues.iter().map(|issue| issue.path.as_str()).collect::<Vec<_>>(),
        ["/ops/0/records/0/values/Field 2", "/ops/0/records/1/values/Field 2", "/ops/0/records/2/values/Field 2",]
    );
    assert!(issues.iter().all(|issue| issue.message.ends_with("the field titled \"Field 2\" is named `f2`")), "{issues:?}");
}
