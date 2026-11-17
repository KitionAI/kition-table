//! An explicit table or field `name` is kept as given, or refused: `table.tables.create`
//! (`name`, `fields[i].name`) and `table.fields.create` (`name`) answer `invalid_params` with one
//! `data.errors` entry per name the engine would have rewritten, one not in internal-name form or
//! already taken, and the call changes nothing. Names derived from titles are as before.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use kition_headless::{Headless, HostFolder, KitInfo};
use kition_kit_server::protocol::Origin;
use kition_kit_server::{ErrorReason, KitBackend, OpenRequest};
use kition_table_kit::TableDocument;
use serde_json::{Map, Value, json};

const MANIFEST: &str = include_str!("../../../kit.json");

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn document(name: &str) -> (Headless<TableDocument>, String, HostFolder) {
    let root = std::env::temp_dir().join(format!("kition-table-names-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let folder = HostFolder::new(&std::fs::canonicalize(&root).unwrap()).unwrap();
    let mut kit = Headless::<TableDocument>::new(KitInfo::from_manifest(MANIFEST, &[], &[]).unwrap());
    kit.set_host(folder.host());
    let file = folder.file(&format!("{name}.ktable")).unwrap();
    let id = kit.open(OpenRequest { file: &file, read_only: false, create: Some(("ktable", None)) }).unwrap().id;
    (kit, id, folder)
}

fn run(kit: &mut Headless<TableDocument>, id: &str, command: &str, call: Value) -> Value {
    kit.execute(id, command, params(call), Origin::Agent).unwrap().result.unwrap()
}

/// The `invalid_params` answer's `data.errors` as `(path, message)` pairs.
fn refused(kit: &mut Headless<TableDocument>, id: &str, command: &str, call: Value) -> Vec<(String, String)> {
    let error = match kit.execute(id, command, params(call), Origin::Agent) {
        Ok(answer) => panic!("{command} ran: {:?}", answer.result),
        Err(error) => error,
    };
    assert_eq!(error.reason(), Some(ErrorReason::InvalidParams), "{command}: {error:?}");
    let data = error.data.unwrap();
    data["errors"]
        .as_array()
        .unwrap_or_else(|| panic!("no data.errors: {data}"))
        .iter()
        .map(|entry| {
            assert!(entry["expected"].as_str().unwrap().contains("^[a-z](?:[a-z0-9_]*[a-z0-9])?$"), "{entry}");
            (entry["path"].as_str().unwrap().to_owned(), entry["message"].as_str().unwrap().to_owned())
        })
        .collect()
}

fn table_count(kit: &mut Headless<TableDocument>, id: &str) -> usize {
    let answer = run(kit, id, "table.tables.list", json!({}));
    answer["result"]["items"].as_array().map(Vec::len).unwrap_or_else(|| panic!("tables.list: {answer}"))
}

#[test]
fn tables_create_refuses_names_it_would_rewrite_and_writes_nothing() {
    let (mut kit, id, _folder) = document("create");
    let before = table_count(&mut kit, &id);
    // The issue's request: two explicit names the engine rewrote to `x` and `name_2`.
    let fields = json!([
        { "title": "Name", "name": "name", "type": "text", "primary": true },
        { "title": "电话" },
        { "title": "Ünïcode Field-2" },
        { "title": "Phone", "name": "电话x" },
        { "title": "Dup", "name": "name" },
    ]);
    let errors = refused(&mut kit, &id, "table.tables.create", json!({ "name": "c", "fields": fields }));
    assert_eq!(
        errors,
        [
            ("/fields/3/name".to_owned(), "`fields[3].name` \"电话x\" is not an internal name; as one it would be `x`".to_owned()),
            ("/fields/4/name".to_owned(), "`fields[4].name` \"name\" is taken by another field of this request".to_owned()),
        ]
    );
    assert_eq!(table_count(&mut kit, &id), before, "nothing was written");

    // An explicit name a derived one took first is refused too: the host names both.
    let clash = json!([{ "title": "Name" }, { "title": "Full name", "name": "name" }]);
    assert_eq!(refused(&mut kit, &id, "table.tables.create", json!({ "title": "People", "fields": clash }))[0].0, "/fields/1/name");
    // The table's own name: in form, and not another table's.
    assert_eq!(refused(&mut kit, &id, "table.tables.create", json!({ "name": "Contacts" }))[0].0, "/name");
    run(&mut kit, &id, "table.tables.create", json!({ "title": "Contacts", "name": "contacts" }));
    assert_eq!(
        refused(&mut kit, &id, "table.tables.create", json!({ "title": "Contacts", "name": "contacts" }))[0].1,
        "`name` \"contacts\" is taken by a table of this document"
    );

    // Explicit names in form are kept; names derived from titles are as before.
    let fields = json!([
        { "title": "Name", "name": "name", "type": "text", "primary": true },
        { "title": "电话" },
        { "title": "Ünïcode Field-2" },
        { "title": "Phone", "name": "phone_number" },
        { "title": "Name" },
    ]);
    let table = run(&mut kit, &id, "table.tables.create", json!({ "title": "People", "fields": fields }));
    let names: Vec<&str> = table["result"]["fields"].as_array().unwrap().iter().map(|field| field["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["name", "field", "n_code_field_2", "phone_number", "name_2"]);
}

#[test]
fn fields_create_refuses_a_name_in_use_or_out_of_form() {
    let (mut kit, id, _folder) = document("field");
    let table =
        run(&mut kit, &id, "table.tables.create", json!({ "title": "Contacts", "fields": [{ "title": "Email", "name": "email", "primary": true }] }));
    let table_id = table["result"]["id"].clone();
    let fields = |kit: &mut Headless<TableDocument>| {
        let answer = run(kit, &id, "table.fields.list", json!({ "tableId": table_id }));
        answer["result"]["items"].as_array().or_else(|| answer["result"].as_array()).map(Vec::len).unwrap_or_else(|| panic!("fields.list: {answer}"))
    };
    let before = fields(&mut kit);
    assert_eq!(
        refused(&mut kit, &id, "table.fields.create", json!({ "tableId": table_id, "title": "Mail", "name": "email" })),
        [("/name".to_owned(), "`name` \"email\" is taken by a field of this table".to_owned())]
    );
    assert_eq!(refused(&mut kit, &id, "table.fields.create", json!({ "tableId": table_id, "title": "Phone", "name": "Phone" }))[0].0, "/name");
    assert_eq!(fields(&mut kit), before, "nothing was written");
    let created = run(&mut kit, &id, "table.fields.create", json!({ "tableId": table_id, "title": "Phone", "name": "phone" }));
    assert_eq!(created["result"]["name"], "phone");
}
