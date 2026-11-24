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
        "sorts": [{ "field_name": "status", "direction": "desc" }],
        "group_field_name": "status",
        "hidden_field_names": ["notes"],
        "filter_tree": { "kind": "group", "conjunction": "and", "children": [
            { "kind": "condition", "field_name": "title", "operator": "contains", "value": "a" }
        ] },
    });
    let updated = ok(here.update_view(c.table, view, &ViewPatch { config: Some(config), ..ViewPatch::default() }));
    // The projection, which the API and the editor read, names the fields by name.
    assert_eq!(updated.config.get("search_query"), None);
    let sorts = updated.config["sorts"].as_array().cloned().unwrap_or_default();
    assert_eq!(sorts.len(), 1, "{sorts:?}");
    assert_eq!((&sorts[0]["field_name"], &sorts[0]["direction"]), (&json!("status"), &json!("desc")), "{sorts:?}");
    assert_eq!(updated.config["group_field_name"], json!("status"));
    assert_eq!(updated.config["hidden_field_names"], json!(["notes"]));
    assert_eq!(updated.config["filter_tree"]["children"][0]["field_name"], json!("title"));
    // The document names them by object id, and holds no search.
    let object = here.replica().objects_of_type("View").into_iter().next().unwrap_or_else(|| panic!("no view"));
    let register = ok(here.replica().get(&object, "config"));
    let stored = ok(kition_table_file::kdm::view_store::assemble(here.replica(), &object));
    let status = status_object(&here);
    assert_eq!(stored["sorts"], json!([{ "field": status, "direction": "desc" }]));
    assert_eq!(ok(here.replica().list(&object, "sorts")).len(), 1, "the sorts are Sort objects");
    assert!(register.get("sorts").is_none(), "not in the config register: {register}");
    assert_eq!(register["group_field"], json!(status));
    assert!(register.get("search_query").is_none() && register.get("group_field_name").is_none());
    assert!(!String::from_utf8_lossy(&ok(here.replica().snapshot())).contains("secret words"), "the search is in no byte of the replica");
    // The API's sorts name the field by its integer id; the document does not.
    let est = ok(here.create_field(c.table, &field("Est", "number"))).id;
    ok(here.replace_view_sorts(view, &[kition_table_file::ViewSort { field_id: est, direction: "asc".into() }]));
    assert_eq!(ok(here.list_view_sorts(view)), vec![kition_table_file::ViewSort { field_id: est, direction: "asc".into() }]);
    let stored = ok(kition_table_file::kdm::view_store::assemble(here.replica(), &object));
    assert!(stored["sorts"][0].get("field_id").is_none() && stored["sorts"][0]["field"].is_string(), "{stored}");
}

/// The view's config as the editor reads it, with `change` applied to it.
fn edit_view(file: &mut TableFile, table: i64, change: impl FnOnce(&mut Map<String, Value>)) -> Map<String, Value> {
    let view = ok(file.get_table(table)).views.first().cloned().unwrap_or_else(|| panic!("no view"));
    let mut config = view.config.clone();
    change(&mut config);
    ok(file.update_view(table, view.id, &ViewPatch { config: Some(Value::Object(config)), ..ViewPatch::default() })).config
}

fn view_config(file: &TableFile, table: i64) -> Map<String, Value> {
    ok(file.get_table(table)).views.first().map(|view| view.config.clone()).unwrap_or_default()
}

fn condition(id: &str, field: &str, value: &str) -> Value {
    json!({ "id": id, "kind": "condition", "field_name": field, "operator": "contains", "value": value })
}

