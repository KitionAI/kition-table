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

/// The internal names of table 1's fields, sorted.
fn field_names(doc: &mut TableDocument) -> Vec<String> {
    let fields = run(doc, "field.list", json!({ "tableId": 1 }));
    let fields = if fields.is_array() { fields } else { fields["items"].clone() };
    let mut names: Vec<String> = fields.as_array().unwrap().iter().map(|field| field["name"].as_str().unwrap().to_owned()).collect();
    names.sort();
    names
}

#[test]
fn a_draft_s_preview_runs_the_repair_that_merging_it_runs() {
    let mut doc = document();
    let (mut draft, base) = draft(&doc);
    run(&mut draft, "field.add", json!({ "tableId": 1, "title": "Reviewer", "type": "text" }));
    let changes = draft.export_changes(&base).unwrap();
    // Meanwhile the document gets a field of the same name: after the merge the repair renames
    // the one with the higher id (`reviewer_2`).
    run(&mut doc, "field.add", json!({ "tableId": 1, "title": "Reviewer", "type": "text" }));
    let unrepaired = doc.file().replica().merged_state(&changes).unwrap();
    let previewed = doc.file().merged_replica_state(&changes).unwrap();
    assert_ne!(previewed, unrepaired, "the merge leaves two fields named `reviewer`, which the repair renames");
    let version = doc.version();
    let preview = doc.compare_changes(&changes).unwrap();
    assert_eq!(doc.version(), version, "comparing changes nothing");
    let answer = doc.merge_changes(&changes, &Origin::agent("helper")).unwrap();
    assert_eq!(doc.file().replica().state_json().unwrap(), previewed, "the preview's merged state is the state after merging");
    assert_eq!(answer["summary"], preview, "the comparison named what the merge and its repair did: {answer}");
    let names = field_names(&mut doc);
    assert!(names.iter().any(|name| name == "reviewer") && names.iter().any(|name| name == "reviewer_2"), "{names:?}");
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

/// The id and row key of the record of table 1 titled `title`.
fn record_titled(doc: &mut TableDocument, title: &str) -> (Value, String) {
    let page = run(doc, "record.list", json!({ "tableId": 1 }));
    let record = page["items"].as_array().unwrap().iter().find(|record| record["values"]["title"] == title).unwrap().clone();
    (record["id"].clone(), record["row_key"].as_str().unwrap().to_owned())
}

#[test]
fn a_draft_the_repair_cancels_records_nothing_and_undo_takes_back_the_person_s_step() {
    let mut doc = document();
    run(
        &mut doc,
        "field.add",
        json!({ "tableId": 1, "name": "related", "title": "Related", "type": "link_to_record", "options": { "target_table_id": 1, "allow_multiple": true } }),
    );
    add(&mut doc, "Target");
    let (kept, _) = record_titled(&mut doc, "Kept");
    let (target, target_key) = record_titled(&mut doc, "Target");
    // An agent's draft links the kept record to the target.
    let (mut draft, base) = draft(&doc);
    run(&mut draft, "record.set", json!({ "tableId": 1, "recordId": kept, "values": { "related": [target_key] } }));
    assert_eq!(draft.file().replica().objects_of_type("Link").len(), 1, "the draft made one link");
    let changes = draft.export_changes(&base).unwrap();
    // Meanwhile the person deletes the target: the repair deletes a link whose end is gone, so
    // merging the draft leaves the document exactly as it is.
    let (before_delete, before_delete_view) = (state_hash(&doc), engine_view(&mut doc));
    run(&mut doc, "record.delete", json!({ "tableId": 1, "recordId": target }));
    doc.saved_bytes().unwrap();
    doc.mark_saved();
    let (hash, view, version, revision) = (state_hash(&doc), engine_view(&mut doc), doc.version(), doc.revision());
    let depth = doc.file().replica().undo_depth(UndoScope::Person);
    let answer = doc.merge_changes(&changes, &Origin::agent("helper")).unwrap();
    assert!(answer["scope"].is_null(), "a merge that changes nothing records no step: {answer}");
    assert_eq!(answer["damage"]["kind"], "none", "{answer}");
    assert_eq!(state_hash(&doc), hash, "the state is unchanged");
    assert_eq!(engine_view(&mut doc), view, "and the engine's view of it");
    assert_eq!(doc.version(), version, "no change is logged");
    assert_eq!(doc.revision(), revision, "the revision stays");
    assert_eq!(doc.file().replica().undo_depth(UndoScope::Person), depth, "no undo step is recorded");
    assert!(!doc.dirty(), "the kit's own history has no step for it: the document is still saved");
    assert!(doc.file().replica().objects_of_type("Link").is_empty());
    // The next undo takes back the person's deletion, exactly.
    let (done, _) = doc.undo().unwrap();
    assert!(done);
    assert_eq!(state_hash(&doc), before_delete, "the undo takes back the person's deletion exactly");
    assert_eq!(engine_view(&mut doc), before_delete_view);
    assert_eq!(doc.file().replica().undo_depth(UndoScope::Person), depth - 1);
    assert!(doc.dirty());
}

/// [`engine_view`] without the projection's write times (`created_at`, `updated_at`): the undo
/// projects the renamed field's row again, at the time of the undo; the document's state, which
/// the state hash covers, holds no such time.
fn timeless_view(doc: &mut TableDocument) -> Value {
    let mut view = engine_view(doc);
    if let Some(fields) = view["fields"]["items"].as_array_mut() {
        for field in fields {
            if let Some(field) = field.as_object_mut() {
                field.remove("created_at");
                field.remove("updated_at");
            }
        }
    }
    view
}

#[test]
fn a_merge_and_its_repair_undo_as_one_step_to_the_exact_state_before() {
    let mut doc = document();
    let (mut draft, base) = draft(&doc);
    run(&mut draft, "field.add", json!({ "tableId": 1, "title": "Reviewer", "type": "text" }));
    let changes = draft.export_changes(&base).unwrap();
    // Meanwhile the document gets a field of the same name: the repair renames one `reviewer_2`.
    run(&mut doc, "field.add", json!({ "tableId": 1, "title": "Reviewer", "type": "text" }));
    let (before, before_view, before_names) = (state_hash(&doc), timeless_view(&mut doc), field_names(&mut doc));
    assert!(!before_names.iter().any(|name| name == "reviewer_2"), "{before_names:?}");
    let depth = doc.file().replica().undo_depth(UndoScope::Person);
    let answer = doc.merge_changes(&changes, &Origin::agent("helper")).unwrap();
    assert_eq!(answer["scope"], "person", "{answer}");
    let names = field_names(&mut doc);
    assert!(names.iter().any(|name| name == "reviewer") && names.iter().any(|name| name == "reviewer_2"), "{names:?}");
    assert_eq!(doc.file().replica().undo_depth(UndoScope::Person), depth + 1, "the merge and its repair are one step");
    let (merged, merged_view) = (state_hash(&doc), timeless_view(&mut doc));
    let (done, _) = doc.undo().unwrap();
    assert!(done);
    assert_eq!(state_hash(&doc), before, "one undo of the person restores the exact state before the merge");
    assert_eq!(timeless_view(&mut doc), before_view);
    assert_eq!(field_names(&mut doc), before_names, "no `reviewer_2` is left behind");
    let (redone, _) = doc.redo().unwrap();
    assert!(redone);
    assert_eq!(state_hash(&doc), merged, "redo restores the merged state exactly");
    assert_eq!(timeless_view(&mut doc), merged_view);
}
