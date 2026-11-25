//! What an agent reads of a table file (KHP A52, A24a, A25, A46, A47), as the headless kit
//! answers it (`view.describe`, the `details` of `table_inspect`): every table with its fields
//! (ids, names, labels, types, select choices, link targets), its views (what they filter, sort
//! and group by), its record count and its records with their ids and values, a part at a time;
//! and the ids it answers are the ones the commands take. An edit changes the answer and the
//! change survives a save and a reopen. Before, the answer was `{"tables": [{"id": 1}]}`: the
//! host leaves out `title` keys, and nothing else was answered.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::path::PathBuf;

use kition_headless::{Headless, HostFolder, KitInfo};
use kition_kit_server::protocol::Origin;
use kition_kit_server::{KitBackend, OpenRequest};
use kition_table_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Map, Value, json};

const MANIFEST: &str = include_str!("../../../kit.json");

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn result(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params).unwrap_or_else(|error| panic!("{command}: {error}"))["result"].clone()
}

/// Two tables, People (3 records) and Projects (30 records, each led by a person through a link
/// field), and a view of Projects that keeps the open ones, by title from Z to A.
fn fixture() -> TableDocument {
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    doc.execute("table.tables.delete", json!({ "tableId": 1, "asOrigin": "system:setup" })).unwrap();
    let people = result(
        &mut doc,
        "table.tables.create",
        json!({ "title": "People", "name": "people", "fields": [{ "title": "Name", "name": "name", "type": "text", "primary": true }] }),
    );
    let people_id = people["id"].as_i64().unwrap();
    for name in ["Ada", "Grace", "Linus"] {
        result(&mut doc, "table.records.create", json!({ "tableId": people_id, "values": { "name": name } }));
    }
    let projects = result(
        &mut doc,
        "table.tables.create",
        json!({ "title": "Projects", "name": "projects", "fields": [
            { "title": "Title", "name": "title", "type": "text", "primary": true },
            { "title": "Status", "name": "status", "type": "single_select", "options": { "choices": ["Open", "Done"] } },
            { "title": "Lead", "name": "lead", "type": "link_to_record", "options": { "target_table_id": people_id } },
        ] }),
    );
    let projects_id = projects["id"].as_i64().unwrap();
    let ada = result(&mut doc, "table.records.list", json!({ "tableId": people_id }))["items"][0]["id"].clone();
    for n in 1..=30 {
        let status = if n % 3 == 0 { "Done" } else { "Open" };
        result(
            &mut doc,
            "table.records.create",
            json!({ "tableId": projects_id, "values": { "title": format!("Project {n:02}"), "status": status, "lead": [ada] } }),
        );
    }
    let view = result(&mut doc, "table.views.create", json!({ "tableId": projects_id, "title": "Open work", "type": "grid" }));
    let status = projects["fields"].as_array().unwrap().iter().find(|field| field["name"] == "status").unwrap()["id"].clone();
    let title = projects["fields"].as_array().unwrap().iter().find(|field| field["name"] == "title").unwrap()["id"].clone();
    let view_id = view["id"].clone();
    result(
        &mut doc,
        "table.views.putFilterTree",
        json!({ "tableId": projects_id, "viewId": view_id, "tree": { "conjunction": "and", "children": [{ "condition": { "field_id": status, "operator": "neq", "value": "Done" } }] } }),
    );
    result(
        &mut doc,
        "table.views.putSorts",
        json!({ "tableId": projects_id, "viewId": view_id, "items": [{ "field_id": title, "direction": "desc" }] }),
    );
    doc
}

struct Workspace {
    root: PathBuf,
    kit: Headless<TableDocument>,
    folder: HostFolder,
}

