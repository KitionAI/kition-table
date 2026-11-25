//! `TableFile` on the KDM: every mutation is one stored change and one undo step, writes from
//! several handles on one line of changes reach each other, and failures leave file and memory
//! consistent.

#![allow(clippy::disallowed_methods, reason = "native timing tests read the clock")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use kition_kdm::Origin;
use kition_table_file::kdm::store;
use kition_table_file::{
    CellEdit, CreateOptions, FieldPatch, FieldSpec, FilterRule, FilterTree, FilterTreeChild, MergeOutcome, TableFile, TableOp, TablePatch, TableSpec,
    ViewFieldPatch, ViewPatch, ViewSort, ViewSpec,
};
use rusqlite::Connection;
use serde_json::{Map, Value, json};

fn ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|err| panic!("unexpected error: {err}"))
}

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().unwrap_or_else(|err| panic!("tempdir: {err}"))
}

fn values(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn field(title: &str, field_type: &str) -> FieldSpec {
    FieldSpec { title: title.into(), field_type: field_type.into(), ..FieldSpec::default() }
}

fn create(dir: &Path, name: &str) -> (PathBuf, TableFile) {
    let path = dir.join(name);
    let mut file = ok(TableFile::create(&path, &CreateOptions { title: "Doc".into(), ..CreateOptions::default() }));
    file.set_origin(Origin::person("user:test"));
    (path, file)
}

fn open(path: &Path) -> TableFile {
    let mut file = ok(TableFile::open(path));
    file.set_origin(Origin::person("user:test"));
    file
}

fn change_rows(file: &TableFile) -> i64 {
    ok(file.conn().query_row("SELECT COUNT(*) FROM kition_kdm_changes", [], |row| row.get(0)))
}

/// A table "Tasks" (title, status, done) with one record.
fn tasks(file: &mut TableFile) -> (i64, i64) {
    let table = ok(file.create_table(&TableSpec {
        title: "Tasks".into(),
        fields: vec![FieldSpec { primary: true, ..field("Title", "text") }, field("Status", "text"), field("Done", "checkbox")],
        ..TableSpec::default()
    }));
    let record = ok(file.create_record(table.id, &values(json!({ "title": "Write" }))));
    (table.id, record.id)
}

/// Replaces `path` with `source` the way a sync tool does: a new file renamed over it.
fn replace_with(path: &Path, source: &Path) {
    let staging = path.with_extension("incoming");
    ok(fs::copy(source, &staging));
    ok(fs::rename(&staging, path));
}

/// Everything the relational API reads, by integer id.
fn picture(file: &TableFile) -> Value {
    let mut tables = Vec::new();
    for table in ok(file.list_tables()) {
        let records: Vec<Value> =
            ok(file.list_records(table.id)).into_iter().map(|record| json!([record.id, record.row_key, record.order, record.values])).collect();
        let fields: Vec<Value> = table
            .fields
            .iter()
            .map(|field| json!([field.id, field.name, field.title, field.field_type, field.order, field.options, field.is_primary]))
            .collect();
        let views: Vec<Value> = table.views.iter().map(|view| json!([view.id, view.title, view.view_type, view.order, view.config])).collect();
        tables.push(json!({
            "id": table.id,
            "name": table.name,
            "title": table.title,
            "order": table.order,
            "primary": table.primary_field_id,
            "fields": fields,
            "views": views,
            "records": records,
        }));
    }
    Value::Array(tables)
}

#[test]
fn every_mutation_is_one_change_row_and_one_undo_step() {
    let dir = temp();
    let (_, mut file) = create(dir.path(), "steps.ktable");
    let mut rows = change_rows(&file);
    let mut undo_steps = 0;
    let mut step = |file: &TableFile, undoable: bool, what: &str| {
        let now = change_rows(file);
        assert_eq!(now, rows + 1, "{what} wrote one change row");
        rows = now;
        if undoable {
            undo_steps += 1;
        }
    };

    let table = ok(file.create_table(&TableSpec { title: "Tasks".into(), ..TableSpec::default() }));
    step(&file, true, "create_table");
    let people = ok(file.create_table(&TableSpec { title: "People".into(), ..TableSpec::default() }));
    step(&file, true, "create_table");
    ok(file.update_table(table.id, &TablePatch { title: Some("Work".into()), ..TablePatch::default() }));
    step(&file, true, "update_table");
    let status = ok(file.create_field(table.id, &field("Status", "text")));
    step(&file, true, "create_field");
    let copy = ok(file.duplicate_field(table.id, status.id));
    step(&file, true, "duplicate_field");
    ok(file.update_field(table.id, status.id, &FieldPatch { title: Some("State".into()), ..FieldPatch::default() }));
    step(&file, true, "update_field");
    let link = ok(file.create_field(table.id, &FieldSpec { options: json!({ "target_table_id": people.id }), ..field("Owner", "link_to_record") }));
    step(&file, true, "create_field (link)");
    assert_eq!(link.options.get("target_table_id"), Some(&json!(people.id)));
    let view = ok(file.create_view(table.id, &ViewSpec { title: "Board".into(), view_type: "kanban".into(), config: Value::Null }));
    step(&file, true, "create_view");
    ok(file.update_view(table.id, view.id, &ViewPatch { config: Some(json!({ "row_height": "tall" })), ..ViewPatch::default() }));
    step(&file, true, "update_view");
    let record = ok(file.create_record(table.id, &values(json!({ "title": "A" }))));
    step(&file, true, "create_record");
    let person = ok(file.create_record(people.id, &values(json!({ "title": "Ann" }))));
    step(&file, true, "create_record");
    ok(file.update_record(table.id, record.id, &values(json!({ status.name.clone(): "todo" })), None));
    step(&file, true, "update_record");
    let refs = ok(file.replace_record_links(table.id, record.id, link.id, std::slice::from_ref(&person.row_key)));
    assert_eq!(refs.len(), 1);
    step(&file, true, "replace_record_links");
    ok(file.delete_record_links(table.id, record.id, link.id, std::slice::from_ref(&person.row_key)));
    step(&file, true, "delete_record_links");
    let tree = FilterTree {
        conjunction: "and".into(),
        children: vec![FilterTreeChild {
            group: None,
            condition: Some(FilterRule { field_id: status.id, operator: "is".into(), value: json!("todo") }),
        }],
    };
    ok(file.save_filter_tree(view.id, Some(&tree)));
    step(&file, true, "save_filter_tree");
    assert_eq!(ok(file.load_filter_tree(view.id)), Some(tree));
    let sorts = vec![ViewSort { field_id: status.id, direction: "desc".into() }];
    ok(file.replace_view_sorts(view.id, &sorts));
    step(&file, true, "replace_view_sorts");
    assert_eq!(ok(file.list_view_sorts(view.id)), sorts);
    ok(file.replace_view_groups(view.id, &sorts));
    step(&file, true, "replace_view_groups");
    assert_eq!(ok(file.list_view_groups(view.id)), sorts);
    // The table editor writes whole configs without sorts or groups; they are kept.
    ok(file.update_view(table.id, view.id, &ViewPatch { config: Some(json!({ "row_height": "short" })), ..ViewPatch::default() }));
    step(&file, true, "update_view");
    assert_eq!(ok(file.list_view_sorts(view.id)), sorts);
    let layout = ok(file.upsert_view_field(view.id, status.id, &ViewFieldPatch { width: Some(300), ..ViewFieldPatch::default() }));
    assert_eq!(layout.width, 300);
    step(&file, false, "upsert_view_field (width)");
    let layout = ok(file.upsert_view_field(view.id, status.id, &ViewFieldPatch { visible: Some(false), ..ViewFieldPatch::default() }));
    assert!(!layout.visible);
    step(&file, true, "upsert_view_field (visible)");
    let copied = ok(file.bulk_copy_records(table.id, std::slice::from_ref(&record.row_key), 0));
    assert_eq!(copied.copied_count, 1);
    step(&file, true, "bulk_copy_records");
    let deleted = ok(file.bulk_delete_records(table.id, &copied.new_row_keys));
    assert_eq!(deleted.deleted_count, 1);
    step(&file, true, "bulk_delete_records");
    let (created, _) = ok(file.transact(&[
        TableOp::RecordsCreate { table_id: table.id, at: None, records: vec![values(json!({ "title": "B" })), values(json!({ "title": "C" }))] },
        TableOp::CellsSet {
            table_id: table.id,
            records: vec![CellEdit { row_key: record.row_key.clone(), values: values(json!({ status.name.clone(): "done" })) }],
        },
    ]));
    assert_eq!(created.len(), 2);
    step(&file, true, "transact");
    ok(file.delete_record(table.id, record.id));
    step(&file, true, "delete_record");
    ok(file.delete_view(table.id, view.id));
    step(&file, true, "delete_view");
    ok(file.delete_field(table.id, copy.id));
    step(&file, true, "delete_field");
    ok(file.delete_table(people.id));
    step(&file, true, "delete_table");

    let mut undone = 0;
    while ok(file.undo()).0 {
        undone += 1;
        assert!(undone <= undo_steps, "more undo steps than mutations");
    }
    assert_eq!(undone, undo_steps);
    assert!(ok(file.list_tables()).is_empty(), "everything was undone");
    assert!(!file.can_undo());
    assert!(file.can_redo());
}

#[test]
fn undo_of_create_table_removes_it_and_redo_restores_the_same_ids() {
    let dir = temp();
    let (_, mut file) = create(dir.path(), "undo.ktable");
    let table = ok(file.create_table(&TableSpec {
        title: "Tasks".into(),
        fields: vec![field("Title", "text"), field("Notes", "long_text")],
        ..TableSpec::default()
    }));
    let (done, change) = ok(file.undo());
    assert!(done);
    assert!(change.tables.contains(&table.id));
    assert!(ok(file.list_tables()).is_empty());
    let (done, _) = ok(file.redo());
    assert!(done);
    let restored = ok(file.get_table(table.id));
    assert_eq!(restored.name, table.name);
    let ids = |table: &kition_table_file::Table| {
        (table.fields.iter().map(|f| f.id).collect::<Vec<_>>(), table.views.iter().map(|v| v.id).collect::<Vec<_>>(), table.primary_field_id)
    };
    assert_eq!(ids(&restored), ids(&table));
    let (done, _) = ok(file.redo());
    assert!(!done, "nothing left to redo");
}

#[test]
fn two_handles_on_one_file_keep_each_others_writes() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "two.ktable");
    let (table_id, record_id) = tasks(&mut file);
    let mut second = open(&path);
    let created = ok(file.create_record(table_id, &values(json!({ "title": "From one" }))));
    // `second` has not seen the record `file` created; its write must not drop it.
    ok(second.update_record(table_id, record_id, &values(json!({ "status": "late" })), None));
    let keys: Vec<String> = ok(second.list_records(table_id)).into_iter().map(|record| record.row_key).collect();
    assert!(keys.contains(&created.row_key));
    ok(file.update_record(table_id, created.id, &values(json!({ "status": "early" })), None));
    file.close();
    second.close();
    let reopened = open(&path);
    assert_eq!(ok(reopened.get_record(table_id, record_id)).values.get("status"), Some(&json!("late")));
    assert_eq!(ok(reopened.get_record(table_id, created.id)).values.get("status"), Some(&json!("early")));
}

