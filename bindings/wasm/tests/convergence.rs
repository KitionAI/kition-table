//! Co-editing at the engine level: the Kition platform convergence suite (L6,
//! `kition-convergence`) against `TableDocument`, driven as the editor's worker and the sync relay
//! drive it (the engine's own `table.*` commands, and the contract 2 members `open_with_replica`,
//! `version`, `export_changes`, `import_changes`, `snapshot`, `undo`, `redo`, `blobs`,
//! `add_blobs`), and the table's own two-editor cases: the edit Kition One's co-editing cases make
//! (`table.records.create` on the first table), edits on both sides while one is offline, and a
//! first open from the room alongside one from the file.
//!
//! The probe is the first table: its `title` and `description` are the two fields the scripted
//! steps set, `Delete` deletes it, `Create` adds a record to it, and `Reorder` moves one of its
//! records to the front.

use kition_convergence::{Blobs, KitAdapter, Net, Options, Rng, Script};
use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

const NAME: &str = "Tasks.ktable";
const SCHEMA: &str = include_str!("../../../model/schema.json");

struct TableKit;

fn run(doc: &mut TableDocument, command: &str, params: Value) -> Result<Value, String> {
    doc.execute(command, params).map_err(|error| format!("{command}: {error}"))
}

/// A request's `result`.
fn call(doc: &mut TableDocument, method: &str, params: Value) -> Result<Value, String> {
    doc.call(method, &params).map(|answer| answer["result"].clone()).map_err(|error| format!("{method}: {error}"))
}

/// The tables in the document's order.
fn tables(doc: &mut TableDocument) -> Result<Vec<Value>, String> {
    Ok(call(doc, "documents.get", Value::Null)?["tables"].as_array().cloned().unwrap_or_default())
}

/// The probe table: its id and its primary field's name.
fn probe(doc: &mut TableDocument) -> Result<(i64, String), String> {
    let table = tables(doc)?.into_iter().next().ok_or("the document has no table")?;
    Ok((table["id"].as_i64().ok_or("a table without an id")?, primary(&table)?))
}

fn primary(table: &Value) -> Result<String, String> {
    let id = table["primary_field_id"].as_i64();
    let fields = table["fields"].as_array().ok_or("a table without fields")?;
    let field = fields.iter().find(|field| field["id"].as_i64() == id).or_else(|| fields.first()).ok_or("a table without fields")?;
    Ok(field["name"].as_str().unwrap_or_default().to_owned())
}

/// A table's records in their order: id and order.
fn records(doc: &mut TableDocument, table_id: i64) -> Result<Vec<(i64, i64)>, String> {
    let page = call(doc, "records.list", json!({ "tableId": table_id, "limit": 1000 }))?;
    Ok(page["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|record| (record["id"].as_i64().unwrap_or(0), record["order"].as_i64().unwrap_or(0)))
        .collect())
}

/// Every record's primary value in every table, sorted: what two converged editors both show.
fn shown(doc: &mut TableDocument) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for table in tables(doc)? {
        let (id, field) = (table["id"].as_i64().unwrap_or(0), primary(&table)?);
        let page = call(doc, "records.list", json!({ "tableId": id, "limit": 1000 }))?;
        for record in page["items"].as_array().into_iter().flatten() {
            out.push(format!("{}: {}", table["title"].as_str().unwrap_or_default(), record["values"][&field].as_str().unwrap_or_default()));
        }
    }
    out.sort();
    Ok(out)
}

fn decode_blobs(section: &[u8]) -> Result<Blobs, String> {
    Ok(kition_table_file::blobs::decode_blob_section(section).map_err(|error| error.to_string())?.into_iter().collect())
}

impl KitAdapter for TableKit {
    type Doc = TableDocument;

    fn name(&self) -> &str {
        "kit.table"
    }

    fn schema(&self) -> &str {
        SCHEMA
    }

    fn create(&self) -> Result<TableDocument, String> {
        let mut doc = TableDocument::create(NAME, 0, 0)?;
        let (table_id, field) = probe(&mut doc)?;
        for title in ["Plan", "Build", "Ship"] {
            run(&mut doc, "table.records.create", json!({ "tableId": table_id, "values": { &field: title } }))?;
        }
        run(&mut doc, "table.tables.create", json!({ "title": "Notes" }))?;
        Ok(doc)
    }

    fn join(&self, snapshot: &[u8], blobs: &Blobs) -> Result<TableDocument, String> {
        TableDocument::open_with_replica(NAME, &[], snapshot, &kition_convergence::wasm::encode_blobs(blobs))
    }

