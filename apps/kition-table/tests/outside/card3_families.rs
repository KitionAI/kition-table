//! Creation also reads the namespace used to derive its unique name.
use super::*;
use base64::Engine as _;

#[tokio::test]
async fn derived_receipt_edits_match_the_published_schema() {
    // The guard's own derived facts are `premiseOnly`, so a receipt reports no
    // `~` fact, and every edit it reports satisfies the published schema.
    let f = Fixture::new(Guard::Premise);
    let basis = f.basis().await;
    f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{"notes":"Unrelated"}}));
    let answer = f.write(basis, "record.add", json!({"tableId":1,"values":{"title":"Created"}})).await.unwrap();
    let schemas = tools::schemas::tool_schemas();
    let run = schemas["tools"].as_object().unwrap().iter().find(|(name, _)| name.ends_with("_run")).unwrap().1;
    let required = &run["output"]["properties"]["changes"]["properties"]["changed"]["items"]["properties"]["edits"]["items"]["required"];
    assert_eq!(required, &json!(["field"]));
    let changes = &answer["changes"];
    for change in changes["changed"].as_array().unwrap() {
        for field in change["fields"].as_array().unwrap() {
            assert!(!field.as_str().unwrap().starts_with('~'), "a guard-only derived fact in a receipt: {changes}");
        }
        for edit in change["edits"].as_array().unwrap() {
            assert!(edit["field"].is_string(), "{changes}");
            assert!(!edit["field"].as_str().unwrap().starts_with('~'), "a guard-only derived fact in a receipt: {changes}");
        }
    }
    let created = changes["created"].as_array().unwrap();
    assert_eq!(created.len(), 1, "{changes}");
    assert_eq!(created[0]["type"], "Record");
    assert!(created[0]["values"]["cells"].as_object().unwrap().values().any(|value| value == "Created"), "{changes}");
    f.kits.shutdown().await;
}

