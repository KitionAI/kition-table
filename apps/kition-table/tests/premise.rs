//! The Table kit's call context, receipts and ids end to end: the
//! headless `kition-table` executable under the kits' MCP server's tools (`kition-kits`, the
//! library `kition-kits-mcp` serves, at the pinned kition-sdk revision), on a `.ktable` in a
//! workspace.
//!
//! The commands, `on` and `table_inspect` name tables, fields, views and records by the file's
//! integer ids; receipts (`changes`, `others`) name them by the object model's ids, which are the
//! replica's object ids (a record's is its `row_key`): the replica holds no integer ids, so no
//! tool id can be declared.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use kition_kits::tools::{self, CallOptions};
use kition_kits::{KitDefinition, KitManager};
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::{Value, json};

const ME: &str = "agent-me";
const OTHER: &str = "agent-other";

fn kit() -> &'static KitDefinition {
    KitDefinition::from_manifest_text(include_str!("../../../kit.json")).unwrap()
}

fn result(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params).unwrap_or_else(|error| panic!("{command}: {error}"))["result"].clone()
}

/// Table 1 (the starter: records 1 and 2) and table 2 `Tasks` (fields 4 title and 5 done, records
/// 3 to 5, views 2 `All` and 3 `Open`).
fn file() -> Vec<u8> {
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    for title in ["Write the plan", "Review the plan"] {
        result(&mut doc, "table.records.create", json!({ "tableId": 1, "values": { "title": title } }));
    }
    result(
        &mut doc,
        "table.tables.create",
        json!({ "title": "Tasks", "name": "tasks", "fields": [
            { "title": "Title", "name": "title", "type": "text", "primary": true },
            { "title": "Done", "name": "done", "type": "checkbox" },
        ] }),
    );
    for title in ["Book", "Pack", "Go"] {
        result(&mut doc, "table.records.create", json!({ "tableId": 2, "values": { "title": title } }));
    }
    result(&mut doc, "table.views.create", json!({ "tableId": 2, "title": "Open", "type": "grid" }));
    doc.save(None).unwrap()
}

struct Fixture {
    kits: KitManager,
    workspace: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("Work.ktable"), file()).unwrap();
        Self { kits: Self::server(workspace.path()), workspace }
    }

    fn server(workspace: &Path) -> KitManager {
        KitManager::new(Some(workspace.to_path_buf()), vec![(kit(), Some(PathBuf::from(env!("CARGO_BIN_EXE_kition-table"))))])
    }

    fn bytes(&self) -> Vec<u8> {
        std::fs::read(self.workspace.path().join("Work.ktable")).unwrap()
    }

    async fn call_as(&self, agent: &str, action: &str, mut input: Value) -> Result<Value, Value> {
        input["path"] = json!("Work.ktable");
        match tools::call_as(&self.kits, &format!("table_{action}"), &input, CallOptions::default(), agent).await {
            Some(Ok(output)) => Ok(output.output),
            Some(Err(error)) => Err(error.structured()),
            None => panic!("table_{action} is no tool"),
        }
    }

    async fn ok(&self, action: &str, input: Value) -> Value {
        self.by(ME, action, input).await
    }

    async fn by(&self, agent: &str, action: &str, input: Value) -> Value {
        self.call_as(agent, action, input).await.unwrap_or_else(|error| panic!("{agent} table_{action}: {error}"))
    }

    async fn refused(&self, action: &str, input: Value) -> Value {
        match self.call_as(ME, action, input).await {
            Err(error) => error,
            Ok(answer) => panic!("table_{action} was expected to be refused: {answer}"),
        }
    }

    /// The tables as `table_inspect` lists them, and the answer's `revision`.
    async fn tables(&self) -> (Vec<Value>, String) {
        let seen = self.ok("inspect", json!({})).await;
        (seen["details"]["tables"].as_array().unwrap().clone(), seen["revision"].as_str().unwrap().to_owned())
    }

    /// Record `id` of table `table` as `table.records.list` answers it.
    async fn record(&self, table: i64, id: i64) -> Value {
        let ran = self.ok("run", json!({ "steps": [{ "command": "table.records.list", "params": { "tableId": table } }] })).await;
        ran["completed"][0]["result"]["result"]["items"].as_array().unwrap().iter().find(|r| r["id"] == json!(id)).cloned().unwrap()
    }

    async fn shutdown(self) {
        self.kits.shutdown().await;
    }
}

