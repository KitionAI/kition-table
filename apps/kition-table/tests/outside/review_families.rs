use super::*;
use base64::Engine as _;

fn ids(f: &Fixture) -> (Value, Value, Value) {
    let doc = f.disk();
    (
        doc.inspect().unwrap()["tables"][0]["tool_id"].clone(),
        json!(doc.file().get_record(1, 1).unwrap().row_key),
        doc.inspect().unwrap()["tables"][0]["fields"][2]["tool_id"].clone(),
    )
}

#[tokio::test]
async fn type_changes_and_field_deletion_hold_the_column_values() {
    for delete in [true, false] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            let (table, _, field) = ids(&f);
            let basis = f.basis().await;
            f.colleague("table.records.update", json!({"tableId":1,"recordId":2,"values":{if related {"notes"} else {"title"}:"Theirs"}}));
            let mut params = json!({"tableId":table,"fieldId":field});
            if !delete {
                params["type"] = json!("text");
            }
            let answer = f.write(basis, if delete { "table.fields.delete" } else { "table.fields.update" }, params).await;
            // A type change converts no cell, so a cell written meanwhile stays under the new
            // type.
            if related && delete {
                conflict(answer, "changed");
            } else {
                answer.unwrap();
            }
            f.kits.shutdown().await;
        }
    }
}

#[tokio::test]
async fn bulk_operations_hold_selected_sources_only() {
    for method in ["bulkCopy", "bulkDelete"] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            let (table, record, _) = ids(&f);
            let basis = f.basis().await;
            f.colleague("table.records.update", json!({"tableId":1,"recordId":if related {1} else {2},"values":{"notes":"Theirs"}}));
            let answer = f.write(basis, &format!("table.records.{method}"), json!({"tableId":table,"row_keys":[record]})).await;
            if related {
                conflict(answer, "changed");
            } else {
                answer.unwrap();
            }
            f.kits.shutdown().await;
        }
    }
}

#[tokio::test]
async fn imports_hold_schema_and_written_rows() {
    for mode in ["append", "replace", "upsert"] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            let (table, _, _) = ids(&f);
            let basis = f.basis().await;
            if mode == "append" && related {
                f.colleague("table.fields.update", json!({"tableId":1,"fieldId":3,"title":"Changed mapping"}));
            } else if mode == "replace" && !related {
                f.colleague("table.tables.update", json!({"tableId":1,"title":"Theirs"}));
            } else {
                f.colleague("table.records.update", json!({"tableId":1,"recordId":if related {1} else {2},"values":{"notes":"Theirs"}}));
            }
            let answer = f.write(basis,"table.import.run",json!({"fileName":"in.csv","content":base64::engine::general_purpose::STANDARD.encode("Title,Notes\nOne,Mine\n"),"target":{"kind":"existing_table","table_id":table},"write_mode":mode,"schema_mode":"preserve","unique_by":if mode == "upsert" {json!(["Title"])} else {json!([])}})).await;
            if related {
                conflict(answer, "changed");
            } else {
                answer.unwrap();
            }
            f.kits.shutdown().await;
        }
    }
}

#[tokio::test]
async fn outside_rereads_do_not_undo_or_redo_the_colleague() {
    for redo in [true, false] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            let (table, record, _) = ids(&f);
            let basis = f.basis().await;
            f.write(basis, "table.records.update", json!({"tableId":table,"recordId":record,"values":{"notes":"Mine"}})).await.unwrap();
            if redo {
                let basis = f.basis().await;
                f.write(basis, "table.undo", json!({})).await.unwrap();
            }
            let basis = f.basis().await;
            f.colleague("table.records.update", json!({"tableId":1,"recordId":if related {1} else {2},"values":{"notes":"Theirs"}}));
            let answer = f.write(basis, if redo { "table.redo" } else { "table.undo" }, json!({})).await.unwrap();
            // A long text merges per character: the colleague's typing stays, and the step takes
            // back or puts again only the agent's own characters.
            let notes = f.disk().file().get_record(1, if related { 1 } else { 2 }).unwrap().values["notes"].as_str().unwrap_or_default().to_owned();
            assert!(notes.contains("Theirs"), "{notes}");
            if related {
                assert_eq!(notes.contains("Mine"), redo, "{notes}");
            } else {
                assert_eq!(notes, "Theirs");
            }
            // The session reconciles the outside reread and keeps its own undo history. The
            // step takes back
            // or puts again only the agent's own edit; the colleague's value stays.
            assert_eq!(answer["completed"][0]["result"]["result"]["done"], true, "the reread keeps the session's undo history");
            if !related {
                assert_eq!(f.disk().file().get_record(1, 1).unwrap().values["notes"], if redo { "Mine" } else { "N" });
            }
            f.kits.shutdown().await;
        }
    }
}