#[test]
fn a_failed_append_leaves_file_and_replica_consistent() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "fail.ktable");
    let (table_id, record_id) = tasks(&mut file);
    let state = ok(file.replica().state_hash());
    let rows = change_rows(&file);
    ok(file.conn().execute_batch(
        "CREATE TRIGGER fail_append BEFORE INSERT ON kition_kdm_changes \
         BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
    ));
    let failed = file.update_record(table_id, record_id, &values(json!({ "status": "lost" })), None);
    assert!(failed.is_err());
    assert_eq!(change_rows(&file), rows);
    assert_eq!(ok(file.replica().state_hash()), state, "memory did not run ahead");
    assert_eq!(ok(file.get_record(table_id, record_id)).values.get("status"), None);
    ok(file.conn().execute_batch("DROP TRIGGER fail_append;"));
    ok(file.update_record(table_id, record_id, &values(json!({ "status": "kept" })), None));
    assert_eq!(change_rows(&file), rows + 1);
    let state = ok(file.replica().state_hash());
    file.close();
    let reopened = open(&path);
    assert_eq!(ok(reopened.replica().state_hash()), state);
    assert_eq!(ok(reopened.get_record(table_id, record_id)).values.get("status"), Some(&json!("kept")));
}

#[test]
fn a_thousand_cell_edits_are_one_change_row() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "bulk.ktable");
    let (table_id, _) = tasks(&mut file);
    let rows: Vec<Map<String, Value>> = (0..1000).map(|index| values(json!({ "title": format!("Row {index}") }))).collect();
    let start = Instant::now();
    let (created, change) = ok(file.transact(&[TableOp::RecordsCreate { table_id, at: None, records: rows }]));
    let create_time = start.elapsed();
    assert_eq!(created.len(), 1000);
    assert_eq!(change.records_upserted.len(), 1000);
    let before = change_rows(&file);
    let edits: Vec<CellEdit> = created
        .iter()
        .enumerate()
        .map(|(index, key)| CellEdit { row_key: key.clone(), values: values(json!({ "status": format!("s{index}"), "done": index % 2 == 0 })) })
        .collect();
    let start = Instant::now();
    let (_, change) = ok(file.transact(&[TableOp::CellsSet { table_id, records: edits }]));
    let set_time = start.elapsed();
    println!("transact: 1,000 records.create in {create_time:?}, 1,000 cells.set in {set_time:?}");
    assert_eq!(change_rows(&file), before + 1);
    assert_eq!(change.records_upserted.len(), 1000);
    let (done, _) = ok(file.undo());
    assert!(done);
    assert!(ok(file.list_records(table_id)).iter().all(|record| record.values.get("status").is_none()), "one undo step reverts every edit");
    file.close();
    let reopened = open(&path);
    assert_eq!(ok(reopened.count_records(table_id)), 1001);
}

