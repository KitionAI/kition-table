//! Declarations are checked separately from command execution.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_table_kit::{KitDocument, TableDocument};
use serde_json::Value;

// This denotes the host's singleton document, not an object in the table replica.
const ALLOW: &[(&str, &str)] = &[("table.import.run", "target.document_id")];

fn walk(node: &Value, path: &str, visit: &mut impl FnMut(&str, &Value)) {
    visit(path, node);
    if let Some(properties) = node.get("properties").and_then(Value::as_object) {
        for (key, value) in properties {
            walk(value, &format!("{path}.{key}"), visit);
        }
    }
    if let Some(items) = node.get("items") {
        walk(items, &format!("{path}[]"), visit);
    }
    for branch in ["oneOf", "anyOf"] {
        for value in node.get(branch).and_then(Value::as_array).into_iter().flatten() {
            walk(value, path, visit);
        }
    }
}

#[test]
fn every_id_parameter_is_marked_and_typed() {
    let doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
    let commands = doc.commands();
    kition_kit::testing::check_id_params(&commands, ALLOW).unwrap();
    let mut count = 0;
    for command in &commands {
        let paths = command
            .targets
            .into_iter()
            .chain(command.objects.iter().copied())
            .chain(command.places.iter().copied().filter(|path| command.object_types.iter().any(|(declared, _)| declared == path)))
            .collect::<std::collections::BTreeSet<_>>();
        for path in paths {
            assert!(command.object_types.iter().any(|(p, types)| *p == path && !types.is_empty()), "{} {path}", command.id);
            count += 1;
        }
    }
    println!("ID declarations: {count} marked; {} allowed: target.document_id is the host singleton", ALLOW.len());
    // The SDK scanner also has to detect a declaration removed later.
    let mut missing = commands;
    missing.iter_mut().find(|c| c.id == "table.tables.update").unwrap().objects = &[];
    missing.iter_mut().find(|c| c.id == "table.tables.update").unwrap().places = &[];
    assert!(kition_kit::testing::check_id_params(&missing, ALLOW).is_err());
}

#[test]
fn name_inputs_are_creation_names_or_have_a_declaration() {
    let doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
    let mut found = Vec::new();
    for command in doc.commands() {
        let Some(properties) = command.schema.as_ref().and_then(Value::as_object) else { continue };
        for (key, node) in properties {
            walk(node, key, &mut |path, node| {
                if path.rsplit('.').next() != Some("name") || node["type"] != "string" {
                    return;
                }
                let reason = match (command.id.as_str(), path) {
                    ("table.tables.create", "name" | "fields[].name") | ("table.fields.create", "name") => "assigns a new internal name",
                    ("table.attachments.put", "name") => "assigns a blob filename",
                    _ => {
                        assert!(command.name_params.iter().any(|(p, _)| *p == path), "{} {path}", command.id);
                        "resolves an existing object"
                    }
                };
                found.push(format!("{} {path}: {reason}", command.id));
            });
        }
    }
    assert_eq!(found.len(), 4, "{found:?}");
    println!("Name inputs: {} allowed, each assigns a new name; existing field names are values-map keys (tested in places)", found.len());
}

#[test]
fn nested_references_follow_the_resolver() {
    let doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
    let commands = doc.commands();
    for (id, path, ty) in [
        ("table.ai.apply", "results[].recordId", "Record"),
        ("table.fields.create", "options.target_table_id", "Table"),
        ("table.fields.update", "options.ai_config.source_field_id", "Field"),
        ("table.views.putSorts", "items[].field_id", "Field"),
        ("table.transact", "ops[].fieldIds", "Field"),
        ("table.transact", "ops[].records[].rowKey", "Record"),
        ("table.import.run", "target.table_id", "Table"),
    ] {
        let command = commands.iter().find(|c| c.id == id).unwrap();
        assert!(command.objects.contains(&path), "{id} {path}");
        assert!(command.object_types.iter().any(|(p, types)| *p == path && *types == [ty]), "{id} {path}");
    }
}

#[test]
fn source_field_names_are_declared_as_names_not_new_labels() {
    let doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
    for id in ["table.fields.create", "table.fields.update"] {
        let command = doc.commands().into_iter().find(|c| c.id == id).unwrap();
        assert!(command.name_params.contains(&("options.ai_config.source_field_name", "name")), "{id}");
    }
}
