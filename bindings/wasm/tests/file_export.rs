//! `file.export`: the editor's Export (CSV, Markdown) and the agent's `table_export` take one
//! table's file from the engine as base64 and hand it to the host, which writes the new file (Kit
//! UI protocol `host/write` mode `export`, KHP A41). The engine writes nothing, the document stays
//! as it was, and `file.exportCsv` and `file.exportMarkdown` still answer their text.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use base64::Engine as _;
use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

/// A table "People" with two records, and a second table.
fn people() -> (TableDocument, i64, i64) {
    let mut doc = TableDocument::create("Team.ktable", 640, 420).unwrap();
    let first = doc.execute("file.get", json!({})).unwrap()["result"]["tables"][0].clone();
    let table = first["id"].as_i64().unwrap();
    let primary = first["fields"][0]["name"].as_str().unwrap().to_owned();
    doc.execute("table.set", json!({ "tableId": table, "title": "People" })).unwrap();
    for name in ["Ada, Countess", "Grace | Hopper"] {
        doc.execute("record.add", json!({ "tableId": table, "values": { primary.clone(): name } })).unwrap();
    }
    let second = doc.execute("table.add", json!({ "title": "Second" })).unwrap()["result"]["id"].as_i64().unwrap();
    doc.mark_saved();
    (doc, table, second)
}

fn text(answer: &Value) -> String {
    String::from_utf8(base64::engine::general_purpose::STANDARD.decode(answer["data"].as_str().unwrap()).unwrap()).unwrap()
}

#[test]
fn answers_a_table_as_csv_bytes_without_changing_the_document() {
    let (mut doc, table, _) = people();
    let (revision, saved) = (doc.revision(), doc.saved_bytes().unwrap());
    let answer = doc.execute("file.export", json!({ "format": "csv", "tableId": table })).unwrap();
    assert_eq!((answer["format"].as_str(), answer["extension"].as_str(), answer["mime"].as_str()), (Some("csv"), Some("csv"), Some("text/csv")));
    let csv = text(&answer);
    assert!(csv.contains("\"Ada, Countess\"") && csv.contains("Grace | Hopper"), "{csv}");
    assert_eq!(answer["bytes"], csv.len());
    assert_eq!(answer["tableId"], table);
    assert!(answer.get("path").is_none() && answer.get("result").is_none(), "the file is at the top level and the host names it: {answer}");
    // The same text the agent's `file.exportCsv` answers.
    let old = doc.execute("file.exportCsv", json!({ "tableId": table })).unwrap();
    assert_eq!(old["result"]["content"].as_str(), Some(csv.as_str()));
    assert_eq!((doc.revision(), doc.dirty()), (revision, false));
    assert_eq!(doc.saved_bytes().unwrap(), saved);
}

#[test]
fn answers_a_table_as_markdown_and_the_first_table_by_default() {
    let (mut doc, table, second) = people();
    let answer = doc.execute("file.export", json!({ "format": "md" })).unwrap();
    assert_eq!((answer["extension"].as_str(), answer["mime"].as_str()), (Some("md"), Some("text/markdown")));
    assert_eq!(answer["tableId"], table, "the first table");
    let markdown = text(&answer);
    assert!(markdown.starts_with('|') && markdown.contains("Grace \\| Hopper"), "{markdown}");
    let old = doc.execute("file.exportMarkdown", json!({ "tableId": table })).unwrap();
    assert_eq!(old["result"]["content"].as_str(), Some(markdown.as_str()));
    let other = doc.execute("file.export", json!({ "format": "csv", "tableId": second })).unwrap();
    assert_eq!((other["tableId"].as_i64(), other["name"].as_str()), (Some(second), Some("Second")));
    assert!(!doc.dirty());
}

#[test]
fn refuses_a_format_or_table_it_cannot_write() {
    let (mut doc, _, _) = people();
    let saved = doc.saved_bytes().unwrap();
    let error = doc.execute("file.export", json!({ "format": "xlsx" })).unwrap_err();
    assert!(error.contains("csv or md"), "{error}");
    assert!(doc.execute("file.export", json!({})).is_err(), "a format is required");
    assert!(doc.execute("file.export", json!({ "format": "csv", "tableId": 999 })).is_err());
    assert!(doc.execute("file.export", json!({ "format": "csv", "path": "/tmp/x.csv" })).is_err(), "no file system");
    assert_eq!(doc.saved_bytes().unwrap(), saved);
}

/// kit.json declares what `file.export` writes, for the host's export check and the agent's
/// `table_export` (`exports`, `agent.export`), and the command is in the catalogue as a read.
#[test]
fn the_manifest_declares_the_exports_and_the_agent_export() {
    let manifest: Value = serde_json::from_str(include_str!("../../../kit.json")).unwrap();
    let exports: Vec<(&str, &str, &str)> = manifest["exports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| (entry["extension"].as_str().unwrap(), entry["mime"].as_str().unwrap(), entry["command"].as_str().unwrap()))
        .collect();
    assert_eq!(exports, [("csv", "text/csv", "file.export"), ("md", "text/markdown", "file.export")]);
    assert_eq!(manifest["agent"]["export"], json!({ "command": "file.export", "data": "data" }));
    let (mut doc, _, _) = people();
    let command = doc.commands().into_iter().find(|command| command.id == "file.export").unwrap();
    assert!(!command.mutates);
    for (extension, mime, id) in exports {
        let answer = doc.execute(id, json!({ "format": extension })).unwrap();
        assert_eq!((answer["extension"].as_str(), answer["mime"].as_str()), (Some(extension), Some(mime)));
        assert!(!text(&answer).is_empty());
    }
}
