//! Receipts and public reads must describe the saved document.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use kition_kit::{KitDocument, session::model::Exchange};
use kition_table_kit::TableDocument;
use serde_json::json;

fn agrees(doc: &mut TableDocument, label: &str) {
    let again = TableDocument::open("Saved.ktable", &doc.saved_bytes().unwrap()).unwrap();
    assert_eq!(doc.file().replica().state_json().unwrap(), again.file().replica().state_json().unwrap(), "{label}: replica");
    assert_eq!(
        serde_json::to_value(doc.derived(&[], &[]).unwrap()).unwrap(),
        serde_json::to_value(again.derived(&[], &[]).unwrap()).unwrap(),
        "{label}: derived"
    );
    let mut before = doc.inspect().unwrap();
    let mut after = again.inspect().unwrap();
    for value in [&mut before, &mut after] {
        value.as_object_mut().unwrap().remove("canUndo");
        value.as_object_mut().unwrap().remove("canRedo");
    }
    assert_eq!(before, after, "{label}: public inspection");
}

#[test]
fn representative_commands_on_every_conformance_file_reopen_as_told() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
    let mut files: Vec<_> =
        std::fs::read_dir(root).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "ktable")).collect();
    files.sort();
    let mut writes = 0;
    for path in &files {
        let mut doc = TableDocument::open("Saved.ktable", &std::fs::read(path).unwrap()).unwrap();
        agrees(&mut doc, "open");
        let table = doc.file().list_tables().unwrap().remove(0);
        let table = doc.file().get_table(table.id).unwrap();
        let view = table.views[0].id;
        let field = table.fields[0].id;
        for (command, params) in [
            ("table.tables.update", json!({"tableId":table.id,"title":"Saved title"})),
            ("table.fields.update", json!({"tableId":table.id,"fieldId":field,"title":"Saved field"})),
            ("table.views.patchConfig", json!({"tableId":table.id,"viewId":view,"row_height":"tall"})),
            ("table.views.patchField", json!({"tableId":table.id,"viewId":view,"fieldId":field,"width":5000,"visible":false})),
            ("table.fields.create", json!({"tableId":table.id,"title":"Saved addition","order":2})),
        ] {
            doc.execute(command, params).unwrap();
            agrees(&mut doc, command);
            writes += 1;
        }
    }
    println!("saved invariant: {} fixture files, {writes} writes", files.len());
    assert_eq!(files.len(), 10);
}

#[test]
fn fractional_model_width_is_the_integer_width_the_api_and_file_hold() {
    let mut doc = TableDocument::create("Saved.ktable", 0, 0).unwrap();
    let view = doc.file().replica().objects_of_type("View")[0].clone();
    let field = doc.file().replica().objects_of_type("Field")[0].clone();
    let field_id: i64 = doc.file().conn().query_row("SELECT id FROM kition_fields WHERE kdm_id = ?1", [field.as_str()], |r| r.get(0)).unwrap();
    doc.execute("table.views.patchField", json!({"tableId":1,"viewId":1,"fieldId":field_id,"width":200})).unwrap();
    let answer = Exchange::new()
        .batch(
            &mut doc,
            &json!({"ops":[
        {"op":"map.set","target":view.as_str(),"field":"widths","key":field.as_str(),"value":333.4}
    ],"load":[{"target":view.as_str(),"fields":["widths"]}]}),
            false,
        )
        .unwrap();
    let saved = TableDocument::open("Saved.ktable", &doc.saved_bytes().unwrap()).unwrap();
    let fields = saved.file().list_view_fields(1).unwrap();
    assert_eq!(answer["loaded"][0]["fields"]["widths"][field.as_str()], json!(fields.iter().find(|f| f.field_id == field_id).unwrap().width));
    assert_eq!(answer["loaded"][0]["fields"]["widths"][field.as_str()], 333);
    agrees(&mut doc, "fractional width");
}

#[test]
fn column_layout_changes_are_in_the_replica_receipt() {
    let mut doc = TableDocument::create("Saved.ktable", 0, 0).unwrap();
    let before = doc.version();
    doc.execute("table.views.patchField", json!({"tableId":1,"viewId":1,"fieldId":1,"position":2,"frozen":true})).unwrap();
    assert_ne!(doc.version(), before, "a public document write must have a replica change");
    let mut again = TableDocument::open("Saved.ktable", &doc.saved_bytes().unwrap()).unwrap();
    let fields = again.execute("table.views.listFields", json!({"tableId":1,"viewId":1})).unwrap();
    assert_eq!(fields["result"]["items"][0]["position"], 2);
    assert_eq!(fields["result"]["items"][0]["frozen"], true);
    doc.execute("edit.undo", json!({})).unwrap();
    let fields = doc.execute("table.views.listFields", json!({"tableId":1,"viewId":1})).unwrap();
    assert_eq!(fields["result"]["items"][0]["position"], 0, "undo removes the layout from the projection");
    assert_eq!(fields["result"]["items"][0]["frozen"], false);
}