    fn version(&self, doc: &mut TableDocument) -> Result<Vec<u8>, String> {
        Ok(doc.version())
    }

    fn export_changes(&self, doc: &mut TableDocument, since: &[u8]) -> Result<Vec<u8>, String> {
        doc.export_changes(since)
    }

    fn import_changes(&self, doc: &mut TableDocument, bytes: &[u8]) -> Result<(), String> {
        doc.import_changes(bytes).map(|_| ())
    }

    fn snapshot(&self, doc: &mut TableDocument) -> Result<Vec<u8>, String> {
        doc.snapshot()
    }

    fn state(&self, doc: &mut TableDocument) -> Result<Value, String> {
        doc.file().replica().state_json().map_err(|error| error.to_string())
    }

    fn undo(&self, doc: &mut TableDocument) -> Result<bool, String> {
        Ok(doc.undo()?.0)
    }

    fn redo(&self, doc: &mut TableDocument) -> Result<bool, String> {
        Ok(doc.redo()?.0)
    }

    fn random_edit(&self, doc: &mut TableDocument, rng: &mut Rng) -> Result<Option<String>, String> {
        let all = tables(doc)?;
        let Some(table) = rng.pick(&all).cloned() else {
            return Ok(run(doc, "table.tables.create", json!({ "title": "Again" })).ok().map(|_| "tables.create".to_owned()));
        };
        let table_id = table["id"].as_i64().unwrap_or(0);
        let field = primary(&table)?;
        let fields: Vec<Value> = table["fields"].as_array().cloned().unwrap_or_default();
        let others: Vec<&Value> = fields.iter().filter(|other| other["name"].as_str() != Some(field.as_str())).collect();
        let listed = records(doc, table_id)?;
        let record = rng.pick(&listed).copied();
        let value = rng.below(1000);
        let (command, params) = match rng.below(16) {
            0..=3 => ("table.records.create", json!({ "tableId": table_id, "values": { &field: format!("row {value}") } })),
            4..=6 => {
                let Some((record_id, _)) = record else { return Ok(None) };
                let name = rng.pick(&fields).and_then(|field| field["name"].as_str()).unwrap_or(&field).to_owned();
                ("table.records.update", json!({ "tableId": table_id, "recordId": record_id, "values": { name: format!("v{value}") } }))
            }
            7 => {
                let Some((record_id, _)) = record else { return Ok(None) };
                ("table.records.delete", json!({ "tableId": table_id, "recordId": record_id }))
            }
            8 => ("table.fields.create", json!({ "tableId": table_id, "title": format!("F{value}"), "type": "text" })),
            9 => {
                let Some(other) = rng.pick(&others) else { return Ok(None) };
                ("table.fields.update", json!({ "tableId": table_id, "fieldId": other["id"], "title": format!("G{value}") }))
            }
            10 => {
                let Some(other) = rng.pick(&others) else { return Ok(None) };
                ("table.fields.delete", json!({ "tableId": table_id, "fieldId": other["id"] }))
            }
            11 => ("table.tables.update", json!({ "tableId": table_id, "title": format!("T{value}") })),
            12 => ("table.views.create", json!({ "tableId": table_id, "title": format!("View {value}"), "type": "grid" })),
            13 => {
                let (Some((record_id, _)), Some((_, first))) = (record, listed.first().copied()) else { return Ok(None) };
                ("table.records.update", json!({ "tableId": table_id, "recordId": record_id, "values": {}, "order": first }))
            }
            14 => ("table.tables.create", json!({ "title": format!("Table {value}") })),
            _ if all.len() > 1 => ("table.tables.delete", json!({ "tableId": table_id })),
            _ => return Ok(None),
        };
        Ok(run(doc, command, params).ok().map(|_| command.to_owned()))
    }

    fn script(&self, doc: &mut TableDocument, step: &Script) -> Result<bool, String> {
        let (table_id, field) = probe(doc)?;
        match step {
            Script::SetField { slot: 0, value } => {
                run(doc, "table.tables.update", json!({ "tableId": table_id, "title": format!("Title {value}") }))?
            }
            Script::SetField { value, .. } => {
                run(doc, "table.tables.update", json!({ "tableId": table_id, "description": format!("About {value}") }))?
            }
            Script::Create => run(doc, "table.records.create", json!({ "tableId": table_id, "values": { &field: "Created" } }))?,
            Script::Delete => run(doc, "table.tables.delete", json!({ "tableId": table_id }))?,
            Script::Reorder { which } => {
                let listed = records(doc, table_id)?;
                let (Some((_, first)), true) = (listed.first().copied(), listed.len() > 2) else {
                    return Err("the probe table has fewer than 3 records".into());
                };
                // `which` 0 moves the last record to the front, 1 the middle one.
                let moved = if *which == 0 { listed[listed.len() - 1].0 } else { listed[listed.len() / 2].0 };
                run(doc, "table.records.update", json!({ "tableId": table_id, "recordId": moved, "values": {}, "order": first }))?
            }
            _ => return Ok(false),
        };
        Ok(true)
    }

