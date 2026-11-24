//! Every object id an eval case's `expected` names is in its input (no model is called): a case
//! whose input was rebuilt with new ids (fields re-created, as 853aab4, 93f535f and 42e60aa did)
//! can never pass, whatever the agent does, and the real-model run is the only other place that
//! would show it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kition_table_file::TableFile;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cases");

fn case_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            case_files(&path, out);
        } else if path.file_name().is_some_and(|name| name == "case.yaml") {
            out.push(path);
        }
    }
}

/// The object ids (UUIDs in the KDM's form) in `text`.
fn ids(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let shape = |window: &[u8]| {
        window.iter().enumerate().all(|(at, byte)| if [8, 13, 18, 23].contains(&at) { *byte == b'-' } else { byte.is_ascii_hexdigit() })
    };
    let edge = |at: Option<&u8>| at.is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'-');
    let mut found = BTreeSet::new();
    let mut at = 0;
    while at + 36 <= bytes.len() {
        if shape(&bytes[at..at + 36]) && edge(at.checked_sub(1).and_then(|before| bytes.get(before))) && edge(bytes.get(at + 36)) {
            found.insert(text[at..at + 36].to_ascii_lowercase());
            at += 36;
        } else {
            at += 1;
        }
    }
    found
}

/// The lines of the top-level key `key` of a case file (its value, inline or as a block).
fn section<'a>(yaml: &'a str, key: &str) -> Vec<&'a str> {
    let mut lines = yaml.lines().skip_while(|line| !line.starts_with(&format!("{key}:")));
    let Some(first) = lines.next() else { return Vec::new() };
    let mut out = vec![first.trim_start_matches(&format!("{key}:"))];
    out.extend(lines.take_while(|line| line.is_empty() || line.starts_with(' ') || line.starts_with('-')));
    out
}

/// The case's input paths: `input: [a, b]` or a block list.
fn inputs(yaml: &str) -> Vec<String> {
    section(yaml, "input")
        .iter()
        .flat_map(|line| {
            line.trim().trim_start_matches('-').trim().trim_start_matches('[').trim_end_matches(']').split(',').map(str::to_owned).collect::<Vec<_>>()
        })
        .map(|item| item.trim().trim_matches(['"', '\'']).to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

#[test]
fn every_id_an_eval_cases_expected_names_is_in_its_input() {
    let mut files = Vec::new();
    case_files(Path::new(CASES), &mut files);
    files.sort();
    let mut checked = 0;
    let mut missing = Vec::new();
    for path in files {
        let yaml = std::fs::read_to_string(&path).unwrap();
        if !yaml.lines().any(|line| line.trim() == "layer: eval") {
            continue;
        }
        let named = ids(&section(&yaml, "expected").join("\n"));
        if named.is_empty() {
            continue;
        }
        let dir = path.parent().unwrap();
        let mut held = BTreeSet::new();
        for input in inputs(&yaml).iter().filter(|input| input.ends_with(".ktable")) {
            let file =
                TableFile::open_bytes(&std::fs::read(dir.join(input)).unwrap()).unwrap_or_else(|err| panic!("{}: {input}: {err}", path.display()));
            held.extend(ids(&file.replica().state_json().unwrap().to_string()));
        }
        checked += 1;
        for id in named.difference(&held) {
            missing.push(format!("{}: {id}", path.strip_prefix(CASES).unwrap_or(&path).display()));
        }
    }
    assert!(checked > 0, "no eval case names an id");
    assert!(missing.is_empty(), "ids an eval case's expected names that its input does not hold:\n{}", missing.join("\n"));
}

#[test]
fn reads_ids_sections_and_inputs() {
    let yaml = "id: x\ninput: [input/a.ktable, input/b.csv]\nexpected:\n  - value: { fields.cells.01A169AA-ff23-7691-b372-5f1eec0dcd5e: 1 }\n  - x-01a169aa-ff23-7691-b372-5f1eec0dcd5ef\nsolution: 01a169aa-ff23-7691-b372-5f20579c9f54\n";
    assert_eq!(inputs(yaml), ["input/a.ktable", "input/b.csv"]);
    assert_eq!(ids(&section(yaml, "expected").join("\n")).into_iter().collect::<Vec<_>>(), ["01a169aa-ff23-7691-b372-5f1eec0dcd5e"]);
    assert_eq!(inputs("input:\n  - input/a.ktable\n  - input/c.ktable\nsteps: []\n"), ["input/a.ktable", "input/c.ktable"]);
}
