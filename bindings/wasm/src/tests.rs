use kition_kit::KitDocument;
use serde_json::{Value, json};

use crate::{MODEL_SCHEMA, READ_METHODS, TableDocument, VACUUM_FREE_PERCENT, known_method};

fn new_document() -> TableDocument {
    TableDocument::create("Tasks.ktable", 0, 0).unwrap()
}

/// The first table's id and its primary field's name.
fn first_table(doc: &mut TableDocument) -> (i64, String) {
    let document = doc.call("documents.get", &Value::Null).unwrap()["result"].clone();
    let table = &document["tables"][0];
    let primary = table["primary_field_id"].as_i64().unwrap();
    let field = table["fields"].as_array().unwrap().iter().find(|field| field["id"].as_i64() == Some(primary)).unwrap();
    (table["id"].as_i64().unwrap(), field["name"].as_str().unwrap().to_owned())
}

fn add_record(doc: &mut TableDocument, title: &str) -> Value {
    let (table_id, field) = first_table(doc);
    doc.execute("record.add", json!({ "tableId": table_id, "values": { field: title } })).unwrap()
}

fn titles(doc: &mut TableDocument) -> Vec<String> {
    let (table_id, field) = first_table(doc);
    let page = doc.call("records.list", &json!({ "tableId": table_id })).unwrap();
    page["result"]["items"].as_array().unwrap().iter().map(|record| record["values"][&field].as_str().unwrap_or_default().to_owned()).collect()
}

