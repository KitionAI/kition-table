//! The colleague is a separate engine that rewrites the actual file.
//! Mixed-table rules, fixed before running: a title write by numeric id needs that title;
//! a cell write by field name needs that field definition and that cell; a config-key write
//! needs that key; an insertion at field index 2 needs the original gap. A deletion of the
//! member before that gap necessarily invalidates it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_kits::tools::{self, CallOptions, Guard};
use kition_kits::{KitDefinition, KitManager};
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::{Value, json};
use std::path::PathBuf;

#[path = "outside/card3_address.rs"]
mod card3_address;
#[path = "outside/card3_derived.rs"]
mod card3_derived;
#[path = "outside/card3_families.rs"]
mod card3_families;
#[path = "outside/card3_seeds.rs"]
mod card3_seeds;
#[path = "outside/read_narrowed.rs"]
mod read_narrowed;
#[path = "outside/review_dependencies.rs"]
mod review_dependencies;
#[path = "outside/review_families.rs"]
mod review_families;
#[path = "outside/review_ids.rs"]
mod review_ids;
#[path = "outside/review_precision.rs"]
mod review_precision;
#[path = "outside/review_receipts.rs"]
mod review_receipts;
#[path = "outside/review_sdk.rs"]
mod review_sdk;

struct Fixture {
    dir: tempfile::TempDir,
    kits: KitManager,
    options: CallOptions,
}

impl Fixture {
    fn new(guard: Guard) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
        for title in ["One", "Two", "Three"] {
            doc.execute("record.add", json!({"tableId":1,"values":{"title":title,"notes":"N"}})).unwrap();
        }
        std::fs::write(dir.path().join("Work.ktable"), doc.saved_bytes().unwrap()).unwrap();
        let kit = KitDefinition::from_manifest_text(include_str!("../../../kit.json")).unwrap();
        let kits = KitManager::new(Some(dir.path().to_owned()), vec![(kit, Some(PathBuf::from(env!("CARGO_BIN_EXE_kition-table"))))]);
        Self { dir, kits, options: CallOptions { guard, ..Default::default() } }
    }

    fn disk(&self) -> TableDocument {
        TableDocument::open("Work.ktable", &self.bytes()).unwrap()
    }

    fn bytes(&self) -> Vec<u8> {
        std::fs::read(self.dir.path().join("Work.ktable")).unwrap()
    }

    fn colleague(&self, command: &str, params: Value) {
        let mut doc = self.disk();
        doc.execute(command, params).unwrap();
        std::fs::write(self.dir.path().join("Work.ktable"), doc.saved_bytes().unwrap()).unwrap();
    }

    fn colleague_model(&self, ops: Value) {
        let mut doc = self.disk();
        kition_kit::session::model::Exchange::new().batch(&mut doc, &json!({"ops":ops}), false).unwrap();
        std::fs::write(self.dir.path().join("Work.ktable"), doc.saved_bytes().unwrap()).unwrap();
    }

    async fn call(&self, action: &str, mut input: Value) -> Result<Value, Value> {
        input["path"] = json!("Work.ktable");
        match tools::call_as(&self.kits, &format!("table_{action}"), &input, self.options, "agent-me").await.unwrap() {
            Ok(output) => Ok(output.output),
            Err(error) => Err(error.structured()),
        }
    }

    async fn basis(&self) -> Value {
        self.call("inspect", json!({})).await.unwrap()["revision"].clone()
    }

    async fn write(&self, basis: Value, command: &str, params: Value) -> Result<Value, Value> {
        self.call("run", json!({"basis":basis,"steps":[{"command":command,"params":params}],"check":false})).await
    }
}

fn conflict(answer: Result<Value, Value>, reason: &str) -> Value {
    let error = answer.unwrap_err();
    assert_eq!(error["error"]["code"], "conflict", "{error}");
    assert!(error["conflicts"].as_array().unwrap().iter().any(|c| c["reason"] == reason), "{error}");
    error
}

#[tokio::test]
async fn same_field_refuses_but_a_different_cell_of_the_same_record_applies() {
    for same in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let basis = f.basis().await;
        f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{if same {"title"} else {"notes"}:"Theirs"}}));
        let before = f.bytes();
        let answer = f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}})).await;
        if same {
            conflict(answer, "changed");
            assert_eq!(before, f.bytes());
        } else {
            let answer = answer.unwrap();
            assert_eq!(answer["others"]["by"], json!(["outside"]));
            assert_eq!(answer["others"]["changed"].as_array().unwrap().len(), 1, "{answer}");
            let record = f.disk().file().get_record(1, 1).unwrap();
            assert_eq!(record.values["title"], "Mine");
            assert_eq!(record.values["notes"], "Theirs");
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn target_deleted_is_refused_before_the_kit() {
    let f = Fixture::new(Guard::Premise);
    let basis = f.basis().await;
    f.colleague("record.delete", json!({"tableId":1,"recordId":1}));
    conflict(f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}})).await, "deleted");
    f.kits.shutdown().await;
}

