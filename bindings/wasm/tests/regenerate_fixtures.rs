//! TEMPORARY (schema freeze round 4, D4): rewrites every checked-in `.ktable` (conformance and
//! case inputs) as a document of the View shape without `sortItems`, `groupItems` and `hidden`:
//! the document's state as the schema now reads it, written by object id into a new replica of
//! the same document (no change of the old shape left in it), projected afresh and sealed. It
//! checks the document id, the state, the field and record numbers, names and values, and the
//! blobs, and prints the files as base64 for the change that commits them. Deleted in that change.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use base64::Engine;
use kition_kit::kdm::{KdmError, Origin, Replica};
use kition_kit::session::{state, write_state};
use kition_table_file::TableFile;
use serde_json::{Map, Value};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../");

const FIXTURES: [&str; 15] = [
    "cases/table.agent.act/new-task/input/tasks.ktable",
    "cases/table.agent.context/two-tables-one-run/input/trip.ktable",
    "cases/table.agent.conversation/open-work-view/input/tasks.ktable",
    "cases/table.agent.observe/done-follows/input/tasks.ktable",
    "cases/table.agent.recover/number-as-word/input/effort.ktable",
    "conformance/basic.ktable",
    "conformance/check-broken-link.ktable",
    "conformance/check-broken-lookup.ktable",
    "conformance/check-dashboard-missing-table.ktable",
    "conformance/check-duplicate-primary.ktable",
    "conformance/check-field-type-mismatch.ktable",
    "conformance/check-formula-error.ktable",
    "conformance/check-formula-unknown-field.ktable",
    "conformance/check-required-empty.ktable",
    "conformance/check-select-option-missing.ktable",
];

type Rows = Vec<(i64, String, String)>;

/// `(id, name, kdm id)` of the tables, fields and records (records: `(id, values, row key)`).
fn projection(file: &TableFile) -> (Rows, Rows, Rows) {
    let rows = |sql: &str| -> Rows {
        let mut statement = file.conn().prepare(sql).unwrap();
        statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap()
    };
    (
        rows("SELECT id, name, IFNULL(kdm_id, '') FROM kition_tables WHERE deleted_at IS NULL ORDER BY id"),
        rows("SELECT id, name, IFNULL(kdm_id, '') FROM kition_fields WHERE deleted_at IS NULL ORDER BY id"),
        rows("SELECT id, values_json, row_key FROM kition_records WHERE deleted_at IS NULL ORDER BY id"),
    )
}

fn rewrite(path: &str) -> Vec<u8> {
    let old = TableFile::open_bytes(&std::fs::read(format!("{ROOT}{path}")).unwrap()).unwrap();
    let objects: Map<String, Value> = state(old.replica()).unwrap();
    let schema = kition_table_file::kdm::schema();
    let (replica, ()) =
        Replica::create_with(schema.clone(), old.replica().doc_id(), kition_table_file::ids::actor(), &Origin::system("create"), |tx| {
            write_state(tx, &schema, &Map::new(), &objects).map_err(|err| KdmError::Encoding(err.to_string()))
        })
        .unwrap();
    let mut fresh = TableFile::create_from_replica_in_memory(&replica.snapshot().unwrap()).unwrap();
    let blobs = old.all_blobs_section().unwrap();
    if !blobs.is_empty() {
        fresh.add_blob_section(&blobs).unwrap();
    }
    fresh.seal().unwrap();
    let out = fresh.content_bytes().unwrap();
    let reopened = TableFile::open_bytes(&out).unwrap();
    assert_eq!(reopened.replica().doc_id(), old.replica().doc_id(), "{path}: the same document");
    assert_eq!(state(reopened.replica()).unwrap(), objects, "{path}: the same objects, ids and values");
    assert_eq!(projection(&reopened), projection(&old), "{path}: the same tables, fields and records");
    assert_eq!(reopened.all_blobs_section().unwrap(), blobs, "{path}: the same blobs");
    out
}

#[test]
fn print_regenerated_fixtures() {
    let mut lines = Vec::new();
    for path in FIXTURES {
        lines.push(format!("FIXTURE {path} {}", base64::engine::general_purpose::STANDARD.encode(rewrite(path))));
    }
    panic!("regenerated fixtures (temporary test):\n{}\nEND FIXTURES", lines.join("\n"));
}