#[test]
fn reopening_after_compaction_reads_the_same() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "compact.ktable");
    let (table_id, record_id) = tasks(&mut file);
    for index in 0..520 {
        ok(file.update_record(table_id, record_id, &values(json!({ "status": format!("v{index}") })), None));
    }
    let (generation, _) = ok(store::stored_position(file.conn()));
    assert!(generation > 1, "the log was compacted");
    assert!(change_rows(&file) < 500);
    let before = picture(&file);
    let state = ok(file.replica().state_hash());
    file.close();
    let mut reopened = open(&path);
    assert_eq!(picture(&reopened), before);
    assert_eq!(ok(reopened.replica().state_hash()), state);
    ok(reopened.update_record(table_id, record_id, &values(json!({ "status": "after" })), None));
    reopened.close();
    let again = open(&path);
    assert_eq!(ok(again.get_record(table_id, record_id)).values.get("status"), Some(&json!("after")));
}

#[test]
fn reopen_merging_switches_to_another_document() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "mine.ktable");
    tasks(&mut file);
    assert_eq!(ok(file.reopen_merging()), MergeOutcome::Unchanged);
    let (other_path, mut other) = create(dir.path(), "theirs.ktable");
    ok(other.create_table(&TableSpec { title: "Elsewhere".into(), ..TableSpec::default() }));
    other.close();
    replace_with(&path, &other_path);
    assert!(file.changed_on_disk());
    assert_eq!(ok(file.reopen_merging()), MergeOutcome::DifferentDocument);
    let titles: Vec<String> = ok(file.list_tables()).into_iter().map(|table| table.title).collect();
    assert_eq!(titles, vec!["Elsewhere".to_owned()]);
    assert!(!file.changed_on_disk());
}

