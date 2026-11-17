//! A record `values` key that names no field of the table is refused, not dropped:
//! `table.records.create`, `table.records.update` and `table.transact` (its `cells.set` and
//! `records.create` ops) answer `invalid_params` with one `data.errors` entry per unknown key, at
//! its JSON Pointer, and the call changes nothing. A key that is a field's title, or its name in
//! another case or with spaces for underscores, is told the field's name. The engine refuses it,
//! so the in-page editor, which runs the same `execute`, gets the same refusal.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use kition_headless::{Headless, HostFolder, KitInfo};
use kition_kit_server::protocol::Origin;
use kition_kit_server::{ErrorReason, KitBackend, OpenRequest};
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::{Map, Value, json};

const MANIFEST: &str = include_str!("../../../kit.json");

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// The headless kit with a new document holding the issue's `Contacts` table (table 2: the
/// starter table is 1), its fields titled `Primary channel` and `Email`.
fn contacts(name: &str) -> (Headless<TableDocument>, String, HostFolder) {
    let root = std::env::temp_dir().join(format!("kition-table-values-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let folder = HostFolder::new(&std::fs::canonicalize(&root).unwrap()).unwrap();
    let mut kit = Headless::<TableDocument>::new(KitInfo::from_manifest(MANIFEST, &[], &[]).unwrap());
    kit.set_host(folder.host());
    let file = folder.file(&format!("{name}.ktable")).unwrap();
    let id = kit.open(OpenRequest { file: &file, read_only: false, create: Some(("ktable", None)) }).unwrap().id;
    let fields = json!([{ "title": "Primary channel", "type": "text", "primary": true }, { "title": "Email", "type": "text" }]);
    let table = run(&mut kit, &id, "table.tables.create", json!({ "title": "Contacts", "fields": fields }));
    assert_eq!(table["result"]["id"], 2);
    let names: Vec<&str> = table["result"]["fields"].as_array().unwrap().iter().map(|field| field["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["primary_channel", "email"], "the names derived from the titles");
    (kit, id, folder)
}

fn run(kit: &mut Headless<TableDocument>, id: &str, command: &str, call: Value) -> Value {
    kit.execute(id, command, params(call), Origin::Agent).unwrap().result.unwrap()
}

/// The `invalid_params` answer's `detail` and `data.errors` as `(path, message)` pairs.
fn refused(kit: &mut Headless<TableDocument>, id: &str, command: &str, call: Value) -> (String, Vec<(String, String)>) {
    let error = match kit.execute(id, command, params(call), Origin::Agent) {
        Ok(answer) => panic!("{command} ran: {:?}", answer.result),
        Err(error) => error,
    };
    assert_eq!(error.reason(), Some(ErrorReason::InvalidParams), "{command}: {error:?}");
    let data = error.data.clone().unwrap();
    let errors = data["errors"]
        .as_array()
        .unwrap_or_else(|| panic!("{command}: no data.errors: {data}"))
        .iter()
        .map(|entry| {
            assert!(entry["expected"].as_str().is_some_and(|expected| expected.contains("`primary_channel`")), "{entry}");
            (entry["path"].as_str().unwrap().to_owned(), entry["message"].as_str().unwrap().to_owned())
        })
        .collect();
    (data["detail"].as_str().unwrap_or_default().to_owned(), errors)
}

fn records(kit: &mut Headless<TableDocument>, id: &str) -> Vec<Value> {
    run(kit, id, "table.records.list", json!({ "tableId": 2 }))["result"]["items"].as_array().unwrap().clone()
}

#[test]
fn records_create_refuses_a_key_that_names_no_field_and_writes_nothing() {
    let (mut kit, id, _folder) = contacts("create");
    let (detail, errors) = refused(
        &mut kit,
        &id,
        "table.records.create",
        json!({ "tableId": 2, "values": { "Primary channel": "Email", "a/b~c": 1, "email": "ada@example.com" } }),
    );
    let title = "`values.Primary channel` names no field of table 2; the field titled \"Primary channel\" is named `primary_channel`";
    let unknown = "`values.a/b~c` names no field of table 2";
    assert_eq!(errors, [("/values/Primary channel".to_owned(), title.to_owned()), ("/values/a~1b~0c".to_owned(), unknown.to_owned())]);
    assert_eq!(detail, format!("{title}; {unknown}"));
    assert!(records(&mut kit, &id).is_empty(), "the call changed nothing");

    // A name in another case, or with spaces for underscores, is told the name too.
    for key in ["PRIMARY_CHANNEL", "primary channel"] {
        let (_, errors) = refused(&mut kit, &id, "table.records.create", json!({ "tableId": 2, "values": { key: "Email" } }));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].1.ends_with("is named `primary_channel`"), "{key}: {}", errors[0].1);
    }

    // Keyed by name, the same record is written.
    let created = run(&mut kit, &id, "table.records.create", json!({ "tableId": 2, "values": { "primary_channel": "Email" } }));
    assert_eq!(created["result"]["values"]["primary_channel"], "Email");
}

#[test]
fn records_update_refuses_a_key_that_names_no_field_and_writes_nothing() {
    let (mut kit, id, _folder) = contacts("update");
    let created =
        run(&mut kit, &id, "table.records.create", json!({ "tableId": 2, "values": { "primary_channel": "Email", "email": "ada@example.com" } }));
    let record_id = created["result"]["id"].clone();
    let (_, errors) = refused(
        &mut kit,
        &id,
        "table.records.update",
        json!({ "tableId": 2, "recordId": record_id, "values": { "no_such_field": "x", "email": "bob@example.com" } }),
    );
    assert_eq!(errors, [("/values/no_such_field".to_owned(), "`values.no_such_field` names no field of table 2".to_owned())]);
    assert_eq!(records(&mut kit, &id)[0]["values"]["email"], "ada@example.com", "the known key was not written either");
}

#[test]
fn transact_refuses_a_key_that_names_no_field_in_any_op_and_writes_nothing() {
    let (mut kit, id, _folder) = contacts("transact");
    let created = run(&mut kit, &id, "table.records.create", json!({ "tableId": 2, "values": { "email": "ada@example.com" } }));
    let row_key = created["result"]["row_key"].clone();
    let ops = json!([
        { "op": "cells.set", "tableId": 2, "records": [{ "rowKey": row_key, "values": { "email": "bob@example.com", "Email": "bob@example.com" } }] },
        { "op": "records.create", "tableId": 2, "records": [{ "primary_channel": "Phone" }, { "Bogus": 1 }] },
    ]);
    let (_, errors) = refused(&mut kit, &id, "table.transact", json!({ "ops": ops }));
    assert_eq!(
        errors,
        [
            (
                "/ops/0/records/0/values/Email".to_owned(),
                "`ops[0].records[0].values.Email` names no field of table 2; the field titled \"Email\" is named `email`".to_owned()
            ),
            ("/ops/1/records/1/Bogus".to_owned(), "`ops[1].records[1].Bogus` names no field of table 2".to_owned()),
        ]
    );
    let after = records(&mut kit, &id);
    assert_eq!(after.len(), 1, "no record was created");
    assert_eq!(after[0]["values"]["email"], "ada@example.com", "no cell was set");
}

#[test]
fn the_in_page_engine_refuses_with_the_same_errors() {
    let mut doc = TableDocument::create("Table.ktable", 0, 0).unwrap();
    let fields = json!([{ "title": "Primary channel", "type": "text", "primary": true }]);
    doc.execute("table.tables.create", json!({ "title": "Contacts", "fields": fields })).unwrap();
    let revision = doc.revision();
    let error = doc.execute("table.records.create", json!({ "tableId": 2, "values": { "Primary channel": "Email" } })).unwrap_err();
    // `kition_kit::schema::refusal`: the message, the record separator, the errors as JSON.
    let (message, errors) = error.split_once('\u{1e}').unwrap_or_else(|| panic!("not a refusal: {error:?}"));
    let title = "`values.Primary channel` names no field of table 2; the field titled \"Primary channel\" is named `primary_channel`";
    assert_eq!(message, title, "what the editor shows");
    let errors: Value = serde_json::from_str(errors).unwrap();
    assert_eq!(errors.as_array().map(Vec::len), Some(1));
    assert_eq!(errors[0]["path"], "/values/Primary channel");
    assert_eq!(doc.revision(), revision, "the document did not move");
}