/// A table's id and its fields', views' and records' ids, as `table_inspect` lists them.
type Ids = (Value, Vec<Value>, Vec<Value>, Vec<Value>);

fn ids(list: &Value) -> Vec<String> {
    list.as_array().map(|items| items.iter().filter_map(|item| item["id"].as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_step_with_on_and_at_part_updates_exactly_that_record_and_the_receipt_names_it_with_before_and_after() {
    let f = Fixture::new();
    let pack = f.record(2, 4).await;
    let ran = f
        .ok(
            "run",
            json!({ "steps": [{ "command": "table.records.update", "at": { "part": { "table": 2 } }, "on": [4], "params": { "tableId": 2, "values": { "title": "Pack light" } } }] }),
        )
        .await;
    assert_eq!(ran["completed"][0]["result"]["result"]["id"], json!(4), "{ran}");
    assert_eq!(ran["completed"][0]["result"]["change"]["recordsUpserted"], json!([[2, pack["row_key"]]]), "{ran}");
    // The receipt names that record alone, by its object id (its row_key), and its cells before
    // and after.
    assert_eq!(ids(&ran["changes"]["changed"]), [pack["row_key"].as_str().unwrap().to_owned()], "{ran}");
    let edit = &ran["changes"]["changed"][0]["edits"][0];
    let saved = TableDocument::open("Work.ktable", &f.bytes()).unwrap();
    let title: String = saved.file().conn().query_row("SELECT kdm_id FROM kition_fields WHERE id = 4", [], |row| row.get(0)).unwrap();
    assert_eq!(edit["field"], format!("cells.{title}"), "{ran}");
    assert_eq!((&edit["before"], &edit["after"]), (&json!("Pack"), &json!("Pack light")), "{ran}");
    for table in [1, 2] {
        for id in if table == 1 { [1, 2].to_vec() } else { [3, 5].to_vec() } {
            assert_ne!(f.record(table, id).await["values"]["title"], json!("Pack light"));
        }
    }
    f.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_field_rename_is_reported_with_its_title_before_and_after() {
    let f = Fixture::new();
    let ran = f.ok("run", json!({ "steps": [{ "command": "table.fields.update", "on": [5], "params": { "tableId": 2, "title": "Packed" } }] })).await;
    let changed = ran["changes"]["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{ran}");
    assert_eq!(
        (changed[0]["type"].clone(), changed[0]["edits"].clone()),
        (json!("Field"), json!([{ "field": "title", "before": "Done", "after": "Packed" }])),
        "{ran}"
    );
    f.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_later_step_changes_nothing() {
    let f = Fixture::new();
    let before = f.bytes();
    let refused = f
        .refused(
            "run",
            json!({ "target": { "table": 2 }, "steps": [
                { "command": "table.records.update", "on": [4], "params": { "tableId": 2, "values": { "title": "kept?" } } },
                { "command": "table.records.update", "on": [99], "params": { "tableId": 2, "values": {} } }
            ] }),
        )
        .await;
    assert_eq!(refused["error"]["index"], json!(1), "{refused}");
    assert_eq!(f.bytes(), before, "the run was rolled back");
    f.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_part_the_file_lacks_a_time_or_a_stray_on_is_refused() {
    let f = Fixture::new();
    let before = f.bytes();
    for (input, says) in [
        (json!({ "target": { "table": 9 }, "steps": [{ "command": "table.records.list", "params": { "tableId": 2 } }] }), "no table 9"),
        (json!({ "steps": [{ "command": "table.records.list", "at": { "part": { "view": 9 } }, "params": { "tableId": 2 } }] }), "no view 9"),
        (json!({ "steps": [{ "command": "table.records.list", "at": { "time": 1 }, "params": { "tableId": 2 } }] }), "no timeline"),
        (json!({ "steps": [{ "command": "table.views.update", "on": [3], "params": { "tableId": 2, "viewId": 3, "title": "X" } }] }), "on"),
        (json!({ "steps": [{ "command": "table.records.update", "on": [4], "params": { "tableId": 2, "recordId": 4, "values": {} } }] }), "recordId"),
    ] {
        let refused = f.refused("run", input.clone()).await;
        assert!(refused.to_string().contains(says), "{input}: {refused}");
    }
    assert_eq!(f.bytes(), before, "nothing changed in the file");
    f.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn integer_ids_from_an_earlier_inspect_name_the_same_objects_after_a_new_server_opens_the_saved_file() {
    let f = Fixture::new();
    let (before, _) = f.tables().await;
    f.ok(
        "run",
        json!({ "steps": [
            { "command": "table.tables.update", "params": { "tableId": 2, "title": "Trip" } },
            { "command": "table.fields.update", "on": [4], "params": { "tableId": 2, "title": "What" } },
            { "command": "table.records.update", "on": [5], "params": { "tableId": 2, "values": { "title": "Leave" } } }
        ] }),
    )
    .await;
    let Fixture { kits, workspace } = f;
    kits.shutdown().await;
    let f = Fixture { kits: Fixture::server(workspace.path()), workspace };
    let (after, _) = f.tables().await;
    let ids_of = |tables: &[Value]| -> Vec<Ids> {
        tables
            .iter()
            .map(|t| {
                let list = |key: &str| t[key].as_array().unwrap().iter().map(|item| item["id"].clone()).collect::<Vec<_>>();
                (t["id"].clone(), list("fields"), list("views"), t["records"]["rows"].as_array().unwrap().iter().map(|r| r["id"].clone()).collect())
            })
            .collect()
    };
    assert_eq!(ids_of(&after), ids_of(&before), "every table, field, view and record keeps its id");
    assert_eq!(after[1]["label"], "Trip");
    assert_eq!(f.record(2, 5).await["values"]["title"], "Leave");
    let ran = f
        .ok("run", json!({ "steps": [{ "command": "table.records.update", "on": [5], "params": { "tableId": 2, "values": { "done": true } } }] }))
        .await;
    assert_eq!(ran["completed"][0]["result"]["result"]["id"], json!(5), "{ran}");
    f.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn another_agents_edit_of_another_record_is_reported_and_my_write_applies() {
    let f = Fixture::new();
    let (_, basis) = f.tables().await;
    let theirs = f.record(2, 3).await["row_key"].as_str().unwrap().to_owned();
    f.by(
        OTHER,
        "run",
        json!({ "steps": [{ "command": "table.records.update", "on": [3], "params": { "tableId": 2, "values": { "title": "Book early" } } }] }),
    )
    .await;
    let ran = f.ok("run", json!({ "basis": basis, "steps": [{ "command": "table.records.update", "on": [4], "params": { "tableId": 2, "values": { "done": true } } }] })).await;
    assert_eq!(ids(&ran["others"]["changed"]), [theirs], "{ran}");
    assert_eq!(ran["others"]["by"], json!([OTHER]), "{ran}");
    assert_eq!(ran["completed"][0]["result"]["result"]["id"], json!(4), "{ran}");
    f.shutdown().await;
}

/// `table_inspect` with `preview` draws the named table or view through `table.file.render`,
/// whatever the person's picture shows.
#[tokio::test(flavor = "multi_thread")]
async fn a_preview_of_a_named_view_draws_that_view() {
    let f = Fixture::new();
    let seen = f.ok("inspect", json!({ "target": { "view": 3 }, "preview": true })).await;
    assert_eq!((seen["preview"]["tableId"].clone(), seen["preview"]["viewId"].clone()), (json!(2), json!(3)), "{seen}");
    let default = f.ok("inspect", json!({ "preview": true })).await;
    assert_eq!((default["preview"]["tableId"].clone(), default["preview"]["viewId"].clone()), (json!(1), json!(1)), "{default}");
    f.shutdown().await;
}