fn workspace(name: &str) -> Workspace {
    let root = std::env::temp_dir().join(format!("kition-table-inspect-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    std::fs::write(root.join("Work.ktable"), fixture().saved_bytes().unwrap()).unwrap();
    let mut kit = Headless::<TableDocument>::with_replica_port(KitInfo::from_manifest(MANIFEST, &[], kition_table_kit::EDITOR_STATE).unwrap());
    let folder = HostFolder::new(&root).unwrap();
    kit.set_host(folder.host());
    Workspace { root, kit, folder }
}

impl Workspace {
    fn open(&mut self) -> String {
        let file = self.folder.file("Work.ktable").unwrap();
        self.kit.open(OpenRequest { file: &file, read_only: false, create: None }).unwrap().id
    }

    /// What the agent reads: `view.describe`'s state.
    fn inspect(&mut self, id: &str) -> Value {
        Value::Object(self.kit.describe(id).unwrap().state)
    }

    fn run(&mut self, id: &str, command: &str, value: Value) -> Value {
        let executed = self.kit.execute(id, command, params(value), Origin::Agent).unwrap_or_else(|error| panic!("{command}: {}", error.message));
        executed.result.unwrap_or_default()
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn table<'a>(inspected: &'a Value, name: &str) -> &'a Value {
    inspected["tables"].as_array().unwrap().iter().find(|table| table["name"] == name).unwrap_or_else(|| panic!("no table {name}: {inspected}"))
}

fn field<'a>(table: &'a Value, name: &str) -> &'a Value {
    table["fields"].as_array().unwrap().iter().find(|field| field["name"] == name).unwrap_or_else(|| panic!("no field {name}: {table}"))
}

/// A record's cell under `column`.
fn cell<'a>(records: &'a Value, row: usize, column: &str) -> &'a Value {
    let at = records["columns"].as_array().unwrap().iter().position(|name| name == column).unwrap_or_else(|| panic!("no column {column}"));
    &records["rows"][row]["cells"][at]
}

#[test]
fn inspect_answers_tables_fields_views_and_records_with_the_ids_the_commands_take() {
    let mut w = workspace("content");
    let id = w.open();
    let inspected = w.inspect(&id);

    let people = table(&inspected, "people");
    assert_eq!(people["label"], "People");
    assert_eq!(people["recordCount"], 3);
    let projects = table(&inspected, "projects");
    assert_eq!(projects["label"], "Projects");
    assert_eq!(projects["recordCount"], 30);
    assert_eq!(field(projects, "title")["label"], "Title");
    assert_eq!(field(projects, "title")["primary"], true);
    assert_eq!(field(projects, "status")["type"], "single_select");
    assert_eq!(field(projects, "status")["options"]["choices"], json!(["Open", "Done"]));
    assert_eq!(field(projects, "lead")["type"], "link_to_record");
    assert_eq!(field(projects, "lead")["options"]["target_table_id"], people["id"]);
    let view = &projects["views"].as_array().unwrap().iter().find(|view| view["label"] == "Open work").unwrap_or_else(|| panic!("{projects}"));
    assert_eq!(view["type"], "grid");
    assert_eq!(view["filter"], "status neq \"Done\"");
    assert_eq!(view["sorts"], json!(["title desc"]));

    // The records, under the field names the commands key values by, with their record ids; a
    // link's records by record id, the id People lists them by.
    let records = &projects["records"];
    assert_eq!(records["columns"], json!(["title", "status", "lead"]));
    assert_eq!(cell(records, 0, "title"), "Project 01");
    assert_eq!(cell(records, 0, "status"), "Open");
    let lead = cell(records, 0, "lead")[0].clone();
    assert_eq!(lead["display"], "Ada");
    assert_eq!(lead["id"], people["records"]["rows"][0]["id"], "a link names a record by its record id");
    assert_eq!(cell(&people["records"], 0, "name"), "Ada");

    // The ids work in the commands as they are.
    let (table_id, record_id) = (projects["id"].clone(), records["rows"][1]["id"].clone());
    let updated = w.run(&id, "table.records.update", json!({ "tableId": table_id, "recordId": record_id, "values": { "status": "Done" } }));
    assert_eq!(updated["result"]["values"]["title"], "Project 02");
    let view_id = view["id"].clone();
    w.run(
        &id,
        "table.views.putSorts",
        json!({ "tableId": table_id, "viewId": view_id, "items": [{ "field_id": field(projects, "status")["id"], "direction": "asc" }] }),
    );
}

