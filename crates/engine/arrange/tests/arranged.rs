//! The arrangement of a table's records: group headers, search, order, and relocation after
//! edits matching a whole arrangement. The group and search assertions are the editor's
//! (`web/src/editor/table/lib/tableEditorShared.spec.ts`), ported when its arrangement moved into
//! the engine.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeSet;

use kition_table_arrange::{Arranged, Arrangement, Axis, group_id};
use kition_table_file::{CreateOptions, Field, FieldSpec, TableFile, TableSpec};
use serde_json::{Map, Value, json};

/// A document with one table of `fields` (`(name, type)`; the first is primary) and `rows`.
fn table(fields: &[(&str, &str)], rows: &[Value]) -> (TableFile, i64, Vec<Field>) {
    let mut file = TableFile::create_in_memory(&CreateOptions { title: "Arrange".into(), ..Default::default() }).unwrap();
    let specs = fields
        .iter()
        .enumerate()
        .map(|(index, (name, kind))| FieldSpec {
            name: (*name).into(),
            title: (*name).into(),
            field_type: (*kind).into(),
            primary: index == 0,
            options: if kind.ends_with("select") { json!({ "choices": ["Done", "Todo", "Doing", "red", "green", "blue"] }) } else { Value::Null },
            ..Default::default()
        })
        .collect();
    let created = file.create_table(&TableSpec { title: "Rows".into(), fields: specs, ..Default::default() }).unwrap();
    let rows: Vec<Map<String, Value>> = rows.iter().map(|row| row.as_object().unwrap().clone()).collect();
    if !rows.is_empty() {
        file.create_records(created.id, &rows).unwrap();
    }
    let fields = file.get_table(created.id).unwrap().fields;
    (file, created.id, fields)
}

fn field<'f>(fields: &'f [Field], name: &str) -> &'f Field {
    fields.iter().find(|field| field.name == name).unwrap()
}

fn grouped(fields: &[Field], names: &[&str]) -> Arrangement {
    Arrangement { groups: names.iter().map(|name| (field(fields, name).clone(), false)).collect(), ..Arrangement::default() }
}

fn axis(file: &TableFile, table_id: i64, fields: &[Field], arrangement: &Arrangement, query: &str, collapsed: &[&str]) -> Axis {
    let collapsed: BTreeSet<String> = collapsed.iter().map(|id| (*id).to_owned()).collect();
    Arranged::new(file, table_id, fields, arrangement, query).unwrap().axis(1, &collapsed)
}

/// The headers and record runs as the editor's group points read: `H<depth>:<value>`, `R<count>`.
fn points(axis: &Axis) -> Vec<String> {
    let (mut out, mut run, mut next) = (Vec::new(), 0, 0);
    for index in 0..axis.ids.len() {
        while let Some(group) = axis.groups.get(next).filter(|group| group.at as usize == index) {
            if run > 0 {
                out.push(format!("R{run}"));
                run = 0;
            }
            out.push(format!("H{}:{}", group.depth, group.value.as_str().unwrap_or("null")));
            next += 1;
        }
        run += 1;
    }
    if run > 0 {
        out.push(format!("R{run}"));
    }
    out
}

#[test]
fn group_ids_join_each_level_s_value_as_json() {
    assert_eq!(group_id(&[json!("Done")]), "g0:\"Done\"");
    assert_eq!(group_id(&[json!("Done"), json!("Alice")]), "g0:\"Done\"::g1:\"Alice\"");
    assert_eq!(group_id(&[Value::Null]), "g0:null");
    assert_ne!(group_id(&[json!("Done"), json!("Alice")]), group_id(&[json!("Todo"), json!("Alice")]));
}

#[test]
fn no_records_or_no_groups_make_no_headers() {
    let (file, id, fields) = table(&[("title", "text"), ("status", "text")], &[]);
    assert!(axis(&file, id, &fields, &grouped(&fields, &["status"]), "", &[]).groups.is_empty());
    let (file, id, fields) = table(&[("title", "text"), ("status", "text")], &[json!({ "status": "Done" })]);
    let plain = axis(&file, id, &fields, &Arrangement::default(), "", &[]);
    assert!(plain.groups.is_empty());
    assert_eq!(plain.ids.len(), 1);
}