#[test]
fn keeps_the_kit_sdk_contract() {
    let fixture = new_document().saved_bytes().unwrap();
    kition_kit::testing::check_contract::<TableDocument>(&fixture, "Tasks.ktable", |doc| {
        add_record(doc, "Write the plan");
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_new_document_has_the_runtime_starter_table() {
    let mut doc = new_document();
    let document = doc.call("documents.get", &Value::Null).unwrap()["result"].clone();
    assert_eq!(document["id"], 1);
    assert_eq!(document["title"], "Tasks");
    let table = &document["tables"][0];
    assert_eq!(table["fields"].as_array().unwrap().len(), 3);
    assert_eq!(table["views"].as_array().unwrap().len(), 1);
    assert!(!doc.dirty());
}

#[test]
fn writes_answer_their_change_and_reads_none() {
    let mut doc = new_document();
    let (table_id, _) = first_table(&mut doc);
    let revision = doc.revision();
    let read = doc.call("records.list", &json!({ "tableId": table_id })).unwrap();
    assert_eq!(read["change"], Value::Null);
    assert_eq!(doc.revision(), revision);
    assert!(!doc.dirty());

    let written = add_record(&mut doc, "Ada");
    assert_eq!(written["result"]["values"].as_object().unwrap().values().next().unwrap(), "Ada");
    let change = &written["change"];
    assert_eq!(change["documentId"], "1");
    assert_eq!(change["origin"], "person:kit");
    assert_eq!(change["recordsUpserted"].as_array().unwrap().len(), 1);
    assert!(doc.revision() > revision);
    assert!(doc.dirty());
}

#[test]
fn undo_and_redo_are_the_edit_commands() {
    let mut doc = new_document();
    add_record(&mut doc, "Ada");
    assert_eq!(titles(&mut doc), ["Ada"]);
    assert!(doc.inspect().unwrap()["canUndo"].as_bool().unwrap());
    let undone = doc.execute("edit.undo", json!({})).unwrap();
    assert_eq!(undone["result"]["done"], true);
    assert!(titles(&mut doc).is_empty());
    doc.execute("edit.redo", json!({})).unwrap();
    assert_eq!(titles(&mut doc), ["Ada"]);
}

#[test]
fn unknown_methods_and_commands_fail() {
    let mut doc = new_document();
    assert!(doc.call("records.nothing", &Value::Null).is_err());
    assert!(doc.execute("table.records.nothing", json!({})).is_err());
    assert!(doc.execute("records.list", json!({})).is_err());
}

#[test]
fn every_read_method_is_known_and_its_command_does_not_mutate() {
    let doc = new_document();
    let commands = doc.commands();
    for method in READ_METHODS {
        assert!(known_method(method), "{method}");
        let command = commands.iter().find(|command| command.id == format!("table.{method}")).unwrap();
        assert!(!command.mutates, "{method}");
    }
    let create = commands.iter().find(|command| command.id == "record.add").unwrap();
    assert!(create.mutates);
}

#[test]
fn save_leaves_edits_after_it_unsaved() {
    let mut doc = new_document();
    add_record(&mut doc, "Ada");
    doc.saved_bytes().unwrap();
    add_record(&mut doc, "Grace");
    doc.mark_saved();
    assert!(doc.dirty(), "an edit after the save is not in the file");
    doc.saved_bytes().unwrap();
    doc.mark_saved();
    assert!(!doc.dirty());
}

/// The calls the table editor makes as it opens a table: every read, and the view config sent
/// back as it was read.
fn open_like_the_editor(doc: &mut TableDocument) {
    let document = doc.call("documents.get", &Value::Null).unwrap()["result"].clone();
    doc.call("historyState", &Value::Null).unwrap();
    doc.call("dashboards.list", &Value::Null).unwrap();
    for table in document["tables"].as_array().unwrap() {
        let table_id = &table["id"];
        doc.call("records.list", &json!({ "tableId": table_id, "limit": 200, "offset": 0 })).unwrap();
        for view in table["views"].as_array().unwrap() {
            let view_id = &view["id"];
            doc.call("views.listFields", &json!({ "tableId": table_id, "viewId": view_id })).unwrap();
            doc.call("records.list", &json!({ "tableId": table_id, "viewId": view_id })).unwrap();
            doc.call("views.update", &json!({ "tableId": table_id, "viewId": view_id, "config": view["config"] })).unwrap();
        }
    }
}

#[test]
fn opening_a_file_and_reading_it_leaves_it_saved() {
    let bytes = new_document().saved_bytes().unwrap();
    let mut doc = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    assert!(!doc.dirty());
    let revision = doc.revision();
    open_like_the_editor(&mut doc);
    assert_eq!(doc.revision(), revision);
    assert!(!doc.dirty());
    assert!(!doc.can_undo(), "opening is no undo step");
}

#[test]
fn undo_back_to_the_saved_state_makes_the_document_saved() {
    let bytes = new_document().saved_bytes().unwrap();
    let mut doc = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    add_record(&mut doc, "Ada");
    assert!(doc.dirty());
    doc.execute("edit.undo", json!({})).unwrap();
    assert!(!doc.dirty(), "undone back to the file");
    doc.execute("edit.redo", json!({})).unwrap();
    assert!(doc.dirty());
    add_record(&mut doc, "Grace");
    doc.execute("edit.undo", json!({})).unwrap();
    assert!(doc.dirty());
    doc.execute("edit.undo", json!({})).unwrap();
    assert!(!doc.dirty());

    // Saved after an edit: undo leaves the file's state, and redo comes back to it.
    add_record(&mut doc, "Ada");
    doc.saved_bytes().unwrap();
    doc.mark_saved();
    doc.execute("edit.undo", json!({})).unwrap();
    assert!(doc.dirty());
    doc.execute("edit.redo", json!({})).unwrap();
    assert!(!doc.dirty());

    // An edit after an undo replaces what redo would have brought back.
    doc.execute("edit.undo", json!({})).unwrap();
    add_record(&mut doc, "Grace");
    assert!(doc.dirty());
    let redone = doc.execute("edit.redo", json!({})).unwrap();
    assert_eq!(redone["result"]["done"], false);
    assert!(doc.dirty());
}

#[test]
fn a_request_is_one_undo_step() {
    let mut doc = new_document();
    let before = doc.call("documents.get", &Value::Null).unwrap()["result"]["tables"].as_array().unwrap().len();
    doc.call("tables.create", &json!({ "title": "Second" })).unwrap();
    assert_eq!(doc.call("documents.get", &Value::Null).unwrap()["result"]["tables"].as_array().unwrap().len(), before + 1);
    assert!(doc.dirty());
    doc.execute("edit.undo", json!({})).unwrap();
    assert_eq!(doc.call("documents.get", &Value::Null).unwrap()["result"]["tables"].as_array().unwrap().len(), before);
    assert!(!doc.dirty());
    assert!(!doc.can_undo());
}

#[test]
fn a_change_undo_does_not_revert_keeps_the_document_unsaved() {
    let bytes = new_document().saved_bytes().unwrap();
    let mut right = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    // Another replica on the same line of changes (the KDM takes changes in order).
    let mut left = TableDocument::open_with_replica("Tasks.ktable", &[], &right.snapshot().unwrap(), &[]).unwrap();
    add_record(&mut right, "Mine");
    left.import_changes(&right.export_changes(&left.version()).unwrap()).unwrap();
    add_record(&mut left, "Theirs");
    right.import_changes(&left.export_changes(&right.version()).unwrap()).unwrap();
    assert!(right.dirty());
    right.execute("edit.undo", json!({})).unwrap();
    assert_eq!(titles(&mut right), ["Theirs"]);
    assert!(right.dirty(), "the imported change is not in the file");
    right.saved_bytes().unwrap();
    right.mark_saved();
    assert!(!right.dirty());

    // A system write is no undo step either.
    let mut doc = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    add_record(&mut doc, "Ada");
    let (table_id, field) = first_table(&mut doc);
    doc.execute_as("record.add", json!({ "tableId": table_id, "values": { field: "Repair" } }), &kition_kdm::Origin::system("repair")).unwrap();
    doc.execute("edit.undo", json!({})).unwrap();
    assert_eq!(titles(&mut doc), ["Repair"]);
    assert!(doc.dirty());
}

#[test]
fn saved_bytes_reopen_with_the_edits_and_the_same_replica() {
    let mut doc = new_document();
    add_record(&mut doc, "Ada");
    let bytes = doc.saved_bytes().unwrap();
    let mut reopened = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    assert_eq!(titles(&mut reopened), ["Ada"]);
    assert_eq!(reopened.doc_id(), doc.doc_id());
    // The file holds the content, not the version: the reopened replica holds the same objects
    // under the same ids, on a line of changes of its own.
    let state = |doc: &TableDocument| kition_kit::session::state(doc.file().replica()).unwrap();
    assert_eq!(state(&reopened), state(&doc), "the same objects, ids and values");
}

#[test]
fn two_replicas_on_one_line_exchange_changes() {
    let fixture = new_document().saved_bytes().unwrap();
    let mut left = TableDocument::open("Tasks.ktable", &fixture).unwrap();
    // Another replica of the document on the same line of changes (the KDM takes changes in order).
    let mut right = TableDocument::open_with_replica("Tasks.ktable", &[], &left.snapshot().unwrap(), &[]).unwrap();
    add_record(&mut left, "Left");
    let to_right = left.export_changes(&right.version()).unwrap();
    assert_eq!(right.import_changes(&to_right).unwrap()["kind"], "full");
    // Imported again, nothing changes.
    assert_eq!(right.import_changes(&to_right).unwrap()["kind"], "none");
    add_record(&mut right, "Right");
    let to_left = right.export_changes(&left.version()).unwrap();
    left.import_changes(&to_left).unwrap();

    let mut left_titles = titles(&mut left);
    let mut right_titles = titles(&mut right);
    left_titles.sort();
    right_titles.sort();
    assert_eq!(left_titles, ["Left", "Right"]);
    assert_eq!(left_titles, right_titles);

    let imported = right.take_imported();
    assert_eq!(imported.len(), 1);
    assert_eq!(imported[0]["origin"], "import:sync");
    assert_eq!(imported[0]["documentPath"], "Tasks.ktable");
    assert_eq!(imported[0]["recordsUpserted"].as_array().unwrap().len(), 1);
    assert!(right.take_imported().is_empty());

    // Another replica's change is no undo step of this person's.
    right.execute("edit.undo", json!({})).unwrap();
    assert_eq!(titles(&mut right), ["Left"]);
}

#[test]
fn csv_export_answers_its_content() {
    let mut doc = new_document();
    add_record(&mut doc, "Ada");
    let (table_id, _) = first_table(&mut doc);
    let exported = doc.call("csv.export", &json!({ "tableId": table_id })).unwrap();
    assert!(exported["result"]["content"].as_str().unwrap().contains("Ada"));
}

#[test]
fn bytes_that_are_no_table_fail_to_open() {
    assert!(TableDocument::open("Tasks.ktable", b"not a table").is_err());
    assert!(TableDocument::open("Tasks.ktable", b"").is_err());
}

#[test]
fn the_method_list_names_every_method_with_its_route() {
    let methods: Vec<(String, String)> = serde_json::from_str(&crate::methods_json()).unwrap();
    assert!(methods.iter().any(|(method, route)| method == "records.list" && route == "GET /tables/{tableId}/records"));
    assert!(methods.iter().all(|(method, _)| known_method(method)));
}

fn free_pages(doc: &TableDocument) -> (i64, i64) {
    let conn = doc.file().conn();
    let pages: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0)).unwrap();
    let free: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0)).unwrap();
    (pages, free)
}

