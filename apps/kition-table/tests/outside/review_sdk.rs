use super::*;
use base64::Engine as _;

#[tokio::test]
async fn all_command_windows_and_move_destinations_refuse_outside_prefix_removal() {
    for moved_out in [false, true] {
        for form in 0..4 {
            for related in [true, false] {
                let f = Fixture::new(Guard::Premise);
                f.colleague("table.fields.create", json!({"tableId":1,"name":"four","title":"Four"}));
                f.colleague("table.fields.create", json!({"tableId":1,"name":"five","title":"Five"}));
                f.colleague("table.tables.create", json!({"title":"Other"}));
                let doc = f.disk();
                let table = doc.inspect().unwrap()["tables"][0]["tool_id"].clone();
                let other = doc.inspect().unwrap()["tables"][1]["tool_id"].clone();
                let field = |name| {
                    json!(
                        doc.file()
                            .replica()
                            .objects_of_type("Field")
                            .iter()
                            .find(|id| doc.file().replica().get(id, "name").unwrap() == name)
                            .unwrap()
                            .as_str()
                    )
                };
                let (status, five) = (field("status"), field("five"));
                let basis = f.basis().await;
                if related && moved_out {
                    f.colleague_model(json!([
                        {"op":"list.remove","target":table,"field":"fields","index":1},
                        {"op":"list.insert","target":other,"field":"fields","index":1,"item":status}
                    ]));
                } else if related {
                    f.colleague("table.fields.delete", json!({"tableId":1,"fieldId":2}));
                } else {
                    f.colleague("table.fields.update", json!({"tableId":2,"fieldId":6,"title":"Unrelated"}));
                }
                let (command, params) = match form {
                    0 => ("table.fields.create", json!({"tableId":table,"name":"mine","order":4})),
                    1 => ("table.fields.update", json!({"tableId":table,"fieldId":4,"title":"Mine"})),
                    2 => ("table.transact", json!({"ops":[{"op":"fields.move","tableId":table,"fieldIds":[5],"toIndex":3}]})),
                    _ => ("table.transact", json!({"ops":[{"op":"fields.move","tableId":table,"fieldIds":[five],"toIndex":3}]})),
                };
                let before = f.bytes();
                let answer = f.write(basis, command, params).await;
                if related {
                    conflict(answer, "relocated");
                    assert_eq!(before, f.bytes());
                } else {
                    answer.unwrap();
                }
                f.kits.shutdown().await;
            }
        }
    }
}

#[tokio::test]
async fn batch_index_guards_refuse_an_outside_prefix_change() {
    // A raw batch's index rests on the members before it (the SDK's insertion
    // window of a `direct` contract); an unrelated change elsewhere still applies.
    for change in ["delete", "move-out", "unrelated"] {
        for form in 0..3 {
            let f = Fixture::new(Guard::Premise);
            f.colleague("table.tables.create", json!({"title":"Other"}));
            let doc = f.disk();
            let table = doc.inspect().unwrap()["tables"][0]["tool_id"].clone();
            let other = doc.inspect().unwrap()["tables"][1]["tool_id"].clone();
            let notes = doc.inspect().unwrap()["tables"][0]["fields"][2]["tool_id"].clone();
            let status = doc.inspect().unwrap()["tables"][0]["fields"][1]["tool_id"].clone();
            let basis = f.basis().await;
            match change {
                "move-out" => f.colleague_model(json!([
                    {"op":"list.remove","target":table,"field":"fields","index":1},
                    {"op":"list.insert","target":other,"field":"fields","index":1,"item":status}
                ])),
                "delete" => f.colleague("table.fields.delete", json!({"tableId":1,"fieldId":2})),
                _ => f.colleague("table.fields.update", json!({"tableId":2,"fieldId":4,"title":"Unrelated"})),
            }
            let op = match form {
                0 => {
                    json!({"op":"create","type":"Field","in":format!("{}.fields",table.as_str().unwrap()),"at":2,"fields":{"name":"mine","title":"Mine","type":"text","required":false,"readonly":false,"formula":"","aiPrompt":""}})
                }
                1 => json!({"op":"list.insert","target":table,"field":"fields","item":notes,"index":2}),
                _ => json!({"op":"list.move","target":table,"field":"fields","from":1,"to":0}),
            };
            let before = f.bytes();
            let answer = f.call("batch", json!({"basis":basis,"ops":[op],"check":false})).await;
            if change == "unrelated" {
                answer.unwrap_or_else(|error| panic!("{change} / form {form}: {error}"));
            } else {
                // Moving the deleted member itself names its deletion.
                let expected = if change == "delete" && form == 2 { 'D' } else { 'R' };
                assert_eq!(super::card3_address::outcome(&answer), expected, "{change} / form {form}: {answer:?}");
                assert_eq!(before, f.bytes(), "{change} / form {form}");
            }
            f.kits.shutdown().await;
        }
    }
}

