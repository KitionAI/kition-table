use kition_table_api::tables::{default_table_seed, seed_to_spec};
use kition_table_file::{CreateOptions, FieldSpec, TableSpec};
use serde_json::json;

use super::*;

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
}

fn new_file() -> TableFile {
    let mut file = TableFile::create_in_memory(&CreateOptions::default()).unwrap();
    file.set_origin(Origin::person("test"));
    file
}

const PEOPLE: &str = "Name,Score,Joined\nAda,10,2024-01-02\nGrace,7.5,2024-02-03\n";

#[test]
fn preview_answers_the_contracts_preview_response() {
    let answer = preview(&json!({ "fileName": "people.csv", "content": b64(PEOPLE) })).unwrap();
    assert_eq!(answer["import_token"], import_token(PEOPLE.as_bytes()));
    assert!(answer["import_token"].as_str().unwrap().starts_with("sha256:"));
    assert!(answer.get("expires_at").is_none(), "the kit's previews do not expire");
    assert_eq!(answer["source"], json!({ "kind": "upload", "upload_name": "people.csv" }));
    assert_eq!(answer["filename"], "people.csv");
    assert_eq!(answer["format"], "csv");
    assert_eq!(answer["encoding"], "utf-8");
    assert_eq!(answer["delimiter"], ",");
    assert_eq!((answer["row_count"].as_i64(), answer["field_count"].as_i64()), (Some(2), Some(3)));
    let types: Vec<&str> = answer["fields"].as_array().unwrap().iter().map(|field| field["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["text", "number", "date"]);
    assert_eq!(answer["sample_rows"][1], json!(["Grace", 7.5, "2024-02-03"]));
    assert_eq!(answer["warnings"], json!([]));
    assert_eq!(answer["sheets"], json!([]));
}

#[test]
fn preview_reads_a_workspace_source_and_refuses_bad_ones() {
    let answer = preview(&json!({
        "source": { "kind": "workspace", "workspace_path": "./data\\people.tsv" },
        "content": b64("a\tb\n1\t2\n"),
    }))
    .unwrap();
    assert_eq!(answer["source"], json!({ "kind": "workspace", "workspace_path": "data/people.tsv" }));
    assert_eq!(answer["filename"], "people.tsv");
    assert_eq!(answer["format"], "tsv");
    let refuse = |params: Value| preview(&params).unwrap_err().to_string();
    assert_eq!(refuse(json!({ "source": { "kind": "workspace", "workspace_path": "../x.csv" }, "content": "" })), "Import source path is invalid");
    assert_eq!(refuse(json!({ "content": b64(PEOPLE) })), "Import file is required");
    assert_eq!(refuse(json!({ "fileName": "a.csv", "content": "not base64!" })), "Import file content must be base64");
    assert_eq!(refuse(json!({ "fileName": "a.txt", "content": b64(PEOPLE) })), "Only CSV, TSV, and XLSX files can be imported");
    assert_eq!(refuse(json!({ "fileName": "a.xlsx", "content": b64("not a zip") })), "invalid XLSX workbook");
    assert_eq!(refuse(json!({ "fileName": "a.csv", "content": b64("\n\n") })), "invalid delimited file");
}

/// [`run`] as the import origin, as the binding runs it.
fn run_as_import(file: &mut TableFile, document_id: i64, path: &str, params: &Value) -> ApiResult<Value> {
    let previous = file.origin().clone();
    file.set_origin(import_origin());
    let answer = run(file, document_id, path, params);
    file.set_origin(previous);
    answer
}

fn run_params(target: Value, write_mode: &str, schema_mode: &str, content: &str) -> Value {
    json!({ "target": target, "write_mode": write_mode, "schema_mode": schema_mode, "fileName": "people.csv", "content": b64(content) })
}

#[test]
fn run_into_a_new_table_creates_its_fields_and_records_as_the_import_origin() {
    let mut file = new_file();
    let params = run_params(json!({ "kind": "new_table", "document_id": 1, "table_title": "People" }), "append", "auto", PEOPLE);
    let result = run_as_import(&mut file, 1, "People.ktable", &params).unwrap();
    assert_eq!(
        result,
        json!({
            "document_id": 1, "table_id": result["table_id"], "path": "People.ktable",
            "rows_total": 2, "rows_created": 2, "rows_updated": 0, "rows_skipped": 0,
            "fields_created": 3, "fields_updated": 0, "warnings": [],
        })
    );
    let table = file.get_table(result["table_id"].as_i64().unwrap()).unwrap();
    assert_eq!(table.title, "People");
    let shape: Vec<(&str, &str, bool)> = table.fields.iter().map(|f| (f.title.as_str(), f.field_type.as_str(), f.is_primary)).collect();
    assert_eq!(shape, [("Name", "text", true), ("Score", "number", false), ("Joined", "date", false)]);
    assert_eq!(table.views.len(), 1);
    let records = file.list_records(table.id).unwrap();
    assert_eq!(records[1].values[&table.fields[1].name], json!(7.5));
    assert_eq!(records[0].values[&table.fields[1].name], json!(10));
    // Import writes are no undo step.
    assert!(!file.can_undo());
    let (history, _) = file.list_record_history(table.id, records[0].id, 0, 0).unwrap();
    assert!(history.iter().all(|entry| entry.actor == IMPORT_PRINCIPAL), "{history:?}");
    // A `new_document` target is a new table in the open document.
    let params = run_params(json!({ "kind": "new_document", "path": "Other.ktable", "table_title": "Copy" }), "append", "auto", PEOPLE);
    let copy = run_as_import(&mut file, 1, "People.ktable", &params).unwrap();
    assert_eq!(file.get_table(copy["table_id"].as_i64().unwrap()).unwrap().title, "Copy");
    assert_eq!(file.list_tables().unwrap().len(), 2);
}

#[test]
fn run_into_a_starter_table_replaces_its_default_schema() {
    let mut file = new_file();
    let table = file.create_table(&seed_to_spec(&default_table_seed("Table"))).unwrap();
    file.create_record(table.id, json!({ table.fields[0].name.clone(): "Old" }).as_object().unwrap()).unwrap();
    let params = run_params(json!({ "kind": "existing_table", "table_id": table.id }), "append", "auto", "Title,Points\nA,1\nB,2\n");
    let result = run_as_import(&mut file, 1, "", &params).unwrap();
    assert_eq!((result["fields_updated"].as_i64(), result["fields_created"].as_i64()), (Some(2), Some(0)));
    assert!(result.get("path").is_none());
    let after = file.get_table(table.id).unwrap();
    let shape: Vec<(&str, &str)> = after.fields.iter().map(|f| (f.title.as_str(), f.field_type.as_str())).collect();
    assert_eq!(shape, [("Title", "text"), ("Points", "number")]);
    // The schema was replaced, so the records were too.
    assert_eq!(file.list_records(table.id).unwrap().len(), 2);
}

fn scores_table(file: &mut TableFile) -> (i64, String, String) {
    let spec = TableSpec {
        title: "Scores".into(),
        fields: vec![
            FieldSpec { title: "Name".into(), field_type: "text".into(), primary: true, ..FieldSpec::default() },
            FieldSpec { title: "Score".into(), field_type: "number".into(), ..FieldSpec::default() },
        ],
        ..TableSpec::default()
    };
    let table = file.create_table(&spec).unwrap();
    let (name, score) = (table.fields[0].name.clone(), table.fields[1].name.clone());
    file.create_record(table.id, json!({ name.clone(): "Ada", score.clone(): 1 }).as_object().unwrap()).unwrap();
    (table.id, name, score)
}

#[test]
fn run_upserts_by_key_and_adds_missing_fields() {
    let mut file = new_file();
    let (table_id, name, score) = scores_table(&mut file);
    let mut params = run_params(
        json!({ "kind": "existing_table", "table_id": table_id }),
        "upsert",
        "preserve",
        "Name,Score,Team\nAda,9,Red\nLin,4,Blue\n,3,Gray\n",
    );
    params["unique_by"] = json!(["name"]);
    let result = run_as_import(&mut file, 1, "", &params).unwrap();
    assert_eq!(
        (result["rows_created"].as_i64(), result["rows_updated"].as_i64(), result["rows_skipped"].as_i64(), result["fields_created"].as_i64()),
        (Some(1), Some(1), Some(1), Some(1))
    );
    let records = file.list_records(table_id).unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!((records[0].values[&name].clone(), records[0].values[&score].clone()), (json!("Ada"), json!(9)));
    let team = file.get_table(table_id).unwrap().fields.iter().find(|f| f.title == "Team").unwrap().name.clone();
    assert_eq!(records[1].values[&team], "Blue");
}

#[test]
fn run_replace_writes_large_sources_in_batches() {
    let mut file = new_file();
    let (table_id, _, _) = scores_table(&mut file);
    let mut content = String::from("Name,Score\n");
    for index in 0..(write::IMPORT_BATCH * 2 + 7) {
        content.push_str(&format!("Item {index},{index}\n"));
    }
    let params = run_params(json!({ "kind": "existing_table", "table_id": table_id }), "replace", "preserve", &content);
    let result = run_as_import(&mut file, 1, "", &params).unwrap();
    assert_eq!(result["rows_created"].as_u64(), Some(write::IMPORT_BATCH as u64 * 2 + 7));
    assert_eq!(file.count_records(table_id).unwrap(), write::IMPORT_BATCH as i64 * 2 + 7);
}

#[test]
fn run_refuses_bad_requests() {
    let mut file = new_file();
    let (table_id, _, _) = scores_table(&mut file);
    let target = json!({ "kind": "existing_table", "table_id": table_id });
    let mut refuse = |params: Value| run_as_import(&mut file, 1, "", &params).unwrap_err().to_string();
    assert_eq!(refuse(run_params(target.clone(), "merge", "auto", PEOPLE)), "write_mode must be append, replace, or upsert");
    assert_eq!(refuse(run_params(target.clone(), "append", "keep", PEOPLE)), "schema_mode must be auto, preserve, or replace");
    assert_eq!(refuse(run_params(target.clone(), "upsert", "auto", PEOPLE)), "upsert requires unique_by fields");
    assert_eq!(refuse(run_params(json!({ "kind": "new_table" }), "append", "auto", PEOPLE)), "new_table target requires table_title");
    assert_eq!(refuse(run_params(json!({ "kind": "elsewhere" }), "append", "auto", PEOPLE)), "unsupported import target");
    assert_eq!(refuse(run_params(json!({ "kind": "existing_table", "table_id": 99 }), "append", "auto", PEOPLE)), "Table does not exist");
    let mut stale = run_params(target, "append", "auto", PEOPLE);
    stale["import_token"] = json!(import_token(b"another file"));
    assert_eq!(refuse(stale), "Import preview token is invalid or expired");
}

#[test]
fn field_overrides_retitle_and_retype_but_never_into_computed_types() {
    let fields = parse::parse("a.csv", ImportSource::default(), "", PEOPLE.as_bytes()).unwrap().fields;
    let overrides = vec![
        FieldOverride { index: 0, title: " Person ".into(), field_type: "long_text".into(), options: None },
        FieldOverride { index: 1, title: String::new(), field_type: "formula".into(), options: None },
        FieldOverride { index: 7, title: "Nobody".into(), field_type: "text".into(), options: None },
    ];
    let out = write::apply_overrides(&fields, &overrides);
    assert_eq!((out[0].title.as_str(), out[0].field_type.as_str()), ("Person", "long_text"));
    assert_eq!((out[1].title.as_str(), out[1].field_type.as_str()), ("Score", "number"));
}
