//! # kition-table-import
//!
//! Importing CSV, TSV, and XLSX files into a table document: parsing with type inference
//! ([`parse`]), and writing the rows into an open [`TableFile`] ([`write`]): fields created or
//! adapted, values converted to the field types, and new records written in batches. Plain
//! functions, no server, no jobs: [`preview`] and [`run`] take the JSON params of the engine
//! commands `file.previewImport` and `file.runImport` and answer the JSON of Kition's
//! `contracts/runtime/data-import.schema.json` (`DataImportPreviewResponse`, and the job's
//! `DataImportResult`).
//!
//! Kition's import wrote as [`import_origin`] (`import:import`): not a person's or an agent's, and
//! never an undo step. The functions here write as the file's current origin; the caller sets it.

pub mod parse;
pub mod write;

use base64::Engine as _;
use kition_kdm::{Origin, OriginKind};
use kition_table_api::{ApiError, ApiResult};
use kition_table_file::TableFile;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use parse::{ImportField, ImportPreview, ImportSheet, ImportSource, ImportWarning};
pub use write::{ExecuteRequest, FieldOverride, ImportResult, Target};

/// The largest file an import reads.
pub const MAX_SOURCE_BYTES: usize = 100 << 20;

/// The principal of [`import_origin`].
pub const IMPORT_PRINCIPAL: &str = "import";

/// An import write's origin (`import:import`): not a person or an agent, and (like `system`) not
/// an undo step.
pub fn import_origin() -> Origin {
    Origin::new(OriginKind::Import, IMPORT_PRINCIPAL)
}

/// A float as JSON, written as an integer when it has no fraction.
pub fn float_value(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        Value::from(value as i64)
    } else {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// Shortest decimal text for a float, without exponent for whole numbers.
pub fn format_float(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 { (value as i64).to_string() } else { value.to_string() }
}

/// Deserializes `null` (and a missing field, with `#[serde(default)]`) as the default value.
pub(crate) fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// The file both commands carry, where Kition's routes took a multipart upload or read a
/// workspace file: `fileName`, `content` (standard base64 of the file's bytes), and the request's
/// own `source` and `sheet`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SourceParams {
    #[serde(default, rename = "fileName", deserialize_with = "nullable")]
    pub file_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub content: String,
    #[serde(default)]
    pub source: Option<WorkspaceSource>,
    #[serde(default, deserialize_with = "nullable")]
    pub sheet: String,
}

/// A preview request's `source`: `{ kind: "workspace", workspace_path }` names the workspace file
/// the host read the bytes from; anything else is an upload named by `fileName`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct WorkspaceSource {
    #[serde(default, deserialize_with = "nullable")]
    pub kind: String,
    #[serde(default, deserialize_with = "nullable")]
    pub workspace_path: String,
}

/// A source file, read from the params.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub filename: String,
    pub source: ImportSource,
    pub sheet: String,
    pub bytes: Vec<u8>,
}

/// A workspace-relative path with `/` separators, without `.` segments; `None` when it is
/// absolute or climbs out with `..`.
fn clean_workspace_path(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    if path.starts_with('/') || path.chars().nth(1) == Some(':') {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            part => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or_default().trim().to_owned()
}

/// Reads the source file from the params, as Kition's `read_source` read an upload or a
/// workspace file.
pub fn read_source(params: &SourceParams) -> ApiResult<SourceFile> {
    let workspace = params.source.as_ref().filter(|source| source.kind == "workspace");
    let (filename, source) = match workspace {
        Some(source) => {
            let cleaned = clean_workspace_path(&source.workspace_path).ok_or_else(|| ApiError::BadRequest("Import source path is invalid".into()))?;
            let filename = match base_name(&params.file_name) {
                name if name.is_empty() => base_name(&cleaned),
                name => name,
            };
            (filename, ImportSource { kind: "workspace".into(), workspace_path: cleaned, ..ImportSource::default() })
        }
        None => {
            let filename = base_name(&params.file_name);
            if filename.is_empty() {
                return Err(ApiError::BadRequest("Import file is required".into()));
            }
            (filename.clone(), ImportSource { kind: "upload".into(), upload_name: filename, ..ImportSource::default() })
        }
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(params.content.trim())
        .map_err(|_| ApiError::BadRequest("Import file content must be base64".into()))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(ApiError::BadRequest(format!("import file exceeds {} MB", MAX_SOURCE_BYTES >> 20)));
    }
    Ok(SourceFile { filename, source, sheet: params.sheet.trim().to_owned(), bytes })
}

/// The preview's `import_token`: the SHA-256 of the file's bytes (`sha256:<hex>`). The kit keeps
/// no previews; `file.runImport` carries the file again and, when it names a token, checks it.
pub fn import_token(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

/// Parses a source file into its preview and its normalized rows (header first).
pub fn parse_source(file: &SourceFile) -> ApiResult<(ImportPreview, Vec<Vec<String>>)> {
    let mut parsed = parse::parse(&file.filename, file.source.clone(), &file.sheet, &file.bytes).map_err(ApiError::BadRequest)?;
    if parsed.field_count == 0 {
        return Err(ApiError::BadRequest("Import source does not contain a header row".into()));
    }
    let rows = parse::data_rows(&file.bytes, &file.filename, &parsed.selected_sheet);
    parsed.import_token = import_token(&file.bytes);
    Ok((parsed, rows))
}

/// `file.previewImport`: the preview request's params (`source`, `sheet`) with `fileName` and
/// `content`; answers `DataImportPreviewResponse`.
pub fn preview(params: &Value) -> ApiResult<Value> {
    let params: SourceParams = serde_json::from_value(params.clone()).map_err(|_| ApiError::InvalidBody)?;
    let source = read_source(&params)?;
    let (preview, _) = parse_source(&source)?;
    Ok(serde_json::to_value(preview)?)
}

/// `file.runImport`: the execute request's params (`target`, `write_mode`, `schema_mode`,
/// `unique_by`, `field_overrides`, and optionally `import_token`) with `fileName`, `content`,
/// and `sheet` (the preview's `selected_sheet`); writes the rows into `file` and answers
/// `DataImportResult`. `document_id` and `path` are what the result names the document by.
///
/// Writes act as the file's current origin: run it as [`import_origin`], as Kition's import ran.
/// Writes made before a failure stay: a caller that needs the import all or nothing restores the
/// document it held before (the kit's binding does).
pub fn run(file: &mut TableFile, document_id: i64, path: &str, params: &Value) -> ApiResult<Value> {
    let request: ExecuteRequest = serde_json::from_value(params.clone()).map_err(|_| ApiError::InvalidBody)?;
    let source: SourceParams = serde_json::from_value(params.clone()).map_err(|_| ApiError::InvalidBody)?;
    write::validate(&request).map_err(|message| ApiError::BadRequest(message.into()))?;
    let source = read_source(&source)?;
    let (preview, rows) = parse_source(&source)?;
    if !request.import_token.trim().is_empty() && request.import_token.trim() != preview.import_token {
        return Err(ApiError::BadRequest("Import preview token is invalid or expired".into()));
    }
    let fields = write::apply_overrides(&preview.fields, &request.field_overrides);
    let result = write::write_import(file, document_id, path, request, &rows, fields).map_err(ApiError::BadRequest)?;
    Ok(serde_json::to_value(result)?)
}

#[cfg(test)]
mod tests;