#[tokio::test]
async fn stable_id_ownership_holds_survive_an_unrelated_same_server_creation() {
    let f = Fixture::new(Guard::Premise);
    let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
    let record = f.disk().file().get_record(1, 1).unwrap().row_key;
    let basis = f.basis().await;
    tools::call_as(&f.kits,"table_run",&json!({"path":"Work.ktable","steps":[{"command":"table.records.create","params":{"tableId":table,"values":{"title":"Theirs"}}}],"check":false}),f.options,"other-agent").await.unwrap().unwrap();
    let answer = f.write(basis, "table.records.update", json!({"tableId":table,"recordId":record,"values":{"title":"Mine"}})).await;
    println!("same-server structural change followed by stable-id cell write: {}", if answer.is_ok() { "applied" } else { "refused" });
    answer.unwrap();
    f.kits.shutdown().await;
}

#[tokio::test]
async fn list_preimages_on_same_server_local_addresses() {
    // The guard's list facts are per table and small, so the journal keeps
    // their before. Another agent on the same server adding a record to table 2 leaves our
    // local-integer write to table 1 independent; inserting a record above ours in table 1
    // shifts what the integer names and refuses `relocated`.
    for related in [false, true] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("table.tables.create", json!({"title":"Other"}));
        let row_key = f.disk().file().get_record(1, 1).unwrap().row_key;
        let basis = f.basis().await;
        let step = if related {
            json!({"command":"table.transact","params":{"ops":[{"op":"records.create","tableId":1,"at":{"before":row_key},"records":[{"title":"Theirs"}]}]}})
        } else {
            json!({"command":"table.records.create","params":{"tableId":2,"values":{"title":"Theirs"}}})
        };
        tools::call_as(&f.kits, "table_run", &json!({"path":"Work.ktable","steps":[step],"check":false}), f.options, "other-agent")
            .await
            .unwrap()
            .unwrap();
        let before = f.bytes();
        let answer = f.write(basis, "table.records.update", json!({"tableId":1,"recordId":1,"values":{"title":"Mine"}})).await;
        if related {
            conflict(answer, "relocated");
            assert_eq!(before, f.bytes());
        } else {
            answer.unwrap_or_else(|error| panic!("an unrelated same-server creation refused a local address: {error}"));
            assert_eq!(f.disk().file().get_record(1, 1).unwrap().values["title"], "Mine");
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn a_schema_read_holds_its_own_tables_field_order() {
    // An import into table 1 reads its schema in field order, held as table
    // 1's own `~fieldOrder`. A field another writer adds to table 2 leaves it independent; a
    // move of table 1's fields refuses, by another agent on the same server or by an outside
    // writer.
    let mut mismatches = Vec::new();
    for same_server in [true, false] {
        for related in [false, true] {
            let f = Fixture::new(Guard::Premise);
            f.colleague("table.tables.create", json!({"title":"Other"}));
            let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
            let basis = f.basis().await;
            let (command, params) = if related {
                ("table.transact", json!({"ops":[{"op":"fields.move","tableId":1,"fieldIds":[3],"toIndex":1}]}))
            } else {
                ("table.fields.create", json!({"tableId":2,"name":"extra","title":"Extra"}))
            };
            if same_server {
                tools::call_as(
                    &f.kits,
                    "table_run",
                    &json!({"path":"Work.ktable","steps":[{"command":command,"params":params}],"check":false}),
                    f.options,
                    "other-agent",
                )
                .await
                .unwrap()
                .unwrap();
            } else {
                f.colleague(command, params);
            }
            let before = f.bytes();
            let answer = f
                .write(
                    basis,
                    "table.import.run",
                    json!({"fileName":"in.csv","content":base64::engine::general_purpose::STANDARD.encode("Title,Notes\nImported,Mine\n"),"target":{"kind":"existing_table","table_id":table},"write_mode":"append","schema_mode":"preserve"}),
                )
                .await;
            let got = super::card3_address::outcome(&answer);
            let label = format!("same server {same_server}, related {related}: {got}");
            println!("{label}");
            if related {
                if !matches!(got, 'C' | 'R') {
                    mismatches.push(format!("{label}: expected a refusal, got {answer:?}"));
                }
                if answer.is_err() {
                    assert_eq!(before, f.bytes(), "{label}");
                }
            } else if got != 'A' {
                mismatches.push(format!("{label}: expected apply, got {answer:?}"));
            }
            f.kits.shutdown().await;
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
