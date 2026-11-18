use kition_table_file::{CreateOptions, FieldSpec, TableSpec};

use super::*;

fn field(id: i64, name: &str, options: Value) -> Field {
    Field {
        id,
        name: name.into(),
        title: name.into(),
        field_type: "text".into(),
        options: options.as_object().cloned().unwrap_or_default(),
        ..Field::default()
    }
}

#[test]
fn parses_and_validates_configs() {
    let config = |raw: Value| {
        let mut options = Map::new();
        options.insert("ai_config".into(), raw);
        parse_config(&options, "")
    };
    assert!(config(json!({ "type": "summarize", "source_field_id": 1 })).is_ok());
    assert!(config(json!({ "type": "summarize" })).is_err());
    assert!(config(json!({ "type": "translate", "source_field_id": 1 })).is_err());
    assert!(config(json!({ "type": "classify", "source_field_id": 1, "categories": ["a"] })).is_err());
    let image = config(json!({ "type": "image_generation", "source_field_id": 1, "aspect_ratio": "16:9" })).unwrap_or_default();
    assert_eq!((image.image_size.as_str(), image.image_count, image.image_quality.as_str()), ("1536x1024", 3, "low"));
    assert!(config(json!({ "type": "summarize", "source_field_id": 1, "enabled": false })).is_err());
    let legacy = parse_config(&Map::new(), "Write a tagline").unwrap_or_default();
    assert_eq!(legacy.action, "customize");
    let model = config(json!({ "type": "summarize", "source_field_id": 1, "runtime_model": "writer" })).unwrap_or_default();
    assert_eq!(model.model, "writer");
}

#[test]
fn resolves_ai_and_source_fields() {
    let fields = vec![
        field(1, "body", json!({})),
        field(2, "summary", json!({ "ai_config": { "type": "summarize", "source_field_id": 1 } })),
        field(3, "loop", json!({ "ai_config": { "type": "summarize", "source_field_id": 3 } })),
    ];
    let (target, source, _) = resolve_field(&fields, 2).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!((target.id, source.map(|s| s.id)), (2, Some(1)));
    assert!(resolve_field(&fields, 1).is_err());
    assert!(resolve_field(&fields, 3).is_err());
    let prompt = build_prompt(&parse_config(&fields[1].options, "").unwrap_or_default(), "Summary", "Body", "Long text");
    assert!(prompt.contains("Task: Summarize the source content.") && prompt.ends_with("Long text"));
}