    /// The tables the editor reads (the SQLite projection) match a fresh read of the replica.
    fn check_view(&self, doc: &mut TableDocument) -> Result<(), String> {
        let snapshot = doc.snapshot()?;
        let mut fresh = TableDocument::open_with_replica(NAME, &[], &snapshot, &doc.blobs()?)?;
        let (mine, theirs) = (shown(doc)?, shown(&mut fresh)?);
        if mine != theirs {
            return Err(format!("the editor shows {mine:?}, the replica holds {theirs:?}"));
        }
        let titles = |doc: &mut TableDocument| -> Result<Vec<String>, String> {
            let mut out: Vec<String> = tables(doc)?
                .iter()
                .map(|table| {
                    let mut fields: Vec<String> = table["fields"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|field| field["title"].as_str().unwrap_or_default().to_owned())
                        .collect();
                    fields.sort();
                    format!("{} {:?}", table["title"].as_str().unwrap_or_default(), fields)
                })
                .collect();
            out.sort();
            Ok(out)
        };
        let (mine, theirs) = (titles(doc)?, titles(&mut fresh)?);
        if mine != theirs {
            return Err(format!("the editor's tables are {mine:?}, the replica's {theirs:?}"));
        }
        Ok(())
    }

    fn blobs(&self, doc: &mut TableDocument) -> Result<Blobs, String> {
        decode_blobs(&doc.blobs()?)
    }

    fn add_blobs(&self, doc: &mut TableDocument, blobs: &Blobs) -> Result<(), String> {
        doc.add_blobs(&kition_convergence::wasm::encode_blobs(blobs)).map(|_| ())
    }

    fn export_file(&self, doc: &mut TableDocument) -> Result<Option<Vec<u8>>, String> {
        doc.save(None).map(Some)
    }
}

#[test]
fn converges() {
    let report = kition_convergence::run(&TableKit, &Options::from_env());
    println!("{}", report.render());
    report.save_if_requested().unwrap_or_else(|error| panic!("{error}"));
    assert!(report.level6(), "{}", report.render());
}

fn ok<T>(result: Result<T, String>) -> T {
    result.unwrap_or_else(|error| panic!("{error}"))
}

/// Two replicas hold the same changes: their versions decode equal (the bytes may order the
/// authors differently).
fn same_changes(a: &mut TableDocument, b: &mut TableDocument) -> bool {
    let decode = |bytes: Vec<u8>| ok(kition_kdm::Version::decode(&bytes).map_err(|error| error.to_string()));
    decode(a.version()) == decode(b.version())
}

/// Sends `to` what it lacks of `from`, blobs first, as the relay does.
fn send(from: &mut TableDocument, to: &mut TableDocument) {
    let since = to.version();
    ok(to.add_blobs(&ok(from.blobs_since(&since))));
    ok(to.import_changes(&ok(from.export_changes(&since))));
}

/// Kition One's co-editing case (MC-17/18): two editors of one file add a record to table 1 at
/// the same moment, through `table.records.create` with the primary field `title`; both hold both
/// records, the same replica, and each editor hears of the other's record.
#[test]
fn two_editors_adding_records_at_once_converge() {
    let bytes = ok(TableDocument::create(NAME, 0, 0).and_then(|doc| doc.save(None)));
    let mut ann = ok(TableDocument::open(NAME, &bytes));
    let mut bob = ok(TableDocument::open(NAME, &bytes));
    ok(run(&mut ann, "table.records.create", json!({ "tableId": 1, "values": { "title": "Ann wrote this" } })));
    ok(run(&mut bob, "table.records.create", json!({ "tableId": 1, "values": { "title": "Bob wrote this" } })));
    send(&mut ann, &mut bob);
    send(&mut bob, &mut ann);
    assert!(same_changes(&mut ann, &mut bob));
    assert_eq!(
        ok(ann.file().replica().state_hash().map_err(|error| error.to_string())),
        ok(bob.file().replica().state_hash().map_err(|error| error.to_string()))
    );
    assert_eq!(ok(shown(&mut ann)), ["Table: Ann wrote this", "Table: Bob wrote this"]);
    assert_eq!(ok(shown(&mut ann)), ok(shown(&mut bob)));
    // The editors refresh the rows the other one added (`takeImported`).
    for (doc, theirs) in [(&mut ann, "Bob wrote this"), (&mut bob, "Ann wrote this")] {
        let heard = doc.take_imported();
        assert_eq!(heard.len(), 1, "{heard:?}");
        let upserted = heard[0]["recordsUpserted"].as_array().cloned().unwrap_or_default();
        assert_eq!(upserted.len(), 1, "{heard:?}");
        assert!(heard[0].to_string().contains(theirs) || !upserted.is_empty());
    }
}

