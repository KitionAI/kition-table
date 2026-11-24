//! A record's history survives closing the document (feature `table.records.history-kept`): the
//! file holds no history (data model 7.3), the replica the host keeps does (Kit UI protocol 7),
//! and `records.history` derives what came before the session from it, back to the file's seal
//! (`Replica::earliest_readable`), one entry per change with its author, at the time of the
//! host's checkpoint (7m) that holds it. Nothing is stored for it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use base64::Engine;
use kition_table_kit::{KitDocument, Origin, TableDocument};
use serde_json::{Value, json};

const FILE: &str = "Plan.ktable";

thread_local! {
    /// The title field's id in the file `saved_file` made.
    static TITLE: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

fn run(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params.clone()).unwrap_or_else(|err| panic!("{command} {params}: {err}"))["result"].clone()
}

fn history(doc: &mut TableDocument, record: i64, checkpoints: Option<Value>) -> Vec<Value> {
    let mut params = json!({ "tableId": 1, "recordId": record });
    if let Some(checkpoints) = checkpoints {
        params["checkpoints"] = checkpoints;
    }
    run(doc, "table.records.history", params)["items"].as_array().cloned().unwrap_or_default()
}

/// The updates of the title (the primary field) in `items`, as `(before, after, actor, time)`.
fn title_updates(items: &[Value]) -> Vec<(Value, Value, String, i64)> {
    items
        .iter()
        .filter(|entry| entry["action"] == "update" && entry["field_id"] == TITLE.with(|title| title.get()))
        .map(|entry| {
            (
                entry["before"].clone(),
                entry["after"].clone(),
                entry["actor"].as_str().unwrap_or_default().to_owned(),
                entry["created_at"].as_i64().unwrap_or(-1),
            )
        })
        .collect()
}

/// A saved file with one record ("Draft"), opened alone: the person's session starts at its seal.
fn saved_file() -> (Vec<u8>, i64) {
    let mut doc = TableDocument::create(FILE, 0, 0).unwrap();
    let record = run(&mut doc, "table.records.create", json!({ "tableId": 1, "values": { "title": "Draft" } }))["id"].as_i64().unwrap();
    let title = run(&mut doc, "table.documents.get", json!({}))["tables"][0]["primary_field_id"].as_i64().unwrap();
    TITLE.with(|cell| cell.set(title));
    (doc.sealed_save().unwrap(), record)
}

/// A checkpoint as the host lists it, at `doc`'s version: the save that wrote the file it holds.
fn checkpoint(doc: &TableDocument, time: i64) -> Value {
    json!({ "version": base64::engine::general_purpose::STANDARD.encode(doc.version()), "time": time })
}

/// Saved as a host saves (sealed), with the replica taken with the file; answers both.
fn host_save(doc: &mut TableDocument) -> (Vec<u8>, Vec<u8>) {
    let saved = doc.sealed_save().unwrap();
    doc.mark_saved();
    (saved, doc.snapshot().unwrap())
}

#[test]
fn a_record_keeps_its_history_and_author_through_the_stored_replica() {
    let (bytes, record) = saved_file();
    let mut doc = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "Reviewed" } }));
    let session = title_updates(&history(&mut doc, record, None));
    assert_eq!(session.len(), 1, "{session:?}");
    let author = session[0].2.clone();
    assert!(!author.is_empty(), "the session names its author");
    let (saved, stored) = host_save(&mut doc);
    // The file alone holds no history: only the content.
    let mut alone = TableDocument::open(FILE, &saved).unwrap();
    assert!(title_updates(&history(&mut alone, record, None)).is_empty(), "a file opened alone has no history before its session");
    // Opened with the stored replica: the change and its author, without any checkpoint.
    let mut reopened = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    let kept = title_updates(&history(&mut reopened, record, None));
    assert_eq!(kept, vec![(json!("Draft"), json!("Reviewed"), author.clone(), 0)], "the stored replica tells the change and its author");
    // The session's own changes come first, then the stored ones.
    run(&mut reopened, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "Final" } }));
    let both = title_updates(&history(&mut reopened, record, None));
    assert_eq!(both.iter().map(|(_, after, ..)| after.clone()).collect::<Vec<_>>(), vec![json!("Final"), json!("Reviewed")], "{both:?}");
}

#[test]
fn two_sessions_saved_and_reopened_keep_both_edits_and_their_authors() {
    let (bytes, record) = saved_file();
    let mut first = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut first, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "By the person" } }));
    let person = title_updates(&history(&mut first, record, None))[0].2.clone();
    let (saved, stored) = host_save(&mut first);
    let mut second = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    second
        .execute_as(
            "table.records.update",
            json!({ "tableId": 1, "recordId": record, "values": { "title": "By the agent" } }),
            &Origin::agent("helper"),
        )
        .unwrap();
    let (saved, stored) = host_save(&mut second);
    let mut third = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    assert_eq!(
        title_updates(&history(&mut third, record, None)),
        vec![(json!("By the person"), json!("By the agent"), "helper".to_owned(), 0), (json!("Draft"), json!("By the person"), person, 0)],
        "newest first, both edits with their authors, back to the first file's seal"
    );
}

#[test]
fn a_checkpoint_dates_the_changes_it_holds() {
    let (bytes, record) = saved_file();
    let mut doc = TableDocument::open(FILE, &bytes).unwrap();
    let opened = checkpoint(&doc, 500);
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "By the person" } }));
    let person = title_updates(&history(&mut doc, record, None))[0].2.clone();
    // The host's checkpoint at the first save.
    doc.sealed_save().unwrap();
    doc.mark_saved();
    let first_save = checkpoint(&doc, 1_000);
    doc.execute_as(
        "table.records.update",
        json!({ "tableId": 1, "recordId": record, "values": { "title": "By the agent" } }),
        &Origin::agent("helper"),
    )
    .unwrap();
    let (saved, stored) = host_save(&mut doc);
    let mut reopened = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    let dated = title_updates(&history(&mut reopened, record, Some(json!([opened, first_save]))));
    assert_eq!(
        dated,
        vec![(json!("By the person"), json!("By the agent"), "helper".to_owned(), 0), (json!("Draft"), json!("By the person"), person, 1_000)],
        "newest first, each by its author, at the time of the first checkpoint that holds it"
    );
    // A checkpoint that is no version is refused.
    assert!(
        reopened
            .execute("table.records.history", json!({ "tableId": 1, "recordId": record, "checkpoints": [{ "version": "not base64!" }] }))
            .is_err()
    );
}
