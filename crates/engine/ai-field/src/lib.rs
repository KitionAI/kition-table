//! # kition-table-ai
//!
//! AI fields of a table document: a cell (or a batch of cells) filled by a model from the field's
//! `options.ai_config` and an optional source field, as Kition's runtime ran them, without the
//! model call. The engine stays offline: [`prepare`] reads the configuration and the records and
//! answers the prompts (text, and the images of the source cell: blobs held in the file by hash,
//! or remote URLs); the host runs them with its own model settings (the Kit UI protocol's
//! `host/generate`); [`apply`] writes what the host answered into the cells (generated images
//! become blobs held in the file) and answers what Kition's `.../ai/run` routes answered.
//!
//! Kition's runtime wrote AI cells as [`apply_origin`] (`system:aifield`): never an undo step.
//! [`apply`] writes as the file's current origin; the caller sets it.

pub mod image_prompt;

use std::collections::HashSet;

use base64::Engine as _;
use kition_kdm::Origin;
use kition_table_api::records::stringify_cell;
use kition_table_api::{ApiError, ApiResult, DocRef, parse_id, respond};
use kition_table_file::blobs::{self, blob_hash_of_url};
use kition_table_file::{Field, Record, TableFile};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

pub const MAX_SOURCE_CHARS: usize = 8000;
pub const MAX_OUTPUT_CHARS: usize = 20_000;
/// The most records one batch run takes.
pub const BATCH_MAX_ROWS: usize = 100;
/// The most images of the source cell a prompt carries.
pub const MAX_IMAGES: usize = 4;
pub const MAX_IMAGE_BYTES: u64 = 20 << 20;
pub const MAX_IMAGE_COUNT: i64 = 5;

/// The label of [`apply_origin`].
pub const APPLY_LABEL: &str = "aifield";

/// The origin of AI field cell writes (`system:aifield`).
pub fn apply_origin() -> Origin {
    Origin::system(APPLY_LABEL)
}

/// A parsed `ai_config`.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub raw: Value,
    pub action: String,
    pub source_field_id: i64,
    pub source_field_name: String,
    pub target_language: String,
    pub extract_instruction: String,
    pub improve_instruction: String,
    pub summary_instruction: String,
    pub custom_prompt: String,
    pub requirements: String,
    pub image_size: String,
    pub image_count: i64,
    pub image_quality: String,
    pub image_use_case: String,
    pub categories: Vec<String>,
    pub max_words: i64,
    pub extract_schema: String,
    /// The field's model preference (`runtime_model`), empty when it has none.
    pub model: String,
}

impl Config {
    pub fn is_image(&self) -> bool {
        matches!(self.action.as_str(), "image_generation" | "image_customization")
    }

    fn requires_source(&self) -> bool {
        !matches!(self.action.as_str(), "text_generation" | "customize" | "image_customization")
    }

    fn resolves_source(&self) -> bool {
        self.requires_source() || (self.action == "image_customization" && (self.source_field_id > 0 || !self.source_field_name.is_empty()))
    }
}

