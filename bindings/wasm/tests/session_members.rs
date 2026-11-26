//! The engine's own session members (engine contract 2, rule K3a), which kition-sdk's
//! `export_kit_session!` writes for a kit on `KitSession` and this kit adds by hand
//! (`bindings/wasm/src/web.rs`): `modelDescribe`, `modelBatch`, `changesSince`, `modelReply`,
//! `mergeChanges`, `compareChanges` and `restoreState`. A merged draft or a restored state is
//! projected into the table like any write and is one undo step of the person.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_kdm::UndoScope;
use kition_kit::KitDocument;
use kition_table_kit::{Origin, TableDocument};
use serde_json::{Value, json};

const FILE: &str = "Tasks.ktable";

fn run(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params.clone()).unwrap_or_else(|err| panic!("{command} {params}: {err}"))["result"].clone()
}

fn titles(doc: &mut TableDocument) -> Vec<String> {
    let page = run(doc, "record.list", json!({ "tableId": 1 }));
    let mut titles: Vec<String> =
        page["items"].as_array().unwrap().iter().map(|record| record["values"]["title"].as_str().unwrap_or_default().to_owned()).collect();
    titles.sort();
    titles
}

fn add(doc: &mut TableDocument, title: &str) -> Value {
    run(doc, "record.add", json!({ "tableId": 1, "values": { "title": title } }))
}

/// A document with one record ("Kept"), opened from its file.
fn document() -> TableDocument {
    let mut made = TableDocument::create(FILE, 0, 0).unwrap();
    add(&mut made, "Kept");
    TableDocument::open(FILE, &made.saved_bytes().unwrap()).unwrap()
}

/// A draft of `doc`: another replica of the document on its line of changes, as kit-editor makes
/// one, with the version it starts at.
fn draft(doc: &TableDocument) -> (TableDocument, Vec<u8>) {
    let draft = TableDocument::open_with_replica(FILE, &[], &doc.snapshot().unwrap(), &doc.blobs().unwrap()).unwrap();
    let base = draft.version();
    (draft, base)
}

#[test]
fn model_describe_answers_the_object_model_with_the_commands() {
    let mut doc = document();
    let described = doc.model_describe().unwrap();
    assert_eq!(described["capability"], "KitObjectModel 1.0");
    assert!(described["commands"].as_array().unwrap().iter().any(|command| command["id"] == "record.add"), "{described}");
}

#[test]
fn model_batch_runs_commands_and_model_edits_in_the_engine() {
    let mut doc = document();
    let batch = json!({ "ops": [{ "op": "command", "id": "record.add", "params": { "tableId": 1, "values": { "title": "From a batch" } } }] });
    doc.model_batch(&batch).unwrap();
    assert_eq!(titles(&mut doc), ["From a batch", "Kept"], "the batch's command reached the table");
    let refused = doc.model_batch(&json!({ "ops": [{ "op": "set", "target": "no-such-object", "field": "title", "value": "x" }] })).unwrap_err();
    let error: Value = serde_json::from_str(&refused).unwrap();
    assert!(error["code"].is_string(), "a refusal is the batch error's JSON: {refused}");
}

#[test]
fn changes_since_answers_a_receipt_or_why_it_cannot() {
    let mut doc = document();
    let since = doc.version();
    add(&mut doc, "New");
    let receipt = doc.receipt_since(&since);
    assert!(receipt["changes"].is_object(), "{receipt}");
    assert_ne!(receipt["changes"], doc.receipt_since(&doc.version())["changes"], "the receipt names the write: {receipt}");
    let other = TableDocument::create(FILE, 0, 0).unwrap();
    let refused = doc.receipt_since(&other.version());
    assert!(refused["changes"].is_null() && refused["unavailable"].is_string(), "{refused}");
}

#[test]
fn model_reply_is_unchanged_without_tool_ids() {
    let doc = document();
    let reply = json!({ "target": doc.doc_id(), "note": "nothing to rename" });
    assert_eq!(doc.model_reply(&reply), reply, "the table's schema declares no tool ids");
}

