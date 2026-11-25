//! Inline field seeds use the same external schema references as standalone field creation.
use super::*;

#[tokio::test]
async fn inline_link_seed_holds_target_schema_and_local_address_window() {
    let mut counts = [0; 4];
    let mut mismatches = Vec::new();
    for numeric in [false, true] {
        for change in ["primary_type", "primary_switch", "table_reorder", "unrelated_cell"] {
            let f = Fixture::new(Guard::Premise);
            f.colleague("table.add", json!({"title":"Second"}));
            let doc = f.disk();
            let table = doc.inspect().unwrap()["tables"][0]["tool_id"].clone();
            let base = doc.file().replica().objects_of_type("Base")[0].to_string();
            let target = if numeric { json!(1) } else { table };
            let basis = f.basis().await;
            match change {
                "primary_type" => f.colleague("field.set", json!({"tableId":1,"fieldId":1,"type":"long_text"})),
                "primary_switch" => f.colleague("table.set", json!({"tableId":1,"primary_field_id":3})),
                "table_reorder" => f.colleague_model(json!([{"op":"list.move","target":base,"field":"tables","from":0,"to":2}])),
                _ => f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"notes":"Unrelated"}})),
            }
            // The target's primary field is read only by the cycle check, which refuses the
            // call itself; a link shows the primary live.
            let expected = match change {
                "table_reorder" if numeric => 'R',
                _ => 'A',
            };
            let before = f.bytes();
            let answer = f
                .write(
                    basis,
                    "table.add",
                    json!({"title":"Linked","fields":[{"name":"friends","type":"link_to_record","options":{"target_table_id":target}}]}),
                )
                .await;
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
                mismatches.push(format!("numeric={numeric}, {change}: expected {expected}, got {got}: {answer:?}"));
            }
            if answer.is_err() {
                assert_eq!(before, f.bytes());
            } else if expected == 'A' {
                let disk = f.disk();
                let tables = disk.file().list_tables().unwrap();
                let created = tables.iter().find(|table| table.name == "linked").unwrap();
                assert_eq!(created.fields[0].options["target_table_id"], 1);
            }
            f.kits.shutdown().await;
        }
    }
    println!("inline seed cells/wrong/false/reason: {counts:?}");
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
