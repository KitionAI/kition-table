//! Create and export (KHP A41, K5): every `creates` entry of `kit.json` makes a new document that
//! saves, reopens, and checks clean, and every `exports` entry's command (`agent.export`) answers
//! a file, base64, that an independent reader reads back: the CSV through the import's parser (the field titles and every
//! record), the Markdown as a table of the same size.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::path::Path;

use base64::Engine as _;
use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn manifest() -> Value {
    serde_json::from_str(&std::fs::read_to_string(repo().join("kit.json")).unwrap()).unwrap()
}

fn basic() -> TableDocument {
    TableDocument::open("basic.ktable", &std::fs::read(repo().join("conformance/basic.ktable")).unwrap()).unwrap()
}

#[test]
fn every_creates_entry_makes_a_clean_document() {
    for entry in manifest()["creates"].as_array().unwrap() {
        let name = format!("{}.{}", entry["baseName"].as_str().unwrap(), entry["extension"].as_str().unwrap());
        let size = &entry["size"];
        let doc = TableDocument::create(&name, size["width"].as_u64().unwrap() as u32, size["height"].as_u64().unwrap() as u32).unwrap();
        let bytes = doc.saved_bytes().unwrap();
        let reopened = TableDocument::open(&name, &bytes).unwrap();
        assert!(reopened.check().is_empty(), "{name}: {:?}", reopened.check());
        assert_eq!(reopened.inspect().unwrap()["tables"].as_array().unwrap().len(), 1, "{name}");
    }
}

/// The basic fixture's tables: id, field titles, and record count.
fn tables(doc: &mut TableDocument) -> Vec<(i64, Vec<String>, usize)> {
    let document = doc.execute("table.documents.get", json!({})).unwrap()["result"].clone();
    document["tables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|table| {
            let id = table["id"].as_i64().unwrap();
            let titles = table["fields"].as_array().unwrap().iter().map(|field| field["title"].as_str().unwrap().to_owned()).collect();
            let records = doc.execute("table.records.list", json!({ "tableId": id })).unwrap()["result"]["total"].as_u64().unwrap() as usize;
            (id, titles, records)
        })
        .collect()
}

#[test]
fn every_exports_entry_writes_a_file_a_reader_reads_back() {
    let manifest = manifest();
    let exports = manifest["exports"].as_array().unwrap();
    let extensions: Vec<&str> = exports.iter().map(|entry| entry["extension"].as_str().unwrap()).collect();
    assert_eq!(extensions, ["csv", "md"]);
    let mut doc = basic();
    let bytes = doc.saved_bytes().unwrap();
    let commands: Vec<String> = doc.commands().into_iter().map(|command| command.id).collect();
    for (table, titles, records) in tables(&mut doc) {
        assert!(records > 0);
        for entry in exports {
            let command = entry["command"].as_str().unwrap();
            assert!(commands.iter().any(|id| id == command), "{command} is not a command");
            let extension = entry["extension"].as_str().unwrap();
            let answer = doc.execute(command, json!({ "format": extension, "tableId": table })).unwrap();
            assert_eq!(answer["change"], Value::Null, "{command} reads only");
            assert_eq!((answer["extension"].as_str(), answer["mime"].as_str()), (Some(extension), entry["mime"].as_str()));
            let data = answer[manifest["agent"]["export"]["data"].as_str().unwrap()].as_str().unwrap();
            let content = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(data).unwrap()).unwrap();
            match extension {
                "csv" => {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&content);
                    let read =
                        doc.execute("table.import.preview", json!({ "fileName": "export.csv", "content": encoded })).unwrap()["result"].clone();
                    let names: Vec<String> =
                        read["fields"].as_array().unwrap().iter().map(|field| field["title"].as_str().unwrap().to_owned()).collect();
                    assert_eq!(names, titles, "{read}");
                    assert_eq!(read["row_count"].as_u64().unwrap() as usize, records, "{read}");
                }
                "md" => {
                    let lines: Vec<&str> = content.lines().collect();
                    assert_eq!(lines.len(), records + 2, "{content}");
                    assert!(lines.iter().all(|line| line.starts_with('|') && line.matches(" |").count() == titles.len()), "{content}");
                    assert!(titles.iter().all(|title| lines[0].contains(title.as_str())), "{content}");
                }
                other => panic!("no reader for {other}"),
            }
        }
    }
    // Exporting writes nothing into the document.
    assert_eq!(doc.saved_bytes().unwrap(), bytes);
    assert!(!doc.dirty());
}
