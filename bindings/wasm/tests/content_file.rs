//! A `.ktable` file holds the document's content, not its history (kition-sdk data model 7.3, rule
//! M16b; feature `table.file.content-only`): its store holds one change (the seal) and none of the
//! deleted or overwritten content, and no byte of the file holds them; an unchanged table saves to
//! the same bytes; a file opened alone is the same document, with its ids, and edits; a replica
//! from the file and one with the whole history converge; undo after a save takes back the last
//! edit; a file opens on top of a stored replica, by its changes or by its state. Hosts seal
//! before saving (`prepare_save`); a save nobody sealed for is refused and writes nothing.
//!
//! The needles are written once each and share no run of four characters with anything else in
//! the table, so the engine's compression keeps each one literal in the bytes that hold it: every
//! test checks the session's full snapshot holds them (the control) before it checks the file
//! does not.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use kition_kit::KitDocument;
use kition_kit::kdm::{Origin, Replica};
use kition_table_file::TableFile;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

const FILE: &str = "Plan.ktable";
const TYPED_THEN_DELETED: &str = "Jx4Hb8Ty3Nc6";
const DELETED_RECORD: &str = "Qv7Zp9Wm2Rk5";
const OVERWRITTEN: &str = "Ug5Le1Fo0Dw4";
const NEEDLES: [&str; 3] = [TYPED_THEN_DELETED, DELETED_RECORD, OVERWRITTEN];

fn contains(bytes: &[u8], needle: &str) -> bool {
    let utf16: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
    bytes.windows(needle.len()).any(|window| window == needle.as_bytes()) || bytes.windows(utf16.len()).any(|window| window == utf16.as_slice())
}

fn run(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params.clone()).unwrap_or_else(|err| panic!("{command} {params}: {err}"))["result"].clone()
}

/// The file as a host saves it: sealed first (`prepare_save`), then the bytes.
fn host_save(doc: &mut TableDocument) -> Vec<u8> {
    doc.prepare_save().unwrap();
    doc.save(None).unwrap()
}

/// The store of the file `bytes`: its snapshot and how many change rows follow it.
fn stored(bytes: &[u8]) -> (Vec<u8>, i64) {
    TableFile::open_bytes(bytes).unwrap().stored_replica().unwrap()
}

/// A table whose history holds typed-then-deleted text in a long text, a deleted record and an
/// overwritten value; answers it and the id of the record that stays.
fn table_with_history() -> (TableDocument, i64) {
    let mut doc = TableDocument::create(FILE, 0, 0).unwrap();
    let kept =
        run(&mut doc, "table.records.create", json!({ "tableId": 1, "values": { "title": "Kept", "notes": "Start" } }))["id"].as_i64().unwrap();
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": kept, "values": { "notes": format!("Start {TYPED_THEN_DELETED}") } }));
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": kept, "values": { "notes": "Start", "title": OVERWRITTEN } }));
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": kept, "values": { "title": "Kept" } }));
    let gone = run(&mut doc, "table.records.create", json!({ "tableId": 1, "values": { "title": DELETED_RECORD } }))["id"].as_i64().unwrap();
    run(&mut doc, "table.records.delete", json!({ "tableId": 1, "recordId": gone }));
    (doc, kept)
}