fn text(map: &Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .filter_map(|key| map.get(*key))
        .map(|value| match value {
            Value::String(text) => text.trim().to_owned(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .find(|text| !text.is_empty())
        .unwrap_or_default()
}

fn int(map: &Map<String, Value>, keys: &[&str]) -> i64 {
    keys.iter()
        .filter_map(|key| map.get(*key))
        .filter_map(|value| match value {
            Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        })
        .find(|n| *n != 0)
        .unwrap_or(0)
}

fn is_false(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => !flag,
        Some(Value::String(text)) => matches!(text.trim().to_lowercase().as_str(), "false" | "0" | ""),
        Some(Value::Number(n)) => n.as_f64() == Some(0.0),
        _ => false,
    }
}

fn aspect_to_size(aspect: &str) -> &'static str {
    match aspect {
        "16:9" | "21:9" | "3:2" | "4:3" => "1536x1024",
        "9:16" | "2:3" | "3:4" => "1024x1536",
        _ => "1024x1024",
    }
}

/// Whether the field is an AI field: of type `ai`, or with an `ai_config` that is not disabled.
pub fn has_ai_config(field: &Field) -> bool {
    if field.field_type == "ai" {
        return true;
    }
    match field.options.get("ai_config") {
        Some(Value::Object(config)) => !is_false(config.get("enabled")) || config.get("enabled").is_none(),
        _ => false,
    }
}

/// Parses and checks a field's `options.ai_config` (a legacy `ai_prompt` is a `customize`
/// action).
pub fn parse_config(options: &Map<String, Value>, ai_prompt: &str) -> Result<Config, String> {
    let raw = options.get("ai_config").and_then(Value::as_object).cloned().unwrap_or_default();
    let aspect = text(&raw, &["aspect_ratio"]);
    let mut image_size = if aspect.is_empty() { String::new() } else { aspect_to_size(&aspect).to_owned() };
    if image_size.is_empty() {
        image_size = text(&raw, &["size", "image_size"]);
    }
    let mut config = Config {
        action: text(&raw, &["type", "action_type"]),
        source_field_id: int(&raw, &["source_field_id"]),
        source_field_name: text(&raw, &["source_field_name"]),
        target_language: text(&raw, &["target_language"]),
        extract_instruction: text(&raw, &["extract_instruction"]),
        improve_instruction: text(&raw, &["improve_instruction"]),
        summary_instruction: text(&raw, &["summary_instruction"]),
        custom_prompt: text(&raw, &["prompt", "custom_prompt"]),
        requirements: text(&raw, &["requirements"]),
        image_size,
        image_count: int(&raw, &["n", "image_count"]),
        image_quality: text(&raw, &["quality", "image_quality"]),
        image_use_case: text(&raw, &["image_use_case"]),
        categories: raw
            .get("categories")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|c| c.trim().to_owned())
            .filter(|c| !c.is_empty())
            .collect(),
        max_words: int(&raw, &["max_words"]),
        extract_schema: text(&raw, &["schema"]),
        model: text(&raw, &["runtime_model"]),
        raw: Value::Object(raw.clone()),
    };
    if config.action.is_empty() && !ai_prompt.trim().is_empty() {
        config.action = "customize".into();
        config.custom_prompt = ai_prompt.trim().to_owned();
    }
    if config.action.is_empty() {
        return Err("AI field has no action type configured".into());
    }
    if is_false(raw.get("enabled")) {
        return Err("AI field configuration is disabled".into());
    }
    if config.requires_source() && config.source_field_id <= 0 && config.source_field_name.is_empty() {
        return Err("AI field has no source field configured".into());
    }
    match config.action.as_str() {
        "summarize"
        | "translate"
        | "extract"
        | "improve"
        | "customize"
        | "image_generation"
        | "image_customization"
        | "text_generation"
        | "classify" => {}
        _ => return Err("Unsupported AI action type".into()),
    }
    if config.action == "translate" && config.target_language.is_empty() {
        return Err("Translation action requires a target language".into());
    }
    if config.action == "extract" && config.extract_instruction.is_empty() && config.extract_schema.is_empty() {
        return Err("Extraction action requires extraction instructions".into());
    }
    if matches!(config.action.as_str(), "customize" | "text_generation" | "image_customization") && config.custom_prompt.is_empty() {
        return Err("Action requires a prompt".into());
    }
    if config.action == "classify" && config.categories.len() < 2 {
        return Err("Classify action requires at least two categories".into());
    }
    if config.is_image() {
        if config.image_size.is_empty() {
            config.image_size = "1024x1024".into();
        }
        if config.image_quality.is_empty() {
            config.image_quality = "low".into();
        }
        if config.image_count <= 0 {
            config.image_count = 3;
        }
        config.image_count = config.image_count.min(MAX_IMAGE_COUNT);
        if !["1024x1024", "1024x1536", "1536x1024"].contains(&config.image_size.as_str()) {
            return Err("Image generation has an invalid image size".into());
        }
        if !["low", "medium", "high"].contains(&config.image_quality.as_str()) {
            return Err("Image generation has an invalid image quality".into());
        }
        config.image_use_case = image_prompt::parse_use_case(&config.image_use_case).to_owned();
    }
    Ok(config)
}

