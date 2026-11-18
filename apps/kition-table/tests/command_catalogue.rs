//! What a host that turns the kit's commands into tools reads from `commands.list` (KHP A1, A5):
//! every command carries the kit's description and an output schema whose
//! `result` is typed, the answer nests the route's answer in `result` beside `change` as that
//! schema says, and an unknown field type is `invalid_params` naming `type` and the valid values.
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

/// The headless kit with a new document (`document.open` with `create`), as a host makes one.
fn new_document(name: &str) -> (Headless<TableDocument>, String, HostFolder) {
    let root = std::env::temp_dir().join(format!("kition-table-catalogue-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let folder = HostFolder::new(&std::fs::canonicalize(&root).unwrap()).unwrap();
    let mut kit = Headless::<TableDocument>::new(KitInfo::from_manifest(MANIFEST, &["table.import.preview", "table.import.run"], &[]).unwrap());
    kit.set_host(folder.host());
    let file = folder.file(&format!("{name}.ktable")).unwrap();
    let id = kit.open(OpenRequest { file: &file, read_only: false, create: Some(("ktable", None)) }).unwrap().id;
    (kit, id, folder)
}

/// Whether a JSON Schema node says what a value is (a type, its keys, or its branches), rather
/// than accepting anything.
fn typed(node: &Value) -> bool {
    ["type", "properties", "oneOf", "enum"].iter().any(|key| node.get(*key).is_some())
}

#[test]
fn every_listed_command_has_a_description_and_a_typed_output() {
    let (mut kit, id, _folder) = new_document("described");
    let listed = kit.commands(Some(&id)).unwrap();
    assert_eq!(listed.len(), 57);
    let mut problems = Vec::new();
    for command in &listed {
        let description = command.description.as_deref().unwrap_or_default();
        if description.len() < 30 {
            problems.push(format!("{}: description {description:?}", command.id));
        }
        let Some(output) = &command.output else {
            problems.push(format!("{}: no output schema", command.id));
            continue;
        };
        let properties = output["properties"].as_object().cloned().unwrap_or_default();
        // The engine's commands answer `{result, change}`; the export answers the file's fields.
        let answer: Vec<(String, Value)> = match properties.get("result") {
            Some(result) => vec![("result".into(), result.clone())],
            None => properties.into_iter().filter(|(key, _)| key != "error").collect(),
        };
        for (key, node) in answer {
            if !typed(&node) {
                problems.push(format!("{}: output `{key}` is untyped: {node}", command.id));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_answer_nests_the_routes_answer_in_result_as_the_output_says() {
    let (mut kit, id, _folder) = new_document("nesting");
    let listed = kit.commands(Some(&id)).unwrap();
    let output = listed.iter().find(|command| command.id == "table.tables.list").unwrap().output.clone().unwrap();
    let description = output["description"].as_str().unwrap_or_default();
    assert!(description.contains("`result`") && description.contains("`change`"), "the output says where the answer is: {description}");
    let result = &output["properties"]["result"];
    assert_eq!(result["properties"]["items"]["type"], "array");
    let executed = kit.execute(&id, "table.tables.list", params(json!({})), Origin::Agent).unwrap();
    let answer = executed.result.unwrap();
    assert_eq!(answer["change"], Value::Null, "a read changes nothing");
    assert_eq!(answer["result"]["total"], 1);
    assert_eq!(answer["result"]["items"][0]["title"], "Table", "the starter table a new document holds");
}

#[test]
fn an_unknown_field_type_is_invalid_params_naming_type_and_the_valid_values() {
    let (mut kit, id, _folder) = new_document("types");
    for (command, call, at) in [
        ("table.fields.create", json!({ "tableId": 1, "title": "Email", "type": "no-such-type" }), "`type`"),
        ("table.tables.create", json!({ "title": "Contacts", "fields": [{ "title": "Email", "type": "email" }] }), "`fields[0].type`"),
    ] {
        let error = kit.execute(&id, command, params(call), Origin::System).unwrap_err();
        assert_eq!(error.reason(), Some(ErrorReason::InvalidParams), "{command}: {error:?}");
        let detail = error.data.as_ref().and_then(|data| data["detail"].as_str()).unwrap_or_default().to_owned();
        assert!(detail.contains(at) && detail.contains("\"single_select\"") && detail.contains("\"url\""), "{command}: {detail}");
    }
    let tables = kit.execute(&id, "table.tables.list", params(json!({})), Origin::Agent).unwrap().result.unwrap();
    assert_eq!(tables["result"]["total"], 1, "nothing was made");
}

/// Command search matches words in titles (KHP A5c): every title is words a person reads
/// ("Update a table"), not the command's route name ("tables.update").
#[test]
fn every_listed_command_has_a_human_title() {
    let (mut kit, id, _folder) = new_document("titled");
    let mut problems = Vec::new();
    for command in kit.commands(Some(&id)).unwrap() {
        let title = command.title.as_str();
        let route = command.id.strip_prefix("table.").unwrap_or(&command.id);
        // A camelCase word (`linkCandidates`) is a route's; an all-capital one (CSV, AI) is a word.
        let camel = |word: &str| word.chars().any(char::is_lowercase) && word.chars().skip(1).any(char::is_uppercase);
        let words = title.chars().next().is_some_and(char::is_uppercase) && !title.contains('.') && !title.split(' ').any(camel);
        if title == route || !words {
            problems.push(format!("{}: {title:?}", command.id));
        }
    }
    assert!(problems.is_empty(), "titles that are not words: {problems:#?}");
}