#[test]
fn applying_the_execution_config_keeps_the_stored_action() {
    let stored =
        parse_config(&json!({ "ai_config": { "type": "customize", "prompt": "Stored" } }).as_object().cloned().unwrap_or_default(), "").unwrap();
    let raw = json!({ "type": "customize", "prompt": "For this record" }).as_object().cloned().unwrap_or_default();
    assert_eq!(apply_execution(stored.clone(), "customize", &raw).unwrap().custom_prompt, "For this record");
    assert!(apply_execution(stored.clone(), "summarize", &raw).is_err());
    let other = json!({ "type": "text_generation", "prompt": "x" }).as_object().cloned().unwrap_or_default();
    assert!(apply_execution(stored, "", &other).is_err());
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nnot really a picture";

struct Fixture {
    file: TableFile,
    table: i64,
    body: Field,
    photo: Field,
}

/// A table with a text source (`Body`), an attachment source (`Photo`), and AI fields made by
/// the test.
fn fixture() -> Fixture {
    let mut file = TableFile::create_in_memory(&CreateOptions::default()).unwrap();
    file.set_origin(Origin::person("test"));
    let spec = TableSpec {
        title: "Notes".into(),
        fields: vec![
            FieldSpec { title: "Body".into(), field_type: "long_text".into(), primary: true, ..FieldSpec::default() },
            FieldSpec { title: "Photo".into(), field_type: "attachment".into(), ..FieldSpec::default() },
        ],
        ..TableSpec::default()
    };
    let table = file.create_table(&spec).unwrap();
    let body = table.fields[0].clone();
    let photo = table.fields[1].clone();
    Fixture { file, table: table.id, body, photo }
}

impl Fixture {
    fn ai_field(&mut self, title: &str, field_type: &str, config: Value) -> Field {
        let spec = FieldSpec { title: title.into(), field_type: field_type.into(), options: json!({ "ai_config": config }), ..FieldSpec::default() };
        self.file.create_field(self.table, &spec).unwrap()
    }

    fn record(&mut self, values: Value) -> Record {
        self.file.create_record(self.table, values.as_object().unwrap()).unwrap()
    }
}

#[test]
fn prepare_builds_text_prompts_and_skips_records_it_cannot_run() {
    let mut fx = fixture();
    let summary = fx.ai_field("Summary", "text", json!({ "type": "summarize", "source_field_id": fx.body.id, "runtime_model": "writer" }));
    let full = fx.record(json!({ fx.body.name.clone(): "A long story about tables." }));
    let empty = fx.record(json!({}));
    let params =
        json!({ "tableId": fx.table, "fieldId": summary.id, "record_ids": [full.id, empty.id, full.id, 999], "scope": "selection", "force": true });
    let answer = prepare(&fx.file, &params).unwrap();
    assert_eq!(answer["fieldId"], summary.id);
    assert_eq!(answer["output"], "text");
    assert_eq!(answer["model"], "writer");
    let requests = answer["requests"].as_array().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["recordId"], full.id);
    let message = &requests[0]["messages"][0];
    assert_eq!(message["role"], "user");
    let prompt = message["parts"][0]["text"].as_str().unwrap();
    assert!(prompt.contains("Target field: Summary") && prompt.contains("Source field: Body") && prompt.ends_with("A long story about tables."));
    assert_eq!(
        answer["skipped"],
        json!([{ "recordId": empty.id, "reason": "Source field is empty" }, { "recordId": 999, "reason": "Record does not exist" }])
    );
    // `limit` caps the batch; no records is refused; one cell that cannot run is an error.
    let limited = prepare(&fx.file, &json!({ "tableId": fx.table, "fieldId": summary.id, "record_ids": [full.id, empty.id], "limit": 1 })).unwrap();
    assert_eq!(limited["requests"].as_array().unwrap().len(), 1);
    assert_eq!(limited["skipped"], json!([]));
    assert!(prepare(&fx.file, &json!({ "tableId": fx.table, "fieldId": summary.id, "record_ids": [] })).is_err());
    let cell = prepare(&fx.file, &json!({ "tableId": fx.table, "fieldId": summary.id, "recordId": empty.id }));
    assert_eq!(cell, Err(ApiError::BadRequest("Source field is empty".into())));
    // A field without an AI configuration is refused.
    assert!(prepare(&fx.file, &json!({ "tableId": fx.table, "fieldId": fx.body.id, "recordId": full.id })).is_err());
}

#[test]
fn prepare_names_images_held_in_the_file_by_hash() {
    let mut fx = fixture();
    let info = fx.file.put_blob(PNG, "image/png", "cat.png").unwrap();
    let remote = json!({ "url": "https://example.com/dog.jpg", "name": "dog.jpg", "mimeType": "image/jpeg" });
    let pdf = json!({ "url": "https://example.com/a.pdf", "name": "a.pdf", "mimeType": "application/pdf" });
    let poster = fx.ai_field(
        "Poster",
        "attachment",
        json!({ "type": "image_customization", "prompt": "Make it a poster", "source_field_id": fx.photo.id, "aspect_ratio": "16:9", "n": 2 }),
    );
    let record = fx.record(json!({ fx.photo.name.clone(): [blobs::attachment_value(&info, "cat.png"), remote, pdf] }));
    let answer = prepare(&fx.file, &json!({ "tableId": fx.table, "fieldId": poster.id, "recordId": record.id })).unwrap();
    assert_eq!(answer["output"], "image");
    let request = &answer["requests"][0];
    let parts = request["messages"][0]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert!(parts[0]["text"].as_str().unwrap().contains("Primary request: Make it a poster"));
    assert_eq!(parts[1], json!({ "type": "image", "hash": info.hash, "mime": "image/png" }));
    assert_eq!(parts[2], json!({ "type": "image", "url": "https://example.com/dog.jpg", "mime": "image/jpeg" }));
    assert_eq!(request["options"]["count"], 2);
    assert_eq!(request["options"]["size"], "1536x1024");
    assert_eq!(request["options"]["width"], 1536);
    // A cell naming a file the engine cannot read, or no image at all, cannot run.
    let local = fx.record(json!({ fx.photo.name.clone(): [{ "url": "/uploads/a.png", "mimeType": "image/png" }] }));
    let refused = prepare(&fx.file, &json!({ "tableId": fx.table, "fieldId": poster.id, "recordId": local.id }));
    assert_eq!(refused, Err(ApiError::BadRequest("Unsupported attachment URL: /uploads/a.png".into())));
    let none = fx.record(json!({}));
    let refused = prepare(&fx.file, &json!({ "tableId": fx.table, "fieldId": poster.id, "recordId": none.id }));
    assert_eq!(refused, Err(ApiError::BadRequest("Source field contains no usable images".into())));
}