#[test]
fn one_group_of_contiguous_records_is_one_header() {
    let (file, id, fields) =
        table(&[("title", "text"), ("status", "text")], &[json!({ "status": "Done" }), json!({ "status": "Done" }), json!({ "status": "Done" })]);
    let axis = axis(&file, id, &fields, &grouped(&fields, &["status"]), "", &[]);
    assert_eq!(axis.groups.len(), 1);
    let header = &axis.groups[0];
    assert_eq!(
        (header.id.as_str(), header.depth, header.value.clone(), header.collapsed, header.at, header.count),
        ("g0:\"Done\"", 0, json!("Done"), false, 0, 3)
    );
    assert_eq!(points(&axis), ["H0:Done", "R3"]);
}

#[test]
fn a_new_outer_header_starts_where_the_value_changes() {
    let (file, id, fields) =
        table(&[("title", "text"), ("status", "text")], &[json!({ "status": "Done" }), json!({ "status": "Todo" }), json!({ "status": "Todo" })]);
    let axis = axis(&file, id, &fields, &grouped(&fields, &["status"]), "", &[]);
    assert_eq!(points(&axis), ["H0:Done", "R1", "H0:Todo", "R2"]);
    assert_eq!(axis.groups.iter().map(|group| group.count).collect::<Vec<_>>(), [1, 2]);
}

#[test]
fn inner_headers_nest_under_outer_ones() {
    let rows =
        [json!({ "status": "Done", "owner": "Alice" }), json!({ "status": "Done", "owner": "Bob" }), json!({ "status": "Todo", "owner": "Alice" })];
    let (file, id, fields) = table(&[("title", "text"), ("status", "text"), ("owner", "text")], &rows);
    let axis = axis(&file, id, &fields, &grouped(&fields, &["status", "owner"]), "", &[]);
    assert_eq!(points(&axis), ["H0:Done", "H1:Alice", "R1", "H1:Bob", "R1", "H0:Todo", "H1:Alice", "R1"]);
    assert_eq!(axis.groups.iter().map(|group| group.count).collect::<Vec<_>>(), [2, 1, 1, 1, 1]);
    assert_eq!(axis.groups[1].id, "g0:\"Done\"::g1:\"Alice\"");
}

#[test]
fn a_missing_or_null_value_groups_as_null() {
    let (file, id, fields) = table(&[("title", "text"), ("status", "text")], &[json!({}), json!({ "status": null })]);
    let axis = axis(&file, id, &fields, &grouped(&fields, &["status"]), "", &[]);
    assert_eq!(axis.groups.len(), 1);
    assert_eq!((axis.groups[0].value.clone(), axis.groups[0].count), (Value::Null, 2));
}

#[test]
fn collapsed_groups_are_marked_and_keep_their_records() {
    let (file, id, fields) = table(&[("title", "text"), ("status", "text")], &[json!({ "status": "Done" })]);
    let axis = axis(&file, id, &fields, &grouped(&fields, &["status"]), "", &["g0:\"Done\""]);
    assert!(axis.groups[0].collapsed);
    assert_eq!(axis.ids.len(), 1);
}

#[test]
fn search_keeps_records_with_any_field_holding_it_case_ignored() {
    let rows = [json!({ "name": "Project Apollo", "tags": ["urgent"] }), json!({ "name": "X" })];
    let (file, id, fields) = table(&[("name", "text"), ("tags", "multi_select")], &rows);
    let shown = |query: &str| axis(&file, id, &fields, &Arrangement::default(), query, &[]).ids.len();
    // An empty or blank search shows every record.
    assert_eq!(shown(""), 2);
    assert_eq!(shown("   "), 2);
    assert_eq!(shown("apollo"), 1);
    assert_eq!(shown("URGENT"), 1);
    assert_eq!(shown("zeta"), 0);
    // The hits name the cells that hold it.
    let hits = axis(&file, id, &fields, &Arrangement::default(), "urgent", &[]).hits;
    assert_eq!(hits, vec![(0, field(&fields, "tags").id)]);
}

