//! What each command acts on besides its parameters, and the parameter a step's `on` fills
//! (kition-sdk `KitCommand::reads` and `KitCommand::targets`; Kit Process Protocol sections 6 and 7a).
//!
//! A table file's only editor state is what its picture shows (`preview.show`), and no
//! command reads it: every request names its table, view, field and record by id, and
//! `file.render` draws the view its call names. `on` fills the record a command acts on
//! (`recordId`, of the command's `tableId`) or, for a command on a field, the field (`fieldId`).
//! `tests/acts.rs` derives both from the code: the editor state from what reaches `self.shown`,
//! the targets from the ids each request's dispatch reads.

use kition_kit::EditorState;

/// The editor state command `id` reads besides its parameters: none.
pub fn reads(_id: &str) -> &'static [EditorState] {
    &[]
}

/// The parameter naming the record or field command `id` acts on, which a step's `on` fills.
pub fn targets(id: &str) -> Option<&'static str> {
    Some(match id {
        "record.set" | "record.delete" | "record.history" | "recordLink.list" | "recordLink.replace" | "recordLink.delete" => "recordId",
        "field.set" | "field.delete" | "field.duplicate" | "field.linkCandidates" | "tableView.setField" => "fieldId",
        _ => return None,
    })
}

/// Commands that read a record or field id that is not what `on` names, with why.
pub const NOT_TARGETS: &[(&str, &str, &str)] = &[
    ("ai.prepare", "fieldId", "the host's run of an AI field over records, which names both a field and records"),
    ("ai.prepare", "recordId", "the host's run of an AI field over records, which names both a field and records"),
    ("ai.apply", "fieldId", "writes what the host generated for an AI field's run, named by the run"),
    ("ai.apply", "recordId", "writes what the host generated for an AI field's run, named by the run"),
];

/// The commands whose required parameters are published as required (`notation_required`),
/// proven by `testing::check_required` in `tests/acts.rs`. `record.add` is not among
/// them: with only `tableId` it fails on a table with a required field (the starter's `title`),
/// so `values` is required there in practice although its schema cannot say so for every table.
pub const REQUIRED_PUBLISHED: &[&str] = &["record.set", "record.delete", "field.add", "field.set", "tableView.add"];