/// The stored config, with the client's per-record prompt applied (`action` and `config` of
/// the run request).
pub fn apply_execution(stored: Config, action: &str, raw: &Map<String, Value>) -> Result<Config, String> {
    if raw.is_empty() {
        return Ok(stored);
    }
    let mut options = Map::new();
    options.insert("ai_config".into(), Value::Object(raw.clone()));
    let execution = parse_config(&options, "")?;
    if !action.trim().is_empty() && action.trim() != stored.action {
        return Err("AI execution action does not match the field configuration".into());
    }
    if execution.action != stored.action {
        return Err("AI execution config does not match the field configuration".into());
    }
    let mut stored = stored;
    if !execution.custom_prompt.is_empty() {
        stored.custom_prompt = execution.custom_prompt;
    }
    Ok(stored)
}

/// The AI field, its source field, and its config.
pub fn resolve_field(fields: &[Field], field_id: i64) -> Result<(Field, Option<Field>, Config), String> {
    let field = fields.iter().find(|f| f.id == field_id).cloned().ok_or("AI field does not exist")?;
    if !has_ai_config(&field) {
        return Err("Target field is not an AI field".into());
    }
    let config = parse_config(&field.options, &field.ai_prompt)?;
    if !config.resolves_source() {
        return Ok((field, None, config));
    }
    let source = fields
        .iter()
        .find(|f| {
            (config.source_field_id > 0 && f.id == config.source_field_id)
                || (!config.source_field_name.is_empty() && f.name == config.source_field_name)
        })
        .cloned()
        .ok_or("Source field does not exist")?;
    if source.id == field.id {
        return Err("An AI field cannot reference itself".into());
    }
    let attachment_into_image = config.action == "image_customization" && source.field_type.trim().eq_ignore_ascii_case("attachment");
    if has_ai_config(&source) && !attachment_into_image {
        return Err("Source field cannot be an AI field".into());
    }
    Ok((field, Some(source), config))
}

fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The text prompt for one cell.
pub fn build_prompt(config: &Config, field_title: &str, source_title: &str, source_text: &str) -> String {
    let mut out = String::from(
        "You are filling a single cell in a structured table.\nReturn only the final cell value. Do not include explanations, markdown fences, or extra labels.\n\n",
    );
    out.push_str(&format!("Target field: {}\n", field_title.trim()));
    if source_title.trim().is_empty() {
        out.push('\n');
    } else {
        out.push_str(&format!("Source field: {}\n\n", source_title.trim()));
    }
    match config.action.as_str() {
        "summarize" => {
            out.push_str("Task: Summarize the source content.\n");
            if !config.summary_instruction.is_empty() {
                out.push_str(&format!("Summary instruction: {}\n", config.summary_instruction));
            }
            if config.max_words > 0 {
                out.push_str(&format!("Maximum word count: {}\n", config.max_words));
            }
        }
        "translate" => out.push_str(&format!("Task: Translate the source content.\nTarget language: {}\n", config.target_language)),
        "extract" => {
            out.push_str("Task: Extract information from the source content.\n");
            if !config.extract_instruction.is_empty() {
                out.push_str(&format!("Extraction instruction: {}\n", config.extract_instruction));
            }
            if !config.extract_schema.is_empty() {
                out.push_str(&format!("Return JSON matching this schema: {}\n", config.extract_schema));
            }
        }
        "improve" => {
            out.push_str("Task: Improve the source content.\n");
            if !config.improve_instruction.is_empty() {
                out.push_str(&format!("Improvement instruction: {}\n", config.improve_instruction));
            }
        }
        "customize" => out.push_str(&format!("Task: Follow this custom instruction.\n{}\n", config.custom_prompt)),
        "text_generation" => out.push_str(&format!("Task: Generate text following this instruction.\n{}\n", config.custom_prompt)),
        "classify" => out.push_str(&format!(
            "Task: Classify the source content into exactly one of the categories below.\nCategories: {}\nReturn only the chosen category label, exactly as written above.\n",
            config.categories.join(", ")
        )),
        _ => {}
    }
    if !config.requirements.is_empty() {
        out.push_str(&format!("Additional requirements: {}\n", config.requirements));
    }
    if !source_text.is_empty() {
        out.push_str("\nSource content:\n");
        out.push_str(source_text);
    }
    out
}

