//! TEMPORARY (schema freeze round 4): rebuilds the inputs of the eval cases new-task,
//! done-follows, open-work-view (`tasks.ktable`) and number-as-word (`effort.ktable`) so their
//! fields carry the ids their `expected` names again (the ids of the inputs before 853aab4), and
//! prints them as base64 for the change that commits them. Deleted in that change.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use base64::Engine;
use kition_kit::kdm::Origin;
use kition_kit::session::{state, write_state};
use kition_table_file::{TableFile, TableFileError};
use serde_json::{Map, Value};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../cases/");

/// Fields `(name, id now, id the case names)`.
const TASKS: [(&str, &str, &str); 3] = [
    ("title", "01a208fa-fc88-727e-9c00-3e8edc9230ba", "01a169aa-ff23-7691-b372-5f1eec0dcd5e"),
    ("status", "01a208fa-fc88-727e-9c00-3e8fd142cb61", "01a169aa-ff23-7691-b372-5f1f53a79ace"),
    ("notes", "01a208fa-fc88-727e-9c00-3e93e82cf60b", "01a169aa-ff23-7691-b372-5f20579c9f54"),
];
const EFFORT: [(&str, &str, &str); 4] = [
    ("title", "01a208fa-fd4b-7163-8752-1f16bbe071fb", "01a169aa-ff41-7257-9248-148cc2984ea0"),
    ("status", "01a208fa-fd4b-7163-8752-1f1776b8dd70", "01a169aa-ff41-7257-9248-148d9c4155ca"),
    ("notes", "01a208fa-fd4b-7163-8752-1f1b3708de0f", "01a169aa-ff41-7257-9248-148e40f1f662"),
    ("effort", "01a208fa-fd52-752c-9762-ad7775006ab9", "01a169aa-ff42-7189-b76e-6dac6ca3cda0"),
];

/// `(id, name, kdm id)` of the fields and `(id, values)` of the records, as the projection holds them.
fn projection(file: &TableFile) -> (Vec<(i64, String, String)>, Vec<(i64, Value)>) {
    let conn = file.conn();
    let mut fields = conn.prepare("SELECT id, name, kdm_id FROM kition_fields WHERE deleted_at IS NULL ORDER BY id").unwrap();
    let fields = fields.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    let mut records = conn.prepare("SELECT id, values_json FROM kition_records WHERE deleted_at IS NULL ORDER BY id").unwrap();
    let records = records
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
        .unwrap()
        .map(|row| row.map(|(id, values)| (id, serde_json::from_str::<Value>(&values).unwrap())))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    (fields, records)
}

fn rebuild(input: &str, fields: &[(&str, &str, &str)]) -> Vec<u8> {
    let bytes = std::fs::read(format!("{ROOT}{input}")).unwrap();
    let mut file = TableFile::open_bytes(&bytes).unwrap();
    let (old_fields, old_records) = projection(&file);
    let before = state(file.replica()).unwrap();
    let mut text = serde_json::to_string(&before).unwrap();
    for (name, now, named) in fields {
        assert!(old_fields.iter().any(|(_, n, id)| n == name && id == now), "{input}: {name} is {now}: {old_fields:?}");
        text = text.replace(now, named);
    }
    let after: Map<String, Value> = serde_json::from_str(&text).unwrap();
    let schema = kition_table_file::kdm::schema();
    file.transact_as(&Origin::system("import"), |tx| write_state(tx, &schema, &before, &after).map_err(|err| TableFileError::Kdm(err.to_string())))
        .unwrap();
    // A file of its own, projected afresh from the rewritten document, sealed: its content only.
    let mut fresh = TableFile::create_from_replica_in_memory(&file.replica().snapshot().unwrap()).unwrap();
    fresh.seal().unwrap();
    let out = fresh.content_bytes().unwrap();
    let reopened = TableFile::open_bytes(&out).unwrap();
    assert_eq!(reopened.replica().doc_id(), file.replica().doc_id(), "{input}: the same document");
    assert_eq!(state(reopened.replica()).unwrap(), after, "{input}: the state with the named ids");
    let (new_fields, new_records) = projection(&reopened);
    let renamed: Vec<(i64, String, String)> = old_fields
        .iter()
        .map(|(id, name, kdm)| {
            (*id, name.clone(), fields.iter().find(|(_, now, _)| now == kdm).map_or_else(|| kdm.clone(), |(_, _, named)| (*named).to_owned()))
        })
        .collect();
    assert_eq!(new_fields, renamed, "{input}: the fields keep their numbers and names, with the named ids");
    assert_eq!(new_records, old_records, "{input}: the records keep their numbers and values");
    out
}

#[test]
fn print_rebuilt_eval_inputs() {
    let tasks = rebuild("table.agent.act/new-task/input/tasks.ktable", &TASKS);
    let effort = rebuild("table.agent.recover/number-as-word/input/effort.ktable", &EFFORT);
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    panic!("rebuilt eval inputs (temporary test):\nINPUT tasks.ktable {}\nINPUT effort.ktable {}\nEND INPUTS", encode(&tasks), encode(&effort));
}