#[test]
fn filters_added_on_both_sides_are_both_kept() {
    let mut c = copies();
    let tree = |children: Vec<Value>| json!({ "id": "root", "kind": "group", "conjunction": "and", "children": children });
    edit_view(&mut c.here, c.table, |config| {
        config.insert("filter_tree".into(), tree(vec![condition("a", "title", "Write")]));
    });
    let start = ok(c.here.export_replica_changes(&c.there.replica_version()));
    ok(c.there.import_replica_changes(&start));
    edit_view(&mut c.here, c.table, |config| {
        config.insert("filter_tree".into(), tree(vec![condition("a", "title", "Write"), condition("b", "title", "Here")]));
    });
    edit_view(&mut c.there, c.table, |config| {
        config.insert("filter_tree".into(), tree(vec![condition("a", "title", "Write"), condition("c", "notes", "There")]));
    });
    c.exchange();
    let (here, there) = (view_config(&c.here, c.table), view_config(&c.there, c.table));
    assert_eq!(here, there, "both copies read the same");
    let ids: Vec<&str> =
        here["filter_tree"]["children"].as_array().unwrap_or_else(|| panic!("{here:?}")).iter().filter_map(|node| node["id"].as_str()).collect();
    assert_eq!(ids.first(), Some(&"a"));
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec!["a", "b", "c"], "{ids:?}");
    // The filters are objects of the document, naming their fields by object id.
    let filters = c.here.replica().objects_of_type("Filter");
    assert_eq!(filters.len(), 4, "the root group and three conditions");
    let view = c.here.replica().objects_of_type("View").into_iter().next().unwrap_or_else(|| panic!("no view"));
    let stored = ok(c.here.replica().get(&view, "config"));
    assert!(stored.get("filter_tree").is_none(), "the tree is not in the config register: {stored}");
}

#[test]
fn sorts_reordered_on_one_side_and_added_on_the_other_merge() {
    let mut c = copies();
    let items = |names: &[&str]| {
        json!(names.iter().map(|name| json!({ "id": format!("s-{name}"), "field_name": name, "direction": "asc" })).collect::<Vec<_>>())
    };
    edit_view(&mut c.here, c.table, |config| {
        config.insert("sorts".into(), items(&["title", "status"]));
    });
    let start = ok(c.here.export_replica_changes(&c.there.replica_version()));
    ok(c.there.import_replica_changes(&start));
    edit_view(&mut c.here, c.table, |config| {
        config.insert("sorts".into(), items(&["status", "title"]));
    });
    edit_view(&mut c.there, c.table, |config| {
        config.insert("sorts".into(), items(&["title", "status", "notes"]));
    });
    c.exchange();
    let (here, there) = (view_config(&c.here, c.table), view_config(&c.there, c.table));
    assert_eq!(here, there);
    let names: Vec<&str> =
        here["sorts"].as_array().unwrap_or_else(|| panic!("{here:?}")).iter().filter_map(|item| item["field_name"].as_str()).collect();
    assert_eq!(names.len(), 3, "{names:?}");
    let at = |name: &str| names.iter().position(|found| *found == name).unwrap_or_else(|| panic!("{name} in {names:?}"));
    assert!(at("status") < at("title"), "the reorder is kept: {names:?}");
    assert_eq!(c.here.replica().objects_of_type("Sort").len(), 3);
}

#[test]
fn columns_hidden_and_placed_on_both_sides_are_all_kept() {
    let mut c = copies();
    edit_view(&mut c.here, c.table, |config| {
        config.insert("hidden_field_names".into(), json!(["notes"]));
    });
    edit_view(&mut c.there, c.table, |config| {
        config.insert("hidden_field_names".into(), json!(["status"]));
    });
    c.exchange();
    let mut hidden: Vec<String> = view_config(&c.here, c.table)["hidden_field_names"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str().map(str::to_owned))
        .collect();
    hidden.sort();
    assert_eq!(hidden, vec!["notes", "status"]);
    assert_eq!(view_config(&c.here, c.table), view_config(&c.there, c.table));
    // A column's place is one entry of the view's layout: two columns placed at once both keep theirs.
    let view = ok(c.here.get_table(c.table)).views[0].id;
    let fields = ok(c.here.list_fields(c.table));
    let (first, second) = (fields[1].id, fields[2].id);
    ok(c.here.upsert_view_field(view, first, &kition_table_file::ViewFieldPatch { position: Some(5), ..Default::default() }));
    ok(c.there.upsert_view_field(view, second, &kition_table_file::ViewFieldPatch { frozen: Some(true), ..Default::default() }));
    c.exchange();
    let layout = view_config(&c.here, c.table)["field_layout"].clone();
    assert_eq!(layout.as_object().map(Map::len), Some(2), "{layout}");
    assert_eq!(view_config(&c.here, c.table), view_config(&c.there, c.table));
}