#[test]
fn model_reply_reads_only_the_objects_it_names_and_matches_the_full_read() {
    let mut doc = document();
    for n in 0..200 {
        add(&mut doc, &format!("Record {n}"));
    }
    let replica = doc.file().replica();
    let record = replica.objects_of_type("Record").into_iter().next().expect("a record");
    // A reply naming one record: the same as the whole document's tool ids show it.
    let naming = json!({ "updated": [record.as_str()], "count": 1 });
    let full = kition_kdm::tool_ids::ToolIds::of_replica(replica);
    let expected = if full.is_active() { full.show_json(&naming) } else { naming.clone() };
    assert_eq!(doc.model_reply(&naming), expected, "the reply's ids as the full read names them");
    // A reply naming no object comes back as it is.
    let plain = json!({ "note": "nothing named", "count": 0 });
    assert_eq!(doc.model_reply(&plain), plain);
}

/// What the editor reads of the table: its fields and its records (ids, keys and values).
fn engine_view(doc: &mut TableDocument) -> Value {
    let fields = run(doc, "field.list", json!({ "tableId": 1 }));
    let records = run(doc, "record.list", json!({ "tableId": 1 }));
    let records: Vec<Value> = records["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| json!({ "id": record["id"], "row_key": record["row_key"], "values": record["values"] }))
        .collect();
    json!({ "fields": fields, "records": records })
}

fn state_hash(doc: &TableDocument) -> String {
    doc.file().replica().state_hash().unwrap()
}

#[test]
fn an_agent_s_draft_merged_is_one_step_of_the_person_and_undo_and_redo_are_exact() {
    let mut doc = document();
    let (mut draft, base) = draft(&doc);
    add(&mut draft, "From the agent's draft");
    let changes = draft.export_changes(&base).unwrap();
    // Meanwhile the person adds another record to the same table: both additions merge.
    add(&mut doc, "Meanwhile");
    let (before, before_view) = (state_hash(&doc), engine_view(&mut doc));
    let answer = doc.merge_changes(&changes, &Origin::agent("helper")).unwrap();
    assert_eq!(answer["scope"], "person", "the merge's step is the person's, as the replica recorded it: {answer}");
    assert_eq!(titles(&mut doc), ["From the agent's draft", "Kept", "Meanwhile"], "both records are there");
    let (merged, merged_view) = (state_hash(&doc), engine_view(&mut doc));
    // The merge is the person's undo step, though an agent made the draft (KitModel M12a): the
    // replica and the engine agree on whose step it is.
    assert!(doc.file().can_step_in(UndoScope::Person, true));
    assert!(!doc.file().can_step_in(UndoScope::Agent, true), "the agent has no step to undo");
    let agent = run(&mut doc, "edit.undo", json!({ "scope": "agent" }));
    assert_eq!(agent["done"], false, "{agent}");
    assert_eq!(state_hash(&doc), merged, "the agent's undo leaves the merge");
    let (done, _) = doc.undo().unwrap();
    assert!(done);
    assert_eq!(state_hash(&doc), before, "one undo of the person restores the exact state before the merge");
    assert_eq!(engine_view(&mut doc), before_view, "and the engine's view of it");
    let (redone, _) = doc.redo().unwrap();
    assert!(redone);
    assert_eq!(state_hash(&doc), merged, "redo restores the merged state exactly");
    assert_eq!(engine_view(&mut doc), merged_view);
}

#[test]
fn a_restored_state_undoes_and_redoes_exactly() {
    let mut doc = document();
    let checkpoint = doc.snapshot().unwrap();
    add(&mut doc, "After the checkpoint");
    let (before, before_view) = (state_hash(&doc), engine_view(&mut doc));
    doc.restore_state(&checkpoint, &Origin::person("ann")).unwrap();
    let (restored, restored_view) = (state_hash(&doc), engine_view(&mut doc));
    assert_ne!(restored, before);
    let (done, _) = doc.undo().unwrap();
    assert!(done);
    assert_eq!(state_hash(&doc), before, "one undo restores the exact state before the restore");
    assert_eq!(engine_view(&mut doc), before_view);
    let (redone, _) = doc.redo().unwrap();
    assert!(redone);
    assert_eq!(state_hash(&doc), restored, "redo restores the restored state exactly");
    assert_eq!(engine_view(&mut doc), restored_view);
}

#[test]
fn a_draft_merges_in_as_one_undo_step_and_the_table_follows() {
    let mut doc = document();
    let (mut draft, base) = draft(&doc);
    add(&mut draft, "From the draft");
    let changes = draft.export_changes(&base).unwrap();
    // The document moved on meanwhile, elsewhere than the draft: the kept record renamed.
    let kept = run(&mut doc, "record.list", json!({ "tableId": 1 }))["items"][0]["id"].clone();
    run(&mut doc, "record.set", json!({ "tableId": 1, "recordId": kept, "values": { "title": "Meanwhile" } }));
    let version = doc.version();
    let preview = doc.compare_changes(&changes).unwrap();
    assert_eq!(doc.version(), version, "comparing changes nothing");
    let origin = Origin::agent("helper");
    let answer = doc.merge_changes(&changes, &origin).unwrap();
    assert_eq!(answer["summary"], preview, "the comparison named what the merge did: {answer}");
    assert_eq!(answer["damage"]["kind"], "full", "{answer}");
    assert_eq!(titles(&mut doc), ["From the draft", "Meanwhile"], "the table follows the merge, and the edit made here stays");
    assert!(doc.dirty());
    let (done, _) = doc.undo().unwrap();
    assert!(done);
    assert_eq!(titles(&mut doc), ["Meanwhile"], "one undo takes the whole draft back");
    let at = doc.version();
    assert!(doc.merge_changes(b"not changes", &origin).is_err());
    assert_eq!(doc.version(), at, "a refused merge changes nothing");
}

#[test]
fn compare_changes_tells_what_a_merge_would_change() {
    let doc = document();
    let (mut draft, base) = draft(&doc);
    let nothing = doc.compare_changes(&draft.export_changes(&base).unwrap()).unwrap();
    add(&mut draft, "Proposed");
    let preview = doc.compare_changes(&draft.export_changes(&base).unwrap()).unwrap();
    assert_ne!(preview, nothing, "a draft that adds a record changes the document: {preview}");
    assert!(doc.compare_changes(b"not changes").is_err());
}

#[test]
fn a_restored_state_keeps_the_ids_and_undoes_as_one_step() {
    let mut doc = document();
    let checkpoint = doc.snapshot().unwrap();
    let kept = run(&mut doc, "record.list", json!({ "tableId": 1 }))["items"][0]["row_key"].clone();
    add(&mut doc, "After the checkpoint");
    let origin = Origin::person("ann");
    let damage = doc.restore_state(&checkpoint, &origin).unwrap();
    assert_eq!(damage["kind"], "full");
    assert_eq!(titles(&mut doc), ["Kept"], "the table follows the restore");
    assert_eq!(run(&mut doc, "record.list", json!({ "tableId": 1 }))["items"][0]["row_key"], kept, "under its id");
    assert!(doc.dirty());
    let at = doc.version();
    assert_eq!(doc.restore_state(&checkpoint, &origin).unwrap()["kind"], "none", "the same state again changes nothing");
    assert_eq!(doc.version(), at);
    let (done, _) = doc.undo().unwrap();
    assert!(done);
    assert_eq!(titles(&mut doc), ["After the checkpoint", "Kept"], "one undo takes the restore back");
    let other = TableDocument::create(FILE, 0, 0).unwrap();
    assert!(doc.restore_state(&other.snapshot().unwrap(), &origin).is_err(), "another document's state is refused");
}
