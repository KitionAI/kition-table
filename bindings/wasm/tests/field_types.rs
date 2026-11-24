//! A host builds a table's schema from the published material alone: the field
//! and view types are enums of the engine's identifiers in the parameter schemas, each with a
//! one-line description, beside the shape of the options the types read; an unknown type is
//! refused naming `type` and the valid values; and the starter table a new document holds is
//! documented, and a host that builds its own schema removes it in one step outside the
//! person's undo.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use kition_kit::KitDocument;
use kition_table_file::{FIELD_TYPES, VIEW_TYPES};
use kition_table_kit::TableDocument;
use kition_table_kit::agent::input_schema;
use serde_json::{Map, Value, json};

const GUIDE: &str = include_str!("../../../agent/guide.md");
const MANIFEST: &str = include_str!("../../../kit.json");

fn schema(command: &str) -> Value {
    input_schema(command).unwrap_or_else(|| panic!("{command} has no schema"))
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// The `type` node of each command that takes a field type, and of each that takes a view type.
fn type_nodes() -> Vec<(&'static str, Value, &'static [&'static str])> {
    vec![
        ("table.tables.create fields[].type", schema("table.tables.create")["fields"]["items"]["properties"]["type"].clone(), &FIELD_TYPES),
        ("table.fields.create type", schema("table.fields.create")["type"].clone(), &FIELD_TYPES),
        ("table.fields.update type", schema("table.fields.update")["type"].clone(), &FIELD_TYPES),
        ("table.tables.create views[].type", schema("table.tables.create")["views"]["items"]["properties"]["type"].clone(), &VIEW_TYPES),
        ("table.views.create type", schema("table.views.create")["type"].clone(), &VIEW_TYPES),
        ("table.views.update type", schema("table.views.update")["type"].clone(), &VIEW_TYPES),
    ]
}

#[test]
fn field_and_view_types_are_enums_of_the_engine_identifiers_each_described() {
    for (at, node, identifiers) in type_nodes() {
        assert_eq!(node["type"], "string", "{at}: {node}");
        assert_eq!(node["enum"], json!(identifiers), "{at}: the engine's identifiers, in its order");
        let description = node["description"].as_str().unwrap_or_else(|| panic!("{at} has no description"));
        for id in identifiers {
            let line = description.lines().find(|line| line.trim_start().starts_with(&format!("{id}:")));
            assert!(line.is_some_and(|line| line.len() > id.len() + 8), "{at}: no line describes {id}:\n{description}");
        }
        kition_kit::schema::validate(&json!({ "type": node })).unwrap_or_else(|error| panic!("{at}: {error}"));
    }
}

#[test]
fn the_options_a_type_reads_are_in_the_schema() {
    for (at, options) in [
        ("table.tables.create fields[].options", schema("table.tables.create")["fields"]["items"]["properties"]["options"].clone()),
        ("table.fields.create options", schema("table.fields.create")["options"].clone()),
        ("table.fields.update options", schema("table.fields.update")["options"].clone()),
    ] {
        assert_eq!(options["type"], "object", "{at}: {options}");
        assert_eq!(options["additionalProperties"], true, "{at}: keys the editor keeps (column_tone, …) stay allowed");
        let properties = &options["properties"];
        assert_eq!(properties["choices"]["type"], "array", "{at}: select choices");
        assert_eq!(properties["choices"]["items"]["type"], "string", "{at}");
        // The three ids take the local integer or the stable id a receipt shows (one id space).
        for key in ["target_table_id", "link_field_id", "target_field_id"] {
            assert_eq!(properties[key]["oneOf"], json!([{ "type": "integer" }, { "type": "string" }]), "{at}: {key}");
        }
        assert_eq!(properties["max"]["type"], "integer", "{at}: max");
        assert_eq!(properties["aggregation"]["enum"], json!(["count", "sum", "avg", "max", "min", "and", "or", "concat"]), "{at}");
        for (key, node) in properties.as_object().unwrap() {
            let description = node["description"].as_str().unwrap_or_default();
            let names_a_type = FIELD_TYPES.iter().any(|id| description.starts_with(id) || description.contains(&format!(", {id}")));
            assert!(names_a_type, "{at}: {key} says which types read it: {description}");
        }
    }
}

#[test]
fn an_unknown_field_type_is_refused_naming_type_and_the_valid_ones() {
    // The schema check the headless host runs before the engine (KHP A5, A18).
    let refused =
        kition_kit::schema::check_params(&schema("table.fields.create"), &object(json!({ "tableId": 1, "title": "Email", "type": "no-such-type" })))
            .unwrap_err();
    assert!(refused.contains("`type`") && refused.contains("\"single_select\"") && refused.contains("\"url\""), "{refused}");
    let nested = kition_kit::schema::check_params(
        &schema("table.tables.create"),
        &object(json!({ "title": "Contacts", "fields": [{ "title": "Name", "type": "text" }, { "title": "Email", "type": "email" }] })),
    )
    .unwrap_err();
    assert!(nested.contains("`fields[1].type`") && nested.contains("\"long_text\""), "{nested}");
    let view = kition_kit::schema::check_params(&schema("table.views.create"), &object(json!({ "tableId": 1, "type": "form" }))).unwrap_err();
    assert!(view.contains("`type`") && view.contains("\"kanban\""), "{view}");

    // The engine itself, as the in-page editor reaches it: the same refusal, the file unchanged.
    let mut doc = TableDocument::create("Contacts.ktable", 0, 0).unwrap();
    let before = doc.sealed_save().unwrap();
    for (command, params) in [
        ("table.fields.create", json!({ "tableId": 1, "title": "Email", "type": "no-such-type" })),
        ("table.tables.create", json!({ "title": "Contacts", "fields": [{ "title": "Email", "type": "no-such-type" }] })),
        ("table.fields.update", json!({ "tableId": 1, "fieldId": 3, "type": "no-such-type" })),
    ] {
        let message = doc.execute(command, params).unwrap_err();
        assert!(
            message.contains("no-such-type") && message.contains("`type`") && FIELD_TYPES.iter().all(|id| message.contains(id)),
            "{command}: {message}"
        );
        assert_eq!(doc.sealed_save().unwrap(), before, "{command} failed and changed the file");
    }
}

#[test]
fn a_new_document_holds_the_documented_starter_table_and_a_host_removes_it() {
    let mut doc = TableDocument::create("contacts.ktable", 0, 0).unwrap();
    let tables = doc.execute("table.tables.list", json!({})).unwrap()["result"].clone();
    assert_eq!(tables["total"], 1);
    let starter = &tables["items"][0];
    assert_eq!((starter["id"].as_i64(), starter["title"].as_str(), starter["name"].as_str()), (Some(1), Some("Table"), Some("table")));
    let fields: Vec<(&str, &str)> =
        starter["fields"].as_array().unwrap().iter().map(|field| (field["name"].as_str().unwrap(), field["type"].as_str().unwrap())).collect();
    assert_eq!(fields, [("title", "text"), ("status", "single_select"), ("notes", "long_text")]);
    let views: Vec<(&str, &str)> =
        starter["views"].as_array().unwrap().iter().map(|view| (view["title"].as_str().unwrap(), view["type"].as_str().unwrap())).collect();
    assert_eq!(views, [("All", "grid")]);
    for documented in ["starter table", "`title` (text, primary)", "`status` (single_select", "`notes` (long_text)"] {
        assert!(GUIDE.contains(documented), "agent/guide.md describes the starter table: {documented}");
    }
    assert!(
        GUIDE.contains(r#"{"command": "table.tables.delete", "params": {"tableId": 1, "asOrigin": "system:"#),
        "agent/guide.md shows a host removing it"
    );
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert!(manifest["agent"]["describe"]["covers"].as_str().unwrap().contains("starter table"), "kit.json names the starter table");

    // A host that builds its own schema removes it as a system write: not a step the person undoes.
    doc.execute("table.tables.delete", json!({ "tableId": 1, "asOrigin": "system:setup" })).unwrap();
    let inspected = doc.inspect().unwrap();
    assert_eq!(inspected["tables"], json!([]));
    assert_eq!(inspected["canUndo"], false, "the person's Undo does not bring the starter back");
    let created = doc
        .execute(
            "table.tables.create",
            json!({ "title": "Contacts", "fields": [{ "title": "Name", "type": "text", "primary": true }, { "title": "Email", "type": "url" }] }),
        )
        .unwrap();
    assert_eq!(created["result"]["id"], 2, "ids are never reused");
    let reopened = TableDocument::open("contacts.ktable", &doc.sealed_save().unwrap()).unwrap();
    let titles: Vec<Value> = reopened.inspect().unwrap()["tables"].as_array().unwrap().iter().map(|table| table["title"].clone()).collect();
    assert_eq!(titles, [json!("Contacts")]);
    assert!(reopened.check().is_empty(), "{:?}", reopened.check());

    // With no table at all, the document is still valid and saves.
    let mut empty = TableDocument::create("blank.ktable", 0, 0).unwrap();
    empty.execute("table.tables.delete", json!({ "tableId": 1, "asOrigin": "system:setup" })).unwrap();
    let reopened = TableDocument::open("blank.ktable", &empty.sealed_save().unwrap()).unwrap();
    assert_eq!(reopened.inspect().unwrap()["tables"], json!([]));
    assert!(reopened.check().is_empty());
}