#[test]
fn reopen_merging_an_identical_copy_is_unchanged() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "same.ktable");
    tasks(&mut file);
    let copy = dir.path().join("copy.ktable");
    ok(fs::copy(&path, &copy));
    replace_with(&path, &copy);
    assert!(file.changed_on_disk());
    assert_eq!(ok(file.reopen_merging()), MergeOutcome::Unchanged);
    assert!(!file.changed_on_disk());
}

#[test]
fn changed_on_disk_notices_a_copy_over_but_not_own_writes() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "watched.ktable");
    assert!(!file.changed_on_disk());
    let (table_id, record_id) = tasks(&mut file);
    ok(file.update_record(table_id, record_id, &values(json!({ "status": "x" })), None));
    assert!(!file.changed_on_disk());
    let (other, other_file) = create(dir.path(), "other.ktable");
    other_file.close();
    replace_with(&path, &other);
    assert!(file.changed_on_disk());
}

/// Timings of one-record writes into a growing table (each one transaction): run with
/// `--release --ignored --nocapture`.
#[test]
#[ignore]
fn sequential_write_timings() {
    let dir = temp();
    let (_, mut file) = create(dir.path(), "timings.ktable");
    let (table_id, record_id) = tasks(&mut file);
    let start = Instant::now();
    for index in 0..2000 {
        ok(file.create_record(table_id, &values(json!({ "title": format!("Row {index}") }))));
        if (index + 1) % 500 == 0 {
            println!("{} create_record calls: {:?}", index + 1, start.elapsed());
        }
    }
    let start = Instant::now();
    for index in 0..200 {
        ok(file.update_record(table_id, record_id, &values(json!({ "status": format!("v{index}") })), None));
    }
    println!("200 update_record calls at 2,000 records: {:?}", start.elapsed());

    let rows: Vec<Map<String, Value>> = (0..10_000).map(|index| values(json!({ "title": format!("Bulk {index}"), "status": "new" }))).collect();
    let start = Instant::now();
    for chunk in rows.chunks(500) {
        ok(file.create_records(table_id, chunk));
    }
    println!("10,000 rows in batches of 500: {:?}", start.elapsed());
    let path = file.path().to_path_buf();
    file.close();
    let start = Instant::now();
    let reopened = open(&path);
    println!("open with {} records: {:?}", ok(reopened.count_records(table_id)), start.elapsed());
}