/// Every record's row key and values: what both sides of a merge must read alike.
fn records(doc: &TableDocument) -> Vec<(String, Value)> {
    let mut rows: Vec<(String, Value)> =
        doc.file().list_records(1).unwrap().into_iter().map(|record| (record.row_key, Value::Object(record.values))).collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

fn version_of(bytes: &[u8]) -> kition_kit::kdm::Version {
    TableFile::open_bytes(bytes).unwrap().replica_version()
}

#[test]
fn the_file_holds_the_content_and_of_the_history_only_the_seal() {
    let (mut doc, kept) = table_with_history();
    let full = doc.snapshot().unwrap();
    for needle in NEEDLES {
        assert!(contains(&full, needle), "the control: the session's history holds {needle}");
    }
    assert!(Replica::snapshot_origins(&full).unwrap().len() > 1, "the control: the history holds every change");
    let bytes = host_save(&mut doc);
    for needle in NEEDLES {
        assert!(!contains(&bytes, needle), "the file holds {needle}");
    }
    let (content, changes) = stored(&bytes);
    assert_eq!(changes, 0, "no change row");
    let held = Replica::snapshot_origins(&content).unwrap();
    assert_eq!(held, vec![Some(Origin::system("content-seal"))], "of the history, the file holds the seal");
    // The direct path: the seal is the session's own.
    assert_eq!(doc.file().replica_version(), version_of(&bytes));
    // The content is all there.
    let back = TableDocument::open(FILE, &bytes).unwrap();
    assert_eq!(records(&back), records(&doc));
    assert_eq!(back.file().get_record(1, kept).unwrap().values["notes"], "Start");
    println!(
        "a content file of {} bytes: its replica {} bytes; the session's database {} bytes, its full snapshot {} bytes",
        bytes.len(),
        content.len(),
        doc.file().to_bytes().unwrap().len(),
        full.len()
    );
}

#[test]
fn an_unchanged_table_saves_to_the_same_bytes() {
    let (mut doc, _) = table_with_history();
    let first = host_save(&mut doc);
    let version = doc.file().replica_version();
    assert_eq!(host_save(&mut doc), first, "two saves with nothing between");
    assert_eq!(doc.file().replica_version(), version, "a sealed table seals nothing more");
    doc.mark_saved();
    assert_eq!(host_save(&mut doc), first, "and after the host stored the first");
    let mut opened = TableDocument::open(FILE, &first).unwrap();
    assert_eq!(host_save(&mut opened), first, "and from the file opened again");
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
    run(&mut opened, "table.records.update", json!({ "tableId": 1, "recordId": kept, "values": { "notes": "Start, then more" } }));
    assert_eq!(opened.file().get_record(1, kept).unwrap().values["notes"], "Start, then more");
    let again = host_save(&mut opened);
    assert_eq!(TableDocument::open(FILE, &again).unwrap().file().get_record(1, kept).unwrap().values["notes"], "Start, then more");
}

#[test]
fn a_replica_from_the_file_and_one_with_the_whole_history_converge() {
    let (mut doc, kept) = table_with_history();
    let bytes = host_save(&mut doc);
    let mut file = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": kept, "values": { "title": "From the history" } }));
    run(&mut file, "table.records.update", json!({ "tableId": 1, "recordId": kept, "values": { "notes": "Start from the file" } }));
    let to_file = doc.export_changes(&file.version()).unwrap();
    let to_doc = file.export_changes(&doc.version()).unwrap();
    file.import_changes(&to_file).unwrap();
    doc.import_changes(&to_doc).unwrap();
    assert_eq!(records(&doc), records(&file));
    let record = doc.file().get_record(1, kept).unwrap();
    assert_eq!((record.values["title"].clone(), record.values["notes"].clone()), (json!("From the history"), json!("Start from the file")));
    assert_eq!(doc.version(), file.version());
}

#[test]
fn undo_after_a_save_takes_back_the_last_edit() {
    let (mut doc, kept) = table_with_history();
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": kept, "values": { "title": "Last" } }));
    host_save(&mut doc);
    doc.mark_saved();
    let (done, _) = doc.undo().unwrap();
    assert!(done, "the edit before the save is still an undo step");
    assert_eq!(doc.file().get_record(1, kept).unwrap().values["title"], "Kept");
}

#[test]
fn a_save_nobody_sealed_for_is_refused_and_writes_nothing() {
    let (mut doc, _) = table_with_history();
    let before = doc.file().replica_version();
    let refused = doc.save(None).unwrap_err();
    assert!(refused.contains("prepareSave"), "{refused}");
    assert_eq!(doc.file().replica_version(), before, "a refused save writes nothing into the session");
    assert!(!doc.file().is_sealed());
    assert!(doc.dirty(), "a refused save leaves the document unsaved");
    // Sealed first, as a host saves: the bytes hold the content and, of the history, the seal.
    let bytes = host_save(&mut doc);
    for needle in NEEDLES {
        assert!(!contains(&bytes, needle), "the file holds {needle}");
    }
    let (content, changes) = stored(&bytes);
    assert_eq!(changes, 0);
    assert_eq!(Replica::snapshot_origins(&content).unwrap(), vec![Some(Origin::system("content-seal"))]);
    doc.mark_saved();
    assert!(doc.file().is_sealed());
    assert_eq!(doc.file().replica_version(), version_of(&bytes));
}

#[test]
fn a_file_opens_on_the_replica_a_host_stored_by_its_changes_or_by_its_state() {
    let (mut doc, kept) = table_with_history();
    let bytes = host_save(&mut doc);
    let history = doc.snapshot().unwrap();
    // The stored replica has the file's version: the document opens saved, with its history.
    let opened = TableDocument::open_with_replica(FILE, &bytes, &history, &[]).unwrap();
    assert_eq!(opened.doc_id(), doc.doc_id());
    assert!(!opened.file().replica().is_shallow(), "the stored replica, with its history, is the document's");
    assert!(!opened.dirty());
    assert_eq!(records(&opened), records(&doc));
    // A file saved elsewhere from changes the store never saw: its state, written over the
    // stored replica.
    let mut elsewhere = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut elsewhere, "table.records.update", json!({ "tableId": 1, "recordId": kept, "values": { "title": "Saved elsewhere" } }));
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
        "table.fields.create",
        json!({ "tableId": 1, "name": "related", "title": "Related", "type": "link_to_record", "options": { "target_table_id": 1, "allow_multiple": true } }),
    );
    let key = elsewhere.file().get_record(1, kept).unwrap().row_key;
    run(&mut elsewhere, "table.records.create", json!({ "tableId": 1, "values": { "title": "Linking", "related": [key] } }));
    assert_eq!(elsewhere.file().replica().objects_of_type("Link").len(), 1, "the edit made one link");
    let later = host_save(&mut elsewhere);
    let reopened = TableDocument::open_with_replica(FILE, &later, &history, &[]).unwrap();
    assert_eq!(reopened.file().replica().objects_of_type("Link").len(), 1, "the link is taken in");
    assert_eq!(records(&reopened), records(&elsewhere));
}