#[tokio::test]
async fn new_table_import_holds_generated_name_dependencies() {
    let mut counts = [0; 4];
    let mut mismatches = Vec::new();
    for kind in ["new_table", "new_document"] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
            let basis = f.basis().await;
            if related {
                f.colleague_model(json!([{"op":"set","target":table,"field":"name","value":"renamed"}]));
            } else {
                f.colleague("table.set", json!({"tableId":1,"title":"Other title"}));
            }
            let before = f.bytes();
            let answer = f.write(basis,"file.runImport",json!({"fileName":"in.csv","content":base64::engine::general_purpose::STANDARD.encode("Title,Notes\nImported,N\n"),"target":{"kind":kind,"table_title":"Table"},"write_mode":"append","schema_mode":"preserve"})).await;
            let expected = if related { 'R' } else { 'A' };
            let got = super::card3_address::outcome(&answer);
            counts[0] += 1;
            if got != expected {
                counts[if got == 'A' {
                    1
                } else if expected == 'A' {
                    2
                } else {
                    3
                }] += 1;
                mismatches.push(format!("{kind}, related={related}: expected {expected}, got {got}: {answer:?}"));
            }
            if answer.is_err() {
                assert_eq!(before, f.bytes());
            } else if !related {
                assert_eq!(f.disk().file().list_tables().unwrap().last().unwrap().name, "table_2");
            }
            f.kits.shutdown().await;
        }
    }
    println!("import creation cells/wrong/false/reason: {counts:?}");
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn creation_names_hold_collisions_without_holding_unrelated_names() {
    let mut counts = [0; 4];
    let mut mismatches = Vec::new();
    for kind in ["tables.create", "fields.create", "fields.duplicate"] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            if kind == "fields.duplicate" {
                f.colleague("field.add", json!({"tableId":1,"name":"notes_copy","title":"Collision"}));
            }
            let doc = f.disk();
            let inspect = doc.inspect().unwrap();
            let table = inspect["tables"][0]["tool_id"].clone();
            let fields = &inspect["tables"][0]["fields"];
            let basis = f.basis().await;
            let (object, name) = if kind == "tables.create" {
                (table.clone(), if related { "renamed" } else { "table" })
            } else {
                (fields[if related { if kind == "fields.duplicate" { 3 } else { 2 } } else { 1 }]["tool_id"].clone(), "renamed")
            };
            if kind == "tables.create" && !related {
                f.colleague("table.set", json!({"tableId":1,"title":"Unrelated title"}));
            } else {
                f.colleague_model(json!([{"op":"set","target":object,"field":"name","value":name}]));
            }
            let (params, expected_name) = match kind {
                "tables.create" => (json!({"title":"Table"}), "table_2"),
                "fields.create" => (json!({"tableId":table,"title":"Notes"}), "notes_2"),
                _ => (json!({"tableId":table,"fieldId":fields[2]["tool_id"]}), "notes_copy_2"),
            };
            let before = f.bytes();
            let answer = f.write(basis, &format!("table.{kind}"), params).await;
            let got = super::card3_address::outcome(&answer);
            let expected = if related { 'R' } else { 'A' };
            counts[0] += 1;
            if got != expected {
                counts[if got == 'A' {
                    1
                } else if expected == 'A' {
                    2
                } else {
                    3
                }] += 1;
                mismatches.push(format!("{kind}, related={related}: expected {expected}, got {got}: {answer:?}"));
            }
            if answer.is_err() {
                assert_eq!(before, f.bytes());
            } else if !related {
                let doc = f.disk();
                if kind == "tables.create" {
                    assert_eq!(doc.file().list_tables().unwrap().last().unwrap().name, expected_name);
                } else {
                    assert!(doc.file().list_fields(1).unwrap().iter().any(|field| field.name == expected_name));
                }
            }
            f.kits.shutdown().await;
        }
    }
    println!("creation cells/wrong/false/reason: {counts:?}");
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn deleted_table_premise_in_upload() {
    for attachment in [false, true] {
        for related in [true, false] {
            let f = Fixture::new(Guard::Premise);
            if attachment {
                f.colleague("table.add", json!({"title":"Other"}));
            }
            let inspect = f.disk().inspect().unwrap();
            let table = inspect["tables"][0]["tool_id"].clone();
            let notes = inspect["tables"][0]["fields"][2]["tool_id"].clone();
            let basis = f.basis().await;
            if attachment && related {
                f.colleague("table.delete", json!({"tableId":1}));
            } else if related {
                f.colleague("field.delete", json!({"tableId":1,"fieldId":3}));
            } else {
                f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"notes":"Unrelated value"}}));
            }
            let before = f.bytes();
            let (command, params) = if attachment {
                ("attachment.add", json!({"tableId":table,"name":"file.txt","mimeType":"text/plain","data":"QQ=="}))
            } else {
                ("tableView.add", json!({"tableId":table,"title":"Filtered","config":{"sorts":[{"field_id":notes,"direction":"asc"}]}}))
            };
            let answer = f.write(basis, command, params).await;
            // The deleted table is named by the tool id the agent observed.
            if related {
                conflict(answer, "deleted");
                assert_eq!(before, f.bytes());
            } else {
                answer.unwrap();
            }
            f.kits.shutdown().await;
        }
    }
}

#[tokio::test]
async fn last_table_deletion_keeps_tool_ids() {
    let f = Fixture::new(Guard::Premise);
    let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
    let basis = f.basis().await;
    f.colleague("table.delete", json!({"tableId":1}));
    let before = f.bytes();
    let answer = f.write(basis, "attachment.add", json!({"tableId":table,"name":"file.txt","mimeType":"text/plain","data":"QQ=="})).await;
    let error = conflict(answer, "deleted");
    assert!(error["conflicts"].as_array().unwrap().iter().any(|c| c["reason"] == "deleted" && c["id"] == table), "{error}");
    assert_eq!(before, f.bytes());
    f.kits.shutdown().await;
}

#[tokio::test]
async fn default_creation_without_a_place_parameter() {
    let f = Fixture::new(Guard::Premise);
    let table = f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone();
    let basis = f.basis().await;
    f.colleague_model(json!([{"op":"set","target":table,"field":"name","value":"renamed"}]));
    let before = f.bytes();
    let answer = f.write(basis, "table.add", json!({})).await;
    // The generated name rests on the root's names even with no place parameter.
    assert_eq!(super::card3_address::outcome(&answer), 'R', "{answer:?}");
    assert_eq!(before, f.bytes());
    f.kits.shutdown().await;
    let f = Fixture::new(Guard::Premise);
    let basis = f.basis().await;
    f.colleague("table.set", json!({"tableId":1,"title":"Unrelated title"}));
    f.write(basis, "table.add", json!({})).await.unwrap();
    assert_eq!(f.disk().file().list_tables().unwrap().last().unwrap().name, "table_2");
    f.kits.shutdown().await;
}