#[test]
fn save_vacuums_a_sparse_database_and_keeps_every_edit() {
    let mut doc = new_document();
    let row = add_record(&mut doc, "Start");
    let (table_id, field) = first_table(&mut doc);
    let key = row["result"]["row_key"].as_str().unwrap().to_owned();
    // Single edits until compacting the change log has freed enough pages (its rows and the old
    // snapshot).
    let mut last = String::new();
    for round in 0..2000 {
        last = format!("Edit {round}");
        let values = json!({ field.clone(): last.clone() });
        doc.call("transact", &json!({ "ops": [{ "op": "cells.set", "tableId": table_id, "records": [{ "rowKey": key, "values": values }] }] }))
            .unwrap();
        let (pages, free) = free_pages(&doc);
        if free * 100 > pages * VACUUM_FREE_PERCENT {
            break;
        }
    }
    let (pages, free) = free_pages(&doc);
    assert!(free * 100 > pages * VACUUM_FREE_PERCENT, "{free} of {pages} pages free");
    let bytes = doc.saved_bytes().unwrap();
    let (vacuumed, free) = free_pages(&doc);
    assert_eq!(free, 0);
    assert!(vacuumed < pages);
    // The file holds the content of the vacuumed session's database, and no free page either.
    let page_size = doc.file().conn().query_row("PRAGMA page_size", [], |row| row.get::<_, i64>(0)).unwrap();
    assert!(bytes.len() as i64 <= vacuumed * page_size && bytes.len() as i64 % page_size == 0, "{} bytes", bytes.len());
    let saved = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    assert_eq!(free_pages(&saved).1, 0, "the file has no free page");
    let mut reopened = TableDocument::open("Tasks.ktable", &bytes).unwrap();
    assert_eq!(titles(&mut reopened), vec![last.clone()]);
    assert_eq!(reopened.doc_id(), doc.doc_id());
    // The vacuumed document takes edits, undo included, as before.
    add_record(&mut doc, "After");
    assert_eq!(titles(&mut doc), vec![last.clone(), "After".to_owned()]);
    doc.execute("edit.undo", Value::Null).unwrap();
    assert_eq!(titles(&mut doc), vec![last]);
}