// ---------------------------------------------------------------------------
// Review fixes: read-only files, store identity, document ids, resync, repair after undo.

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = ok(fs::metadata(path)).permissions();
    permissions.set_readonly(read_only);
    ok(fs::set_permissions(path, permissions));
}

fn assert_refused<T: std::fmt::Debug>(result: kition_table_file::Result<T>) {
    assert!(matches!(result, Err(kition_table_file::TableFileError::ReadOnly(_))), "expected a read-only refusal, got {result:?}");
}

#[test]
fn a_read_only_file_opens_read_only() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "new.ktable");
    let (table_id, record_id) = tasks(&mut file);
    file.close();
    set_read_only(&path, true);
    let mut file = open(&path);
    assert!(file.read_only().is_some());
    assert_eq!(ok(file.list_records(table_id)).len(), 1);
    assert_refused(file.delete_record(table_id, record_id));
    set_read_only(&path, false);
}

#[test]
fn changes_missing_from_an_older_copy_survive_a_failed_write() {
    let dir = temp();
    let (path, mut file) = create(dir.path(), "older.ktable");
    let (table_id, record_id) = tasks(&mut file);
    file.close();
    let older = dir.path().join("older-copy.ktable");
    ok(fs::copy(&path, &older));

    let mut here = open(&path);
    ok(here.update_record(table_id, record_id, &values(json!({ "status": "only here" })), None));
    // The file goes back to the older copy, in place: it lacks the edit above.
    ok(fs::copy(&older, &path));
    let failed = here.transact(&[TableOp::RecordsMove { table_id, row_keys: vec!["no-such-record".into()], before: None }]);
    assert!(failed.is_err());
    here.close();

    let again = open(&path);
    assert_eq!(
        ok(again.get_record(table_id, record_id)).values.get("status"),
        Some(&json!("only here")),
        "the edit the older copy lacked was stored again"
    );
}

#[test]
fn an_undo_that_revives_a_taken_name_is_repaired() {
    let dir = temp();
    let (path, mut first) = create(dir.path(), "names.ktable");
    let (table_id, _) = tasks(&mut first);
    let status = ok(first.list_fields(table_id)).into_iter().find(|field| field.name == "status").unwrap_or_else(|| panic!("a status field"));
    ok(first.delete_field(table_id, status.id));
    let mut second = open(&path);
    let taken = ok(second.create_field(table_id, &field("Status", "text")));
    assert_eq!(taken.name, "status");
    let (done, _) = ok(first.undo());
    assert!(done);
    let mut names: Vec<String> = ok(first.list_fields(table_id)).into_iter().map(|field| field.name).collect();
    let count = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), count, "field names stay unique: {names:?}");
    assert!(names.contains(&"status".to_owned()));
    assert!(names.contains(&"status_2".to_owned()));
}
