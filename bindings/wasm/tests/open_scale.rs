//! Opening a large table reads only what the editor shows first (case
//! regressions/issue-table-open-100k).
//!
//! A host asks a document opened from its bytes for its overview (`inspect`), the document
//! (`documents.get`) and its picture's size (`width`, `height`) before the editor shows anything.
//! Each of them used to read every record of the table: the dashboards and the base's label looked
//! for the base among every object of the replica, which decodes every record of a replica loaded
//! from its snapshot, and the picture's size was had by drawing the picture, which parsed every
//! record. Each is timed against reading every record once (`list_records`) on the same document,
//! so the machine's speed cancels out.
//!
//! The picture's size is measured without laying a grid's cells out; it is the size of the
//! picture drawn, for every view of every conformance file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
#![allow(clippy::disallowed_methods, reason = "native timing tests read the clock")]

use std::path::PathBuf;
use std::time::Instant;

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use kition_table_picture::Shown;
use serde_json::{Value, json};

const RECORDS: usize = 10_000;
const FIELDS: usize = 12;

fn call(doc: &mut TableDocument, method: &str, params: Value) -> Value {
    doc.call(method, &params).unwrap_or_else(|error| panic!("{method}: {error}"))["result"].clone()
}

/// A document of one table of [`RECORDS`] records in [`FIELDS`] text fields and one dashboard,
/// saved and opened again from its bytes, as a host opens a file; answers it and the table's id.
fn large() -> (TableDocument, i64) {
    let mut doc = TableDocument::create("Large.ktable", 0, 0).unwrap();
    call(&mut doc, "tables.delete", json!({ "tableId": 1 }));
    let fields: Vec<Value> = (0..FIELDS)
        .map(|index| json!({ "name": format!("f{index}"), "title": format!("Field {index}"), "type": "text", "primary": index == 0 }))
        .collect();
    let table = call(&mut doc, "tables.create", json!({ "title": "Rows", "fields": fields }));
    let table_id = table["id"].as_i64().unwrap();
    call(&mut doc, "dashboards.upsert", json!({ "dashboardId": "overview", "title": "Overview", "source_table_id": table_id, "widgets": [] }));
    for start in (0..RECORDS).step_by(1_000) {
        let records: Vec<Value> = (start..start + 1_000)
            .map(|row| (0..FIELDS).map(|column| (format!("f{column}"), json!(format!("row {row} review shipping invoice {column}")))).collect())
            .collect();
        call(&mut doc, "transact", json!({ "ops": [{ "op": "records.create", "tableId": table_id, "records": records }] }));
    }
    let bytes = doc.save(None).unwrap();
    (TableDocument::open("Large.ktable", &bytes).unwrap(), table_id)
}

/// How long one run of `run` takes, in seconds.
fn timed(run: impl FnOnce()) -> f64 {
    let started = Instant::now();
    run();
    started.elapsed().as_secs_f64()
}

/// The fastest of three runs of `run`, in seconds.
fn fastest(mut run: impl FnMut()) -> f64 {
    (0..3).map(|_| timed(&mut run)).fold(f64::INFINITY, f64::min)
}

#[test]
fn what_a_host_asks_on_opening_does_not_read_every_record() {
    let (mut doc, table_id) = large();
    // What the open costs itself: every record read once.
    let every_record = fastest(|| assert_eq!(doc.file().list_records(table_id).unwrap().len(), RECORDS));
    let overview = fastest(|| {
        let inspected = doc.inspect().unwrap();
        assert_eq!(inspected["dashboards"].as_array().unwrap().len(), 1);
    });
    let document = fastest(|| {
        let answer = doc.call("documents.get", &Value::Null).unwrap();
        assert!(answer["result"]["tool_id"].is_string(), "{answer}");
        assert_eq!(answer["result"]["dashboards"].as_array().unwrap().len(), 1);
    });
    // A new revision before each run, so the size is not the one kept from the run before.
    let size = (0..3)
        .map(|run| {
            call(&mut doc, "tables.update", json!({ "tableId": table_id, "title": format!("Rows {run}") }));
            timed(|| assert!(doc.width() > 1 && doc.height() > 1))
        })
        .fold(f64::INFINITY, f64::min);
    eprintln!("every record {every_record:.4} s, inspect {overview:.4} s, documents.get {document:.4} s, size {size:.4} s");
    // The size still arranges the view (reading each record's place, not its cells) and reads the
    // records the picture lays out whole.
    for (what, took, share) in [("inspect", overview, 4.0), ("documents.get", document, 4.0), ("the picture's size", size, 2.0)] {
        assert!(took < every_record / share, "{what} took {took:.4} s, reading every record once {every_record:.4} s");
    }
}

fn repo() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn the_measured_size_is_the_drawn_pictures() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(repo().join("conformance"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ktable"))
        .collect();
    files.sort();
    assert!(files.len() >= 10, "{files:?}");
    let mut compared = 0;
    for path in files {
        let mut doc = TableDocument::open("Conformance.ktable", &std::fs::read(&path).unwrap()).unwrap();
        let mut shown = vec![Shown::default()];
        for table in call(&mut doc, "documents.get", Value::Null)["tables"].as_array().unwrap() {
            let table_id = table["id"].as_i64();
            shown.push(Shown { table: table_id, plain: true, ..Shown::default() });
            shown.push(Shown { table: table_id, rows: Some((1, 2)), ..Shown::default() });
            for view in table["views"].as_array().unwrap() {
                shown.push(Shown { table: table_id, view: view["id"].as_i64(), ..Shown::default() });
            }
        }
        for shown in shown {
            let drawn = kition_table_picture::draw(doc.file(), &shown, None).map(|picture| (picture.width(), picture.height()));
            let measured = kition_table_picture::size(doc.file(), &shown);
            assert_eq!(measured, drawn, "{} showing {shown:?}", path.display());
            compared += 1;
        }
    }
    assert!(compared >= 30, "{compared} pictures compared");
}
