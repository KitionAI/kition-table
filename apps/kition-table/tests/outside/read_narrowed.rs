//! Writes that rest only on what their handlers read.
//! A colleague's separate engine rewrites the file between observe and write; the unrelated
//! change must apply, the necessary one must still refuse.
use super::*;
use base64::Engine as _;

/// The starter table's, a field's and a record's tool ids (records 1 to 3 are One, Two, Three).
fn ids(f: &Fixture, field: &str, record: i64) -> (Value, Value, Value) {
    let inspect = f.disk().inspect().unwrap();
    let table = &inspect["tables"][0];
    let field = table["fields"].as_array().unwrap().iter().find(|item| item["name"] == field).unwrap()["tool_id"].clone();
    (table["tool_id"].clone(), field, json!(f.disk().file().get_record(1, record).unwrap().row_key))
}

/// Where it is, for a call that does not depend on it: a delete by id after the record moved.
#[tokio::test]
async fn a_record_deleted_by_id_does_not_rest_on_where_it_is() {
    for necessary in [false, true] {
        let f = Fixture::new(Guard::Premise);
        let (table, _, two) = ids(&f, "notes", 2);
        let one = f.disk().file().get_record(1, 1).unwrap().row_key;
        let basis = f.basis().await;
        if necessary {
            f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{"notes":"Theirs"}}));
            let before = f.bytes();
            conflict(f.write(basis, "record.delete", json!({"tableId":table,"recordId":two})).await, "changed");
            assert_eq!(before, f.bytes());
        } else {
            f.colleague("edit.transact", json!({"ops":[{"op":"records.move","tableId":1,"rowKeys":[two],"before":one}]}));
            f.write(basis, "record.delete", json!({"tableId":table,"recordId":two})).await.unwrap();
            assert!(f.disk().file().get_record(1, 2).is_err());
        }
        f.kits.shutdown().await;
    }
}

/// Every field of an object a parameter names when one fact of it is read: a sort writes the
/// field's name beside its id, not its title.
#[tokio::test]
async fn a_sort_rests_on_its_field_name_not_its_title() {
    for necessary in [false, true] {
        let f = Fixture::new(Guard::Premise);
        let (table, notes, _) = ids(&f, "notes", 1);
        let view = f.disk().inspect().unwrap()["tables"][0]["views"][0]["tool_id"].clone();
        let basis = f.basis().await;
        if necessary {
            f.colleague("field.delete", json!({"tableId":1,"fieldId":3}));
            conflict(
                f.write(basis, "tableView.setSorts", json!({"tableId":table,"viewId":view,"items":[{"field_id":notes,"direction":"desc"}]})).await,
                "deleted",
            );
        } else {
            f.colleague("field.set", json!({"tableId":1,"fieldId":3,"title":"Remarks"}));
            f.write(basis, "tableView.setSorts", json!({"tableId":table,"viewId":view,"items":[{"field_id":notes,"direction":"desc"}]}))
                .await
                .unwrap();
            let sorts = f.disk().file().get_view(1, 1).unwrap().config["sorts"].clone();
            assert_eq!(sorts[0]["field_name"], "notes", "{sorts}");
        }
        f.kits.shutdown().await;
    }
}

/// Reads that only refuse the call, or that no write makes: a cell write after the field was
/// made read-only (never checked) or another field, which the record fills, required (the check
/// only refuses); a type change makes the written value a computed field's, which the write
/// drops.
#[tokio::test]
async fn a_cell_write_does_not_rest_on_validation_flags() {
    for change in ["readonly", "required elsewhere", "type"] {
        let f = Fixture::new(Guard::Premise);
        let (table, _, one) = ids(&f, "notes", 1);
        f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"status":"Done"}}));
        let basis = f.basis().await;
        match change {
            "readonly" => f.colleague("field.set", json!({"tableId":1,"fieldId":3,"readonly":true})),
            "required elsewhere" => f.colleague("field.set", json!({"tableId":1,"fieldId":2,"required":true})),
            _ => f.colleague("field.set", json!({"tableId":1,"fieldId":3,"type":"formula","formula":"{title}"})),
        }
        let answer = f.write(basis, "record.set", json!({"tableId":table,"recordId":one,"values":{"notes":"Mine"}})).await;
        if change == "type" {
            conflict(answer, "changed");
        } else {
            answer.unwrap();
            assert_eq!(f.disk().file().get_record(1, 1).unwrap().values["notes"], "Mine");
        }
        f.kits.shutdown().await;
    }
}