/// The image prompt for one cell; empty when there is nothing to draw.
pub fn image_prompt_for(config: &Config, source: &Value) -> String {
    let mut prompt = if config.action == "image_customization" { config.custom_prompt.clone() } else { stringify_cell(source).trim().to_owned() };
    if !config.requirements.is_empty() {
        prompt = if prompt.is_empty() { config.requirements.clone() } else { format!("{prompt}\n{}", config.requirements) };
    }
    let prompt = truncate(prompt.trim(), MAX_SOURCE_CHARS);
    if prompt.is_empty() {
        return String::new();
    }
    image_prompt::wrap(&prompt, &config.image_use_case, &config.image_size)
}

/// One image of a source cell: a blob held in the file, or a remote or inline URL.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceImage {
    pub hash: Option<String>,
    pub url: Option<String>,
    pub mime: String,
    /// What Kition hashed for the cell's state: the image as a `data:` URL (a blob's bytes), or
    /// its URL.
    pub digest: String,
}

impl SourceImage {
    fn part(&self) -> Value {
        let mut part = json!({ "type": "image", "mime": self.mime });
        if let Some(hash) = &self.hash {
            part["hash"] = Value::from(hash.clone());
        }
        if let Some(url) = &self.url {
            part["url"] = Value::from(url.clone());
        }
        part
    }
}

fn item_text<'a>(item: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().find_map(|key| item[*key].as_str().map(str::trim).filter(|text| !text.is_empty())).unwrap_or_default()
}

/// The image attachments of a cell (at most [`MAX_IMAGES`]), as Kition's runtime read them:
/// blobs of the file (whose stored MIME type stands in for a missing one), and `data:`,
/// `http:`, or `https:` URLs. Any other URL names a file the engine cannot read.
pub fn cell_images(file: &TableFile, value: &Value) -> Result<Vec<SourceImage>, String> {
    let mut out = Vec::new();
    for entry in value.as_array().into_iter().flatten() {
        let mut mime = item_text(entry, &["mimeType", "mime_type"]).to_owned();
        let url = item_text(entry, &["url"]);
        if let Some(hash) = blob_hash_of_url(url) {
            let blob = file.blob(hash).map_err(|err| err.to_string())?;
            let Some(blob) = blob else {
                return Err(format!("Unsupported attachment URL: {url}"));
            };
            if mime.is_empty() {
                mime = blob.info.mime.clone();
            }
            if !mime.to_lowercase().starts_with("image/") {
                continue;
            }
            if blob.info.size > MAX_IMAGE_BYTES {
                return Err(format!("Attachment exceeds {}MB", MAX_IMAGE_BYTES >> 20));
            }
            let data_url = format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&blob.bytes));
            out.push(SourceImage { hash: Some(blob.info.hash.clone()), url: None, digest: sha256(&data_url), mime });
        } else {
            if !mime.to_lowercase().starts_with("image/") || url.is_empty() {
                continue;
            }
            if !(url.starts_with("data:") || url.starts_with("http://") || url.starts_with("https://")) {
                return Err(format!("Unsupported attachment URL: {url}"));
            }
            out.push(SourceImage { hash: None, url: Some(url.to_owned()), digest: sha256(url), mime });
        }
        if out.len() >= MAX_IMAGES {
            break;
        }
    }
    Ok(out)
}

/// An AI field ready to run: the field, its source, and the config with the run's prompt.
#[derive(Debug, Clone)]
pub struct Target {
    pub table_id: i64,
    pub field: Field,
    pub source: Option<Field>,
    pub config: Config,
}