#[tokio::test]
async fn destructive_commands_hold_removed_links_and_child_values() {
    let mut mismatches = Vec::new();
    for method in ["transact", "import.run", "tables.delete", "records.bulkCopy"] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            f.colleague("table.fields.create", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}));
            f.colleague("table.tables.create", json!({"title":"Other"}));
            let (table, record, _) = ids(&f);
            let target = f.disk().file().get_record(1, 2).unwrap().row_key;
            let basis = f.basis().await;
            if related {
                f.colleague("table.links.replace", json!({"tableId":1,"recordId":1,"field_id":4,"target_row_keys":[target]}));
            } else {
                f.colleague("table.tables.update", json!({"tableId":2,"title":"Theirs"}));
            }
            let params = match method {
                "transact" => json!({"ops":[{"op":"records.delete","tableId":table,"rowKeys":[record]}]}),
                "import.run" => {
                    json!({"fileName":"in.csv","content":base64::engine::general_purpose::STANDARD.encode("Title,Notes\nOne,Mine\n"),"target":{"kind":"existing_table","table_id":table},"write_mode":"replace","schema_mode":"preserve"})
                }
                "tables.delete" => json!({"tableId":table}),
                _ => json!({"tableId":table,"row_keys":[record]}),
            };
            let before = f.bytes();
            let answer = f.write(basis, &format!("table.{method}"), params).await;
            // Bulk copy copies stored cells; link display values and Link objects are omitted.
            let must_refuse = related && method != "records.bulkCopy";
            if answer.is_err() != must_refuse {
                mismatches.push(format!("{method}, related={related}: {answer:?}"));
            } else if must_refuse {
                assert_eq!(answer.unwrap_err()["error"]["code"], "conflict");
                assert_eq!(before, f.bytes());
            } else if method == "records.bulkCopy" {
                assert_eq!(f.disk().file().list_records(1).unwrap().last().unwrap().values["friends"], json!([]));
            }
            f.kits.shutdown().await;
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn table_deletion_holds_child_cell_values() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("table.tables.create", json!({"title":"Other"}));
        let (table, _, _) = ids(&f);
        let basis = f.basis().await;
        if related {
            f.colleague("table.records.update", json!({"tableId":1,"recordId":1,"values":{"notes":"Theirs"}}));
        } else {
            f.colleague("table.tables.update", json!({"tableId":2,"title":"Theirs"}));
        }
        let before = f.bytes();
        let answer = f.write(basis, "table.tables.delete", json!({"tableId":table})).await;
        assert_eq!(answer.is_err(), related, "{answer:?}");
        if related {
            assert_eq!(answer.unwrap_err()["error"]["code"], "conflict");
            assert_eq!(before, f.bytes());
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn import_field_overrides_hold_external_schema_dependencies() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("table.tables.create", json!({"title":"Other"}));
        let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
        let basis = f.basis().await;
        if related {
            f.colleague("table.tables.delete", json!({"tableId":2}));
        } else {
            f.colleague("table.records.update", json!({"tableId":1,"recordId":1,"values":{"notes":"Theirs"}}));
        }
        let before = f.bytes();
        let answer = f.write(basis,"table.import.run",json!({"fileName":"in.csv","content":base64::engine::general_purpose::STANDARD.encode("Title,Linked\nOne,\n"),"target":{"kind":"existing_table","table_id":table},"write_mode":"append","schema_mode":"preserve","field_overrides":[{"index":1,"type":"link_to_record","options":{"target_table_id":2}}]})).await;
        if related {
            conflict(answer, "deleted");
            assert_eq!(before, f.bytes());
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}
