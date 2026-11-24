//! `kition-table` served headless keeps the editor's state (the table, view or dashboard its picture
//! shows, `table.preview.show`) out of what an agent reads, and sets it for one call and puts it
//! back through the Kit Process Protocol's `context.get`, `context.set` and `context.restore`
//! (section 7a), so nothing an agent does depends on or leaves behind what the person looks at.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_headless::{Headless, HostFolder, KitInfo};
use kition_kit_server::protocol::Origin;
use kition_kit_server::{KitBackend, OpenRequest};
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::{Map, Value, json};

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn kit() -> Headless<TableDocument> {
    Headless::<TableDocument>::with_replica_port(
        KitInfo::from_manifest(include_str!("../../../kit.json"), &[], kition_table_kit::EDITOR_STATE).unwrap(),
    )
}

fn result(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params).unwrap_or_else(|error| panic!("{command}: {error}"))["result"].clone()
}

/// The starter table 1 (`Table`: title, status, notes; view 1 `All`) with two records, and table 2
/// `Tasks` (fields 4 title and 5 done) with records 3 to 5 and a second view `Open` (view 3).
fn document() -> TableDocument {
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    for title in ["Write the plan", "Review the plan"] {
        result(&mut doc, "table.records.create", json!({ "tableId": 1, "values": { "title": title } }));
    }
    let tasks = result(
        &mut doc,
        "table.tables.create",
        json!({ "title": "Tasks", "name": "tasks", "fields": [
            { "title": "Title", "name": "title", "type": "text", "primary": true },
            { "title": "Done", "name": "done", "type": "checkbox" },
        ] }),
    );
    assert_eq!(tasks["id"], json!(2));
    for title in ["Book", "Pack", "Go"] {
        result(&mut doc, "table.records.create", json!({ "tableId": 2, "values": { "title": title } }));
    }
    result(&mut doc, "table.views.create", json!({ "tableId": 2, "title": "Open", "type": "grid" }));
    doc
}