#[test]
fn sorts_order_text_and_numbers_and_ties_keep_the_table_s_order() {
    let rows = [json!({ "name": "b", "n": 10 }), json!({ "name": "A", "n": 2 }), json!({ "name": "c", "n": 2 })];
    let (file, id, fields) = table(&[("name", "text"), ("n", "number")], &rows);
    let names = |arrangement: &Arrangement| {
        let axis = axis(&file, id, &fields, arrangement, "", &[]);
        let records = file.list_records(id).unwrap();
        axis.ids
            .iter()
            .map(|id| records.iter().find(|record| record.id == *id).unwrap().values["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    let by = |name: &str, descending: bool| Arrangement { sorts: vec![(field(&fields, name).clone(), descending)], ..Arrangement::default() };
    assert_eq!(names(&by("name", false)), ["A", "b", "c"]);
    assert_eq!(names(&by("name", true)), ["c", "b", "A"]);
    assert_eq!(names(&by("n", false)), ["A", "c", "b"]);
    // 2 and 2 tie: the table's order, also descending.
    assert_eq!(names(&by("n", true)), ["b", "A", "c"]);
}

/// A small deterministic generator for the relocation property.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }

    fn values(&mut self) -> Map<String, Value> {
        let mut values = Map::new();
        let words = ["alpha", "Beta", "item 2", "item 10", "", "zeta", "Alpha"];
        if self.below(5) > 0 {
            values.insert("name".into(), json!(self.pick(&words)));
        }
        if self.below(4) > 0 {
            values.insert("n".into(), json!(self.below(5) as i64 - 2));
        }
        if self.below(4) > 0 {
            values.insert("stage".into(), json!(self.pick(&["Done", "Todo", "Doing"])));
        }
        if self.below(3) > 0 {
            values.insert("tags".into(), json!([self.pick(&["red", "green", "blue"])]));
        }
        values.insert("flag".into(), json!(self.below(2) == 0));
        values
    }
}

/// After every batch of edits, the arrangement moved record by record is the one arranged anew,
/// for views with filters, sorts, groups and searches.
#[test]
fn relocating_edited_records_matches_arranging_again() {
    let kinds = [("name", "text"), ("n", "number"), ("stage", "single_select"), ("tags", "multi_select"), ("flag", "checkbox")];
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    for case in 0..12 {
        let rows: Vec<Value> = (0..40).map(|_| Value::Object(random.values())).collect();
        let (mut file, id, fields) = table(&kinds, &rows);
        let filter = match case % 3 {
            0 => None,
            1 => Some(
                json!({ "kind": "group", "conjunction": "and", "children": [{ "kind": "condition", "field_name": "n", "operator": "isGreaterEqual", "value": 0 }] }),
            ),
            _ => Some(json!({ "kind": "group", "conjunction": "or", "children": [
                { "kind": "condition", "field_name": "stage", "operator": "isAnyOf", "value": ["Done", "Doing"] },
                { "kind": "condition", "field_name": "flag", "operator": "is", "value": true },
            ] })),
        };
        let arrangement = Arrangement {
            filter: filter.as_ref().and_then(|tree| kition_table_arrange::filter::tree_of_config(tree, &fields)),
            groups: if case % 2 == 0 { vec![(field(&fields, "stage").clone(), case % 4 == 0)] } else { Vec::new() },
            sorts: vec![(field(&fields, "name").clone(), case % 5 == 0), (field(&fields, "n").clone(), true)],
        };
        let query = ["", "a", "item", "RED"][case % 4];
        let mut arranged = Arranged::new(&file, id, &fields, &arrangement, query).unwrap();
        for _ in 0..25 {
            let records = file.list_records(id).unwrap();
            let mut touched = BTreeSet::new();
            for _ in 0..1 + random.below(4) {
                match random.below(6) {
                    0 => {
                        let created = file.create_records(id, &[random.values()]).unwrap();
                        touched.insert(created[0].row_key.clone());
                    }
                    1 if !records.is_empty() => {
                        let record = &records[random.below(records.len() as u64) as usize];
                        if file.delete_record(id, record.id).is_ok() {
                            touched.insert(record.row_key.clone());
                        }
                    }
                    _ if !records.is_empty() => {
                        let record = &records[random.below(records.len() as u64) as usize];
                        if file.update_record(id, record.id, &random.values(), None).is_ok() {
                            touched.insert(record.row_key.clone());
                        }
                    }
                    _ => {}
                }
            }
            let before = arranged.axis(1, &BTreeSet::new());
            let changed = arranged.relocate(&file, &touched).unwrap();
            let moved = arranged.axis(1, &BTreeSet::new());
            let again = Arranged::new(&file, id, &fields, &arrangement, query).unwrap().axis(1, &BTreeSet::new());
            assert_eq!(moved, again, "case {case}");
            assert!(changed || moved == before, "case {case}: the axis changed unreported");
        }
    }
}
