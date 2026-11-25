use super::*;

fn table(f: &Fixture) -> Value {
    f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone()
}
fn field(f: &Fixture, name: &str) -> Value {
    let doc = f.disk();
    json!(doc.file().replica().objects_of_type("Field").iter().find(|id| doc.file().replica().get(id, "name").unwrap() == name).unwrap().as_str())
}
fn row(f: &Fixture, id: i64) -> Value {
    json!(f.disk().file().get_record(1, id).unwrap().row_key)
}

#[tokio::test]
async fn numeric_link_targets_hold_their_local_window() {
    for before in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("field.add", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}));
        let (table, record, anchor) = (table(&f), row(&f, 1), row(&f, if before { 1 } else { 3 }));
        let basis = f.basis().await;
        f.colleague("edit.transact", json!({"ops":[{"op":"records.create","tableId":1,"at":{"before":anchor},"records":[{"title":"Theirs"}]}]}));
        let answer = f.write(basis, "record.set", json!({"tableId":table,"recordId":record,"values":{"friends":[2]}})).await;
        if before {
            conflict(answer, "relocated");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn named_fields_moved_out_hold_their_previous_owner() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("table.add", json!({"title":"Second"}));
        let source = field(&f, if related { "notes" } else { "status" });
        let doc = f.disk();
        let second = doc.inspect().unwrap()["tables"][1]["tool_id"].clone();
        let first = table(&f);
        let listing = doc.file().replica().list(&kition_kit::kdm::ObjectId::from_string(first.as_str().unwrap()), "fields").unwrap();
        let index = listing.iter().position(|id| Some(id.as_str()) == source.as_str()).unwrap();
        let basis = f.basis().await;
        f.colleague_model(json!([
            {"op":"list.remove","target":first,"field":"fields","index":index},
            {"op":"list.insert","target":second,"field":"fields","index":1,"item":source}
        ]));
        let answer = f.write(basis, "record.set", json!({"tableId":first,"recordId":row(&f,1),"values":{"notes":"Mine"}})).await;
        if related {
            conflict(answer, "relocated");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn replacing_link_cells_holds_membership_but_not_other_cells() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("field.add", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}));
        let target = row(&f, 2);
        f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"friends":[target]}}));
        let (table, record) = (table(&f), row(&f, 1));
        let basis = f.basis().await;
        if related {
            f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"friends":[row(&f, 3)]}}));
        } else {
            f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"notes":"Theirs"}}));
        }
        let answer = f.write(basis, "record.set", json!({"tableId":table,"recordId":record,"values":{"friends":[target]}})).await;
        if related {
            conflict(answer, "relocated");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn symmetric_link_writes_hold_the_partner_membership() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("field.add", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}));
        f.colleague(
            "field.add",
            json!({"tableId":1,"name":"back","type":"link_to_record","options":{"target_table_id":1,"symmetric_field_id":4,"allow_multiple":false}}),
        );
        let (table, record, target) = (table(&f), row(&f, 1), row(&f, 2));
        let basis = f.basis().await;
        if related {
            f.colleague("record.set", json!({"tableId":1,"recordId":3,"values":{"friends":[target]}}));
        } else {
            f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{"notes":"Theirs"}}));
        }
        let answer = f.write(basis, "record.set", json!({"tableId":table,"recordId":record,"values":{"friends":[target]}})).await;
        if related {
            conflict(answer, "relocated");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn ai_results_hold_the_source_cell() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague(
            "field.add",
            json!({"tableId":1,"name":"summary","type":"ai","options":{"ai_config":{"type":"summarize","source_field_name":"title"}}}),
        );
        let (table, record, output) = (table(&f), row(&f, 1), field(&f, "summary"));
        let basis = f.basis().await;
        f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{if related {"title"} else {"notes"}:"Theirs"}}));
        let answer = f
            .write(
                basis,
                "ai.apply",
                json!({"tableId":table,"fieldId":output,"recordId":record,"results":[{"recordId":record,"text":"Generated from One"}]}),
            )
            .await;
        if related {
            conflict(answer, "changed");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn formula_creation_holds_its_named_schema_sources() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let table = table(&f);
        let basis = f.basis().await;
        f.colleague("field.set", json!({"tableId":1,"fieldId":if related {3} else {2},"type":"text"}));
        let answer = f.write(basis, "field.add", json!({"tableId":table,"name":"formula","type":"formula","formula":"{notes}"})).await;
        // A formula names its sources by name; their type is read only by the cycle check; the
        // source's type change is unrelated.
        answer.unwrap();
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn named_schema_sources_refuse_rename_and_deletion_only_in_their_scope() {
    let mut mismatches = Vec::new();
    for delete in [true, false] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            let table = table(&f);
            let source = field(&f, if related { "notes" } else { "status" });
            let basis = f.basis().await;
            if delete {
                f.colleague("field.delete", json!({"tableId":1,"fieldId":if related {3} else {2}}));
            } else {
                f.colleague_model(json!([{"op":"set","target":source,"field":"name","value":"renamed"}]));
            }
            let before = f.bytes();
            let answer = f.write(basis, "field.add", json!({"tableId":table,"name":"computed","type":"formula","formula":"{notes}"})).await;
            if related {
                let reason = if delete { "deleted" } else { "relocated" };
                match answer {
                    Ok(_) => mismatches.push(format!("delete={delete}, related={related}: applied wrongly")),
                    Err(error) => {
                        assert_eq!(before, f.bytes());
                        if !error["conflicts"].as_array().is_some_and(|items| items.iter().any(|item| item["reason"] == reason)) {
                            mismatches.push(format!("delete={delete}: expected {reason}: {error}"));
                        }
                    }
                }
            } else if let Err(error) = answer {
                mismatches.push(format!("delete={delete}, unrelated: {error}"));
            }
            f.kits.shutdown().await;
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn combined_name_and_order_holds_the_name_even_when_it_disappears() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let source = field(&f, if related { "notes" } else { "status" });
        let basis = f.basis().await;
        f.colleague_model(json!([{"op":"set","target":source,"field":"name","value":"renamed"}]));
        let answer = f.write(basis, "record.set", json!({"tableId":1,"recordId":1,"order":2,"values":{"notes":"Mine"}})).await;
        if related {
            assert_eq!(answer.unwrap_err()["error"]["code"], "conflict");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn duplicate_field_reads_settings_and_does_not_copy_cell_values() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let (table, source) = (table(&f), field(&f, "notes"));
        let basis = f.basis().await;
        if related {
            f.colleague("field.set", json!({"tableId":1,"fieldId":3,"title":"Theirs"}));
        } else {
            f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"notes":"Theirs"}}));
        }
        let answer = f.write(basis, "field.duplicate", json!({"tableId":table,"fieldId":source})).await;
        if related {
            conflict(answer, "changed");
        } else {
            answer.unwrap();
            assert!(f.disk().file().get_record(1, 1).unwrap().values.get("notes_copy").is_none_or(Value::is_null));
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn nested_dashboard_field_names_and_ids_are_dependencies() {
    for by_name in [true, false] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            let basis = f.basis().await;
            f.colleague("field.set", json!({"tableId":1,"fieldId":if related {3} else {2},"type":"text"}));
            let query = if by_name { json!({"field_name":"notes"}) } else { json!({"field_id":3}) };
            let answer = f
                .write(
                    basis,
                    "dashboard.set",
                    json!({"dashboardId":"report","title":"Report","source_table_id":1,"widgets":[{"id":"w","type":"metric","query":query}]}),
                )
                .await;
            // A widget is stored as given: a field it names by id need only be there; the
            // source's type change is unrelated.
            answer.unwrap();
            f.kits.shutdown().await;
        }
    }
}