/// A type change converts no cell: a cell written meanwhile stays, under the new type.
#[tokio::test]
async fn a_type_change_does_not_rest_on_the_column_cells() {
    for necessary in [false, true] {
        let f = Fixture::new(Guard::Premise);
        let (table, notes, _) = ids(&f, "notes", 1);
        let basis = f.basis().await;
        if necessary {
            f.colleague("field.set", json!({"tableId":1,"fieldId":3,"type":"url"}));
            conflict(f.write(basis, "field.set", json!({"tableId":table,"fieldId":notes,"type":"text"})).await, "changed");
        } else {
            f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"notes":"Theirs"}}));
            f.write(basis, "field.set", json!({"tableId":table,"fieldId":notes,"type":"text"})).await.unwrap();
            assert_eq!(f.disk().file().get_record(1, 1).unwrap().values["notes"], "Theirs");
        }
        f.kits.shutdown().await;
    }
}

/// A read that only refuses the call is not held because the kit runs the call again on the
/// file as it is now: when the colleague's change makes that check fail, the engine refuses
/// the call itself and nothing is written, so no result the engine would not produce is saved.
/// Required: the colleague makes a field required that the record leaves blank. Cycle: the
/// colleague makes the formula's source a formula of the field being created.
#[tokio::test]
async fn a_check_that_only_refuses_runs_again_on_the_current_file() {
    for check in ["required", "cycle"] {
        let f = Fixture::new(Guard::Premise);
        let (table, _, one) = ids(&f, "notes", 1);
        let basis = f.basis().await;
        let (command, params) = if check == "required" {
            f.colleague("field.set", json!({"tableId":1,"fieldId":2,"required":true}));
            ("record.set", json!({"tableId":table,"recordId":one,"values":{"notes":"Mine"}}))
        } else {
            f.colleague("field.set", json!({"tableId":1,"fieldId":3,"type":"formula","formula":"{loop}"}));
            ("field.add", json!({"tableId":table,"name":"loop","type":"formula","formula":"{notes}"}))
        };
        let before = f.bytes();
        let error = f.write(basis, command, params).await.unwrap_err();
        assert_ne!(error["error"]["code"], "conflict", "{check}: {error}");
        let text = error.to_string();
        assert!(text.contains(if check == "required" { "required field is blank" } else { "circular reference" }), "{check}: {text}");
        assert_eq!(before, f.bytes(), "{check}");
        f.kits.shutdown().await;
    }
}

/// A schema-replacing import (`auto` on the starter table's shape) retitles the fields in order
/// and deletes the extra ones except the primary field, so which field is primary decides which
/// field survives: the colleague's primary switch to Notes is a fact the result rests on. A
/// table title change is not.
#[tokio::test]
async fn a_schema_replacing_import_rests_on_the_primary_field() {
    for necessary in [false, true] {
        let f = Fixture::new(Guard::Premise);
        let (table, _, _) = ids(&f, "notes", 1);
        let basis = f.basis().await;
        if necessary {
            f.colleague("table.set", json!({"tableId":1,"primary_field_id":3}));
        } else {
            f.colleague("table.set", json!({"tableId":1,"title":"Theirs"}));
        }
        let before = f.bytes();
        let answer = f
            .write(
                basis,
                "file.runImport",
                json!({"fileName":"in.csv","content":base64::engine::general_purpose::STANDARD.encode("A,B\n1,2\n"),"target":{"kind":"existing_table","table_id":table},"write_mode":"append","schema_mode":"auto"}),
            )
            .await;
        if necessary {
            conflict(answer, "changed");
            assert_eq!(before, f.bytes());
        } else {
            answer.unwrap();
            let titles: Vec<String> = f.disk().file().get_table(1).unwrap().fields.into_iter().map(|field| field.title).collect();
            assert_eq!(titles, ["A", "B"]);
        }
        f.kits.shutdown().await;
    }
}
