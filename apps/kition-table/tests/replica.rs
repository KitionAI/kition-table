//! `kition-table` opens a document from its replica (`model.open`, which `kit.json` lists): what
//! `model.export` answers for an open table file, its changes and the attachments (blobs) they
//! name, opens again as the same document, with no file. An agent's headless draft is such a
//! replica (KHP A12); the kit listed `model.open` but refused it and exported no blobs, so its
//! drafts fell back to copies of the file, without the object difference a person reviews.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use base64::Engine as _;
use kition_headless::{Headless, HostFolder, KitInfo};
use kition_kit_server::protocol::Origin;
use kition_kit_server::{KitBackend, OpenRequest};
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::{Map, Value, json};

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// The first table's records, as an agent reads them (ids and values).
fn records(kit: &mut Headless<TableDocument>, id: &str) -> Value {
    let read = kit.execute(id, "table.read", params(json!({ "tableId": 1 })), Origin::Agent).unwrap();
    read.result.unwrap_or_default()["result"]["rows"].clone()
}

#[test]
fn a_table_opens_from_its_exported_replica_with_its_attachments() {
    let root = std::env::temp_dir().join(format!("kition-table-replica-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let mut doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
    doc.execute("table.fields.create", json!({ "tableId": 1, "name": "files", "title": "Files", "type": "attachment" })).unwrap();
    let bytes = b"a picture's bytes".to_vec();
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let attachment =
        doc.execute("table.attachments.put", json!({ "tableId": 1, "name": "note.txt", "mimeType": "text/plain", "data": data })).unwrap()["result"]
            .clone();
    doc.execute("table.records.create", json!({ "tableId": 1, "values": { "title": "With a file", "files": [attachment] } })).unwrap();
    std::fs::write(root.join("Tasks.ktable"), doc.saved_bytes().unwrap()).unwrap();

    let mut kit = Headless::<TableDocument>::with_replica_port(
        KitInfo::from_manifest(include_str!("../../../kit.json"), &[], kition_table_kit::EDITOR_STATE).unwrap(),
    );
    let folder = HostFolder::new(&root).unwrap();
    kit.set_host(folder.host());
    let file = kit.open(OpenRequest { file: &folder.file("Tasks.ktable").unwrap(), read_only: false, create: None }).unwrap().id;
    kit.execute(&file, "table.records.create", params(json!({ "tableId": 1, "values": { "title": "Second" } })), Origin::Agent).unwrap();
    let before = records(&mut kit, &file);
    assert_eq!(before.as_array().map(Vec::len), Some(2), "{before}");

    let exported = kit.model("model.export", json!({ "documentId": file })).unwrap();
    assert!(exported["blobs"].is_string(), "the attachment travels with the changes: {exported}");
    let opened = kit
        .model("model.open", json!({ "name": "Tasks.ktable", "snapshot": exported["changes"], "blobs": exported["blobs"] }))
        .unwrap_or_else(|error| panic!("model.open: {}", error.message));
    let replica = opened["documentId"].as_str().unwrap().to_owned();
    assert_ne!(replica, file);
    assert_eq!(records(&mut kit, &replica), before, "the replica is the document");
    let hash = attachment["url"].as_str().unwrap().trim_start_matches("kition-blob:").to_owned();
    let read = kit.execute(&replica, "table.attachments.read", params(json!({ "hash": hash })), Origin::Agent).unwrap();
    let read = read.result.unwrap_or_default()["result"]["data"].as_str().unwrap_or_default().to_owned();
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(read).unwrap(), bytes, "and holds the attachment's bytes");

    // The replica takes edits of its own and leaves the file's document alone.
    kit.execute(&replica, "table.records.create", params(json!({ "tableId": 1, "values": { "title": "Only in the draft" } })), Origin::Agent)
        .unwrap();
    assert_eq!(records(&mut kit, &file), before);
    assert_ne!(records(&mut kit, &replica), before);
    let saved = kit.model("model.save", json!({ "documentId": replica })).unwrap();
    assert!(saved["data"].as_str().is_some_and(|data| !data.is_empty()), "{saved}");

    // A snapshot that is no replica fails: the kit never opens an empty document instead.
    let refused =
        kit.model("model.open", json!({ "name": "Tasks.ktable", "snapshot": base64::engine::general_purpose::STANDARD.encode(b"not a replica") }));
    assert!(refused.is_err(), "{refused:?}");
    let _ = std::fs::remove_dir_all(root);
}
