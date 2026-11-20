//! The agent's `table_export` (KHP A41, kit.json `agent.export`) on the headless kit: Kition runs
//! the manifest's export command as the agent on the document it opened, the command answers the
//! file as base64 in the field the manifest names, and Kition writes the new file. The kit writes
//! no file of its own.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::Path;

use base64::Engine as _;
use kition_headless::{Headless, HostFolder, KitInfo};
use kition_kit_server::protocol::Origin;
use kition_kit_server::{KitBackend, OpenRequest};
use kition_table_kit::TableDocument;
use serde_json::{Map, Value, json};

const MANIFEST: &str = include_str!("../../../kit.json");

fn params(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

#[test]
fn the_agent_exports_every_declared_format_and_the_kit_writes_no_file() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    let export = &manifest["agent"]["export"];
    let (command, data) = (export["command"].as_str().unwrap(), export["data"].as_str().unwrap());
    let root = std::env::temp_dir().join(format!("kition-table-agent-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    std::fs::copy(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../conformance/basic.ktable")), root.join("basic.ktable")).unwrap();
    let mut kit = Headless::<TableDocument>::new(KitInfo::from_manifest(MANIFEST, &[], kition_table_kit::EDITOR_STATE).unwrap());
    let folder = HostFolder::new(&root).unwrap();
    kit.set_host(folder.host());
    let id = kit.open(OpenRequest { file: &folder.file("basic.ktable").unwrap(), read_only: false, create: None }).unwrap().id;
    let heads = [("csv", ""), ("md", "|")];
    let declared: Vec<&str> = manifest["exports"].as_array().unwrap().iter().map(|entry| entry["extension"].as_str().unwrap()).collect();
    assert_eq!(declared, heads.iter().map(|(extension, _)| *extension).collect::<Vec<_>>());
    for (extension, head) in heads {
        let answer = kit.execute(&id, command, params(json!({ "format": extension })), Origin::Agent).unwrap().result.unwrap();
        let bytes = base64::engine::general_purpose::STANDARD.decode(answer[data].as_str().unwrap()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.is_empty() && text.starts_with(head), "{extension}: {text}");
    }
    let target = root.join("out.csv");
    assert!(kit.execute(&id, command, params(json!({ "format": "csv", "path": target.to_string_lossy() })), Origin::Agent).is_err());
    assert!(!target.exists());
    let before: Vec<_> = std::fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(before.len(), 1, "only the document: {before:?}");
    let _ = std::fs::remove_dir_all(root);
}