#[test]
fn save_leaves_a_dense_database_as_it_is() {
    let mut doc = new_document();
    add_record(&mut doc, "One");
    let (pages, free) = free_pages(&doc);
    assert!(free * 100 <= pages * VACUUM_FREE_PERCENT);
    doc.saved_bytes().unwrap();
    assert_eq!(free_pages(&doc), (pages, free));
}

#[test]
fn the_model_schema_is_the_one_the_engine_embeds() {
    let schema = kition_kdm::Schema::parse(MODEL_SCHEMA).unwrap();
    assert_eq!(schema, *kition_table_file::kdm::schema());
    assert_eq!(schema.kit, "kit.table");
    assert_eq!(schema.root, "Base");
}

#[test]
fn execute_as_records_the_given_origin_and_keeps_the_editors() {
    let mut doc = new_document();
    let (table_id, field) = first_table(&mut doc);
    let origin = kition_kdm::Origin::agent("agent-1");
    let answer = doc.execute_as("record.add", json!({ "tableId": table_id, "values": { field: "From the agent" } }), &origin).unwrap();
    assert_eq!(answer["change"]["origin"], "agent:agent-1");
    let mine = add_record(&mut doc, "Mine");
    assert_eq!(mine["change"]["origin"], "person:kit");
}

#[test]
fn command_params_name_no_files() {
    // As kition-headless reads a command's documented params: such a command is not listed.
    const FILE_WORDS: &[&str] = &["path", "paths", "folder", "dir", "directory", "output", "outputdir", "file", "files", "target"];
    for command in new_document().commands() {
        let params = command.params.unwrap_or_default().to_lowercase();
        for word in params.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            assert!(!FILE_WORDS.contains(&word), "{}: {word}", command.id);
        }
    }
}

/// A picture's bytes: a PNG signature and some content.
fn picture(seed: u8) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend((0..256u32).map(|index| (index as u8).wrapping_mul(seed)));
    bytes
}

fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn unbase64(text: &Value) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(text.as_str().unwrap()).unwrap()
}

/// Adds an attachment field "Files" to the first table; answers the table id and its name.
fn files_field(doc: &mut TableDocument) -> (i64, String) {
    let (table_id, _) = first_table(doc);
    let field = doc.call("fields.create", &json!({ "tableId": table_id, "title": "Files", "type": "attachment" })).unwrap();
    (table_id, field["result"]["name"].as_str().unwrap().to_owned())
}

