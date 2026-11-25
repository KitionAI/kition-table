//! What each command reads and the parameter `on` fills, derived from the code, and the required parameters five commands publish.
//!
//! - **reads**: the only editor state is what the picture shows (`TableDocument::shown`). A
//!   command reads it when its dispatch (`run_command`'s branches, `run`'s arms) reads
//!   `self.shown` or hands a `Shown` to the function it calls; `preview.show` only sets it.
//! - **targets**: `kition-table-api`'s `handle` reads each request's ids (`p.id("recordId")`, …):
//!   a request that reads a `recordId` acts on that record, else one that reads a `fieldId` on that
//!   field; the kit's own commands that take such an id are listed in `acts::NOT_TARGETS`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;

use kition_kit::KitDocument;
use kition_table_kit::{TableDocument, acts};
use serde_json::{Value, json};

fn source(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path)).unwrap()
}

/// The arms of the `match` in function `name` of `text`: `"key" => …` up to the next arm, by key.
fn arms(text: &str, name: &str) -> BTreeMap<String, String> {
    let start = text.find(&format!("fn {name}(")).unwrap_or_else(|| panic!("no fn {name}"));
    let body = &text[start..];
    let end = body.find("\n    }\n").or_else(|| body.find("\n}\n")).unwrap_or(body.len());
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in body[..end].lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('"')
            && let Some((key, after)) = rest.split_once("\" =>")
        {
            current = Some(key.to_owned());
            out.insert(key.to_owned(), after.to_owned());
            continue;
        }
        if trimmed.starts_with("_ =>") || trimmed.starts_with("other =>") {
            current = None;
        }
        if let Some(key) = &current {
            let arm = out.entry(key.clone()).or_default();
            arm.push('\n');
            arm.push_str(line);
        }
    }
    out
}

/// Whether `text` reads the picture's editor state.
fn reads_shown(text: &str) -> bool {
    text.match_indices("self.shown").any(|(at, _)| !text[at + "self.shown".len()..].trim_start().starts_with('=')) || text.contains(": &Shown")
}

/// The ids an arm of `handle` reads.
fn ids_read(arm: &str) -> Vec<String> {
    let mut out: Vec<String> = arm.match_indices("p.id(\"").filter_map(|(at, _)| arm[at + 6..].split('"').next().map(str::to_owned)).collect();
    if arm.contains("p.view()") {
        out.push("viewId".into());
    }
    out
}

#[test]
fn what_each_command_reads_and_the_parameter_on_fills_follow_from_the_code() {
    let lib = source("bindings/wasm/src/lib.rs");
    let preview = source("bindings/wasm/src/preview.rs");
    let handle = arms(&source("crates/engine/api/src/dispatch.rs"), "handle");
    let run = arms(&lib, "run");
    let doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    let mut problems = Vec::new();
    // The render command's and the export's branches, and the functions they call.
    let branch = |command: &str| {
        let at = lib.find(&format!("if command == {command} {{")).unwrap_or_else(|| panic!("no branch for {command}"));
        lib[at..].lines().take(3).collect::<Vec<_>>().join("\n")
    };
    let render = format!("{}\n{}", branch("preview::RENDER_COMMAND"), &preview[preview.find("pub fn render(").unwrap()..]);
    let render = &render[..render.find("\n}\n").unwrap_or(render.len())];
    let mut targets = 0;
    for command in doc.commands() {
        let id = command.id.as_str();
        let method = id.strip_prefix("table.").unwrap_or(id);
        let text = match id {
            "file.render" => render.to_owned(),
            "file.export" => branch("export::EXPORT_COMMAND"),
            "file.outline" => branch("outline::OUTLINE_COMMAND"),
            "edit.undo" | "edit.redo" => run.get(if id == "edit.undo" { "undo" } else { "redo" }).cloned().unwrap_or_default(),
            _ => run.get(method).cloned().unwrap_or_default(),
        };
        if reads_shown(&text) != !command.reads.is_empty() || command.reads != acts::reads(id) {
            problems.push(format!("{id}: lists reads {:?}; its code reads the picture's state: {}", command.reads, reads_shown(&text)));
        }
        let derived = handle.get(method).map(|arm| {
            let read = ids_read(arm);
            if read.iter().any(|id| id == "recordId") {
                Some("recordId")
            } else if read.iter().any(|id| id == "fieldId") {
                Some("fieldId")
            } else {
                None
            }
        });
        let not = |param: &str| acts::NOT_TARGETS.iter().any(|(listed, named, why)| *listed == id && *named == param && !why.trim().is_empty());
        match derived {
            Some(derived) if derived != command.targets => {
                problems.push(format!("{id}: its request reads {derived:?}, and the command lists targets {:?}", command.targets));
            }
            Some(_) => {}
            // The kit's own commands: one taking a record or field id declares it or says why not.
            None => {
                let schema = command.schema.clone().unwrap_or(Value::Null);
                for param in ["recordId", "fieldId"] {
                    if schema.get(param).is_some() && command.targets != Some(param) && !not(param) {
                        problems.push(format!("{id}: takes `{param}`, neither its targets nor in acts::NOT_TARGETS"));
                    }
                }
            }
        }
        if command.targets.is_some() {
            targets += 1;
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
    assert_eq!(targets, 11, "6 commands on a record, 5 on a field");
    // `preview.show` sets what the picture shows: the move the agent's own commands keep.
    assert!(run["preview.show"].contains("self.shown = shown"), "{}", run["preview.show"]);
}

/// The five commands that publish their required parameters run with exactly those on a fresh
/// file and fail without each (kition-sdk `testing::check_required`).
#[test]
fn five_commands_publish_their_required_parameters() {
    let fresh = || {
        let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
        doc.execute("record.add", json!({ "tableId": 1, "values": { "title": "First" } })).unwrap();
        doc
    };
    let calls = [
        ("record.set", json!({ "tableId": 1, "recordId": 1 })),
        ("record.delete", json!({ "tableId": 1, "recordId": 1 })),
        ("field.add", json!({ "tableId": 1 })),
        ("field.set", json!({ "tableId": 1, "fieldId": 2 })),
        ("tableView.add", json!({ "tableId": 1 })),
    ];
    kition_kit::testing::check_required(fresh, &calls, |id| acts::REQUIRED_PUBLISHED.contains(&id)).unwrap();
}
