//! Stored AI answers consume live derived values; copies consume stored cells.
//! Every row's expected outcomes are fixed before running any writer.
use super::*;

const CHANGES: [&str; 8] =
    ["local source", "linked source", "membership", "delete linked", "linked display", "other local cell", "other record", "source reorder"];
const ROWS: [(&str, &str); 5] =
    [("formula", "CAAAAAAA"), ("lookup", "ACRDAAAA"), ("rollup", "ACRDAAAA"), ("count", "AARDAAAA"), ("link display", "AARDCAAA")];

fn table(f: &Fixture) -> Value {
    f.disk().inspect().unwrap()["tables"][0]["tool_id"].clone()
}
fn row(f: &Fixture, id: i64) -> Value {
    json!(f.disk().file().get_record(1, id).unwrap().row_key)
}
fn field(f: &Fixture, name: &str) -> Value {
    let doc = f.disk();
    json!(doc.file().replica().objects_of_type("Field").iter().find(|id| doc.file().replica().get(id, "name").unwrap() == name).unwrap().as_str())
}

#[tokio::test]
async fn derived_value_matrix() {
    let mut counts = [0; 4];
    let mut mismatches = Vec::new();
    for (kind, expected) in ROWS {
        let mut actual = String::new();
        for (column, (change, expected)) in CHANGES.iter().zip(expected.chars()).enumerate() {
            let f = Fixture::new(Guard::Premise);
            f.colleague(
                "table.tables.create",
                json!({"title":"Sources","fields":[{"name":"title","type":"text","primary":true},{"name":"notes","type":"text"}]}),
            );
            for title in ["Linked one", "Linked two"] {
                f.colleague("table.records.create", json!({"tableId":2,"values":{"title":title,"notes":"N"}}));
            }
            f.colleague("table.fields.create", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":2}}));
            let doc = f.disk();
            let targets = doc.file().list_records(2).unwrap();
            f.colleague("table.links.replace", json!({"tableId":1,"recordId":1,"field_id":6,"target_row_keys":[targets[0].row_key]}));
            if kind != "link display" {
                let options = if kind == "count" {
                    json!({"link_field_id":6,"aggregation":"count"})
                } else if kind == "formula" {
                    json!({})
                } else {
                    json!({"link_field_id":6,"target_field_id":5,"aggregation":"concat"})
                };
                f.colleague(
                    "table.fields.create",
                    json!({"tableId":1,"name":"input","type":if kind == "count" {"rollup"} else {kind},"formula":"{notes}","options":options}),
                );
            }
            f.colleague("table.fields.create", json!({"tableId":1,"name":"summary","type":"ai","options":{"ai_config":{"type":"summarize","source_field_name":if kind == "link display" {"friends"} else {"input"}}}}));
            let (table, record, output) = (table(&f), row(&f, 1), field(&f, "summary"));
            let basis = f.basis().await;
            match column {
                0 => f.colleague("table.records.update", json!({"tableId":1,"recordId":1,"values":{"notes":"New local"}})),
                1 => f.colleague("table.records.update", json!({"tableId":2,"recordId":4,"values":{"notes":"New linked"}})),
                2 => f.colleague(
                    "table.links.replace",
                    json!({"tableId":1,"recordId":1,"field_id":6,"target_row_keys":[targets[0].row_key,targets[1].row_key]}),
                ),
                3 => f.colleague("table.records.delete", json!({"tableId":2,"recordId":4})),
                4 => f.colleague("table.records.update", json!({"tableId":2,"recordId":4,"values":{"title":"New display"}})),
                5 => f.colleague("table.records.update", json!({"tableId":1,"recordId":1,"values":{"title":"Other cell"}})),
                6 => f.colleague("table.records.update", json!({"tableId":1,"recordId":3,"values":{"notes":"Other record"}})),
                7 => f.colleague("table.records.update", json!({"tableId":2,"recordId":4,"order":2})),
                _ => unreachable!(),
            }
            let before = f.bytes();
            let answer = f
                .write(
                    basis,
                    "table.ai.apply",
                    json!({"tableId":table,"fieldId":output,"recordId":record,"results":[{"recordId":record,"text":"Prepared answer"}]}),
                )
                .await;
            let got = super::card3_address::outcome(&answer);
            actual.push(got);
            counts[0] += 1;
            if got != expected {
                counts[if got == 'A' {
                    1
                } else if expected == 'A' {
                    2
                } else {
                    3
                }] += 1;
                mismatches.push(format!("{kind} / {change}: expected {expected}, got {got}: {answer:?}"));
            }
            if answer.is_err() {
                assert_eq!(before, f.bytes());
            } else {
                assert_eq!(f.disk().file().get_record(1, 1).unwrap().values["summary"], "Prepared answer");
            }
            f.kits.shutdown().await;
        }
        println!("{kind}: {actual}");
    }
    println!("derived cells/wrong/false/reason: {counts:?}");
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn copy_and_type_matrix() {
    // Copies do not inherit source order or a formula's unstored projection. A type
    // change preserves stored cell values: it converts none, so a cell written meanwhile stays
    // under the new type.
    let mut mismatches = Vec::new();
    let mut counts = [0; 4];
    for kind in ["duplicate record", "type update"] {
        let expected_row = if kind == "type update" { "AAAAA" } else { "CAAAA" };
        for (column, expected) in expected_row.chars().enumerate() {
            let f = Fixture::new(Guard::Premise);
            f.colleague("table.fields.create", json!({"tableId":1,"name":"computed","type":"formula","formula":"{notes}"}));
            let (table, record, notes) = (table(&f), row(&f, 1), field(&f, "notes"));
            let basis = f.basis().await;
            match column {
                0 => f.colleague("table.records.update", json!({"tableId":1,"recordId":1,"values":{"notes":"Theirs"}})),
                1 => f.colleague("table.records.update", json!({"tableId":1,"recordId":2,"values":{"title":"Unrelated"}})),
                2 => f.colleague("table.records.update", json!({"tableId":1,"recordId":1,"order":3})),
                3 => f.colleague("table.views.patchConfig", json!({"tableId":1,"viewId":1,"row_height":"tall"})),
                4 => f.colleague("table.tables.update", json!({"tableId":1,"title":"Renamed table"})),
                _ => unreachable!(),
            }
            let before = f.bytes();
            let (command, params) = if kind == "duplicate record" {
                ("table.records.bulkCopy", json!({"tableId":table,"row_keys":[record]}))
            } else {
                ("table.fields.update", json!({"tableId":table,"fieldId":notes,"type":"text"}))
            };
            let answer = f.write(basis, command, params).await;
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
                mismatches.push(format!("{kind} / {column}: expected {expected}, got {got}: {answer:?}"));
            }
            if answer.is_err() {
                assert_eq!(before, f.bytes());
            } else if kind == "type update" {
                assert_eq!(f.disk().file().get_record(1, 1).unwrap().values["notes"], if column == 0 { "Theirs" } else { "N" });
            } else {
                let doc = f.disk();
                let rows = doc.file().list_records(1).unwrap();
                assert_eq!(rows.len(), 4);
                assert_eq!(rows.last().unwrap().values["notes"], "N");
            }
            f.kits.shutdown().await;
        }
    }
    println!("copy/type cells/wrong/false/reason: {counts:?}");
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
