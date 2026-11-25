//! `table_check` against its catalogue (`agent/checks.json`, KHP A38): every fixture in
//! `conformance/` reports its code with a target and a fix, every code reported is catalogued,
//! errors come first, `basic.ktable` and a new document are clean, and each code's fix (with the
//! value the message asks for, where the author chooses it) clears the code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::path::Path;

use kition_kit::KitDocument;
use kition_kit::diagnostics::{Diagnostic, check_diagnostics, report};
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn catalogue() -> Vec<Value> {
    serde_json::from_str(&std::fs::read_to_string(repo().join("agent/checks.json")).unwrap()).unwrap()
}

fn fixture(name: &str) -> TableDocument {
    let bytes = std::fs::read(repo().join("conformance").join(name)).unwrap_or_else(|error| panic!("{name}: {error}"));
    TableDocument::open(name, &bytes).unwrap()
}

fn codes(found: &[Diagnostic]) -> Vec<String> {
    found.iter().map(|diagnostic| diagnostic.code.clone()).collect()
}

#[test]
fn every_fixture_reports_its_code() {
    let catalogue = catalogue();
    assert_eq!(catalogue.len(), 9);
    let known: Vec<&str> = catalogue.iter().map(|entry| entry["code"].as_str().unwrap()).collect();
    let mut failures = Vec::new();
    let mut previous = "error";
    for entry in &catalogue {
        let code = entry["code"].as_str().unwrap();
        let severity = entry["severity"].as_str().unwrap();
        assert!(code.starts_with("table.") && ["error", "warning", "info"].contains(&severity), "{entry}");
        assert!(!(previous != "error" && severity == "error"), "the catalogue lists errors first: {code}");
        previous = severity;
        let name = entry["fixture"].as_str().unwrap();
        assert_eq!(name, format!("check-{}.ktable", code.trim_start_matches("table.")));
        let found = fixture(name).check();
        check_diagnostics(&found).unwrap();
        match found.iter().find(|diagnostic| diagnostic.code == code) {
            None => failures.push(format!("{name} does not report {code}: {found:?}")),
            Some(diagnostic) => {
                if serde_json::to_value(diagnostic.severity).unwrap() != entry["severity"] {
                    failures.push(format!("{name}: {code} is not {severity}"));
                }
                if diagnostic.target.path.is_none() || diagnostic.fix.is_none() {
                    failures.push(format!("{name}: {code} has no target path or no fix: {diagnostic:?}"));
                }
            }
        }
        for diagnostic in &found {
            if !known.contains(&diagnostic.code.as_str()) {
                failures.push(format!("{name} reports {} outside the catalogue", diagnostic.code));
            }
        }
        let sorted = report(&found)["diagnostics"].clone();
        assert_eq!(serde_json::to_value(&found).unwrap(), sorted, "{name}: errors first");
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn the_basic_fixture_and_a_new_document_are_clean() {
    assert_eq!(codes(&fixture("basic.ktable").check()), Vec::<String>::new());
    assert_eq!(codes(&TableDocument::create("Tasks.ktable", 0, 0).unwrap().check()), Vec::<String>::new());
}

/// The fix's params, with the value the diagnostic's message asks for where it is the author's.
fn completed(code: &str, params: &Value) -> Value {
    let mut params = params.clone();
    let set = |params: &mut Value, key: &str, value: Value| {
        params.as_object_mut().unwrap().insert(key.to_owned(), value);
    };
    match code {
        "table.formula-error" | "table.formula-unknown-field" => set(&mut params, "formula", json!("1 + 1")),
        "table.required-empty" => set(&mut params, "values", json!({ "notes": "Due Friday" })),
        "table.duplicate-primary" => set(&mut params, "values", json!({ "title": "Write the plan again" })),
        _ => {}
    }
    params
}

#[test]
fn each_fix_is_a_valid_command_and_clears_its_code() {
    let mut failures = Vec::new();
    for entry in catalogue() {
        let code = entry["code"].as_str().unwrap();
        let name = entry["fixture"].as_str().unwrap();
        let mut doc = fixture(name);
        let found = doc.check();
        let fix = found.iter().find(|diagnostic| diagnostic.code == code).and_then(|diagnostic| diagnostic.fix.clone()).unwrap();
        assert!(doc.commands().iter().any(|command| command.id == fix.command), "{code}: {} is not a command", fix.command);
        let params = completed(code, &fix.params.clone().unwrap_or(Value::Null));
        let schema = kition_table_kit::agent::input_json_schema(&fix.command).unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        if let Err(error) = validator.validate(&params) {
            failures.push(format!("{code}: the fix params do not match {}'s schema: {error}", fix.command));
        }
        if let Err(error) = doc.execute(&fix.command, params) {
            failures.push(format!("{code}: the fix fails: {error}"));
            continue;
        }
        let after = doc.check();
        if after.iter().any(|diagnostic| diagnostic.code == code) {
            failures.push(format!("{code}: still reported after its fix: {after:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A check reads; it never changes the document.
#[test]
fn a_check_changes_nothing() {
    for entry in catalogue() {
        let doc = fixture(entry["fixture"].as_str().unwrap());
        let (revision, bytes) = (doc.revision(), doc.saved_bytes().unwrap());
        doc.check();
        assert_eq!(doc.revision(), revision);
        assert_eq!(doc.saved_bytes().unwrap(), bytes);
    }
}

/// The fixtures are the generator's (`cargo run -p kition-table-kit --example checks -- conformance`).
#[test]
fn the_edit_steps_change_the_basic_fixture() {
    let edit: Value = serde_json::from_str(&std::fs::read_to_string(repo().join("conformance/edit.json")).unwrap()).unwrap();
    let mut doc = fixture("basic.ktable");
    for step in edit["steps"].as_array().unwrap() {
        doc.execute(step["command"].as_str().unwrap(), step["params"].clone()).unwrap();
    }
    assert!(doc.dirty());
    assert_eq!(codes(&doc.check()), Vec::<String>::new());
}
