//! `table.file.render`, the agent's preview (`agent.render` in kit.json), by explicit view: a
//! function of the file and the call alone. It never reads or moves what the document's picture
//! shows (`table.preview.show`), the same call answers the same bytes, also after a save and a
//! reopen, and a table or view is named by its id.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use kition_kit::KitDocument;
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

fn result(doc: &mut TableDocument, command: &str, params: Value) -> Value {
    doc.execute(command, params).unwrap_or_else(|error| panic!("{command}: {error}"))["result"].clone()
}

/// Table 1 (the starter: two records) and table 2 `Tasks` (three records, views 2 `All` and 3
/// `Open`, which keeps the records not done).
fn document() -> TableDocument {
    let mut doc = TableDocument::create("Work.ktable", 0, 0).unwrap();
    for title in ["Write the plan", "Review the plan"] {
        result(&mut doc, "table.records.create", json!({ "tableId": 1, "values": { "title": title } }));
    }
    result(
        &mut doc,
        "table.tables.create",
        json!({ "title": "Tasks", "name": "tasks", "fields": [
            { "title": "Title", "name": "title", "type": "text", "primary": true },
            { "title": "Done", "name": "done", "type": "checkbox" },
        ] }),
    );
    for (title, done) in [("Book", true), ("Pack", false), ("Go", false)] {
        result(&mut doc, "table.records.create", json!({ "tableId": 2, "values": { "title": title, "done": done } }));
    }
    result(&mut doc, "table.views.create", json!({ "tableId": 2, "title": "Open", "type": "grid" }));
    result(
        &mut doc,
        "table.views.putFilterTree",
        json!({ "tableId": 2, "viewId": 3, "tree": { "conjunction": "and", "children": [{ "condition": { "field_id": 5, "operator": "neq", "value": true } }] } }),
    );
    doc
}

fn render(doc: &mut TableDocument, params: Value) -> Value {
    doc.execute("table.file.render", params.clone()).unwrap_or_else(|error| panic!("{params}: {error}"))
}

#[test]
fn it_never_reads_or_moves_what_the_picture_shows_and_answers_the_same_bytes() {
    let mut doc = document();
    let default = render(&mut doc, json!({}));
    assert_eq!((default["tableId"].clone(), default["viewId"].clone()), (json!(1), json!(1)), "the first table's first view: {default:?}");
    let call = json!({ "target": { "table": 2 }, "scale": 1 });
    let first = render(&mut doc, call.clone());
    let (revision, dirty, state) = (doc.revision(), doc.dirty(), doc.editor_state());
    assert_eq!(render(&mut doc, call.clone()), first, "the same call answers the same bytes");
    assert_eq!((doc.revision(), doc.dirty(), doc.editor_state()), (revision, dirty, state), "rendering changed the file or the editor");
    // The picture shows table 2's Open view now: neither call's picture changes.
    doc.execute("table.preview.show", json!({ "viewId": 3 })).unwrap();
    assert_eq!(render(&mut doc, call.clone()), first, "what the picture shows changed a named view");
    assert_eq!(render(&mut doc, json!({})), default, "what the picture shows changed the default view");
    // Saved and opened again (another file's timestamps, another replica actor): the same bytes.
    let mut reopened = TableDocument::open("Work.ktable", &doc.save(None).unwrap()).unwrap();
    assert_eq!(render(&mut reopened, call), first, "a reopened file draws the same bytes");
}

#[test]
fn a_table_or_a_view_is_named_by_its_id() {
    let mut doc = document();
    let table = render(&mut doc, json!({ "target": { "table": 2 } }));
    assert_eq!((table["tableId"].clone(), table["viewId"].clone()), (json!(2), json!(2)), "a table shows its first view");
    let open = render(&mut doc, json!({ "target": { "view": 3 } }));
    assert_eq!((open["tableId"].clone(), open["viewId"].clone()), (json!(2), json!(3)), "a view names its table");
    assert_ne!(open["png"], table["png"], "the Open view hides a record");
    assert!(open["height"].as_u64() < table["height"].as_u64(), "{open:?}");
    assert_eq!(render(&mut doc, json!({ "target": { "table": 2, "view": 3 } }))["png"], open["png"]);
    assert_ne!(render(&mut doc, json!({ "target": { "table": 1 } }))["png"], table["png"]);
    let size = |answer: &Value| (answer["width"].as_u64().unwrap(), answer["height"].as_u64().unwrap());
    let small = render(&mut doc, json!({ "target": { "table": 2 }, "scale": 2, "maxSize": 100 }));
    assert!(size(&small).0.max(size(&small).1) <= 100, "{small:?}");
    let two = render(&mut doc, json!({ "target": { "table": 2 }, "scale": 2 }));
    assert_eq!(size(&two), (size(&table).0 * 2, size(&table).1 * 2));
}

#[test]
fn a_table_or_view_the_file_lacks_is_refused_naming_it() {
    let mut doc = document();
    let before = (doc.revision(), doc.editor_state());
    for (target, says) in [
        (json!({ "table": 9 }), "not found: table 9"),
        (json!({ "view": 9 }), "not found: view 9"),
        (json!({ "table": 1, "view": 3 }), "view 3 is of table 2"),
        (json!({ "table": "Tasks" }), "is named by its id"),
        (json!({ "table": 2, "path": "tables/1" }), "either"),
    ] {
        let error = doc.execute("table.file.render", json!({ "target": target })).unwrap_err();
        assert!(error.contains(says), "{target}: {error}");
    }
    assert_eq!((doc.revision(), doc.editor_state()), before, "a refused render changed nothing");
}