/// Resolves the field of a run (`tableId`, `fieldId`, `action`, `config`).
pub fn load_target(file: &TableFile, table_id: i64, field_id: i64, action: &str, raw: &Map<String, Value>) -> ApiResult<Target> {
    let table = file.get_table(table_id).map_err(|_| ApiError::NotFound("Table does not exist".into()))?;
    let (field, source, config) = resolve_field(&table.fields, field_id).map_err(ApiError::BadRequest)?;
    let config = apply_execution(config, action, raw).map_err(ApiError::BadRequest)?;
    Ok(Target { table_id, field, source, config })
}

/// One cell's model request, and what its state hashes.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub messages: Vec<Value>,
    pub options: Option<Value>,
    /// The text whose SHA-256 is the cell state's `source_hash`.
    pub hash_input: String,
}

/// The model request for one record, or why it cannot run.
pub fn prepare_record(file: &TableFile, target: &Target, record: &Record) -> Result<Prepared, String> {
    let config = &target.config;
    let source = target.source.as_ref().and_then(|source| record.values.get(&source.name)).cloned().unwrap_or(Value::Null);
    let images = cell_images(file, &source)?;
    let digests: Vec<String> = images.iter().map(|image| image.digest.clone()).collect();
    let user = |text: &str| {
        let mut parts = vec![json!({ "type": "text", "text": text })];
        parts.extend(images.iter().map(SourceImage::part));
        vec![json!({ "role": "user", "parts": parts })]
    };
    if config.is_image() {
        if config.action == "image_customization" && config.resolves_source() && images.is_empty() {
            return Err("Source field contains no usable images".into());
        }
        let prompt = image_prompt_for(config, &source);
        if prompt.is_empty() {
            return Err(if config.requires_source() { "Source field is empty".into() } else { "Image generation requires a prompt".into() });
        }
        let (width, height) =
            config.image_size.split_once('x').and_then(|(w, h)| Some((w.parse::<i64>().ok()?, h.parse::<i64>().ok()?))).unwrap_or((1024, 1024));
        let mut hash_input = prompt.clone();
        if !digests.is_empty() {
            hash_input = format!("{hash_input}|{}", digests.join("|"));
        }
        return Ok(Prepared {
            messages: user(&prompt),
            options: Some(json!({
                "count": config.image_count,
                "size": config.image_size,
                "width": width,
                "height": height,
                "quality": config.image_quality,
                "useCase": config.image_use_case,
            })),
            hash_input,
        });
    }
    let source_text = if images.is_empty() { stringify_cell(&source).trim().to_owned() } else { String::new() };
    if config.requires_source() && source_text.is_empty() && images.is_empty() {
        return Err("Source field is empty".into());
    }
    let source_text = truncate(&source_text, MAX_SOURCE_CHARS);
    let source_title = target.source.as_ref().map(|source| source.title.clone()).unwrap_or_default();
    let prompt = build_prompt(config, &target.field.title, &source_title, &source_text);
    Ok(Prepared { messages: user(&prompt), options: None, hash_input: format!("{source_text}{}", digests.join("|")) })
}

/// The params of `ai.prepare` and `ai.apply`: the `.../ai/run` routes' body (`action`, `config`,
/// `record_ids`, `limit`) with the routes' path ids (`tableId`, `fieldId`, and `recordId` for one
/// cell).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RunParams {
    #[serde(rename = "tableId")]
    pub table_id: Value,
    #[serde(rename = "fieldId")]
    pub field_id: Value,
    #[serde(rename = "recordId")]
    pub record_id: Value,
    pub record_ids: Vec<i64>,
    pub limit: i64,
    pub action: String,
    pub config: Map<String, Value>,
    pub results: Vec<GeneratedResult>,
}

/// One record's generation, as the host answers it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GeneratedResult {
    #[serde(rename = "recordId", alias = "record_id")]
    pub record_id: i64,
    pub text: Option<String>,
    pub image: Option<GeneratedImage>,
    pub images: Vec<GeneratedImage>,
    pub error: Option<String>,
}

/// A generated image: its MIME type and its bytes in standard base64.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct GeneratedImage {
    pub mime: String,
    pub content: String,
}

