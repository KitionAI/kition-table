//! `document.identity` and `document.fork` (Kit Process Protocol section 5.6, KitModel M7a)
//! through the real `kition-table` process: a `.ktable` embeds its replica (`kition_kdm`'s
//! snapshot and the change rows after it), so a byte copy of a file is the same document. A host
//! reads the identity a file carries without opening it, and forks a file into a new document: a
//! new docId (in the replica and in the file's `document_id` and recorded docId), the same tables,
//! fields, views and records with their object ids and values, the attachments its records name,
//! and no change rows, origin log, record history, deleted rows or other history.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use kition_kit::KitDocument;
use kition_kit_host::{ClientError, HeadlessKit, LaunchOptions};
use kition_table_kit::TableDocument;
use serde_json::{Value, json};

const LIMIT: Duration = Duration::from_secs(30);

/// Text of a record deleted before the fork: in the source's bytes (its history and its deleted
/// row), not the fork's.
const SECRET: &str = "SECRET-DELETED-BEFORE-THE-FORK";

fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kition-table-identity-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

async fn launch(dir: &Path) -> HeadlessKit {
    let mut options = LaunchOptions::new(env!("CARGO_BIN_EXE_kition-table"));
    options.workspace = Some(dir.to_path_buf());
    HeadlessKit::launch(options).await.unwrap()
}

fn put(doc: &mut TableDocument, name: &str, bytes: &[u8]) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    doc.execute("table.attachments.put", json!({ "tableId": 1, "name": name, "mimeType": "text/plain", "data": data })).unwrap()["result"].clone()
}

/// The hash of the blob an attachment value names.
fn hash(attachment: &Value) -> String {
    attachment["url"].as_str().unwrap().trim_start_matches("kition-blob:").to_owned()
}

/// A table saved twice: first with two records and the secret one, each with an attachment (the
/// stored snapshot), then with the secret record deleted (change rows after it, a deleted row,
/// and record history).
fn table() -> (Vec<u8>, String, String) {
    let mut doc = TableDocument::create("Plan.ktable", 0, 0).unwrap();
    doc.execute("table.fields.create", json!({ "tableId": 1, "name": "files", "title": "Files", "type": "attachment" })).unwrap();
    let kept = put(&mut doc, "kept.txt", b"an attachment a record keeps");
    let gone = put(&mut doc, "gone.txt", b"an attachment only the deleted record named");
    doc.execute("table.records.create", json!({ "tableId": 1, "values": { "title": "Kept", "files": [kept] } })).unwrap();
    doc.execute("table.records.create", json!({ "tableId": 1, "values": { "title": "Also kept" } })).unwrap();
    let made = doc.execute("table.records.create", json!({ "tableId": 1, "values": { "title": SECRET, "files": [gone] } })).unwrap();
    let secret = made["result"]["id"].as_i64().unwrap_or_else(|| panic!("a record id: {made}"));
    let first = doc.save(None).unwrap();
    let mut doc = TableDocument::open("Plan.ktable", &first).unwrap();
    doc.execute("table.records.delete", json!({ "tableId": 1, "recordId": secret })).unwrap();
    (doc.save(None).unwrap(), hash(&kept), hash(&gone))
}

/// Every object of the table with its fields, by id: what must be equal in a fork.
async fn objects(kit: &HeadlessKit, path: &str) -> Value {
    let state = kit.open(path, false, LIMIT).await.unwrap();
    let document = state["documentId"].clone();
    let batch = |request: Value| {
        let mut request = request;
        request["documentId"] = document.clone();
        kit.request("model.batch", request, LIMIT)
    };
    let inventory = batch(json!({ "ops": [], "objects": true })).await.unwrap();
    let ids: Vec<Value> = inventory["objects"].as_array().unwrap().iter().map(|object| object["id"].clone()).collect();
    assert!(ids.len() >= 5, "the base, a table, its fields and two records: {inventory}");
    let load: Vec<Value> = ids.iter().map(|id| json!({ "target": id })).collect();
    let loaded = batch(json!({ "ops": [], "load": load })).await.unwrap();
    kit.request("document.close", json!({ "documentId": document }), LIMIT).await.unwrap();
    json!({ "objects": inventory["objects"], "loaded": loaded["loaded"] })
}

fn holds(bytes: &[u8], needle: &str) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle.as_bytes())
}