/// MC-16/17: one editor is offline (over 30 s in One's case) while both edit, the same record
/// among others; when it reconnects each side sends what the other lacks since the version it
/// last heard, and they converge. Sending again (a retry after the reconnect) changes nothing.
#[test]
fn an_editor_offline_while_both_edit_converges_on_reconnect() {
    let mut net = ok(Net::new(&TableKit, 2, 41));
    ok(net.settle().map(|_| ()));
    let (table_id, field) = ok(probe(ok(net.doc(0))));
    let first = ok(records(ok(net.doc(0)), table_id))[0].0;
    // Offline: nothing travels while both edit.
    for step in 0..5 {
        ok(run(ok(net.doc(0)), "table.records.create", json!({ "tableId": table_id, "values": { &field: format!("Ann {step}") } })));
        ok(run(ok(net.doc(1)), "table.records.create", json!({ "tableId": table_id, "values": { &field: format!("Bob {step}") } })));
    }
    ok(run(ok(net.doc(0)), "table.records.update", json!({ "tableId": table_id, "recordId": first, "values": { &field: "Ann's name" } })));
    ok(run(ok(net.doc(1)), "table.records.update", json!({ "tableId": table_id, "recordId": first, "values": { "notes": "Bob's note" } })));
    ok(run(ok(net.doc(1)), "table.fields.create", json!({ "tableId": table_id, "title": "Added offline", "type": "text" })));
    // Reconnect: both directions, then a retry of both.
    let [ann, bob] = &mut net.docs[..] else { panic!("two replicas") };
    send(ann, bob);
    send(bob, ann);
    let version = ann.version();
    send(ann, bob);
    send(bob, ann);
    assert_eq!(ann.version(), version, "a retry changes nothing");
    assert!(same_changes(ann, bob));
    let merged = ok(shown(ann));
    assert_eq!(merged, ok(shown(bob)));
    for name in ["Ann 4", "Bob 4", "Ann's name"] {
        assert!(merged.iter().any(|row| row.ends_with(name)), "{name} is in {merged:?}");
    }
    let record = ok(call(ann, "records.list", json!({ "tableId": table_id, "limit": 1000 })))["items"]
        .as_array()
        .and_then(|items| items.iter().find(|record| record["id"] == json!(first)).cloned())
        .unwrap_or_default();
    assert_eq!(record["values"][&field], "Ann's name", "both edits of one record are kept: {record}");
    assert_eq!(record["values"]["notes"], "Bob's note", "both edits of one record are kept: {record}");
    ok(net.settle().map(|_| ()));
}

/// MC-05: two editors open a new file at once; one seeds the room from its file, the other opens
/// the room's document (the stored snapshot, with the same file bytes). Both are the same
/// document, and the first edit reaches the other.
#[test]
fn a_first_open_from_the_room_and_one_from_the_file_are_one_document() {
    let bytes = ok(TableDocument::create(NAME, 0, 0).and_then(|doc| doc.save(None)));
    let mut seeder = ok(TableDocument::open(NAME, &bytes));
    let room = ok(seeder.snapshot());
    let mut joiner = ok(TableDocument::open_with_replica(NAME, &bytes, &room, &ok(seeder.blobs())));
    assert_eq!(joiner.doc_id(), seeder.doc_id());
    assert!(same_changes(&mut joiner, &mut seeder));
    assert!(!joiner.dirty(), "the room holds what the file holds");
    ok(run(&mut seeder, "table.records.create", json!({ "tableId": 1, "values": { "title": "First" } })));
    send(&mut seeder, &mut joiner);
    assert_eq!(ok(shown(&mut joiner)), ["Table: First"]);
}
