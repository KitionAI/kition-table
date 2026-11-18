//! A table's storage grows in proportion to its records (case regressions/issue-table-100k-rows).
//!
//! A large table could not be built: every bulk write rewrote the whole replica
//! snapshot, each rewrite left the old snapshot's pages free, record rows grew from `{}` after
//! their insert and split their pages, and a document opened from bytes lived in one block that
//! doubles as it grows.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_table_file::kdm::store;
use kition_table_file::{CreateOptions, FieldSpec, TableFile, TableSpec};
use rusqlite::Connection;
use rusqlite::serialize::Data;
use serde_json::{Map, Value, json};

const FIELDS: usize = 20;

/// An in-memory document with one table of [`FIELDS`] text fields; answers the table's id and its
/// field names.
fn document() -> (TableFile, i64, Vec<String>) {
    let mut file = TableFile::create_in_memory(&CreateOptions { title: "Scale".into(), ..Default::default() }).unwrap();
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
    let names = table.fields.iter().map(|field| field.name.clone()).collect();
    (file, table.id, names)
}

/// `count` records from `start`, each cell 40 to 120 characters of words.
fn rows(names: &[String], start: usize, count: usize) -> Vec<Map<String, Value>> {
    const WORDS: [&str; 8] = ["alpha", "review", "shipping", "invoice", "customer", "budget", "roadmap", "quarter"];
    (start..start + count)
        .map(|row| {
            names
                .iter()
                .enumerate()
                .map(|(column, name)| {
                    let length = 8 + (row * 7 + column * 13) % 15;
                    let words: Vec<&str> = (0..length).map(|word| WORDS[(row + column + word * 3) % WORDS.len()]).collect();
                    (name.clone(), json!(format!("{row} {}", words.join(" "))))
                })
                .collect()
        })
        .collect()
}

