//! Counted ids only behind the test switch: `SOURCE_DATE_EPOCH`, which hosts pass to every kit for
//! timestamps, leaves ids random, so two processes never mint the same ones; only the
//! `reproducible-ids` feature (enabled here through the dev-dependencies) with
//! `KITION_REPRODUCIBLE_IDS` set counts them. Each run is a process of its own (the ids read the
//! environment once): this test starts itself.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

use kition_table_file::ids;

const OUT: &str = "KITION_TABLE_IDS_OUT";
/// 2026-11-09T00:00:00Z.
const EPOCH: &str = "1794182400";

/// The child's half: mints one id of each kind into the file `OUT` names (nothing without it).
#[test]
fn child_mints_ids() {
    if let Ok(path) = std::env::var(OUT) {
        let minted = [
            ids::document().to_string(),
            ids::object().to_string(),
            ids::record().to_string(),
            ids::uuid().to_string(),
            format!("{:?}", ids::actor()),
        ];
        std::fs::write(path, minted.join("\n")).unwrap();
    }
}

fn mint_in_a_process(path: &Path, env: &[(&str, &str)]) -> Vec<String> {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "child_mints_ids", "--test-threads=1", "--quiet"])
        .env_remove("SOURCE_DATE_EPOCH")
        .env_remove("KITION_REPRODUCIBLE_IDS")
        .env(OUT, path);
    for (key, value) in env {
        command.env(key, value);
    }
    assert!(command.status().unwrap().success(), "the child failed");
    std::fs::read_to_string(path).unwrap().lines().map(str::to_owned).collect()
}

fn dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kition-table-ids-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn source_date_epoch_alone_leaves_ids_random() {
    let dir = dir("epoch");
    let first = mint_in_a_process(&dir.join("0"), &[("SOURCE_DATE_EPOCH", EPOCH)]);
    let second = mint_in_a_process(&dir.join("1"), &[("SOURCE_DATE_EPOCH", EPOCH)]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(first.len(), 5);
    for (a, b) in first.iter().zip(&second) {
        assert_ne!(a, b, "two processes with only SOURCE_DATE_EPOCH minted the same id");
    }
}

#[test]
fn the_feature_and_the_seed_count_ids() {
    let dir = dir("seed");
    let env = [("SOURCE_DATE_EPOCH", EPOCH), ("KITION_REPRODUCIBLE_IDS", "7")];
    let first = mint_in_a_process(&dir.join("0"), &env);
    let second = mint_in_a_process(&dir.join("1"), &env);
    let other = mint_in_a_process(&dir.join("2"), &[("KITION_REPRODUCIBLE_IDS", "8")]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(first.len(), 5);
    assert_eq!(first, second, "the same seed counts the same ids");
    for (a, b) in first.iter().zip(&other) {
        assert_ne!(a, b, "another seed counts other ids");
    }
}
