//! A `.ktable` file holds the document's content, not its history (kition-sdk data model 7.3 and
//! 7.4; feature `table.file.content-only`): its store holds the replica's content (`content.json`,
//! compressed) and no change row, and neither it nor any other byte of the file holds deleted or
//! overwritten content; an unchanged table saves to the same bytes; a file opened alone is the
//! same document, with its ids, and edits; undo after a save takes back the last edit; a file
//! opens on top of a stored replica, by its state.
//!
//! The needles are written once each and share no run of four characters with anything else in
//! the table: every test checks the session's change log holds them (the control) before it
//! checks the file does not.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use kition_kit::KitDocument;
use kition_table_file::TableFile;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

const FILE: &str = "Plan.ktable";
const TYPED_THEN_DELETED: &str = "Jx4Hb8Ty3Nc6";
const DELETED_RECORD: &str = "Qv7Zp9Wm2Rk5";
const OVERWRITTEN: &str = "Ug5Le1Fo0Dw4";
const NEEDLES: [&str; 3] = [TYPED_THEN_DELETED, DELETED_RECORD, OVERWRITTEN];

thread_local! {
    /// The version `table_with_history` made its table at: its changes since then are the history.
    static CREATED: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The session's changes since the table was made ([`table_with_history`]): its history.
fn history_of(doc: &TableDocument) -> Vec<u8> {
    doc.export_changes(&CREATED.with(|created| created.borrow().clone())).unwrap()
}

fn contains(bytes: &[u8], needle: &str) -> bool {
    let utf16: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
    bytes.windows(needle.len()).any(|window| window == needle.as_bytes()) || bytes.windows(utf16.len()).any(|window| window == utf16.as_slice())
}

fn run(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params.clone()).unwrap_or_else(|err| panic!("{command} {params}: {err}"))["result"].clone()
}

/// The file as a host saves it.
fn host_save(doc: &mut TableDocument) -> Vec<u8> {
    doc.save(None).unwrap()
}

/// The store of the file `bytes`: its snapshot and how many change rows follow it.
fn stored(bytes: &[u8]) -> (Vec<u8>, i64) {
    TableFile::open_bytes(bytes).unwrap().stored_replica().unwrap()
}

/// A table whose change log holds typed-then-deleted text in a long text, a deleted record and an
/// overwritten value; answers it and the id of the record that stays.
fn table_with_history() -> (TableDocument, i64) {
    let mut doc = TableDocument::create(FILE, 0, 0).unwrap();
    CREATED.with(|created| *created.borrow_mut() = doc.version());
    let kept = run(&mut doc, "record.add", json!({ "tableId": 1, "values": { "title": "Kept", "notes": "Start" } }))["id"].as_i64().unwrap();
    run(&mut doc, "record.set", json!({ "tableId": 1, "recordId": kept, "values": { "notes": format!("Start {TYPED_THEN_DELETED}") } }));
    run(&mut doc, "record.set", json!({ "tableId": 1, "recordId": kept, "values": { "notes": "Start", "title": OVERWRITTEN } }));
    run(&mut doc, "record.set", json!({ "tableId": 1, "recordId": kept, "values": { "title": "Kept" } }));
    let gone = run(&mut doc, "record.add", json!({ "tableId": 1, "values": { "title": DELETED_RECORD } }))["id"].as_i64().unwrap();
    run(&mut doc, "record.delete", json!({ "tableId": 1, "recordId": gone }));
    (doc, kept)
}

