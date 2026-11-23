//! TEMPORARY (content file without history): opens each conformance fixture and the agent cases'
//! `trip.ktable`, saves it as a content file (sealed, no history; the document id and the object
//! ids kept), and prints the bytes as base64 for the change that commits them. Deleted in that
//! change.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use base64::Engine;
use kition_kit::KitDocument;
use kition_table_kit::TableDocument;

#[test]
fn print_converted_fixtures() {
    let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join("conformance"))
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "ktable"))
        .collect();
    paths.sort();
    paths.push(root.join("cases/table.agent.context/two-tables-one-run/input/trip.ktable"));
    let mut lines = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let mut doc = TableDocument::open(&name, &std::fs::read(&path).unwrap()).unwrap();
        doc.prepare_save().unwrap();
        let bytes = doc.save(None).unwrap();
        let again = TableDocument::open(&name, &bytes).unwrap();
        assert_eq!(again.doc_id(), doc.doc_id(), "{name}");
        lines.push(format!("FIXTURE {name} {}", base64::engine::general_purpose::STANDARD.encode(&bytes)));
    }
    panic!("converted fixtures (temporary test):\n{}\nEND FIXTURES", lines.join("\n"));
}
