//! Expectations are declared before execution. A/C/D/R = apply/changed/deleted/relocated.
//! Local integers hold index windows, UUIDs hold identity, named cells hold name membership.
use super::*;

const CHANGES: [&str; 21] = [
    "same cell",
    "other cell",
    "sort cell",
    "filter cell",
    "delete record",
    "insert record above",
    "reorder records",
    "rename field",
    "field type",
    "delete field",
    "view filter",
    "view sort",
    "view group",
    "rename option",
    "delete option",
    "primary value",
    "linked primary",
    "delete linked",
    "insert field above",
    "reorder fields",
    "unrelated option",
];

// Notes is the addressed field except for the select row, which writes Status = To do.
// Field-id/position rows replace the field title, independently of its name and type.
const ROWS: [(&str, &str); 7] = [
    ("record UUID", "CAAAD AARCD AAA AA AAA AAA"),
    ("record integer", "CAAAD RRRCD AAA AA AAA AAA"),
    ("field UUID", "AAAAA AA AAD AAA AA AAA AAA"),
    ("field name", "CAAAD AARCD AAA AA AAA AAA"),
    ("field integer", "AAAAA AA AAD AAA AA AAA RRA"),
    ("transaction cell", "CAAAD AARCD AAA AA AAA AAA"),
    ("select label", "CAAAD AARCD AAA CC AAA AAA"),
];

struct Address {
    table: Value,
    record: Value,
    field: Value,
    linked: Value,
    rank: i64,
}