/// The rows a query over the file at `path` counts.
fn count(path: &Path, sql: &str) -> i64 {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

/// A `kition_meta` value of the file at `path`.
fn meta(path: &Path, key: &str) -> String {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.query_row("SELECT value FROM kition_meta WHERE key = ?1", [key], |row| row.get(0)).unwrap()
}

/// What the kit said about a refusal (`error.data.detail`).
fn detail(error: &ClientError) -> String {
    error.data().and_then(|data| data["detail"].as_str()).unwrap_or_default().to_owned()
}

#[tokio::test]
async fn a_table_tells_its_identity_and_forks_into_a_new_document_without_history() {
    let dir = workspace("embedded");
    let (bytes, kept, gone) = table();
    std::fs::write(dir.join("Plan.ktable"), &bytes).unwrap();
    let source = dir.join("Plan.ktable");
    assert!(count(&source, "SELECT COUNT(*) FROM kition_kdm_changes") > 0, "the deletion is a change row after the snapshot");
    assert!(count(&source, "SELECT COUNT(*) FROM kition_records WHERE deleted_at IS NOT NULL") > 0, "and a deleted row");
    let kit = launch(&dir).await;
    assert!(kit.serves("document.identity"), "{}", kit.capabilities());

    // The identity the file carries, read without opening it, and the same for a byte copy.
    let source_id = kit.identity("Plan.ktable", LIMIT).await.unwrap().doc_id.expect("a table file has an identity");
    assert_eq!(TableDocument::open("Plan.ktable", &bytes).unwrap().doc_id(), source_id, "the replica's id");
    assert_eq!(meta(&source, "kdm_doc_id"), source_id, "the file records the same document");
    std::fs::copy(&source, dir.join("Copy.ktable")).unwrap();
    assert_eq!(kit.identity("Copy.ktable", LIMIT).await.unwrap().doc_id.as_deref(), Some(source_id.as_str()), "a byte copy is the same document");

    // A fork is a new document with the same objects, ids and values.
    let fork = kit.fork("Plan.ktable", "Fork.ktable", LIMIT).await.unwrap();
    let fork_id = fork.doc_id.clone().expect("the fork's identity");
    assert_ne!(fork_id, source_id);
    assert_eq!(fork.name, "Fork.ktable");
    assert!(!fork.version.is_empty());
    assert_eq!(kit.identity("Fork.ktable", LIMIT).await.unwrap().doc_id.as_deref(), Some(fork_id.as_str()), "the forked file carries the new id");
    assert_eq!(kit.identity("Plan.ktable", LIMIT).await.unwrap().doc_id.as_deref(), Some(source_id.as_str()), "the source is unchanged");
    assert_eq!(std::fs::read(&source).unwrap(), bytes, "the source's bytes are unchanged");
    assert_eq!(objects(&kit, "Fork.ktable").await, objects(&kit, "Plan.ktable").await, "the same objects, ids and values");

    // The fork's replica travels as a whole (an agent's draft of it: `model.export`, then
    // `model.open`), though it starts at the fork and has no earlier changes.
    let state = kit.open("Fork.ktable", true, LIMIT).await.unwrap();
    let exported = kit.request("model.export", json!({ "documentId": state["documentId"] }), LIMIT).await.unwrap();
    let draft = kit
        .request("model.open", json!({ "name": "Fork.ktable", "snapshot": exported["changes"], "blobs": exported["blobs"] }), LIMIT)
        .await
        .unwrap();
    assert_ne!(draft["documentId"], state["documentId"], "{draft}");
    kit.request("document.close", json!({ "documentId": state["documentId"] }), LIMIT).await.unwrap();

    // The fork's file: the new id everywhere the file names its document, no history of any kind,
    // and the blob its records name, not the one only the deleted record named.
    let forked = dir.join("Fork.ktable");
    assert_eq!(meta(&forked, "kdm_doc_id"), fork_id, "the file records the fork's id");
    assert_eq!(meta(&forked, "document_id"), fork_id);
    assert_eq!(count(&forked, "SELECT COUNT(*) FROM kition_documents WHERE id <> (SELECT value FROM kition_meta WHERE key = 'document_id')"), 0);
    assert_eq!(count(&forked, "SELECT COUNT(*) FROM kition_kdm_changes"), 0, "a fork has no change rows");
    assert_eq!(count(&forked, "SELECT COUNT(*) FROM kition_record_history"), 0, "no record history");
    assert_eq!(count(&forked, "SELECT COUNT(*) FROM kition_change_log"), 0, "no change log");
    assert_eq!(count(&forked, "SELECT COUNT(*) FROM sqlite_master WHERE name = 'kition_kdm_origins'"), 0, "no origin log");
    assert_eq!(count(&forked, "SELECT COUNT(*) FROM kition_records WHERE deleted_at IS NOT NULL"), 0, "no deleted rows");
    assert_eq!(count(&forked, "PRAGMA freelist_count"), 0, "no free pages");
    let blobs = |path: &Path| {
        let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let mut statement = conn.prepare("SELECT hash FROM kition_blobs ORDER BY hash").unwrap();
        statement.query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap()
    };
    assert!(blobs(&source).contains(&gone), "the source keeps every blob");
    assert_eq!(blobs(&forked), vec![kept.clone()], "the fork keeps the blob its records name");

    // The fork edits, saves and opens again as any table, under its own id, with its attachment.
    let fork_bytes = std::fs::read(&forked).unwrap();
    let mut opened = TableDocument::open("Fork.ktable", &fork_bytes).unwrap();
    assert_eq!(opened.doc_id(), fork_id);
    let read = opened.execute("table.attachments.read", json!({ "hash": kept })).unwrap();
    let read = read["result"]["data"].as_str().unwrap_or_default().to_owned();
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(read).unwrap(), b"an attachment a record keeps");
    opened.execute("table.records.create", json!({ "tableId": 1, "values": { "title": "New" } })).unwrap();
    let edited = opened.save(None).unwrap();
    let mut reopened = TableDocument::open("Fork.ktable", &edited).unwrap();
    assert_eq!(reopened.doc_id(), fork_id);
    let rows = reopened.execute("table.read", json!({ "tableId": 1 })).unwrap()["result"]["rows"].clone();
    assert_eq!(rows.as_array().map(Vec::len), Some(3), "the two records kept and the new one: {rows}");

    // No history: the deleted record's text is in the source's bytes and not in the fork's.
    assert!(holds(&bytes, SECRET), "the source keeps its history");
    assert!(!holds(&fork_bytes, SECRET), "deleted content does not travel");

    // A fork of a fork is a third document with the same content.
    let again = kit.fork("Fork.ktable", "Again.ktable", LIMIT).await.unwrap().doc_id.expect("the second fork's identity");
    assert!(again != source_id && again != fork_id, "{again}");
    assert_eq!(objects(&kit, "Again.ktable").await, objects(&kit, "Plan.ktable").await);

    // A fork never replaces a file.
    let taken = kit.fork("Plan.ktable", "Fork.ktable", LIMIT).await.unwrap_err();
    assert_eq!(taken.reason(), Some("file_conflict"), "{taken}");
    assert_eq!(std::fs::read(&forked).unwrap(), fork_bytes, "the first fork is kept");
    kit.shutdown(Duration::from_secs(5)).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_unsaved_table_has_no_identity_and_bytes_that_are_no_table_are_refused() {
    let dir = workspace("plain");
    let kit = launch(&dir).await;
    // An empty `.ktable` (a file made for the kit before it saved one) holds no replica.
    std::fs::write(dir.join("New.ktable"), b"").unwrap();
    assert_eq!(kit.identity("New.ktable", LIMIT).await.unwrap().doc_id, None);
    let fork = kit.fork("New.ktable", "NewCopy.ktable", LIMIT).await.unwrap();
    assert_eq!(fork.doc_id, None);
    assert_eq!(std::fs::read(dir.join("NewCopy.ktable")).unwrap(), b"", "a file without a replica forks unchanged");

    // Bytes that are not a table are refused, never answered with no identity: text, and an
    // SQLite database that is no `.ktable`.
    std::fs::write(dir.join("Text.ktable"), b"words a person wrote elsewhere").unwrap();
    let other = dir.join("Other.ktable");
    rusqlite::Connection::open(&other).unwrap().execute_batch("CREATE TABLE notes (text TEXT); INSERT INTO notes VALUES ('hello');").unwrap();
    for name in ["Text", "Other"] {
        let copy = format!("{name}Copy.ktable");
        for refused in
            [kit.identity(&format!("{name}.ktable"), LIMIT).await.unwrap_err(), kit.fork(&format!("{name}.ktable"), &copy, LIMIT).await.unwrap_err()]
        {
            assert_eq!(refused.reason(), Some("unsupported_format"), "{name}: {refused}: {}", detail(&refused));
        }
        assert!(!dir.join(&copy).exists());
    }
    kit.shutdown(Duration::from_secs(5)).await;
    let _ = std::fs::remove_dir_all(dir);
}