fn open(name: &str) -> (Headless<TableDocument>, String, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("kition-table-context-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    std::fs::write(root.join("Work.ktable"), document().sealed_save().unwrap()).unwrap();
    let mut kit = kit();
    let folder = HostFolder::new(&root).unwrap();
    kit.set_host(folder.host());
    let id = kit.open(OpenRequest { file: &folder.file("Work.ktable").unwrap(), read_only: false, create: None }).unwrap().id;
    (kit, id, root)
}

fn run(kit: &mut Headless<TableDocument>, id: &str, command: &str, value: Value, origin: Origin) -> Value {
    kit.execute(id, command, params(value), origin).unwrap_or_else(|error| panic!("{command}: {error:?}")).result.unwrap_or(Value::Null)
}

/// Each table's records' first cells, by table id.
fn titles(kit: &mut Headless<TableDocument>, id: &str, table: i64) -> Vec<Value> {
    let state = kit.describe(id).unwrap().state;
    let table = state["tables"].as_array().unwrap().iter().find(|t| t["id"] == json!(table)).unwrap().clone();
    table["records"]["rows"].as_array().unwrap().iter().map(|row| row["cells"][0].clone()).collect()
}

#[test]
fn what_an_agent_reads_of_the_file_leaves_out_what_the_picture_shows() {
    let (mut kit, id, root) = open("reads");
    let before = kit.describe(&id).unwrap().state;
    let saved = kit.context_get(&id).unwrap();
    run(&mut kit, &id, "table.preview.show", json!({ "viewId": 2 }), Origin::User);
    assert_ne!(kit.context_get(&id).unwrap(), saved, "the person's picture moved");
    assert_eq!(kit.describe(&id).unwrap().state, before, "what the picture shows changed what the agent reads");
    let shown = Value::Object(kit.describe(&id).unwrap().state);
    for key in kition_table_kit::EDITOR_STATE {
        assert!(shown.pointer(key).is_none(), "{key} is the editor's: {shown}");
    }
    let _ = std::fs::remove_dir_all(root);
}

/// A run as `kition-kits` makes it (section 7a): the step's part is set, the command acts on the
/// records its parameters and `on` name, and the person's picture comes back.
#[test]
fn a_run_sets_each_steps_part_and_puts_the_editor_back() {
    let (mut kit, id, root) = open("run");
    run(&mut kit, &id, "table.preview.show", json!({ "tableId": 1 }), Origin::User);
    let saved = kit.context_get(&id).unwrap();
    kit.context_set(&id, &params(json!({ "part": { "table": 2 } }))).unwrap();
    assert_ne!(kit.context_get(&id).unwrap(), saved, "the part moved the editor for the step");
    run(&mut kit, &id, "table.records.update", json!({ "tableId": 2, "recordId": 4, "values": { "title": "Pack light" } }), Origin::Agent);
    kit.context_set(&id, &params(json!({ "part": { "view": 2 } }))).unwrap();
    run(&mut kit, &id, "table.records.update", json!({ "tableId": 2, "recordId": 5, "values": { "done": true } }), Origin::Agent);
    kit.context_restore(&id, &saved).unwrap();
    assert_eq!(kit.context_get(&id).unwrap(), saved, "the picture shows the person's table again");
    assert_eq!(titles(&mut kit, &id, 2), [json!("Book"), json!("Pack light"), json!("Go")]);
    assert_eq!(titles(&mut kit, &id, 1), [json!("Write the plan"), json!("Review the plan")], "table 1 untouched");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_context_the_file_lacks_or_cannot_mean_is_refused_and_moves_nothing() {
    let (mut kit, id, root) = open("refused");
    let saved = kit.context_get(&id).unwrap();
    for (given, says) in [
        (json!({ "part": { "table": 9 } }), "no table 9"),
        (json!({ "part": { "view": 9 } }), "no view 9"),
        (json!({ "part": { "table": 1, "view": 2 } }), "view 2 is of table 2"),
        (json!({ "part": { "sheet": 1 } }), "names no part of a table file"),
        (json!({ "part": { "table": "Tasks" } }), "by its id"),
        (json!({ "part": { "table": 2 }, "on": [99] }), "no record or field 99"),
        (json!({ "part": { "table": 2 }, "on": [4] }), "both a record and a field"),
        (json!({ "on": [4] }), "needs the part's table"),
    ] {
        let error = kit.context_set(&id, &params(given.clone())).unwrap_err();
        assert_eq!(error.code, kition_kit_server::ErrorReason::InvalidParams.code(), "{given}: {error:?}");
        assert!(error.message.contains(says) || error.data.as_ref().is_some_and(|data| data.to_string().contains(says)), "{given}: {error:?}");
        assert_eq!(kit.context_get(&id).unwrap(), saved, "{given} changed the editor");
    }
    // What a table file cannot set: a time, and a selection (it keeps none; `on` fills a
    // command's record or field parameter instead).
    for given in [json!({ "time": 1.5 }), json!({ "part": { "table": 2 }, "on": [3] })] {
        let error = kit.context_set(&id, &params(given.clone())).unwrap_err();
        assert_eq!(error.code, kition_kit_server::ErrorReason::CommandDisabled.code(), "{given}: {error:?}");
        assert_eq!(kit.context_get(&id).unwrap(), saved, "{given} changed the editor");
    }
    let _ = std::fs::remove_dir_all(root);
}

/// An agent's command leaves the person's picture as it was, also when it fails; only
/// `table.preview.show`, whose work is moving the picture, keeps its effect.
#[test]
fn an_agents_command_moves_nothing_but_preview_show() {
    let (mut kit, id, root) = open("named");
    let saved = kit.context_get(&id).unwrap();
    run(&mut kit, &id, "table.records.create", json!({ "tableId": 2, "values": { "title": "Return" } }), Origin::Agent);
    run(&mut kit, &id, "table.file.render", json!({ "target": { "table": 2 } }), Origin::Agent);
    assert_eq!(kit.context_get(&id).unwrap(), saved, "an agent's command moved the picture");
    assert!(kit.execute(&id, "table.records.update", params(json!({ "tableId": 2, "recordId": 99, "values": {} })), Origin::Agent).is_err());
    assert_eq!(kit.context_get(&id).unwrap(), saved, "a failed command moved the picture");
    run(&mut kit, &id, "table.preview.show", json!({ "tableId": 2 }), Origin::Agent);
    assert_ne!(kit.context_get(&id).unwrap(), saved, "preview.show is the agent's move");
    kit.context_restore(&id, &saved).unwrap();
    assert_eq!(kit.context_get(&id).unwrap(), saved);
    let _ = std::fs::remove_dir_all(root);
}

/// `kition_kit::testing::check_context` with a context that really moves the editor: another
/// table's view.
#[test]
fn the_editor_state_hooks_pass_the_sdks_contract_check() {
    let mut doc = document();
    let context = kition_kit::CallContext { on: Vec::new(), part: Some(json!({ "view": 2 })), time: None };
    kition_kit::testing::check_context(&mut doc, &context, kition_table_kit::EDITOR_STATE).unwrap();
    let before = doc.editor_state();
    doc.set_context(&context).unwrap();
    assert_ne!(doc.editor_state(), before, "the context moved the editor");
    doc.restore_editor_state(&before).unwrap();
    assert_eq!(doc.editor_state(), before);
}

/// What the kit publishes: no command reads editor state (`x-khp-reads`), and the record or field
/// parameter `on` fills (`x-khp-role`).
#[test]
fn the_agents_commands_say_what_on_fills_and_read_no_editor_state() {
    let (mut kit, id, root) = open("published");
    let commands = kit.agent_commands(Some(&id)).unwrap();
    let params = |command: &str| commands.iter().find(|listed| listed.id == command).map(|listed| listed.params.clone()).unwrap();
    for command in commands.iter() {
        assert!(command.params.get("x-khp-reads").is_none(), "{} reads {}", command.id, command.params["x-khp-reads"]);
    }
    assert_eq!(params("table.records.update")["properties"]["recordId"]["x-khp-role"], "targets");
    assert_eq!(params("table.records.delete")["properties"]["recordId"]["x-khp-role"], "targets");
    assert_eq!(params("table.fields.update")["properties"]["fieldId"]["x-khp-role"], "targets");
    assert_eq!(params("table.views.update")["properties"]["viewId"]["x-khp-role"], "object", "a view is an explicit object, not what on names");
    let mut view = params("table.file.render")["properties"]["target"]["properties"].as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    view.sort();
    assert_eq!(view, ["path", "rows", "sheet", "table", "view"]);
    let _ = std::fs::remove_dir_all(root);
}