impl Address {
    fn new(f: &Fixture) -> Self {
        f.colleague("field.add", json!({"tableId":1,"name":"rank","type":"number"}));
        f.colleague("table.add", json!({"title":"Linked"}));
        f.colleague("record.add", json!({"tableId":2,"values":{"title":"Target"}}));
        f.colleague("field.add", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":2}}));
        let doc = f.disk();
        let table = doc.file().get_table(1).unwrap();
        let field = |name| table.fields.iter().find(|field| field.name == name).unwrap().id;
        let rank = field("rank");
        let link = field("friends");
        let out = Self {
            table: doc.inspect().unwrap()["tables"][0]["tool_id"].clone(),
            record: json!(doc.file().get_record(1, 2).unwrap().row_key),
            field: doc.inspect().unwrap()["tables"][0]["fields"][2]["tool_id"].clone(),
            linked: json!(doc.file().list_records(2).unwrap()[0].row_key),
            rank,
        };
        f.colleague("recordLink.replace", json!({"tableId":1,"recordId":2,"field_id":link,"target_row_keys":[out.linked]}));
        f.colleague("tableView.setSorts", json!({"tableId":1,"viewId":1,"items":[{"field_id":rank,"direction":"asc"}]}));
        f.colleague(
            "tableView.setFilterTree",
            json!({"tableId":1,"viewId":1,"tree":{"conjunction":"and","children":[{"condition":{"field_id":2,"operator":"neq","value":"Done"}}]}}),
        );
        out
    }

    fn change(&self, f: &Fixture, column: usize, select: bool) {
        let field = if select { 2 } else { 3 };
        let key = if select { "status" } else { "notes" };
        match column {
            0 => f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{key:if select {"Done"} else {"Theirs"}}})),
            1 => f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{"title":"Other cell"}})),
            2 => f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{"rank":42}})),
            3 => f.colleague("record.set", json!({"tableId":1,"recordId":1,"values":{"status":"Done"}})),
            4 => f.colleague("record.delete", json!({"tableId":1,"recordId":2})),
            5 => f.colleague("edit.transact", json!({"ops":[{"op":"records.create","tableId":1,"at":{"before":self.record},"records":[{"title":"Inserted"}]}]})),
            6 => f.colleague("record.set", json!({"tableId":1,"recordId":1,"order":3})),
            7 => {
                let doc = f.disk();
                let id = doc.inspect().unwrap()["tables"][0]["fields"][(field - 1) as usize]["tool_id"].clone();
                f.colleague_model(json!([{"op":"set","target":id,"field":"name","value":"renamed"}]));
            }
            8 => f.colleague("field.set", json!({"tableId":1,"fieldId":field,"type":"text"})),
            9 => f.colleague("field.delete", json!({"tableId":1,"fieldId":field})),
            10 => f.colleague("tableView.setFilterTree", json!({"tableId":1,"viewId":1,"tree":null})),
            11 => f.colleague("tableView.setSorts", json!({"tableId":1,"viewId":1,"items":[]})),
            12 => f.colleague("tableView.setGroups", json!({"tableId":1,"viewId":1,"items":[{"field_id":self.rank,"direction":"asc"}]})),
            13..=14 | 20 => f.colleague("field.set", json!({"tableId":1,"fieldId":2,"options":{"choices":match column {13 => json!(["Renamed","In progress","Done"]),14 => json!(["In progress","Done"]),_ => json!(["To do","In progress","Finished"])}}})),
            15 => f.colleague("record.set", json!({"tableId":1,"recordId":2,"values":{"title":"Primary changed"}})),
            16 => f.colleague("record.set", json!({"tableId":2,"recordId":4,"values":{"title":"Target renamed"}})),
            17 => f.colleague("record.delete", json!({"tableId":2,"recordId":4})),
            18 => f.colleague("field.add", json!({"tableId":1,"name":"inserted","order":1})),
            19 => f.colleague("edit.transact", json!({"ops":[{"op":"fields.move","tableId":1,"fieldIds":[3],"toIndex":0}]})),
            _ => unreachable!(),
        }
    }

    fn write(&self, form: usize) -> (&'static str, Value) {
        match form {
            0 | 3 => ("record.set", json!({"tableId":self.table,"recordId":self.record,"values":{"notes":"Mine"}})),
            1 => ("record.set", json!({"tableId":self.table,"recordId":2,"values":{"notes":"Mine"}})),
            2 => ("field.set", json!({"tableId":self.table,"fieldId":self.field,"title":"Mine"})),
            4 => ("field.set", json!({"tableId":self.table,"fieldId":3,"title":"Mine"})),
            5 => (
                "edit.transact",
                json!({"ops":[{"op":"cells.set","tableId":self.table,"records":[{"rowKey":self.record,"values":{"notes":"Mine"}}]}]}),
            ),
            6 => ("record.set", json!({"tableId":self.table,"recordId":self.record,"values":{"status":"To do"}})),
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn select_labels_hold_membership_without_holding_other_choices_or_order() {
    for multi in [false, true] {
        for (choices, expected) in [
            (json!(["Renamed", "In progress", "Done"]), 'C'),
            (json!(["In progress", "Done"]), 'C'),
            (json!(["To do", "In progress", "Finished"]), 'A'),
            (json!(["New", "To do", "In progress", "Done"]), 'A'),
            (json!(["Done", "In progress", "To do"]), 'A'),
        ] {
            let f = Fixture::new(Guard::Premise);
            if multi {
                f.colleague("field.set", json!({"tableId":1,"fieldId":2,"type":"multi_select"}));
            }
            let doc = f.disk();
            let table = doc.inspect().unwrap()["tables"][0]["tool_id"].clone();
            let record = doc.file().get_record(1, 1).unwrap().row_key;
            let basis = f.basis().await;
            f.colleague("field.set", json!({"tableId":1,"fieldId":2,"options":{"choices":choices}}));
            let before = f.bytes();
            let answer = f
                .write(
                    basis,
                    "record.set",
                    json!({"tableId":table,"recordId":record,"values":{"status":if multi {json!(["To do"])} else {json!("To do")}}}),
                )
                .await;
            assert_eq!(outcome(&answer), expected, "multi={multi}, choices={choices}: {answer:?}");
            if expected != 'A' {
                assert_eq!(before, f.bytes());
            }
            f.kits.shutdown().await;
        }
    }
}

pub(super) fn outcome(answer: &Result<Value, Value>) -> char {
    match answer {
        Ok(_) => 'A',
        Err(e) if e["error"]["code"] == "conflict" => {
            // A deleted identity is more specific than any simultaneous changed dependency.
            let reasons: Vec<_> = e["conflicts"].as_array().unwrap().iter().filter_map(|c| c["reason"].as_str()).collect();
            if reasons.contains(&"deleted") {
                'D'
            } else if reasons.contains(&"relocated") {
                'R'
            } else if reasons.contains(&"changed") {
                'C'
            } else {
                'U'
            }
        }
        Err(_) => 'E',
    }
}

#[tokio::test]
async fn address_matrix() {
    let mut counts = [0; 4]; // cells, wrong writes, false refusals, wrong reasons/kit errors
    let mut mismatches = Vec::new();
    for (form, (name, expected)) in ROWS.iter().enumerate() {
        let expected: Vec<_> = expected.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(expected.len(), CHANGES.len(), "{name}");
        let mut actual = String::new();
        for (column, change) in CHANGES.iter().enumerate() {
            let f = Fixture::new(Guard::Premise);
            let address = Address::new(&f);
            let (command, params) = address.write(form);
            let basis = f.basis().await;
            address.change(&f, column, form == 6);
            let before = f.bytes();
            let answer = f.write(basis, command, params).await;
            let got = outcome(&answer);
            actual.push(got);
            counts[0] += 1;
            if got != expected[column] {
                counts[if got == 'A' {
                    1
                } else if expected[column] == 'A' {
                    2
                } else {
                    3
                }] += 1;
                mismatches.push(format!("{name} / {change}: expected {}, got {got}: {answer:?}", expected[column]));
            }
            if answer.is_err() {
                assert_eq!(before, f.bytes(), "{name} / {change}");
            }
            if got == 'A' && expected[column] == 'A' {
                let doc = f.disk();
                if matches!(form, 2 | 4) {
                    assert_eq!(doc.file().get_field(1, 3).unwrap().title, "Mine");
                } else {
                    assert_eq!(
                        doc.file().get_record(1, 2).unwrap().values[if form == 6 { "status" } else { "notes" }],
                        if form == 6 { "To do" } else { "Mine" }
                    );
                }
            }
            f.kits.shutdown().await;
        }
        println!("{name}: {actual}");
    }
    println!("address cells/wrong/false/reason: {counts:?}");
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn raw_cell_address_matrix() {
    // Raw cells are addressed by both UUIDs. Their values are not coerced by field type,
    // but removing the column must not silently discard the requested write on projection.
    // The raw write's premise contract (`model.cell`) holds the one cell and the
    // field's existence, type and options, so a record's place and its other cells do not
    // refuse it, and a change of the field's type does (the raw value is read through it).
    let expected = "CAAADAAACDAAAAAAAAAAA";
    let mut mismatches = Vec::new();
    let mut counts = [0; 4];
    for (column, (change, expected)) in CHANGES.iter().zip(expected.chars()).enumerate() {
        let f = Fixture::new(Guard::Premise);
        let address = Address::new(&f);
        let basis = f.basis().await;
        address.change(&f, column, false);
        let before = f.bytes();
        let answer = f.call("batch", json!({"basis":basis,"ops":[{"op":"map.set","target":address.record,"field":"cells","key":address.field,"value":"Mine"}],"check":false})).await;
        let got = outcome(&answer);
        counts[0] += 1;
        if got != expected {
            counts[if got == 'A' {
                1
            } else if expected == 'A' {
                2
            } else {
                3
            }] += 1;
            mismatches.push(format!("{change}: expected {expected}, got {got}"));
        }
        if answer.is_err() {
            assert_eq!(before, f.bytes());
        } else {
            let doc = f.disk();
            let record = kition_kit::kdm::ObjectId::from_string(address.record.as_str().unwrap());
            assert_eq!(doc.file().replica().map_get(&record, "cells", address.field.as_str().unwrap()).unwrap(), "Mine", "{change}");
        }
        f.kits.shutdown().await;
    }
    println!("raw cell cells/wrong/false/reason: {counts:?}");
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn a_raw_cell_rests_on_its_fields_type_and_options() {
    // The raw value is read through the field's type and options, so a
    // colleague's change of either refuses the write (`changed`); a rename keeps the field's
    // identity and applies, and so does a change of another field.
    let status_field = |f: &Fixture| {
        let doc = f.disk();
        doc.file().replica().objects_of_type("Field").into_iter().find(|id| doc.file().replica().get(id, "name").unwrap() == "status").unwrap()
    };
    let cases: [(&str, char); 5] = [("type", 'C'), ("rename option", 'C'), ("delete option", 'C'), ("rename field", 'A'), ("other field", 'A')];
    let mut mismatches = Vec::new();
    for (change, expected) in cases {
        let f = Fixture::new(Guard::Premise);
        let doc = f.disk();
        let record = json!(doc.file().get_record(1, 2).unwrap().row_key);
        let field = json!(status_field(&f).to_string());
        let basis = f.basis().await;
        match change {
            "type" => f.colleague("field.set", json!({"tableId":1,"fieldId":2,"type":"text"})),
            "rename option" => f.colleague("field.set", json!({"tableId":1,"fieldId":2,"options":{"choices":["Renamed","In progress","Done"]}})),
            "delete option" => f.colleague("field.set", json!({"tableId":1,"fieldId":2,"options":{"choices":["In progress","Done"]}})),
            "rename field" => f.colleague_model(json!([{"op":"set","target":field,"field":"name","value":"renamed"}])),
            _ => f.colleague("field.set", json!({"tableId":1,"fieldId":3,"type":"text"})),
        }
        let before = f.bytes();
        let answer = f
            .call("batch", json!({"basis":basis,"ops":[{"op":"map.set","target":record,"field":"cells","key":field,"value":"To do"}],"check":false}))
            .await;
        let got = outcome(&answer);
        if got != expected {
            mismatches.push(format!("{change}: expected {expected}, got {got}: {answer:?}"));
        }
        if answer.is_err() {
            assert_eq!(before, f.bytes(), "{change}");
        } else {
            let record = kition_kit::kdm::ObjectId::from_string(record.as_str().unwrap());
            assert_eq!(f.disk().file().replica().map_get(&record, "cells", field.as_str().unwrap()).unwrap(), "To do", "{change}");
        }
        f.kits.shutdown().await;
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn unavailable_write_addresses_are_rejected_without_saving() {
    let f = Fixture::new(Guard::Premise);
    f.colleague("field.add", json!({"tableId":1,"name":"friends","type":"link_to_record","options":{"target_table_id":1}}));
    for (name, command, params) in [
        ("view row", "record.set", json!({"tableId":1,"viewId":1,"row":2,"values":{"notes":"Mine"}})),
        ("primary value", "record.set", json!({"tableId":1,"recordId":"Two","values":{"notes":"Mine"}})),
        ("view name", "tableView.setConfig", json!({"tableId":1,"viewId":"All","row_height":"tall"})),
        ("table name", "table.set", json!({"tableId":"table","title":"Mine"})),
        ("view range", "record.bulkDelete", json!({"tableId":1,"viewId":1,"rows":[1,2]})),
        ("linked display", "record.set", json!({"tableId":1,"recordId":1,"values":{"friends":["Two"]}})),
    ] {
        let basis = f.basis().await;
        let before = f.bytes();
        assert!(f.write(basis, command, params).await.is_err(), "{name}");
        assert_eq!(before, f.bytes(), "{name}");
    }
    f.kits.shutdown().await;
}