#[tokio::test]
async fn a_field_name_swap_is_refused() {
    let f = Fixture::new(Guard::Premise);
    let basis = f.basis().await;
    let doc = f.disk();
    let fields = doc.file().replica().objects_of_type("Field");
    let title = fields.iter().find(|id| doc.file().replica().get(id, "name").unwrap() == "title").unwrap();
    let notes = fields.iter().find(|id| doc.file().replica().get(id, "name").unwrap() == "notes").unwrap();
    f.colleague_model(json!([
        {"op":"set","target":title.as_str(),"field":"name","value":"notes"},
        {"op":"set","target":notes.as_str(),"field":"name","value":"title"}
    ]));
    conflict(f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}})).await, "relocated");
    f.kits.shutdown().await;
}

#[tokio::test]
async fn a_field_renamed_away_is_refused_before_invalid_params() {
    let f = Fixture::new(Guard::Premise);
    let basis = f.basis().await;
    let doc = f.disk();
    let fields = doc.file().replica().objects_of_type("Field");
    let title = fields.iter().find(|id| doc.file().replica().get(id, "name").unwrap() == "title").unwrap();
    f.colleague_model(json!([{"op":"set","target":title.as_str(),"field":"name","value":"renamed"}]));
    conflict(f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}})).await, "relocated");
    f.kits.shutdown().await;
}

#[tokio::test]
async fn a_title_write_by_id_survives_record_and_parent_list_moves() {
    let f = Fixture::new(Guard::Premise);
    f.colleague("table.add", json!({"title":"Second"}));
    let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
    let record = f.disk().file().get_record(1, 1).unwrap().row_key;
    let basis = f.basis().await;
    f.colleague("record.set", json!({"tableId":1,"recordId":1,"order":3}));
    let base = f.disk().file().replica().objects_of_type("Base")[0].clone();
    f.colleague_model(json!([{"op":"list.move","target":base.as_str(),"field":"tables","from":0,"to":2}]));
    f.write(basis, "record.set", json!({"tableId":table,"recordId":record,"values":{"title":"Mine"}})).await.unwrap();
    f.kits.shutdown().await;
}

#[tokio::test]
async fn an_unrelated_field_name_change_applies() {
    let f = Fixture::new(Guard::Premise);
    let basis = f.basis().await;
    let doc = f.disk();
    let fields = doc.file().replica().objects_of_type("Field");
    let notes = fields.iter().find(|id| doc.file().replica().get(id, "name").unwrap() == "notes").unwrap();
    f.colleague_model(json!([{"op":"set","target":notes.as_str(),"field":"name","value":"renamed"}]));
    f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}})).await.unwrap();
    f.kits.shutdown().await;
}

