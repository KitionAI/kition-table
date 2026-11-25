//! Every committed `.ktable` sample (`conformance/`, `cases/`) opens and saves to its own bytes
//! (kition-sdk data model 7.4; feature `table.file.content-only`): its `kition_kdm` row holds the
//! canonical `content.json` the kit writes, DEFLATE-compressed, so the samples are the kit's own
//! files. Opening repairs nothing, so a sample of a broken table (the `check-*` fixtures) stays
//! broken.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::path::{Path, PathBuf};

use kition_table_file::TableFile;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// The committed samples, by their path from the repository root, sorted.
fn samples() -> Vec<String> {
    let root = repo();
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = ["conformance", "cases"].iter().map(|dir| root.join(dir)).collect();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|extension| extension == "ktable") {
                found.push(path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
}

/// `(id, kdm id)` of the live tables, fields and views, and `(id, row key)` of the live records:
/// the projection's numbers and the object ids they name.
fn projection(file: &TableFile) -> Vec<(String, i64, String)> {
    let mut out = Vec::new();
    for (table, key) in [("kition_tables", "kdm_id"), ("kition_fields", "kdm_id"), ("kition_views", "kdm_id"), ("kition_records", "row_key")] {
        let mut statement = file.conn().prepare(&format!("SELECT id, IFNULL({key}, '') FROM {table} WHERE deleted_at IS NULL ORDER BY id")).unwrap();
        let rows = statement.query_map([], |row| Ok((table.to_owned(), row.get(0)?, row.get(1)?))).unwrap();
        out.extend(rows.map(Result::unwrap));
    }
    out
}

#[test]
fn every_committed_sample_opens_and_saves_to_its_own_bytes() {
    let samples = samples();
    assert!(samples.len() >= 15, "the samples: {samples:?}");
    for sample in samples {
        let bytes = std::fs::read(repo().join(&sample)).unwrap();
        let file = TableFile::open_bytes(&bytes).unwrap_or_else(|error| panic!("{sample}: {error}"));
        let (_, changes) = file.stored_replica().unwrap();
        assert_eq!(changes, 0, "{sample}: no change row");
        assert_eq!(file.content_bytes().unwrap(), bytes, "{sample} saves to its own bytes");
    }
}

/// TEMPORARY (plain model sample conversion, deleted in the next commit): each committed sample
/// with its content row holding the converted `content.json` (`tests/regen/<path with / as
/// __>.json`, written by kition-sdk's `tools/convert-content` from the row's old bytes),
/// DEFLATE-compressed, opened and saved through the kit; checks the document id, the object ids
/// and the projection's numbers, and prints the file as hex, then fails so CI shows the output.
#[test]
fn regenerate_samples() {
    use std::io::Write;
    let regen = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/regen");
    let mut printed = 0;
    for sample in samples() {
        let old = std::fs::read(repo().join(&sample)).unwrap();
        let content = std::fs::read(regen.join(format!("{}.json", sample.replace('/', "__")))).unwrap();
        let state: serde_json::Value = serde_json::from_slice(&content).unwrap();
        let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(&content).unwrap();
        let deflated = encoder.finish().unwrap();
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.deserialize_read_exact(rusqlite::MAIN_DB, &old[..], old.len(), false).unwrap();
        let before: Vec<(String, i64, String)> = {
            let mut out = Vec::new();
            for (table, key) in [("kition_tables", "kdm_id"), ("kition_fields", "kdm_id"), ("kition_views", "kdm_id"), ("kition_records", "row_key")]
            {
                let mut statement = conn.prepare(&format!("SELECT id, IFNULL({key}, '') FROM {table} WHERE deleted_at IS NULL ORDER BY id")).unwrap();
                let rows = statement.query_map([], |row| Ok((table.to_owned(), row.get(0)?, row.get(1)?))).unwrap();
                out.extend(rows.map(Result::unwrap));
            }
            out
        };
        conn.execute("UPDATE kition_kdm SET snapshot = ?1 WHERE id = 1", [&deflated]).unwrap();
        let patched = conn.serialize(rusqlite::MAIN_DB).unwrap().to_vec();
        let file = TableFile::open_bytes(&patched).unwrap_or_else(|error| panic!("{sample}: {error}"));
        let saved = file.content_bytes().unwrap();
        let reopened = TableFile::open_bytes(&saved).unwrap();
        assert_eq!(reopened.replica().doc_id().to_string(), state["docId"].as_str().unwrap(), "{sample}: the same document");
        for id in state["objects"].as_object().unwrap().keys() {
            assert!(reopened.replica().exists(&kition_kdm::ObjectId::from_string(id.clone())), "{sample}: object {id} kept");
        }
        assert_eq!(projection(&reopened), before, "{sample}: the same tables, fields, views and records");
        assert_eq!(reopened.content_bytes().unwrap(), saved, "{sample}: saves to its own bytes");
        let hex: String = saved.iter().map(|byte| format!("{byte:02x}")).collect();
        println!("REGEN {sample} {hex}");
        printed += 1;
    }
    panic!("REGEN printed {printed} samples");
}
