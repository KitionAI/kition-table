//! The document shapes frozen for the release (schema version 1): what merges per item or per
//! character is a container. Each test edits two copies of one document concurrently, sends
//! each one's changes to the other, and checks that both edits are there and that both copies
//! read the same:
//!
//! - a select's options are `Choice` objects its `Field.choices` lists, and its cells hold choice
//!   ids (options added on both sides are both kept; a renamed option renames the cell another
//!   copy set meanwhile);
//! - a long text's characters are a `CellText`'s `text` (typing in two places of one cell);
//! - a field's AI prompt is its `aiPrompt` text;
//! - a view config names fields by object id, and never stores the editor's search.

use std::fs;
use std::path::Path;

use kition_kdm::{ObjectId, Origin};
use kition_table_file::kdm::model::{CELL_TEXT, CHOICE};
use kition_table_file::{CreateOptions, FieldPatch, FieldSpec, TableFile, TableSpec, ViewPatch};
use serde_json::{Map, Value, json};

fn ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|err| panic!("unexpected error: {err}"))
}

fn values(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn field(title: &str, field_type: &str) -> FieldSpec {
    FieldSpec { title: title.into(), field_type: field_type.into(), ..FieldSpec::default() }
}

fn open(path: &Path) -> TableFile {
    let mut file = ok(TableFile::open(path));
    file.set_origin(Origin::person("user:test"));
    file
}

/// One document in two files, each opened by its own handle: a table "Tasks" (Title; Status, a
/// select of To do and Done; Notes, a long text; Summary, an AI field with a prompt) and one
/// record. Answers both handles and the table, record and field ids, which the copies share.
struct Copies {
    _dir: tempfile::TempDir,
    here: TableFile,
    there: TableFile,
    table: i64,
    record: i64,
    status: i64,
    summary: i64,
}

fn copies() -> Copies {
    let dir = tempfile::tempdir().unwrap_or_else(|err| panic!("{err}"));
    let path = dir.path().join("here.ktable");
    let mut file = ok(TableFile::create(&path, &CreateOptions { title: "Shared".into(), ..CreateOptions::default() }));
    file.set_origin(Origin::person("user:test"));
    let table = ok(file.create_table(&TableSpec {
        title: "Tasks".into(),
        fields: vec![
            FieldSpec { primary: true, ..field("Title", "text") },
            FieldSpec { options: json!({ "choices": ["To do", "Done"] }), ..field("Status", "single_select") },
            field("Notes", "long_text"),
            FieldSpec { ai_prompt: "Summarize notes".into(), ..field("Summary", "ai") },
        ],
        ..TableSpec::default()
    }));
    let record = ok(file.create_record(table.id, &values(json!({ "title": "Write", "status": "To do", "notes": "hello world" }))));
    let id = |name: &str| table.fields.iter().find(|field| field.name == name).map(|field| field.id).unwrap_or_else(|| panic!("{name}"));
    let (status, summary) = (id("status"), id("summary"));
    file.close();
    let other = dir.path().join("there.ktable");
    ok(fs::copy(&path, &other));
    Copies { here: open(&path), there: open(&other), table: table.id, record: record.id, status, summary, _dir: dir }
}

impl Copies {
    /// Sends each copy's changes to the other, as the relay does.
    fn exchange(&mut self) {
        let to_there = ok(self.here.export_replica_changes(&self.there.replica_version()));
        let to_here = ok(self.there.export_replica_changes(&self.here.replica_version()));
        ok(self.there.import_replica_changes(&to_there));
        ok(self.here.import_replica_changes(&to_here));
    }

    /// What both copies read of the record and of a field: equal on both sides.
    fn converged(&self, field_id: i64) -> (Map<String, Value>, Map<String, Value>, String) {
        let read = |file: &TableFile| {
            let record = ok(file.get_record(self.table, self.record));
            let field = ok(file.get_field(self.table, field_id));
            (record.values, field.options, field.ai_prompt)
        };
        let (here, there) = (read(&self.here), read(&self.there));
        assert_eq!(here, there, "both copies read the same");
        assert_eq!(self.here.replica().version(), self.there.replica().version(), "both replicas hold the same changes");
        here
    }
}

fn choices(options: &Map<String, Value>) -> Vec<String> {
    options.get("choices").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

#[test]
fn options_added_on_both_sides_are_both_kept() {
    let mut c = copies();
    let patch = |choices: Value| FieldPatch { options: Some(json!({ "choices": choices })), ..FieldPatch::default() };
    ok(c.here.update_field(c.table, c.status, &patch(json!(["To do", "Done", "Blocked"]))));
    ok(c.there.update_field(c.table, c.status, &patch(json!(["To do", "Done", "Later"]))));
    c.exchange();
    let (_, options, _) = c.converged(c.status);
    let mut merged = choices(&options);
    assert_eq!(merged.get(..2), Some(&["To do".to_owned(), "Done".to_owned()][..]), "{merged:?}");
    merged.sort();
    assert_eq!(merged, vec!["Blocked", "Done", "Later", "To do"]);
    // The options are objects of the document, listed by the field.
    let field = ok(c
        .here
        .replica()
        .objects_of_type("Field")
        .into_iter()
        .find(|id| c.here.replica().get(id, "name").ok().and_then(|name| name.as_str().map(str::to_owned)).as_deref() == Some("status"))
        .ok_or("no status field"));
    assert_eq!(ok(c.here.replica().list(&field, "choices")).len(), 4);
    assert!(c.here.replica().get(&field, "options").unwrap_or(Value::Null).get("choices").is_none(), "no choices left in the options json");
}

#[test]
fn an_option_renamed_on_one_side_renames_the_cell_set_on_the_other() {
    let mut c = copies();
    ok(c.here.update_field(c.table, c.status, &FieldPatch { options: Some(json!({ "choices": ["To do", "Finished"] })), ..FieldPatch::default() }));
    ok(c.there.update_record(c.table, c.record, &values(json!({ "status": "Done" })), None));
    c.exchange();
    let (record, options, _) = c.converged(c.status);
    assert_eq!(choices(&options), vec!["To do", "Finished"]);
    assert_eq!(record.get("status"), Some(&json!("Finished")), "the cell holds the option, not its name");
    // The cell stores the choice's id.
    let row_key = ok(c.here.get_record(c.table, c.record)).row_key;
    let stored = ok(c.here.replica().map_get(&ObjectId::from_string(row_key), "cells", &status_object(&c.here)));
    let choice = ObjectId::from_string(stored.as_str().unwrap_or_default());
    assert_eq!(ok(c.here.replica().object_type(&choice)), CHOICE);
}

fn status_object(file: &TableFile) -> String {
    file.replica()
        .objects_of_type("Field")
        .into_iter()
        .find(|id| file.replica().get(id, "name").ok().and_then(|name| name.as_str().map(str::to_owned)).as_deref() == Some("status"))
        .map(|id| id.to_string())
        .unwrap_or_else(|| panic!("no status field"))
}

#[test]
fn a_value_that_names_no_option_stays_a_value_the_field_does_not_list() {
    let mut c = copies();
    ok(c.here.update_record(c.table, c.record, &values(json!({ "status": "Blocked" })), None));
    let record = ok(c.here.get_record(c.table, c.record));
    assert_eq!(record.values.get("status"), Some(&json!("Blocked")));
    let field = ok(c.here.get_field(c.table, c.status));
    assert_eq!(choices(&field.options), vec!["To do", "Done"], "the field does not list it");
    // Listing it makes it one of the options: the same choice, so the cell keeps it.
    ok(c.here.update_field(
        c.table,
        c.status,
        &FieldPatch { options: Some(json!({ "choices": ["To do", "Done", "Blocked"] })), ..FieldPatch::default() },
    ));
    ok(c.here.update_field(
        c.table,
        c.status,
        &FieldPatch { options: Some(json!({ "choices": ["To do", "Done", "On hold"] })), ..FieldPatch::default() },
    ));
    assert_eq!(ok(c.here.get_record(c.table, c.record)).values.get("status"), Some(&json!("On hold")), "renamed in place");
}

#[test]
fn typing_in_two_places_of_one_long_text_keeps_both() {
    let mut c = copies();
    ok(c.here.update_record(c.table, c.record, &values(json!({ "notes": "hello brave world" })), None));
    ok(c.there.update_record(c.table, c.record, &values(json!({ "notes": "hello world!" })), None));
    c.exchange();
    let (record, _, _) = c.converged(c.status);
    assert_eq!(record.get("notes"), Some(&json!("hello brave world!")));
    // The characters are a CellText's text; the cell holds its id.
    let row_key = ok(c.here.get_record(c.table, c.record)).row_key;
    let notes = c
        .here
        .replica()
        .objects_of_type("Field")
        .into_iter()
        .find(|id| c.here.replica().get(id, "name").ok().and_then(|name| name.as_str().map(str::to_owned)).as_deref() == Some("notes"))
        .unwrap_or_else(|| panic!("no notes field"));
    let stored = ok(c.here.replica().map_get(&ObjectId::from_string(row_key), "cells", notes.as_str()));
    let text = ObjectId::from_string(stored.as_str().unwrap_or_default());
    assert_eq!(ok(c.here.replica().object_type(&text)), CELL_TEXT);
    assert_eq!(ok(c.here.replica().text(&text, "text")), "hello brave world!");
}

#[test]
fn a_long_text_changes_type_and_back() {
    let mut c = copies();
    let notes = ok(c.here.list_fields(c.table)).into_iter().find(|field| field.name == "notes").map(|field| field.id).unwrap_or_default();
    ok(c.here.update_field(c.table, notes, &FieldPatch { field_type: Some("text".into()), ..FieldPatch::default() }));
    assert_eq!(ok(c.here.get_record(c.table, c.record)).values.get("notes"), Some(&json!("hello world")));
    assert!(c.here.replica().objects_of_type(CELL_TEXT).is_empty(), "a text cell holds its value");
    ok(c.here.update_field(c.table, notes, &FieldPatch { field_type: Some("long_text".into()), ..FieldPatch::default() }));
    assert_eq!(ok(c.here.get_record(c.table, c.record)).values.get("notes"), Some(&json!("hello world")));
    assert_eq!(c.here.replica().objects_of_type(CELL_TEXT).len(), 1);
    // Clearing the cell drops its text.
    ok(c.here.update_record(c.table, c.record, &values(json!({ "notes": null })), None));
    assert!(c.here.replica().objects_of_type(CELL_TEXT).is_empty());
}

#[test]
fn editing_an_ai_prompt_in_two_places_keeps_both() {
    let mut c = copies();
    let prompt = |text: &str| FieldPatch { ai_prompt: Some(text.into()), ..FieldPatch::default() };
    ok(c.here.update_field(c.table, c.summary, &prompt("Summarize the notes")));
    ok(c.there.update_field(c.table, c.summary, &prompt("Summarize notes briefly")));
    c.exchange();
    let (_, _, ai_prompt) = c.converged(c.summary);
    assert_eq!(ai_prompt, "Summarize the notes briefly");
}

#[test]
fn a_view_config_names_fields_by_id_and_never_stores_the_search() {
    let c = copies();
    let mut here = c.here;
    let view = ok(here.get_table(c.table)).views.first().map(|view| view.id).unwrap_or_default();
    let config = json!({
        "search_query": "secret words",
        "sort_items": [{ "field_name": "status", "direction": "desc" }],
        "group_field_name": "status",
        "hidden_field_names": ["notes"],
        "filter_tree": { "kind": "group", "conjunction": "and", "children": [
            { "kind": "condition", "field_name": "title", "operator": "contains", "value": "a" }
        ] },
    });
    let updated = ok(here.update_view(c.table, view, &ViewPatch { config: Some(config), ..ViewPatch::default() }));
    // The projection, which the API and the editor read, names the fields by name.
    assert_eq!(updated.config.get("search_query"), None);
    assert_eq!(updated.config["sort_items"], json!([{ "field_name": "status", "direction": "desc" }]));
    assert_eq!(updated.config["group_field_name"], json!("status"));
    assert_eq!(updated.config["hidden_field_names"], json!(["notes"]));
    assert_eq!(updated.config["filter_tree"]["children"][0]["field_name"], json!("title"));
    // The document names them by object id, and holds no search.
    let object = here.replica().objects_of_type("View").into_iter().next().unwrap_or_else(|| panic!("no view"));
    let stored = ok(here.replica().get(&object, "config"));
    let status = status_object(&here);
    assert_eq!(stored["sort_items"], json!([{ "field": status, "direction": "desc" }]));
    assert_eq!(stored["group_field"], json!(status));
    assert!(stored.get("search_query").is_none() && stored.get("group_field_name").is_none());
    assert!(!String::from_utf8_lossy(&ok(here.replica().snapshot())).contains("secret words"), "the search is in no byte of the replica");
    // The API's sorts name the field by its integer id; the document does not.
    let est = ok(here.create_field(c.table, &field("Est", "number"))).id;
    ok(here.replace_view_sorts(view, &[kition_table_file::ViewSort { field_id: est, direction: "asc".into() }]));
    assert_eq!(ok(here.list_view_sorts(view)), vec![kition_table_file::ViewSort { field_id: est, direction: "asc".into() }]);
    let stored = ok(here.replica().get(&object, "config"));
    assert!(stored["sorts"][0].get("field_id").is_none() && stored["sorts"][0]["field"].is_string(), "{stored}");
}