#[tokio::test]
async fn off_allows_a_same_field_write_while_epoch_and_premise_refuse() {
    for guard in [Guard::Off, Guard::Epoch, Guard::Premise] {
        let f = Fixture::new(guard);
        let basis = f.basis().await;
        f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Theirs"}}));
        let answer = f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}})).await;
        if matches!(guard, Guard::Off) {
            answer.unwrap();
        } else {
            assert_eq!(answer.unwrap_err()["error"]["code"], "conflict");
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn view_keys_and_field_properties_are_independent() {
    for (command, other, mine) in [
        ("tableView.setConfig", json!({"tableId":1,"viewId":1,"frozen_column_count":2}), json!({"tableId":1,"viewId":1,"row_height":"tall"})),
        ("field.set", json!({"tableId":1,"fieldId":3,"required":true}), json!({"tableId":1,"fieldId":3,"title":"Mine"})),
        ("table.set", json!({"tableId":1,"description":"Theirs"}), json!({"tableId":1,"title":"Mine"})),
    ] {
        let f = Fixture::new(Guard::Premise);
        let basis = f.basis().await;
        f.colleague(command, other);
        f.write(basis, command, mine).await.unwrap();
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn insertion_before_a_gap_refuses_and_insertion_after_applies() {
    for before in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let basis = f.basis().await;
        f.colleague("field.add", json!({"tableId":1,"title":"Theirs","order":if before {1} else {4}}));
        let answer = f.write(basis, "field.add", json!({"tableId":1,"title":"Mine","order":2})).await;
        if before {
            conflict(answer, "relocated");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn deletion_before_this_insertion_gap_is_refused() {
    let f = Fixture::new(Guard::Premise);
    let basis = f.basis().await;
    // Remove Status, before the insertion gap at index 2; Title is primary and cannot go.
    f.colleague("field.delete", json!({"tableId":1,"fieldId":2}));
    let answer = f.write(basis, "field.add", json!({"tableId":1,"title":"Mine","order":3})).await;
    conflict(answer, "relocated");
    f.kits.shutdown().await;
}

#[tokio::test]
async fn unrelated_fifty_rounds_premise_epoch_and_off() {
    println!("guard | applied | refused");
    for guard in [Guard::Premise, Guard::Epoch, Guard::Off] {
        let f = Fixture::new(guard);
        let mut applied = 0;
        let mut refused = 0;
        for round in 0..50 {
            let basis = f.basis().await;
            f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{"title":format!("Theirs {round}")}}));
            match f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"values":{"title":format!("Mine {round}")}})).await {
                Ok(_) => applied += 1,
                Err(error) => {
                    assert_eq!(error["error"]["code"], "conflict", "{error}");
                    refused += 1;
                }
            }
        }
        println!("{guard:?} | {applied} | {refused}");
        assert_eq!(refused, if matches!(guard, Guard::Epoch) { 50 } else { 0 });
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn fixed_seed_mixed_table() {
    // The original four columns and necessity rules stay fixed; add two address forms.
    let mut seed = 0x9e3779b97f4a7c15_u64;
    let mut counts = [[[0_usize; 5]; 6]; 10];
    let kinds = [
        "other row",
        "same cell",
        "other cell",
        "table title",
        "field title",
        "view key",
        "insert before",
        "insert after",
        "delete target",
        "delete before gap",
    ];
    for round in 0..50 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let kind = (seed % 10) as usize;
        for (form, tallies) in counts[kind].iter_mut().enumerate() {
            let f = Fixture::new(Guard::Premise);
            let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
            let record = f.disk().file().get_record(1, 1).unwrap().row_key;
            let basis = f.basis().await;
            let (command, params) = match kind {
                0 => ("record.set", json!({"tableId":1,"recordId":2,"values":{"title":"Theirs"}})),
                1 => ("record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Theirs"}})),
                2 => ("record.set", json!({"tableId":1,"recordId":1,"values":{"notes":"Theirs"}})),
                3 => ("table.set", json!({"tableId":1,"title":"Theirs"})),
                4 => ("field.set", json!({"tableId":1,"fieldId":1,"title":"Theirs"})),
                5 => ("tableView.setConfig", json!({"tableId":1,"viewId":1,"row_height":"short"})),
                6 => ("field.add", json!({"tableId":1,"title":"Theirs","order":1})),
                7 => ("field.add", json!({"tableId":1,"title":"Theirs","order":4})),
                8 => ("record.delete", json!({"tableId":1,"recordId":1})),
                _ => ("field.delete", json!({"tableId":1,"fieldId":2})),
            };
            f.colleague(command, params);
            let (command, params, necessary) = match form {
                0 => ("table.set", json!({"tableId":1,"title":"Mine"}), kind == 3),
                1 => ("record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}}), matches!(kind, 1 | 8)),
                2 => ("tableView.setConfig", json!({"tableId":1,"viewId":1,"row_height":"tall"}), kind == 5),
                3 => ("field.add", json!({"tableId":1,"title":format!("Mine {round}"),"order":3}), matches!(kind, 6 | 9)),
                4 => ("record.set", json!({"tableId":table,"recordId":record,"values":{"title":"Mine"}}), matches!(kind, 1 | 8)),
                _ => ("record.set", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}}), matches!(kind, 1 | 8)),
            };
            let answer = f.write(basis, command, params).await;
            let column = match answer {
                Ok(_) if necessary => 3,
                Ok(_) => 0,
                Err(ref e) if e["error"]["code"] == "conflict" && necessary => 1,
                Err(ref e) if e["error"]["code"] == "conflict" => 2,
                Err(_) => 4,
            };
            tallies[column] += 1;
            f.kits.shutdown().await;
        }
    }
    println!(
        "kind | table title id | cell by name | config key | structural index | stable UUID | local integer (applied/necessary/false/wrong/kit)"
    );
    for (kind, row) in kinds.iter().zip(counts) {
        println!("{kind} | {:?} | {:?} | {:?} | {:?} | {:?} | {:?}", row[0], row[1], row[2], row[3], row[4], row[5]);
    }
    let totals: Vec<usize> = (0..5).map(|c| counts.iter().flat_map(|r| r.iter()).map(|r| r[c]).sum()).collect();
    println!("TOTAL {totals:?}");
    assert_eq!(totals[2], 0, "false refusals");
    assert_eq!(totals[4], 0, "failed at kit");
    assert_eq!(totals[3], 0, "applied wrongly");
}