/// Stores `bytes` with `attachments.put` and puts the answer into the first record's `field`.
fn attach(doc: &mut TableDocument, table_id: i64, field: &str, bytes: &[u8]) -> Value {
    let attachment = doc
        .call("attachments.put", &json!({ "tableId": table_id, "name": "beach.png", "mimeType": "image/png", "data": base64(bytes) }))
        .unwrap()["result"]
        .clone();
    let records = doc.call("records.list", &json!({ "tableId": table_id })).unwrap();
    let record_id = records["result"]["items"][0]["id"].as_i64().unwrap();
    doc.call("records.update", &json!({ "tableId": table_id, "recordId": record_id, "values": { field: [attachment.clone()] } })).unwrap();
    attachment
}

#[test]
fn attachments_are_put_into_the_file_and_read_back() {
    let mut doc = new_document();
    add_record(&mut doc, "Beach");
    let (table_id, field) = files_field(&mut doc);
    let bytes = picture(3);
    let attachment = attach(&mut doc, table_id, &field, &bytes);
    let hash = kition_table_file::blobs::blob_hash(&bytes);
    assert_eq!(attachment["url"], format!("kition-blob:{hash}"));
    assert_eq!(attachment["name"], "beach.png");
    assert_eq!(attachment["mimeType"], "image/png");
    assert_eq!(attachment["sizeBytes"], bytes.len());

    // Reading changes nothing, and works in a read-only session.
    let revision = doc.revision();
    let read = doc.call("attachments.read", &json!({ "hash": hash })).unwrap();
    assert_eq!(read["change"], Value::Null);
    assert_eq!(doc.revision(), revision);
    assert_eq!(unbase64(&read["result"]["data"]), bytes);
    assert_eq!(read["result"]["mimeType"], "image/png");
    assert!(READ_METHODS.contains(&"attachments.read"));
    assert!(!READ_METHODS.contains(&"attachments.put"));
    assert!(doc.call("attachments.read", &json!({ "hash": kition_table_file::blobs::blob_hash(b"none") })).is_err());
    assert!(doc.call("attachments.put", &json!({ "tableId": table_id, "name": "x", "data": "not base64!" })).is_err());

    // Saved and reopened (as another application would), the attachment and its bytes are there.
    let saved = doc.saved_bytes().unwrap();
    let mut reopened = TableDocument::open("Tasks.ktable", &saved).unwrap();
    let records = reopened.call("records.list", &json!({ "tableId": table_id })).unwrap();
    assert_eq!(records["result"]["items"][0]["values"][&field], json!([attachment]));
    let read = reopened.call("attachments.read", &json!({ "hash": hash })).unwrap();
    assert_eq!(unbase64(&read["result"]["data"]), bytes);
}

#[test]
fn two_copies_sync_a_blob_attachment() {
    let mut first = new_document();
    add_record(&mut first, "Beach");
    let (table_id, field) = files_field(&mut first);
    let fixture = first.saved_bytes().unwrap();
    let mut left = TableDocument::open("Tasks.ktable", &fixture).unwrap();
    let mut right = TableDocument::open("Tasks.ktable", &fixture).unwrap();
    assert!(left.blobs().unwrap().is_empty());

    let bytes = picture(5);
    let attachment = attach(&mut left, table_id, &field, &bytes);
    let since = right.version();
    let changes = left.export_changes(&since).unwrap();
    let blobs = left.blobs_since(&since).unwrap();
    let hash = kition_table_file::blobs::blob_hash(&bytes);
    assert_eq!(kition_table_file::blobs::decode_blob_section(&blobs).unwrap(), vec![(hash.clone(), bytes.clone())]);
    assert_eq!(left.blobs().unwrap(), blobs);

    // The relay adds the blobs before the changes.
    let revision = right.revision();
    assert_eq!(right.add_blobs(&blobs).unwrap(), 1);
    assert_eq!(right.revision(), revision, "blobs alone are no edit");
    assert_eq!(right.import_changes(&changes).unwrap()["kind"], "full");
    let records = right.call("records.list", &json!({ "tableId": table_id })).unwrap();
    assert_eq!(records["result"]["items"][0]["values"][&field], json!([attachment]));
    let read = right.call("attachments.read", &json!({ "hash": hash })).unwrap();
    assert_eq!(unbase64(&read["result"]["data"]), bytes);
    // Nothing new since: no blobs to send.
    assert!(left.blobs_since(&right.version()).unwrap().is_empty());
}