#[test]
fn widgets_added_and_edited_on_both_sides_are_both_kept_and_name_fields_by_id() {
    let mut c = copies();
    let widget = |id: &str, title: &str| json!({ "id": id, "title": title, "type": "bar", "query": { "aggregation": "count", "group_by_field_name": "status" } });
    let dashboard = |widgets: Vec<Value>, layout: Value| -> (Value, Map<String, Value>) {
        let config = json!({ "order": 1, "source_table_id": 0, "layout": layout });
        (config, widgets.into_iter().map(|w| (w["id"].as_str().unwrap_or_default().to_owned(), w)).collect())
    };
    let table = c.table;
    let config_of = |config: Value| -> Value {
        let mut config = config;
        config["source_table_id"] = json!(table);
        config
    };
    let (config, widgets) = dashboard(vec![widget("w1", "By status")], json!([{ "widget_id": "w1", "x": 0, "y": 0, "w": 4, "h": 3 }]));
    ok(c.here.upsert_dashboard(Some("d1"), "Overview", config_of(config), &widgets));
    let start = ok(c.here.export_replica_changes(&c.there.replica_version()));
    ok(c.there.import_replica_changes(&start));
    let (config, widgets) = dashboard(
        vec![widget("w1", "By status"), widget("w2", "Added here")],
        json!([{ "widget_id": "w1", "x": 0, "y": 0, "w": 4, "h": 3 }, { "widget_id": "w2", "x": 4, "y": 0, "w": 4, "h": 3 }]),
    );
    ok(c.here.upsert_dashboard(Some("d1"), "Overview", config_of(config), &widgets));
    let (config, widgets) = dashboard(vec![widget("w1", "Renamed there")], json!([{ "widget_id": "w1", "x": 0, "y": 0, "w": 4, "h": 3 }]));
    ok(c.there.upsert_dashboard(Some("d1"), "Overview", config_of(config), &widgets));
    c.exchange();
    let (here, there) = (ok(c.here.list_dashboards()), ok(c.there.list_dashboards()));
    assert_eq!(here, there);
    let board = &here[0];
    assert_eq!(board.widgets["w1"]["title"], "Renamed there");
    assert_eq!(board.widgets["w2"]["title"], "Added here");
    assert_eq!(board.widgets["w2"]["query"]["group_by_field_name"], "status", "readers name the field");
    assert_eq!(board.config["layout"].as_array().map(Vec::len), Some(2), "{:?}", board.config);
    // The document names the field by its object id.
    let stored: Vec<Value> = c.here.replica().objects_of_type("Widget").iter().map(|id| ok(c.here.replica().get(id, "query"))).collect();
    let status = status_object(&c.here);
    assert!(stored.iter().all(|query| query["group_by_field"] == json!(status) && query.get("group_by_field_name").is_none()), "{stored:?}");
}

#[test]
fn an_ai_instruction_edited_in_two_places_keeps_both() {
    let mut c = copies();
    let field = ok(c.here.create_field(
        c.table,
        &FieldSpec { options: json!({ "ai_config": { "type": "customize", "enabled": true, "prompt": "Write a summary" } }), ..field("Brief", "ai") },
    ))
    .id;
    let start = ok(c.here.export_replica_changes(&c.there.replica_version()));
    ok(c.there.import_replica_changes(&start));
    let options = |prompt: &str| FieldPatch {
        options: Some(json!({ "ai_config": { "type": "customize", "enabled": true, "prompt": prompt } })),
        ..FieldPatch::default()
    };
    ok(c.here.update_field(c.table, field, &options("Write a short summary")));
    ok(c.there.update_field(c.table, field, &options("Write a summary of the notes")));
    c.exchange();
    let (_, options, ai_prompt) = c.converged(field);
    assert_eq!(options["ai_config"]["prompt"], "Write a short summary of the notes");
    assert_eq!(ai_prompt, "Write a short summary of the notes", "the instruction is the field's aiPrompt");
    // The options in the document hold no copy of it.
    let object = c.here.replica().objects_of_type("Field").into_iter().find(|id| ok(c.here.replica().get(id, "name")) == json!("brief"));
    let stored = ok(c.here.replica().get(&object.unwrap_or_else(|| panic!("no brief field")), "options"));
    assert!(stored["ai_config"].get("prompt").is_none(), "{stored}");
}