/// `(pages, unused bytes)` of one table or index.
fn usage(conn: &Connection, name: &str) -> (i64, i64) {
    conn.query_row("SELECT COUNT(*), COALESCE(SUM(unused), 0) FROM dbstat WHERE name = ?1", [name], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
}

fn free_pages(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA freelist_count", [], |row| row.get(0)).unwrap()
}

fn generation(file: &TableFile) -> i64 {
    store::stored_position(file.conn()).unwrap().0
}

#[test]
fn a_large_snapshot_lets_the_change_log_grow_before_compacting() {
    let (file, _, _) = document();
    let conn = file.conn();
    conn.execute("UPDATE kition_kdm SET snapshot = zeroblob(40 * 1024 * 1024)", []).unwrap();
    let change = |bytes: usize| {
        conn.execute("INSERT INTO kition_kdm_changes (bytes, origin, created_at) VALUES (?1, 'test', 0)", [vec![0u8; bytes]]).unwrap();
    };
    // Past 1 MiB, but a fraction of what a compaction would write again.
    change(4 * 1024 * 1024);
    assert!(!store::should_compact(conn).unwrap());
    // Past a quarter of the snapshot.
    change(7 * 1024 * 1024);
    assert!(store::should_compact(conn).unwrap());
}

#[test]
fn bulk_writes_compact_in_proportion_to_the_document() {
    let (mut file, table, names) = document();
    for batch in 0..32 {
        file.create_records(table, &rows(&names, batch * 500, 500)).unwrap();
    }
    // Each write logs more than 1 MiB: a fixed 1 MiB limit compacted after every one of them, so
    // the snapshot bytes written grew with the square of the document. A share of the snapshot
    // compacts ever more rarely as it grows.
    let compactions = generation(&file) - 1;
    assert!(compactions <= 24, "{compactions} compactions for 32 writes");
    assert_eq!(file.count_records(table).unwrap(), 16_000);
}

#[test]
fn compacting_writes_the_new_snapshot_into_the_old_one_s_pages() {
    let (mut file, table, names) = document();
    for batch in 0..8 {
        file.create_records(table, &rows(&names, batch * 250, 250)).unwrap();
        let (log, _) = usage(file.conn(), "kition_kdm_changes");
        store::compact(file.conn(), file.replica(), 0).unwrap();
        // The new snapshot takes the old one's pages: what stays free is at most what the change
        // rows took beyond the snapshot's growth, never a whole old snapshot.
        let free = free_pages(file.conn());
        assert!(free <= log, "after write {batch}: {free} pages free, the change log took {log}");
    }
}

#[test]
fn new_record_rows_fill_their_pages() {
    let (mut file, table, names) = document();
    file.create_records(table, &rows(&names, 0, 2000)).unwrap();
    let (pages, unused) = usage(file.conn(), "kition_records");
    // The same rows packed afresh, as VACUUM writes them.
    let mut packed = Connection::open_in_memory().unwrap();
    let page_size: i64 = file.conn().query_row("PRAGMA page_size", [], |row| row.get(0)).unwrap();
    packed.execute_batch(&format!("PRAGMA page_size = {page_size}")).unwrap();
    rusqlite::backup::Backup::new(file.conn(), &mut packed).unwrap().step(-1).unwrap();
    packed.execute_batch("VACUUM").unwrap();
    let (fresh, fresh_unused) = usage(&packed, "kition_records");
    assert!(pages <= fresh + 2, "{pages} pages ({unused} bytes unused) where a fresh copy takes {fresh} ({fresh_unused} unused)");
}

#[test]
fn a_document_opened_from_bytes_is_held_page_by_page() {
    let (mut file, table, names) = document();
    file.create_records(table, &rows(&names, 0, 100)).unwrap();
    let bytes = file.to_bytes().unwrap();
    let mut opened = TableFile::open_bytes(&bytes).unwrap();
    // One serialized block (`sqlite3_deserialize`) reads back without a copy; pages are copied.
    assert!(matches!(opened.conn().serialize(rusqlite::MAIN_DB).unwrap(), Data::Owned(_)));
    opened.create_records(table, &rows(&names, 100, 100)).unwrap();
    assert_eq!(opened.count_records(table).unwrap(), 200);
    let again = TableFile::open_bytes(&opened.to_bytes().unwrap()).unwrap();
    assert_eq!(again.count_records(table).unwrap(), 200);
}

#[test]
fn saved_bytes_are_the_database_page_for_page() {
    let (mut file, table, names) = document();
    file.create_records(table, &rows(&names, 0, 300)).unwrap();
    // `sqlite3_serialize` copied the whole database before the bytes were copied again.
    let serialized = file.conn().serialize(rusqlite::MAIN_DB).unwrap().to_vec();
    let bytes = file.to_bytes().unwrap();
    assert_eq!(bytes.len(), bytes.capacity(), "the answer is allocated once, at its size");
    assert!(bytes == serialized, "the saved bytes are the database's");
}

#[test]
fn a_document_held_in_memory_compacts_a_long_change_log_before_it_is_saved() {
    let (mut file, table, names) = document();
    for batch in 0..24 {
        file.create_records(table, &rows(&names, batch * 500, 500)).unwrap();
    }
    file.compact_log().unwrap();
    // Two more writes stay in the log: a quarter of the snapshot is more than they take.
    for batch in 24..26 {
        file.create_records(table, &rows(&names, batch * 500, 500)).unwrap();
    }
    assert!(store::log_is_large(file.conn()).unwrap());
    // An open would replay the log, far slower than it loads the snapshot.
    assert!(file.compact_log().unwrap());
    assert!(!store::log_is_large(file.conn()).unwrap());
    assert!(store::projection_is_current(file.conn()).unwrap());
    // Writes go on from the compacted store, and the bytes reopen with every record.
    file.create_records(table, &rows(&names, 13_000, 10)).unwrap();
    let reopened = TableFile::open_bytes(&file.to_bytes().unwrap()).unwrap();
    assert_eq!(reopened.count_records(table).unwrap(), 13_010);
    assert!(!store::log_is_large(reopened.conn()).unwrap());
}