#[test]
fn inspect_lists_the_tables_and_dashboards_a_view_can_name() {
    let mut doc = new_document();
    let (table_id, _) = first_table(&mut doc);
    doc.execute("dashboard.set", json!({ "dashboardId": "overview", "title": "Overview", "source_table_id": table_id, "layout": [], "widgets": [] }))
        .unwrap();
    let inspected = doc.inspect().unwrap();
    assert_eq!(inspected["tables"][0]["id"], table_id);
    let dashboards = inspected["dashboards"].as_array().unwrap();
    assert_eq!(dashboards.len(), 1);
    assert_eq!(dashboards[0]["title"], "Overview");
    let listed = doc.call("dashboards.list", &Value::Null).unwrap()["result"].clone();
    assert_eq!(dashboards[0]["id"], listed["items"][0]["id"], "the id the editor opens it by: {listed}");
}

/// Engine contract 2: a peer's snapshot opens as the same document with no file (empty bytes),
/// and its blobs come with it.
#[test]
fn a_snapshot_opens_as_the_same_document_without_a_file() {
    let mut a = new_document();
    add_record(&mut a, "Shared row");
    let snapshot = a.snapshot().unwrap();
    let mut b = TableDocument::open_with_replica("Shared.ktable", &[], &snapshot, &a.blobs().unwrap()).unwrap();
    assert_eq!(b.doc_id(), a.doc_id(), "the same KDM document");
    assert_eq!(titles(&mut b), titles(&mut a));
    // From then on the two exchange changes.
    add_record(&mut b, "From b");
    let changes = b.export_changes(&a.version()).unwrap();
    a.import_changes(&changes).unwrap();
    assert_eq!(titles(&mut a), titles(&mut b));
}

/// A single-user host stores the replica it saved with the file: the file already holds it, so
/// the document opens as the file, saved.
#[test]
fn file_bytes_open_saved_with_the_replica_saved_with_them() {
    let mut a = new_document();
    add_record(&mut a, "In the file");
    let bytes = a.saved_bytes().unwrap();
    let snapshot = a.snapshot().unwrap();
    let mut b = TableDocument::open_with_replica("Tasks.ktable", &bytes, &snapshot, &a.blobs().unwrap()).unwrap();
    assert_eq!(b.doc_id(), a.doc_id());
    assert_eq!(b.version(), a.version());
    assert_eq!(titles(&mut b), ["In the file"]);
    assert!(!b.dirty(), "nothing beyond the file");
    assert!(!b.can_undo());
}

/// A `.ktable` holds its replica: with file bytes, a stored snapshot of another document is not
/// the file's, and the file opens as `open` opens it.
#[test]
fn file_bytes_open_with_their_own_replica() {
    let mut a = new_document();
    add_record(&mut a, "In the file");
    let bytes = a.saved_bytes().unwrap();
    let mut other = new_document();
    let mut b = TableDocument::open_with_replica("Tasks.ktable", &bytes, &other.snapshot().unwrap(), &[]).unwrap();
    assert_eq!(b.doc_id(), a.doc_id());
    assert_eq!(titles(&mut b), vec!["In the file".to_owned()]);
    assert_ne!(b.doc_id(), other.doc_id());
    let _ = titles(&mut other);
    assert!(!b.dirty());
}

/// A stored replica that is not one (damaged storage) is ignored: the file opens as it is.
#[test]
fn a_damaged_snapshot_leaves_the_file_as_it_is() {
    let mut a = new_document();
    add_record(&mut a, "In the file");
    let bytes = a.saved_bytes().unwrap();
    let mut b = TableDocument::open_with_replica("Tasks.ktable", &bytes, b"not a replica", &[]).unwrap();
    assert_eq!(b.doc_id(), a.doc_id());
    assert_eq!(titles(&mut b), ["In the file"]);
    assert!(!b.dirty());
}

/// Contract 2's `undo` and `redo` take back the person's step and say whether they did.
#[test]
fn undo_and_redo_answer_done_and_damage() {
    let mut doc = new_document();
    let (done, _) = doc.undo().unwrap();
    assert!(!done, "nothing to undo yet");
    add_record(&mut doc, "Step");
    let (done, damage) = doc.undo().unwrap();
    assert!(done);
    assert_eq!(damage, json!({ "kind": "full" }));
    assert!(titles(&mut doc).is_empty());
    let (done, _) = doc.redo().unwrap();
    assert!(done);
    assert_eq!(titles(&mut doc), vec!["Step".to_owned()]);
}
