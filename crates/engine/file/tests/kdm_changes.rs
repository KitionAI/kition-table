//! `TableFile::record_changes_since`: the records created, updated, and deleted since a version,
//! with the origin of each write, through compaction and the origin log's trimming.

#![allow(clippy::unwrap_used)]

use kition_kdm::{Origin, Version};
use kition_table_file::kdm::changes::UNKNOWN_ORIGIN;
use kition_table_file::kdm::store::ORIGIN_LOG_ROWS;
use kition_table_file::{CreateOptions, FieldSpec, RecordChange, TableFile, TableSpec};
use serde_json::{Map, Value, json};

struct Fixture {
    file: TableFile,
    table: i64,
    name: String,
    notes: String,
    notes_id: i64,
}

fn fixture() -> Fixture {
    let mut file = TableFile::create_in_memory(&CreateOptions::default()).unwrap();
    file.set_origin(Origin::system("create"));
    let spec = TableSpec {
        title: "Tasks".into(),
        fields: vec![
            FieldSpec { title: "Name".into(), field_type: "text".into(), primary: true, ..FieldSpec::default() },
            FieldSpec { title: "Notes".into(), field_type: "long_text".into(), ..FieldSpec::default() },
        ],
        ..TableSpec::default()
    };
    let table = file.create_table(&spec).unwrap();
    let (name, notes, notes_id) = (table.fields[0].name.clone(), table.fields[1].name.clone(), table.fields[1].id);
    Fixture { file, table: table.id, name, notes, notes_id }
}

fn values(pairs: &[(&str, &str)]) -> Map<String, Value> {
    pairs.iter().map(|(key, value)| ((*key).to_owned(), json!(value))).collect()
}

fn change(table: i64, record: i64, fields: Option<Vec<i64>>, origin: &str) -> RecordChange {
    RecordChange { table_id: table, record_id: record, field_ids: fields, origin: origin.to_owned() }
}

#[test]
fn reports_each_write_with_its_origin() {
    let mut fx = fixture();
    let start = fx.file.replica_version();
    fx.file.set_origin(Origin::person("session:a"));
    let ada = fx.file.create_record(fx.table, &values(&[(&fx.name, "Ada")])).unwrap();
    fx.file.set_origin(Origin::system("workflow"));
    let bot = fx.file.create_record(fx.table, &values(&[(&fx.name, "Bot")])).unwrap();
    fx.file.update_record(fx.table, ada.id, &values(&[(&fx.notes, "Checked")]), None).unwrap();
    fx.file.set_origin(Origin::agent("turn-1"));
    fx.file.delete_record(fx.table, bot.id).unwrap();

    let changes = fx.file.record_changes_since(&start).unwrap();
    assert_eq!(changes.created, [change(fx.table, ada.id, None, "person:session:a"), change(fx.table, bot.id, None, "system:workflow")]);
    assert_eq!(changes.updated, [change(fx.table, ada.id, Some(vec![fx.notes_id]), "system:workflow")]);
    assert_eq!(changes.deleted, [change(fx.table, bot.id, None, "agent:turn-1")]);

    // From the current version, nothing; from a version in between, only what came after it.
    assert_eq!(fx.file.record_changes_since(&fx.file.replica_version()).unwrap(), Default::default());
    let middle = fx.file.replica_version();
    fx.file.set_origin(Origin::person("session:a"));
    fx.file.update_record(fx.table, ada.id, &values(&[(&fx.name, "Ada L.")]), None).unwrap();
    fx.file.undo().unwrap();
    let later = fx.file.record_changes_since(&middle).unwrap();
    // The edit and its undo are two writes of the person's; each changed the name.
    assert_eq!(later.updated, [change(fx.table, ada.id, Some(vec![ada_name_field(&fx)]), "person:session:a")]);
    assert!(later.created.is_empty() && later.deleted.is_empty());

    // A version the document never reached is refused.
    let mut other = fixture();
    other.file.create_record(other.table, &values(&[(&other.name, "Elsewhere")])).unwrap();
    assert!(fx.file.record_changes_since(&other.file.replica_version()).is_err());
}

fn ada_name_field(fx: &Fixture) -> i64 {
    fx.file.get_table(fx.table).unwrap().fields[0].id
}

#[test]
fn link_edits_update_the_linking_record() {
    let mut fx = fixture();
    let link = fx
        .file
        .create_field(
            fx.table,
            &FieldSpec {
                title: "Blocks".into(),
                field_type: "link_to_record".into(),
                options: json!({ "target_table_id": fx.table, "allow_multiple": true }),
                ..FieldSpec::default()
            },
        )
        .unwrap();
    let a = fx.file.create_record(fx.table, &values(&[(&fx.name, "A")])).unwrap();
    let b = fx.file.create_record(fx.table, &values(&[(&fx.name, "B")])).unwrap();
    let start = fx.file.replica_version();
    fx.file.set_origin(Origin::person("session:a"));
    fx.file.replace_record_links(fx.table, a.id, link.id, std::slice::from_ref(&b.row_key)).unwrap();
    let changes = fx.file.record_changes_since(&start).unwrap();
    assert_eq!(changes.updated, [change(fx.table, a.id, Some(vec![link.id]), "person:session:a")]);
}

#[test]
fn origins_survive_compaction_of_the_change_log() {
    let mut fx = fixture();
    let start = fx.file.replica_version();
    fx.file.set_origin(Origin::new(kition_kdm::OriginKind::Import, "import"));
    // More than a mebibyte of changes: the write compacts the change log.
    let long = "x".repeat(2000);
    let rows: Vec<Map<String, Value>> = (0..700).map(|index| values(&[(&fx.name, &format!("Row {index}")), (&fx.notes, &long)])).collect();
    fx.file.create_records(fx.table, &rows).unwrap();
    let logged: i64 = fx.file.conn().query_row("SELECT COUNT(*) FROM kition_kdm_changes", [], |row| row.get(0)).unwrap();
    assert_eq!(logged, 0, "the change log was compacted");
    let changes = fx.file.record_changes_since(&start).unwrap();
    assert_eq!(changes.created.len(), 700);
    assert!(changes.created.iter().all(|created| created.origin == "import:import"));
}

#[test]
fn writes_older_than_the_origin_log_are_unattributed() {
    let mut fx = fixture();
    fx.file.set_origin(Origin::person("session:a"));
    let first = fx.file.create_record(fx.table, &values(&[(&fx.name, "First")])).unwrap();
    for index in 0..ORIGIN_LOG_ROWS {
        fx.file.update_record(fx.table, first.id, &values(&[(&fx.notes, &format!("Edit {index}"))]), None).unwrap();
    }
    let kept: i64 = fx.file.conn().query_row("SELECT COUNT(*) FROM kition_kdm_origins", [], |row| row.get(0)).unwrap();
    assert_eq!(kept, ORIGIN_LOG_ROWS);
    let changes = fx.file.record_changes_since(&Version::empty()).unwrap();
    assert_eq!(changes.created, [change(fx.table, first.id, None, UNKNOWN_ORIGIN)]);
    assert_eq!(changes.updated.len(), 1);
    assert_eq!(changes.updated[0].origin, "person:session:a");
}