#[test]
fn inspect_cuts_the_records_and_table_read_lists_the_rest_a_page_at_a_time() {
    let mut w = workspace("paging");
    let id = w.open();
    let inspected = w.inspect(&id);
    let projects = table(&inspected, "projects");
    let records = &projects["records"];
    assert_eq!(records["rows"].as_array().unwrap().len(), 20, "a table's records are cut: {records}");
    assert_eq!((records["from"].clone(), records["to"].clone(), records["total"].clone()), (json!(1), json!(20), json!(30)));
    let next = records["next"].clone();
    assert_eq!(next["command"], "table.read", "the cut answer names the step for the rest: {records}");
    assert!(table(&inspected, "people")["records"].get("next").is_none(), "nothing is cut of People");

    // The step, run as it is, lists the rest; the last page names no next.
    let page = w.run(&id, next["command"].as_str().unwrap(), next["params"].clone())["result"].clone();
    assert_eq!(page["rows"].as_array().unwrap().len(), 10);
    assert_eq!((page["from"].clone(), page["to"].clone(), page["total"].clone()), (json!(21), json!(30), json!(30)));
    assert_eq!(cell(&page, 0, "title"), "Project 21");
    assert!(page.get("next").is_none(), "{page}");

    // One view's records: those its filter keeps, in its order, `limit` at a time.
    let view_id = projects["views"].as_array().unwrap().iter().find(|view| view["label"] == "Open work").unwrap()["id"].clone();
    let first = w.run(&id, "table.read", json!({ "viewId": view_id, "limit": 5 }))["result"].clone();
    assert_eq!(first["total"], 20, "30 projects, 10 done");
    assert_eq!(first["rows"].as_array().unwrap().len(), 5);
    assert_eq!(cell(&first, 0, "title"), "Project 29");
    assert_eq!(cell(&first, 1, "title"), "Project 28");
    let second = w.run(&id, "table.read", first["next"]["params"].clone())["result"].clone();
    assert_eq!(cell(&second, 0, "title"), "Project 22");
    assert!(w.kit.execute(&id, "table.read", params(json!({ "viewId": 999 })), Origin::Agent).is_err());
    assert!(w.kit.execute(&id, "table.read", params(json!({ "tableId": 1, "limit": 0 })), Origin::Agent).is_err());
}

#[test]
fn an_edit_changes_the_answer_and_survives_a_save_and_a_reopen() {
    let mut w = workspace("edit");
    let id = w.open();
    let before = w.inspect(&id);
    let projects = table(&before, "projects");
    let (table_id, status) = (projects["id"].clone(), field(projects, "status")["id"].clone());
    let record = projects["records"]["rows"][0]["id"].clone();
    w.run(&id, "table.fields.update", json!({ "tableId": table_id, "fieldId": status, "title": "Stage" }));
    w.run(&id, "table.records.update", json!({ "tableId": table_id, "recordId": record, "values": { "title": "Renamed project" } }));
    let edited = w.inspect(&id);
    assert_ne!(edited, before);
    let projects = table(&edited, "projects");
    assert_eq!(field(projects, "status")["label"], "Stage");
    assert_eq!(cell(&projects["records"], 0, "title"), "Renamed project");

    w.kit.save(&id, None, None).unwrap();
    w.kit.close(&id, false).unwrap();
    let id = w.open();
    assert_eq!(w.inspect(&id), edited, "the edit survives a save and a reopen");
}

#[test]
fn the_summary_counts_tables_and_records() {
    let mut w = workspace("summary");
    let id = w.open();
    assert_eq!(w.kit.describe(&id).unwrap().summary, "2 tables (33 records)");
}