#[tokio::test]
async fn receipt_sizes() {
    fn bytes(answer: &Value) -> usize {
        serde_json::to_vec(&answer["changes"]).unwrap().len()
    }
    let f = Fixture::new(Guard::Premise);
    let basis = f.basis().await;
    let answer = f.write(basis, "table.set", json!({"tableId":1,"title":"Receipt"})).await.unwrap();
    println!("receipt property: {} bytes", bytes(&answer));
    assert_eq!(answer["changes"]["changed"].as_array().unwrap().len(), 1);
    let basis = f.basis().await;
    let answer = f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"order":3})).await.unwrap();
    println!("receipt record move: {} bytes", bytes(&answer));
    assert_eq!(answer["changes"]["moved"].as_array().unwrap().len(), 1);
    f.kits.shutdown().await;
    for count in [5, 200] {
        let f = Fixture::new(Guard::Premise);
        f.colleague(
            "edit.transact",
            json!({"ops":[{"op":"records.create","tableId":1,"records":(3..count).map(|i|json!({"title":format!("Row {i}")})).collect::<Vec<_>>()}]}),
        );
        f.colleague("table.add", json!({"title":"Second"}));
        let doc = f.disk();
        let base = doc.file().replica().objects_of_type("Base")[0].clone();
        let basis = f.basis().await;
        let answer = f
            .call("batch", json!({"basis":basis,"ops":[{"op":"list.move","target":base.as_str(),"field":"tables","from":0,"to":2}],"check":false}))
            .await
            .unwrap();
        println!("receipt table reorder with {count} records: {} bytes", bytes(&answer));
        assert_eq!(answer["changes"]["moved"].as_array().unwrap().len(), 1);
        // `~lists` is a guard-only (`premiseOnly`) fact, and the move says the
        // list changed, so the receipt names no changed object.
        assert_eq!(answer["changes"]["changed"], json!([]));
        f.kits.shutdown().await;
    }
    let f = Fixture::new(Guard::Premise);
    f.colleague(
        "edit.transact",
        json!({"ops":[{"op":"records.create","tableId":1,"records":(3..201).map(|i|json!({"title":format!("Row {i}")})).collect::<Vec<_>>()}]}),
    );
    let keys: Vec<_> = f.disk().file().list_records(1).unwrap().into_iter().take(200).map(|r| r.row_key).collect();
    let basis = f.basis().await;
    let answer = f.write(basis, "edit.transact", json!({"ops":[{"op":"records.move","tableId":1,"rowKeys":keys,"before":null}]})).await.unwrap();
    println!("receipt reorder 200 records: {} bytes (one equivalent boundary move)", bytes(&answer));
    assert_eq!(answer["changes"]["moved"].as_array().unwrap().len(), 1);
    let reordered: Vec<_> = f.disk().file().list_records(1).unwrap().into_iter().map(|r| r.row_key).collect();
    assert_eq!(&reordered[1..], keys.as_slice(), "receipt's compact move must describe the complete saved order");
    let basis = f.basis().await;
    let answer = f.write(basis,"edit.transact",json!({"ops":[{"op":"cells.set","tableId":1,"records":keys.iter().map(|key|json!({"rowKey":key,"values":{"title":"Same edit"}})).collect::<Vec<_>>()}]})).await.unwrap();
    println!("receipt same cell edit on 200 records: {} bytes", bytes(&answer));
    // Identical assignments are one footprint entry naming every record.
    let stated =
        answer["changes"]["footprint"].as_array().into_iter().flatten().map(|print| print["ids"].as_array().map_or(0, Vec::len)).sum::<usize>();
    assert_eq!(
        answer["changes"]["changed"].as_array().unwrap().len()
            + answer["changes"]["truncated"]["changed"].as_u64().unwrap_or_default() as usize
            + stated,
        200
    );
    assert_eq!(f.disk().file().list_records(1).unwrap().iter().filter(|r| r.values["title"] == "Same edit").count(), 200);
    f.kits.shutdown().await;
}
