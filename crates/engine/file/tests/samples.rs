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
