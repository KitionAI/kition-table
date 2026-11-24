//! A record's history survives closing the document (feature `table.records.history-kept`): the
//! file holds no history (data model 7.3), the replica the host keeps does (Kit UI protocol 7),
//! and `records.history` derives what came before the session from it, cut at the host's
//! checkpoints (7m), each change with its author. Nothing is stored for it.
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

#[test]
fn a_record_keeps_its_history_and_author_through_the_stored_replica() {
    let (bytes, record) = saved_file();
    let mut doc = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "Reviewed" } }));
    let session = title_updates(&history(&mut doc, record, None));
    assert_eq!(session.len(), 1, "{session:?}");
    let author = session[0].2.clone();
    assert!(!author.is_empty(), "the session names its author");
    // Saved as a host saves (sealed), the replica taken with the file; closed, opened again.
    let saved = doc.sealed_save().unwrap();
    doc.mark_saved();
    let stored = doc.snapshot().unwrap();
    // The file alone holds no history: only the content.
    let mut alone = TableDocument::open(FILE, &saved).unwrap();
    assert!(title_updates(&history(&mut alone, record, None)).is_empty(), "a file opened alone has no history before its session");
    let mut reopened = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    let kept = title_updates(&history(&mut reopened, record, None));
    assert_eq!(kept, vec![(json!("Draft"), json!("Reviewed"), author, 0)], "the stored replica tells the change and its author");
    // The session's own changes come first, then the stored ones.
    run(&mut reopened, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "Final" } }));
    let both = title_updates(&history(&mut reopened, record, None));
    assert_eq!(both.iter().map(|(_, after, ..)| after.clone()).collect::<Vec<_>>(), vec![json!("Final"), json!("Reviewed")], "{both:?}");
}

#[test]
fn checkpoints_cut_the_stored_history_and_name_each_stretchs_author() {
    let (bytes, record) = saved_file();
    let mut doc = TableDocument::open(FILE, &bytes).unwrap();
    run(&mut doc, "table.records.update", json!({ "tableId": 1, "recordId": record, "values": { "title": "By the person" } }));
    let person = title_updates(&history(&mut doc, record, None))[0].2.clone();
    // The host's checkpoint at the first save.
    doc.sealed_save().unwrap();
    doc.mark_saved();
    let checkpoint = base64::engine::general_purpose::STANDARD.encode(doc.version());
    doc.execute_as(
        "table.records.update",
        json!({ "tableId": 1, "recordId": record, "values": { "title": "By the agent" } }),
        &Origin::agent("helper"),
    )
    .unwrap();
    let saved = doc.sealed_save().unwrap();
    doc.mark_saved();
    let stored = doc.snapshot().unwrap();
    let mut reopened = TableDocument::open_with_replica(FILE, &saved, &stored, &[]).unwrap();
    let cut = title_updates(&history(&mut reopened, record, Some(json!([{ "version": checkpoint, "time": 1_000 }]))));
    assert_eq!(
        cut,
        vec![
            (json!("By the person"), json!("By the agent"), "helper".to_owned(), 0),
            (json!("Draft"), json!("By the person"), person.clone(), 1_000)
        ],
        "newest first, each stretch by its author, at its checkpoint's time"
    );
    // Without the checkpoints the history before the session is one stretch, by both.
    let whole = title_updates(&history(&mut reopened, record, None));
    assert_eq!(whole.len(), 1, "{whole:?}");
    assert_eq!((&whole[0].0, &whole[0].1), (&json!("Draft"), &json!("By the agent")));
    assert!(whole[0].2.split(", ").any(|name| name == "helper") && whole[0].2.split(", ").any(|name| name == person), "{whole:?}");
    // A checkpoint that is no version is refused.
    assert!(
        reopened
            .execute("table.records.history", json!({ "tableId": 1, "recordId": record, "checkpoints": [{ "version": "not base64!" }] }))
            .is_err()
    );
}
