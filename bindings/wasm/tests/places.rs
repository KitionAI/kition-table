//! Facts held by a call, using the replica's stable addresses.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_kit::Premise;
use kition_table_kit::{KitDocument, TableDocument};
use serde_json::{Value, json};

fn doc() -> TableDocument {
    let mut doc = TableDocument::create("Tasks.ktable", 0, 0).unwrap();
    doc.execute("table.records.create", json!({"tableId":1,"values":{"title":"One","notes":"N"}})).unwrap();
    doc
}

fn object(doc: &TableDocument, ty: &str) -> Value {
    json!(doc.file().replica().objects_of_type(ty)[0].as_str())
}

#[test]
fn a_table_property_holds_only_the_replaced_property() {
    let doc = doc();
    let under = doc.under("table.tables.update", &json!({"tableId":1,"title":"Work"})).unwrap().unwrap();
    assert!(under.premises.contains(&Premise::on(object(&doc, "Table")).fields(["title"]).positions(Vec::<String>::new())), "{under:?}");
}

#[test]
fn a_cell_write_holds_the_named_field_definition_and_only_that_cell() {
    let doc = doc();
    let fields = doc.file().replica().objects_of_type("Field");
    let title = fields.iter().find(|id| doc.file().replica().get(id, "name").unwrap() == "title").unwrap();
    let under = doc.under("table.records.update", &json!({"tableId":1,"recordId":1,"values":{"title":"Changed"}})).unwrap().unwrap();
    assert!(
        under.premises.iter().any(|p| p.id == object(&doc, "Record") && p.fields.as_ref().unwrap().contains(&format!("cells.{title}"))),
        "{under:?}"
    );
    assert!(under.premises.iter().any(|p| p.id == json!(title.as_str()) && p.fields.as_ref().unwrap().contains(&"name".into())), "{under:?}");
}

#[test]
fn a_view_config_patch_holds_only_the_keys_it_replaces() {
    let doc = doc();
    let under = doc.under("table.views.patchConfig", &json!({"tableId":1,"viewId":1,"row_height":"tall"})).unwrap().unwrap();
    assert!(
        under.premises.contains(&Premise::on(object(&doc, "View")).fields(["~owner", "~rowHeight"]).positions(Vec::<String>::new())),
        "{under:?}"
    );
}

#[test]
fn an_order_write_declares_its_destination_gap() {
    let doc = doc();
    let under =
        doc.under("table.records.update", &json!({"tableId":object(&doc,"Table"),"recordId":object(&doc,"Record"),"order":1})).unwrap().unwrap();
    assert_eq!(under.within.len(), 1, "{under:?}");
    assert_eq!(under.within[0].parent, object(&doc, "Table"));
    assert_eq!(under.within[0].field, "records");
    assert_eq!(under.within[0].from, under.within[0].to);
    assert!(doc.commands().iter().find(|c| c.id == "table.records.update").unwrap().places.contains(&"order"));
}

#[test]
fn deriving_context_changes_no_editor_or_document_state() {
    let doc = doc();
    let before = (doc.editor_state(), doc.revision(), doc.dirty(), doc.version());
    doc.under("table.records.update", &json!({"tableId":1,"recordId":1,"values":{"title":"Changed"}})).unwrap();
    assert_eq!(before, (doc.editor_state(), doc.revision(), doc.dirty(), doc.version()));
}

#[test]
fn effective_view_settings_have_derived_values_even_without_stored_config() {
    let doc = doc();
    let values = doc.derived(&[object(&doc, "View")], &["rowHeight".into(), "frozenColumnCount".into()]).unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].values["rowHeight"], Value::Null);
    assert_eq!(values[0].values["frozenColumnCount"], Value::Null);
}

#[test]
fn a_transaction_holds_all_cells_written_to_the_same_record() {
    let doc = doc();
    let row = object(&doc, "Record");
    let under = doc
        .under(
            "table.transact",
            &json!({"ops":[
                {"op":"cells.set","tableId":1,"records":[{"rowKey":row,"values":{"title":"A"}}]},
                {"op":"cells.set","tableId":1,"records":[{"rowKey":row,"values":{"notes":"B"}}]}
            ]}),
        )
        .unwrap()
        .unwrap();
    let premise = under.premises.iter().find(|p| p.id == row).unwrap();
    let mut expected = doc
        .file()
        .replica()
        .objects_of_type("Field")
        .iter()
        .filter_map(|field| {
            let name = doc.file().replica().get(field, "name").ok()?;
            matches!(name.as_str(), Some("title" | "notes")).then(|| format!("cells.{field}"))
        })
        .collect::<Vec<_>>();
    expected.push("~owner".into());
    expected.sort();
    assert_eq!(premise.fields.as_ref().unwrap(), &expected, "{under:?}");
}
