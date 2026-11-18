use super::*;

#[tokio::test]
async fn receipt_ids_are_usable_and_guard_the_same_cell() {
    for related in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let doc = f.disk();
        let table = doc.file().replica().objects_of_type("Table")[0].to_string();
        let record = doc.file().get_record(1, 1).unwrap().row_key;
        let basis = f.basis().await;
        f.colleague("table.records.update", json!({"tableId":1,"recordId":if related {1} else {2},"values":{"title":"Theirs"}}));
        let answer = f.write(basis, "table.records.update", json!({"tableId":table,"recordId":record,"values":{"title":"Mine"}})).await;
        if related {
            conflict(answer, "changed");
        } else {
            answer.unwrap();
            assert_eq!(f.disk().file().get_record(1, 1).unwrap().values["title"], "Mine");
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn local_integer_addresses_hold_their_list_windows() {
    for before in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let anchor = f.disk().file().get_record(1, if before { 1 } else { 3 }).unwrap().row_key;
        let basis = f.basis().await;
        f.colleague("table.transact", json!({"ops":[{"op":"records.create","tableId":1,"at":{"before":anchor},"records":[{"title":"Theirs"}]}]}));
        let answer = f.write(basis, "table.records.update", json!({"tableId":1,"recordId":2,"values":{"title":"Mine"}})).await;
        if before {
            conflict(answer, "relocated");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}

#[tokio::test]
async fn nested_uuid_addresses_do_not_hold_numeric_windows() {
    for numeric in [true, false] {
        let f = Fixture::new(Guard::Premise);
        let doc = f.disk();
        let table = &doc.inspect().unwrap()["tables"][0];
        let stable_table = table["tool_id"].clone();
        let field = if numeric { json!(3) } else { table["fields"][2]["tool_id"].clone() };
        f.colleague("table.dashboards.upsert", json!({"dashboardId":"review","title":"Review"}));
        let basis = f.basis().await;
        f.colleague("table.fields.create", json!({"tableId":1,"name":"before","order":1}));
        let answer = f.write(basis,"table.dashboards.upsert",json!({"dashboardId":"review","title":"Review","source_table_id":stable_table,"widgets":[{"id":"w","type":"metric","config":{"table_id":stable_table,"field_id":field}}]})).await;
        if numeric {
            conflict(answer, "relocated");
        } else {
            answer.unwrap();
        }
        f.kits.shutdown().await;
    }
}
