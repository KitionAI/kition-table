use super::*;

#[tokio::test]
async fn ai_source_names_are_scoped_to_the_target_table() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("table.tables.create", json!({"title":"Other","fields":[{"name":"title","title":"Title","type":"text"}]}));
        let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
        let basis = f.basis().await;
        let target = f.disk().inspect().unwrap()["tables"][if related { 0 } else { 1 }]["fields"][0]["tool_id"].clone();
        f.colleague_model(json!([{"op":"set","target":target,"field":"name","value":"renamed"}]));
        let answer = f
            .write(
                basis,
                "table.fields.create",
                json!({"tableId":table,"name":"summary","type":"ai","options":{"ai_config":{"type":"summarize","source_field_name":"title"}}}),
            )
            .await;
        if related {
            conflict(answer, "relocated");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn layout_paths_are_independent_when_null_becomes_an_object() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let (table, view, field) = {
            let doc = f.disk();
            let table = &doc.inspect().unwrap()["tables"][0];
            (table["tool_id"].clone(), table["views"][0]["tool_id"].clone(), table["fields"][2]["tool_id"].clone())
        };
        let basis = f.basis().await;
        f.colleague("table.views.patchField", json!({"tableId":1,"viewId":1,"fieldId":if related {3} else {2},"position":7}));
        let answer = f.write(basis, "table.views.patchField", json!({"tableId":table,"viewId":view,"fieldId":field,"position":2})).await;
        if related {
            assert_eq!(answer.unwrap_err()["error"]["code"], "conflict");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn link_target_content_is_an_existence_premise_only() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        f.colleague("table.fields.create", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}));
        let doc = f.disk();
        let table = doc.inspect().unwrap()["tables"][0]["tool_id"].clone();
        let record = doc.file().get_record(1, 1).unwrap().row_key;
        let target = doc.file().get_record(1, 2).unwrap().row_key;
        let field = doc.inspect().unwrap()["tables"][0]["fields"][3]["tool_id"].clone();
        let basis = f.basis().await;
        if related {
            f.colleague("table.records.delete", json!({"tableId":1,"recordId":2}));
        } else {
            f.colleague("table.records.update", json!({"tableId":1,"recordId":2,"values":{"notes":"Theirs"}}));
        }
        let answer =
            f.write(basis, "table.links.replace", json!({"tableId":table,"recordId":record,"field_id":field,"target_row_keys":[target]})).await;
        if related {
            conflict(answer, "deleted");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}
