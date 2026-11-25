//! An agent's run through the kit's headless executable saves the table's content, not its
//! history (feature `table.file.content-only`): the file the run writes holds the content
//! (`content.json`: no version, no origin) and no change row, and neither it nor any byte of the
//! file holds what the run overwrote.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::PathBuf;

use kition_kits::tools::{self, CallOptions, Guard};
use kition_kits::{KitDefinition, KitManager};
use kition_table_file::TableFile;
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::json;

const OVERWRITTEN: &str = "Hc3Wq8Lm5Zt1";

#[tokio::test]
async fn an_agent_run_saves_sealed_content() {
    let dir = tempfile::tempdir().unwrap();
    let doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    std::fs::write(dir.path().join("Work.ktable"), doc.save(None).unwrap()).unwrap();
    let definition = Box::leak(Box::new(KitDefinition::from_manifest_text(include_str!("../../../kit.json")).unwrap()));
    let executable = Some(PathBuf::from(env!("CARGO_BIN_EXE_kition-table")));
    let kits = KitManager::new(Some(dir.path().to_owned()), vec![(definition, executable)]);
    let options = CallOptions { guard: Guard::Off, ..Default::default() };
    let steps = json!([
        { "command": "record.add", "params": { "tableId": 1, "values": { "title": OVERWRITTEN } } },
        { "command": "record.set", "params": { "tableId": 1, "recordId": 1, "values": { "title": "From the agent" } } }
    ]);
    tools::call_as(&kits, "table_run", &json!({ "path": "Work.ktable", "steps": steps, "check": false }), options, "agent").await.unwrap().unwrap();
    kits.shutdown().await;
    let bytes = std::fs::read(dir.path().join("Work.ktable")).unwrap();
    assert!(!bytes.windows(OVERWRITTEN.len()).any(|window| window == OVERWRITTEN.as_bytes()), "the file holds the overwritten title");
    let file = TableFile::open_bytes(&bytes).unwrap();
    assert_eq!(file.get_record(1, 1).unwrap().values["title"], "From the agent");
    let (content, changes) = file.stored_replica().unwrap();
    assert_eq!(changes, 0, "no change row");
    assert!(!content.windows(OVERWRITTEN.len()).any(|window| window == OVERWRITTEN.as_bytes()), "the content holds the overwritten title");
    let state: serde_json::Value = serde_json::from_slice(&content).unwrap();
    for key in ["version", "lineage", "origin", "changes"] {
        assert!(state.get(key).is_none(), "the content holds no {key}");
    }
}