#[tokio::test]
async fn view_config_names_hold_the_field_definition() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let (table, view) = (table(&f), f.disk().inspect().unwrap()["tables"][0]["views"][0]["tool_id"].clone());
        let basis = f.basis().await;
        f.colleague("field.set", json!({"tableId":1,"fieldId":if related {3} else {2},"type":"text"}));
        let answer = f
            .write(basis, "tableView.set", json!({"tableId":table,"viewId":view,"config":{"sorts":[{"field_name":"notes","direction":"asc"}]}}))
            .await;
        // A view config is stored as given: a field it names by name is a name's membership; the
        // source's type change is unrelated.
        answer.unwrap();
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn link_targets_exist_for_scalar_and_array_cells() {
    for array in [true, false] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            f.colleague("field.add", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}));
            let (table, record, target) = (table(&f), row(&f, 1), row(&f, 2));
            let basis = f.basis().await;
            if related {
                f.colleague("record.delete", json!({"tableId":1,"recordId":2}));
            } else {
                f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{"notes":"Theirs"}}));
            }
            let value = if array { json!([target]) } else { target };
            let answer = f.write(basis, "record.set", json!({"tableId":table,"recordId":record,"values":{"friends":value}})).await;
            if related {
                conflict(answer, "deleted");
            } else {
                answer.unwrap();
            }
            f.kits.shutdown().await;
        }
    }
}