fn id_of(value: &Value) -> ApiResult<i64> {
    match value {
        Value::Number(number) => parse_id(&number.to_string()),
        Value::String(text) => parse_id(text),
        _ => parse_id(""),
    }
}

fn parse_params(params: &Value) -> ApiResult<RunParams> {
    let params = if params.is_null() { json!({}) } else { params.clone() };
    serde_json::from_value(params).map_err(|_| ApiError::InvalidBody)
}

/// The records of a run: the one cell's record, or the batch's distinct positive ids, at most
/// `limit` (when below [`BATCH_MAX_ROWS`]) and [`BATCH_MAX_ROWS`].
fn selected_records(params: &RunParams) -> ApiResult<Option<Vec<i64>>> {
    if !params.record_id.is_null() {
        return Ok(None);
    }
    let max = match params.limit {
        n if n > 0 && (n as usize) < BATCH_MAX_ROWS => n as usize,
        _ => BATCH_MAX_ROWS,
    };
    let mut ids: Vec<i64> = Vec::new();
    for id in &params.record_ids {
        if *id > 0 && !ids.contains(id) && ids.len() < max {
            ids.push(*id);
        }
    }
    if ids.is_empty() {
        return Err(ApiError::BadRequest("Please select the records to generate".into()));
    }
    Ok(Some(ids))
}

/// `ai.prepare`: the model requests of one cell (`recordId`) or of a batch (`record_ids`):
/// `{ fieldId, output: "text" | "image", model?, requests: [{ recordId, messages, options? }],
/// skipped: [{ recordId, reason }] }`. One cell that cannot run is an error, as the cell route's
/// was; records of a batch that cannot run are skipped with the reason the batch route gave.
pub fn prepare(file: &TableFile, params: &Value) -> ApiResult<Value> {
    let params = parse_params(params)?;
    let table_id = id_of(&params.table_id)?;
    let field_id = id_of(&params.field_id)?;
    let batch = selected_records(&params)?;
    let target = load_target(file, table_id, field_id, &params.action, &params.config)?;
    let record_ids = match &batch {
        Some(ids) => ids.clone(),
        None => vec![id_of(&params.record_id)?],
    };
    let mut requests = Vec::new();
    let mut skipped = Vec::new();
    for record_id in record_ids {
        let prepared = file
            .get_record(table_id, record_id)
            .map_err(|_| "Record does not exist".to_owned())
            .and_then(|record| prepare_record(file, &target, &record));
        match prepared {
            Ok(prepared) => {
                let mut request = json!({ "recordId": record_id, "messages": prepared.messages });
                if let Some(options) = prepared.options {
                    request["options"] = options;
                }
                requests.push(request);
            }
            Err(reason) if batch.is_some() => skipped.push(json!({ "recordId": record_id, "reason": reason })),
            Err(reason) => return Err(ApiError::BadRequest(reason)),
        }
    }
    let mut answer = json!({
        "fieldId": field_id,
        "output": if target.config.is_image() { "image" } else { "text" },
        "requests": requests,
        "skipped": skipped,
    });
    if !target.config.model.is_empty() {
        answer["model"] = Value::from(target.config.model.clone());
    }
    Ok(answer)
}

fn extension_of(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        _ => ".png",
    }
}

/// The cell value a result gives: the text, or the images stored as blobs of the file.
fn output_of(file: &mut TableFile, target: &Target, result: &GeneratedResult) -> Result<Value, String> {
    if let Some(error) = result.error.as_deref().map(str::trim).filter(|error| !error.is_empty()) {
        return Err(error.to_owned());
    }
    if !target.config.is_image() {
        let output = truncate(result.text.as_deref().unwrap_or_default().trim(), MAX_OUTPUT_CHARS);
        if output.is_empty() {
            return Err("AI returned an empty response".into());
        }
        return Ok(Value::String(output));
    }
    let images: Vec<&GeneratedImage> = result.image.iter().chain(&result.images).collect();
    if images.is_empty() {
        return Err("AI image generation returned no assets".into());
    }
    let mut attachments = Vec::new();
    for (index, image) in images.iter().enumerate() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image.content.trim())
            .map_err(|_| "Generated image content must be base64".to_owned())?;
        if bytes.is_empty() {
            return Err("AI image generation returned no assets".into());
        }
        if bytes.len() > blobs::MAX_BLOB_BYTES {
            return Err(format!("Attachment cannot exceed {}MB", blobs::MAX_BLOB_BYTES >> 20));
        }
        let mime = if image.mime.trim().is_empty() { "image/png" } else { image.mime.trim() };
        let name = format!("generated-{}{}", index + 1, extension_of(mime));
        let info = file.put_blob(&bytes, mime, &name).map_err(|_| "Failed to save generated image".to_owned())?;
        attachments.push(blobs::attachment_value(&info, &name));
    }
    Ok(Value::Array(attachments))
}

