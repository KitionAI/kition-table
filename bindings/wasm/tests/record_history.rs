//! A record's history survives closing the document (feature `table.records.history-kept`): the
//! file holds no history (data model 7.3), and nothing holds the changes before the session: the
//! host keeps a snapshot per checkpoint (Kit UI protocol 7m), and `records.history` tells what came
//! before the session from the differences between consecutive checkpoints, at the later one's
//! time and by its author. Nothing is stored for it.
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

/// A saved file with one record ("Draft"), opened alone: the person's session starts at its
/// content. Answers the file, the record and the host's checkpoint of that save (time 100).
fn saved_file() -> (Vec<u8>, i64, Value) {
    let mut doc = TableDocument::create(FILE, 0, 0).unwrap();
    let record = run(&mut doc, "table.records.create", json!({ "tableId": 1, "values": { "title": "Draft" } }))["id"].as_i64().unwrap();
    let title = run(&mut doc, "table.documents.get", json!({}))["tables"][0]["primary_field_id"].as_i64().unwrap();
    TITLE.with(|cell| cell.set(title));
    (doc.saved_bytes().unwrap(), record, checkpoint(&doc, 100, None))
}

/// A checkpoint as the host lists it and reads it (`host/checkpointRead`), at `doc`'s version
/// and with its snapshot: what the editor passes to `records.history`.
fn checkpoint(doc: &TableDocument, time: i64, author: Option<&str>) -> Value {
    let base64 = &base64::engine::general_purpose::STANDARD;
    let mut point = json!({ "version": base64.encode(doc.version()), "time": time, "snapshot": base64.encode(doc.snapshot().unwrap()) });
    if let Some(author) = author {
        point["author"] = json!(author);
    }
    point
}

/// Saved as a host saves, with the replica taken with the file; answers both.
fn host_save(doc: &mut TableDocument) -> (Vec<u8>, Vec<u8>) {
    let saved = doc.saved_bytes().unwrap();
    doc.mark_saved();
    (saved, doc.snapshot().unwrap())
}

#[test]
fn a_record_keeps_its_history_and_author_through_the_stored_replica() {
    let (bytes, record, created) = saved_file();
    let mut doc = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "Reviewed" } }));
    let session = title_updates(&history(&mut doc, record, None));
    assert_eq!(session.len(), 1, "{session:?}");
    let author = session[0].2.clone();
    assert!(!author.is_empty(), "the session names its author");
    let (saved, stored) = host_save(&mut doc);
    // The host's checkpoint of that save, by the person who saved.
    let reviewed = checkpoint(&doc, 200, Some(&author));
    // The file alone holds no history: only the content.
    let mut alone = TableDocument::open(FILE, &saved).unwrap();
    assert!(title_updates(&history(&mut alone, record, None)).is_empty(), "a file opened alone has no history before its session");
    // Opened with the stored replica, without checkpoints: nothing tells what came before.
    let mut reopened = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    assert!(title_updates(&history(&mut reopened, record, None)).is_empty(), "no checkpoint, no history before the session");
    // With the host's checkpoints: the change between them, by the later one's author and at its time.
    let checkpoints = json!([created, reviewed]);
    let kept = title_updates(&history(&mut reopened, record, Some(checkpoints.clone())));
    assert_eq!(kept, vec![(json!("Draft"), json!("Reviewed"), author.clone(), 200)], "the checkpoints tell the change and its author");
    // The session's own changes come first, then the stored ones.
    run(&mut reopened, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "Final" } }));
    let both = title_updates(&history(&mut reopened, record, Some(checkpoints)));
    assert_eq!(both.iter().map(|(_, after, ..)| after.clone()).collect::<Vec<_>>(), vec![json!("Final"), json!("Reviewed")], "{both:?}");
}

#[test]
fn two_sessions_saved_and_reopened_keep_both_edits_and_their_authors() {
    let (bytes, record, created) = saved_file();
    let mut first = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut first, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "By the person" } }));
    let person = title_updates(&history(&mut first, record, None))[0].2.clone();
    let (saved, stored) = host_save(&mut first);
    let by_person = checkpoint(&first, 1_000, Some(&person));
    let mut second = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    second
        .execute_as(
            "table.records.update",
            json!({ "tableId": 1, "recordId": record, "values": { "title": "By the agent" } }),
            &Origin::agent("helper"),
        )
        .unwrap();
    let (saved, stored) = host_save(&mut second);
    // The host's checkpoint after the agent's run names the agent.
    let by_agent = checkpoint(&second, 2_000, Some("helper"));
    let mut third = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    assert_eq!(
        title_updates(&history(&mut third, record, Some(json!([created, by_person, by_agent])))),
        vec![(json!("By the person"), json!("By the agent"), "helper".to_owned(), 2_000), (json!("Draft"), json!("By the person"), person, 1_000)],
        "newest first, both edits with their checkpoints' authors and times, back to the first checkpoint"
    );
}

#[test]
fn a_checkpoint_dates_the_changes_it_holds() {
    let (bytes, record, _) = saved_file();
    let mut doc = TableDocument::open(FILE, &bytes).unwrap();
    let opened = checkpoint(&doc, 500, None);
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "By the person" } }));
    let person = title_updates(&history(&mut doc, record, None))[0].2.clone();
    // The host's checkpoint at the first save.
    doc.saved_bytes().unwrap();
    doc.mark_saved();
    let first_save = checkpoint(&doc, 1_000, Some(&person));
    doc.execute_as(
        "table.records.update",
        json!({ "tableId": 1, "recordId": record, "values": { "title": "By the agent" } }),
        &Origin::agent("helper"),
    )
    .unwrap();
    let (saved, stored) = host_save(&mut doc);
    let mut reopened = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    let dated = title_updates(&history(&mut reopened, record, Some(json!([opened, first_save]))));
    // After the newest checkpoint no checkpoint dates the change, nor names who made it.
    assert_eq!(
        dated,
        vec![(json!("By the person"), json!("By the agent"), String::new(), 0), (json!("Draft"), json!("By the person"), person, 1_000)],
        "newest first, each at the time and by the author of the checkpoint after it"
    );
    // A checkpoint that is no version is refused.
    assert!(
        reopened
            .execute("table.records.history", json!({ "tableId": 1, "recordId": record, "checkpoints": [{ "version": "not base64!" }] }))
            .is_err()
    );
}