/// [`apply`] as the AI field origin, as the binding runs it.
fn apply_as_ai(file: &mut TableFile, doc: &DocRef, params: &Value) -> ApiResult<Value> {
    let previous = file.origin().clone();
    file.set_origin(apply_origin());
    let answer = apply(file, doc, params);
    file.set_origin(previous);
    answer
}

#[test]
fn apply_writes_text_results_as_the_ai_field_origin() {
    let mut fx = fixture();
    let summary = fx.ai_field("Summary", "text", json!({ "type": "summarize", "source_field_id": fx.body.id }));
    let first = fx.record(json!({ fx.body.name.clone(): "First" }));
    let second = fx.record(json!({ fx.body.name.clone(): "Second" }));
    let doc = DocRef { document_id: 1, user_id: 0 };
    let params = json!({
        "tableId": fx.table,
        "fieldId": summary.id,
        "results": [
            { "recordId": first.id, "text": "  The first one.  " },
            { "recordId": second.id, "error": "The model is busy" },
            { "recordId": first.id, "text": "ignored: a record runs once" },
        ],
    });
    let answer = apply_as_ai(&mut fx.file, &doc, &params).unwrap();
    assert_eq!((answer["accepted"].as_i64(), answer["completed"].as_i64(), answer["failed"].as_i64()), (Some(2), Some(1), Some(1)));
    assert_eq!(answer["limit"], BATCH_MAX_ROWS);
    assert_eq!(answer["errors"], json!([{ "record_id": second.id, "message": "The model is busy" }]));
    assert_eq!(answer["field"]["id"], summary.id);
    let item = &answer["items"][0];
    assert_eq!(item["record"]["values"][&summary.name], "The first one.");
    assert_eq!(item["state"]["status"], "done");
    assert_eq!(item["state"]["source_hash"], format!("sha256:{}", sha256("First")));
    assert_eq!(fx.file.get_record(fx.table, first.id).unwrap().values[&summary.name], "The first one.");
    // The writes were the AI field's (a system write leaves no actor).
    let (history, _) = fx.file.list_record_history(fx.table, first.id, 10, 0).unwrap();
    assert!(history.iter().any(|entry| entry.actor.is_empty()), "a system write leaves no actor: {history:?}");
    // One cell: an empty answer is an error, as the cell route's was.
    let empty =
        apply_as_ai(&mut fx.file, &doc, &json!({ "tableId": fx.table, "fieldId": summary.id, "recordId": second.id, "results": [{ "text": " " }] }));
    assert_eq!(empty, Err(ApiError::BadRequest("AI returned an empty response".into())));
}

#[test]
fn apply_stores_generated_images_in_the_file() {
    let mut fx = fixture();
    let cover = fx.ai_field("Cover", "attachment", json!({ "type": "image_generation", "source_field_id": fx.body.id }));
    let record = fx.record(json!({ fx.body.name.clone(): "A red fox" }));
    let doc = DocRef { document_id: 1, user_id: 0 };
    let content = base64::engine::general_purpose::STANDARD.encode(PNG);
    let params = json!({
        "tableId": fx.table,
        "fieldId": cover.id,
        "recordId": record.id,
        "results": [{ "recordId": record.id, "images": [{ "mime": "image/png", "content": content }] }],
    });
    let answer = apply_as_ai(&mut fx.file, &doc, &params).unwrap();
    assert_eq!(answer["field"]["id"], cover.id);
    let value = &answer["record"]["values"][&cover.name][0];
    let hash = blobs::blob_hash(PNG);
    assert_eq!(value["url"], format!("kition-blob:{hash}"));
    assert_eq!(value["name"], "generated-1.png");
    assert_eq!(value["mimeType"], "image/png");
    assert_eq!(fx.file.blob(&hash).unwrap().unwrap().bytes, PNG);
    assert!(answer["state"]["source_hash"].as_str().unwrap().starts_with("sha256:"));
    // No images is an error.
    let none =
        apply_as_ai(&mut fx.file, &doc, &json!({ "tableId": fx.table, "fieldId": cover.id, "recordId": record.id, "results": [{ "images": [] }] }));
    assert_eq!(none, Err(ApiError::BadRequest("AI image generation returned no assets".into())));
}