/// Writes one record's result and answers the record and the cell's state.
fn fill(file: &mut TableFile, target: &Target, result: &GeneratedResult) -> Result<(Record, Value), String> {
    let record = file.get_record(target.table_id, result.record_id).map_err(|_| "Record does not exist".to_owned())?;
    let prepared = prepare_record(file, target, &record)?;
    let output = output_of(file, target, result)?;
    let mut values = Map::new();
    values.insert(target.field.name.clone(), output);
    let updated = file.update_record(target.table_id, record.id, &values, None).map_err(|err| err.to_string())?;
    let state = json!({
        "status": "done",
        "source_hash": format!("sha256:{}", sha256(&prepared.hash_input)),
        "config_hash": format!("sha256:{}", sha256(&target.config.raw.to_string())),
        "model_id": 0,
        "updated_at": kition_time::now_millis(),
        "error": "",
    });
    Ok((updated, state))
}

/// `ai.apply`: writes what the host generated (`results`) into the field's cells, as the file's
/// current origin (run it as [`apply_origin`], as Kition's runtime wrote them). With `recordId` (one cell) it answers `RunDataAIFieldCellResponse`
/// (`{ record, field, state }`) and a failed result is an error; otherwise
/// `RunDataAIFieldBatchResponse` (`{ items, field, accepted, completed, failed, errors, limit }`).
pub fn apply(file: &mut TableFile, doc: &DocRef, params: &Value) -> ApiResult<Value> {
    let params = parse_params(params)?;
    let table_id = id_of(&params.table_id)?;
    let field_id = id_of(&params.field_id)?;
    let cell = if params.record_id.is_null() { None } else { Some(id_of(&params.record_id)?) };
    let target = load_target(file, table_id, field_id, &params.action, &params.config)?;
    apply_results(file, doc, &target, cell, &params.results)
}

fn apply_results(file: &mut TableFile, doc: &DocRef, target: &Target, cell: Option<i64>, results: &[GeneratedResult]) -> ApiResult<Value> {
    if let Some(record_id) = cell {
        let result = results.iter().find(|result| result.record_id == record_id || result.record_id == 0);
        let result = result.ok_or_else(|| ApiError::BadRequest("AI returned an empty response".into()))?;
        let result = GeneratedResult { record_id, ..result.clone() };
        let (record, state) = fill(file, target, &result).map_err(ApiError::BadRequest)?;
        return Ok(json!({
            "record": respond::record(doc, &record),
            "field": respond::field(doc, &target.field),
            "state": state,
        }));
    }
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    let mut errors = Vec::new();
    let mut accepted = 0;
    for result in results {
        if result.record_id <= 0 || !seen.insert(result.record_id) || accepted >= BATCH_MAX_ROWS {
            continue;
        }
        accepted += 1;
        match fill(file, target, result) {
            Ok((record, state)) => items.push(json!({ "record": respond::record(doc, &record), "state": state })),
            Err(message) => errors.push(json!({ "record_id": result.record_id, "message": message })),
        }
    }
    Ok(json!({
        "items": items,
        "field": respond::field(doc, &target.field),
        "accepted": accepted,
        "completed": items.len(),
        "failed": errors.len(),
        "errors": errors,
        "limit": BATCH_MAX_ROWS,
    }))
}

#[cfg(test)]
mod tests;