#[tokio::test]
async fn computed_ai_sources_hold_their_inputs() {
    for source_type in ["formula", "lookup", "rollup"] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            f.colleague("field.add", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}));
            f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"friends":[row(&f,2)]}}));
            f.colleague("field.add", json!({"tableId":1,"name":"input","type":source_type,"formula":"{notes}","options":{"link_field_id":4,"target_field_id":3,"aggregation":"concat"}}));
            f.colleague(
                "field.add",
                json!({"tableId":1,"name":"summary","type":"ai","options":{"ai_config":{"type":"summarize","source_field_name":"input"}}}),
            );
            let (table, record, output) = (table(&f), row(&f, 1), field(&f, "summary"));
            let basis = f.basis().await;
            f.colleague(
                "record.set",
                json!({"tableId":1,"recordId":if source_type == "formula" {1} else {2},"values":{if related {"notes"} else {"title"}:"Theirs"}}),
            );
            let answer = f
                .write(
                    basis,
                    "ai.apply",
                    json!({"tableId":table,"fieldId":output,"recordId":record,"results":[{"recordId":record,"text":"Generated"}]}),
                )
                .await;
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
async fn whole_record_rollup_ai_sources_hold_linked_values() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("table.add", json!({"title":"Other","fields":[{"name":"title","type":"text","primary":true},{"name":"notes","type":"text"}]}));
        f.colleague("record.add", json!({"tableId":2,"values":{"title":"Target","notes":"N"}}));
        let target = f.disk().file().list_records(2).unwrap().remove(0);
        f.colleague("field.add", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":2}}));
        let link = f.disk().file().list_fields(1).unwrap().into_iter().find(|field| field.name == "friends").unwrap().id;
        f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"friends":[target.row_key]}}));
        f.colleague("field.add", json!({"tableId":1,"name":"input","type":"rollup","options":{"link_field_id":link,"aggregation":"concat"}}));
        f.colleague(
            "field.add",
            json!({"tableId":1,"name":"summary","type":"ai","options":{"ai_config":{"type":"summarize","source_field_name":"input"}}}),
        );
        let (table, record, output) = (table(&f), row(&f, 1), field(&f, "summary"));
        let basis = f.basis().await;
        f.colleague("record.set", json!({"tableId":if related {2} else {1},"recordId":if related {target.id} else {3},"values":{"notes":"Theirs"}}));
        let before = f.bytes();
        let answer = f
            .write(basis, "ai.apply", json!({"tableId":table,"fieldId":output,"recordId":record,"results":[{"recordId":record,"text":"Generated"}]}))
            .await;
        if related {
            conflict(answer, "changed");
            assert_eq!(before, f.bytes());
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn view_filter_sort_and_group_ids_hold_field_definitions() {
    for method in ["putFilterTree", "putSorts", "putGroups"] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            let (table, view, source) = (table(&f), f.disk().inspect().unwrap()["tables"][0]["views"][0]["tool_id"].clone(), field(&f, "notes"));
            let basis = f.basis().await;
            f.colleague("field.set", json!({"tableId":1,"fieldId":if related {3} else {2},"type":"text"}));
            let mut params = json!({"tableId":table,"viewId":view});
            if method == "putFilterTree" {
                params["tree"] = json!({"conjunction":"and","children":[{"group":{"conjunction":"or","children":[{"condition":{"field_id":source,"operator":"is","value":"N"}}]}}]});
            } else {
                params["items"] = json!([{"field_id":source,"direction":"asc"}]);
            }
            let answer = f.write(basis, &format!("table.views.{method}"), params).await;
            // A filter, sort or group writes its field's name beside its id, not its type; the
            // source's type change is unrelated.
            answer.unwrap();
            f.kits.shutdown().await;
        }
    }
}
