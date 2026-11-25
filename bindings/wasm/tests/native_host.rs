use kition_table_kit::{KitDocument, Origin, TableDocument};
use serde_json::json;

#[test]
fn native_host_imports() {
    let mut doc = TableDocument::create("contacts.ktable", 0, 0).unwrap();
    let answer = doc.execute_as("table.tables.create", json!({ "title": "Contacts" }), &Origin::system("system:host")).unwrap();
    let record = doc
        .execute_as(
            "table.records.create",
            json!({ "tableId": answer["result"]["id"], "values": { "title": "Expert" } }),
            &Origin::system("system:host"),
        )
        .unwrap();
    assert_eq!(record["change"]["origin"], "system:host");
    let bytes = doc.saved_bytes().unwrap();
    let mut reopened = TableDocument::open("contacts.ktable", &bytes).unwrap();
    let tables = reopened.execute("table.tables.list", json!({})).unwrap();
    assert!(tables["result"]["items"].as_array().unwrap().iter().any(|table| table["title"] == "Contacts"));
}
