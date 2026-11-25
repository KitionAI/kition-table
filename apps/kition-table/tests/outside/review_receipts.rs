use super::*;

#[tokio::test]
async fn text_assignment_footprints_state_identical_assignments_once() {
    // The same text written into many records is one footprint entry naming every record.
    let f = Fixture::new(Guard::Premise);
    f.colleague(
        "edit.transact",
        json!({"ops":[{"op":"records.create","tableId":1,"records":(3..200).map(|_|json!({"title":"Before"})).collect::<Vec<_>>()}]}),
    );
    let records = f.disk().file().list_records(1).unwrap();
    let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
    let title = f.disk().inspect().unwrap()["tables"][0]["fields"][0]["tool_id"].clone();
    let edits = |value: Value| json!({"ops":[{"op":"cells.set","tableId":table,"records":records.iter().map(|r| json!({"rowKey":r.row_key,"values":{"title":value}})).collect::<Vec<_>>()}]});
    let basis = f.basis().await;
    let answer = f.write(basis, "edit.transact", edits(json!("Same edit"))).await.unwrap();
    assert!(answer["changes"]["changed"].as_array().unwrap().is_empty(), "{}", answer["changes"]);
    assert!(answer["changes"].get("truncated").is_none());
    let footprint = answer["changes"]["footprint"].as_array().unwrap();
    assert_eq!(footprint.len(), 1);
    let mut ids: Vec<_> = footprint[0]["ids"].as_array().unwrap().iter().map(|id| id.as_str().unwrap().to_owned()).collect();
    ids.sort();
    let mut keys: Vec<_> = records.iter().map(|r| r.row_key.clone()).collect();
    keys.sort();
    assert_eq!(ids, keys);
    assert_eq!(footprint[0]["edits"], json!([{"field":format!("cells.{}", title.as_str().unwrap()),"after":"Same edit"}]));
    println!("text receipt {} bytes for all 200 UUIDs", answer["changes"].to_string().len());
    let basis = f.basis().await;
    let answer = f
        .call("run", json!({"basis":basis,"steps":[{"command":"edit.transact","params":edits(json!("Next edit"))}],"changes":"all","check":false}))
        .await
        .unwrap();
    assert_eq!(answer["changes"]["changed"].as_array().unwrap().len(), 200);
    assert!(answer["changes"].get("truncated").is_none());
    println!("complete text receipt {} bytes", answer["changes"].to_string().len());
    let basis = f.basis().await;
    f.write(basis, "edit.transact", edits(json!(0))).await.unwrap();
    let basis = f.basis().await;
    let answer = f.write(basis, "edit.transact", edits(json!(1))).await.unwrap();
    assert_eq!(answer["changes"]["footprint"][0]["ids"].as_array().unwrap().len(), 200);
    assert_eq!(answer["changes"]["footprint"][0]["edits"][0]["by"], 1);
    assert!(answer["changes"]["changed"].as_array().unwrap().is_empty());
    assert!(answer["changes"].get("truncated").is_none());
    println!("numeric footprint receipt {} bytes for all 200 UUIDs", answer["changes"].to_string().len());
    f.kits.shutdown().await;
}