/// Every record's row key and values: what both sides of a merge must read alike.
fn records(doc: &TableDocument) -> Vec<(String, Value)> {
    let mut rows: Vec<(String, Value)> =
        doc.file().list_records(1).unwrap().into_iter().map(|record| (record.row_key, Value::Object(record.values))).collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

#[test]
fn an_unchanged_table_saves_to_the_same_bytes() {
    let (mut doc, kept) = table_with_history();
    let history = history_of(&doc);
    for needle in NEEDLES {
        assert!(contains(&history, needle), "the control: the session's change log holds {needle}");
    }
    let version = doc.file().replica_version();
    let first = host_save(&mut doc);
    let (content, changes) = stored(&first);
    assert_eq!(changes, 0, "no change row");
    for needle in NEEDLES {
        assert!(!contains(&first, needle), "the file holds {needle}");
        assert!(!contains(&content, needle), "the content holds {needle}");
    }
    let state: Value = serde_json::from_slice(&content).unwrap();
    for key in ["version", "lineage", "actor", "origin", "changes"] {
        assert!(state.get(key).is_none(), "the content holds no {key}");
    }
    assert_eq!(state["docId"], doc.doc_id(), "the content names its document");
    assert_eq!(doc.file().replica_version(), version, "a save writes nothing into the session");
    assert_eq!(host_save(&mut doc), first, "two saves with nothing between");
    doc.mark_saved();
    assert_eq!(host_save(&mut doc), first, "and after the host stored the first");
    let mut opened = TableDocument::open(FILE, &first).unwrap();
    assert_eq!(host_save(&mut opened), first, "and from the file opened again");
    // The content is all there.
    assert_eq!(records(&opened), records(&doc));
    assert_eq!(opened.file().get_record(1, kept).unwrap().values["notes"], "Start");
    println!(
        "a content file of {} bytes: its content {} bytes; the session's database {} bytes, its snapshot {} bytes",
        first.len(),
        content.len(),
        doc.file().to_bytes().unwrap().len(),
        doc.snapshot().unwrap().len()
    );
}

#[test]
fn a_file_opened_alone_is_the_same_document_with_its_ids_and_edits() {
    let (mut doc, kept) = table_with_history();
    let bytes = host_save(&mut doc);
    let mut opened = TableDocument::open(FILE, &bytes).unwrap();
    assert_eq!(opened.doc_id(), doc.doc_id());
    let ids = |doc: &TableDocument| {
        let mut ids: Vec<String> = ["Table", "Field", "View", "Record", "Choice", "CellText"]
            .iter()
            .flat_map(|ty| doc.file().replica().objects_of_type(ty))
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids
    };
    assert_eq!(ids(&opened), ids(&doc), "every object keeps its id");
    assert_eq!(opened.file().get_record(1, kept).unwrap().row_key, doc.file().get_record(1, kept).unwrap().row_key);
    run(&mut opened, "record.set", json!({ "tableId": 1, "recordId": kept, "values": { "notes": "Start, then more" } }));
    assert_eq!(opened.file().get_record(1, kept).unwrap().values["notes"], "Start, then more");
    let again = host_save(&mut opened);
    assert_eq!(TableDocument::open(FILE, &again).unwrap().file().get_record(1, kept).unwrap().values["notes"], "Start, then more");
}

#[test]
fn undo_after_a_save_takes_back_the_last_edit() {
    let (mut doc, kept) = table_with_history();
    run(&mut doc, "record.set", json!({ "tableId": 1, "recordId": kept, "values": { "title": "Last" } }));
    host_save(&mut doc);
    doc.mark_saved();
    let (done, _) = doc.undo().unwrap();
    assert!(done, "the edit before the save is still an undo step");
    assert_eq!(doc.file().get_record(1, kept).unwrap().values["title"], "Kept");
}

#[test]
fn a_file_opens_on_the_replica_a_host_stored_by_its_changes_or_by_its_state() {
    let (mut doc, kept) = table_with_history();
    let bytes = host_save(&mut doc);
    let history = doc.snapshot().unwrap();
    // The stored replica holds the file's state: the document opens saved, on the stored line.
    let opened = TableDocument::open_with_replica(FILE, &bytes, &history, &[]).unwrap();
    assert_eq!(opened.doc_id(), doc.doc_id());
    assert_eq!(opened.version(), doc.version(), "the stored replica is the document's");
    assert!(!opened.dirty());
    assert_eq!(records(&opened), records(&doc));
    // A file saved elsewhere from changes the store never saw: its state, written over the
    // stored replica.
    let mut elsewhere = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut elsewhere, "record.set", json!({ "tableId": 1, "recordId": kept, "values": { "title": "Saved elsewhere" } }));
    let later = host_save(&mut elsewhere);
    let reopened = TableDocument::open_with_replica(FILE, &later, &history, &[]).unwrap();
    assert_eq!(reopened.file().get_record(1, kept).unwrap().values["title"], "Saved elsewhere");
    assert_eq!(records(&reopened), records(&elsewhere));
}

#[test]
fn a_file_with_a_new_link_is_taken_in_by_its_state() {
    let (mut doc, kept) = table_with_history();
    let bytes = host_save(&mut doc);
    let history = doc.snapshot().unwrap();
    // Saved elsewhere: a link field and a record that links to the kept one, so taking the file
    // in by its state creates a `Link` with its required ends (and a field it names).
    let mut elsewhere = TableDocument::open(FILE, &bytes).unwrap();
    run(
        &mut elsewhere,
        "field.add",
        json!({ "tableId": 1, "name": "related", "title": "Related", "type": "link_to_record", "options": { "target_table_id": 1, "allow_multiple": true } }),
    );
    let key = elsewhere.file().get_record(1, kept).unwrap().row_key;
    run(&mut elsewhere, "record.add", json!({ "tableId": 1, "values": { "title": "Linking", "related": [key] } }));
    assert_eq!(elsewhere.file().replica().objects_of_type("Link").len(), 1, "the edit made one link");
    let later = host_save(&mut elsewhere);
    let reopened = TableDocument::open_with_replica(FILE, &later, &history, &[]).unwrap();
    assert_eq!(reopened.file().replica().objects_of_type("Link").len(), 1, "the link is taken in");
    assert_eq!(records(&reopened), records(&elsewhere));
}
