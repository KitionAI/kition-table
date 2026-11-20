//! The `.ktable` table document format.
//!
//! A `.ktable` file is a self-contained SQLite database holding one data
//! document: its tables, fields, views, view configuration, records, record
//! links, history, and attachments (blobs held in the file, see [`blobs`]). Every timestamp is
//! Unix milliseconds.

pub mod blobs;
mod bulk;
mod computed;
mod dashboards;
mod file;
mod history;
pub mod identity;
pub mod ids;
mod json;
pub mod kdm;
mod links;
mod schema;
mod source;
mod view_config;

pub use blobs::{Blob, BlobInfo};
pub use bulk::{BulkCopyResult, BulkDeleteResult};
pub use computed::formula_references;
pub use dashboards::Dashboard;
pub use file::{CreateOptions, FORMAT, FORMAT_VERSION, Info, SCHEMA_VERSION, TableFile};
pub use history::{HistoryAction, HistoryEntry};
pub use json::internal_name;
pub use kdm::changes::{RecordChange, RecordChanges};
pub use kdm::commit::{MergeOutcome, ReplicaImport};
pub use kdm::ops::{CellEdit, CreateAt, TableOp};
pub use kdm::project::TableChange;
pub use links::LinkRef;
pub use source::{
    Attachment, AttachmentSpec, FIELD_TYPES, Field, FieldPatch, FieldSpec, Record, Table, TablePatch, TableSpec, VIEW_TYPES, View, ViewPatch,
    ViewSpec,
};
pub use view_config::{FilterRule, FilterTree, FilterTreeChild, ViewField, ViewFieldPatch, ViewGroup, ViewSort};

#[derive(Debug, thiserror::Error)]
pub enum TableFileError {
    /// Worded like the legacy driver's message, which API responses expose.
    #[error("sql: no rows in result set")]
    NotFound,
    #[error("ktable file already exists")]
    Exists,
    #[error("{0}")]
    Invalid(String),
    #[error("invalid filter tree: {0}")]
    InvalidFilterTree(String),
    #[error("ktable schema version {0} is newer than supported version {SCHEMA_VERSION}")]
    Newer(i64),
    #[error("kdm: {0}")]
    Kdm(String),
    /// The handle was opened read-only (see [`TableFile::open`]); it refuses every write.
    #[error("ktable file is read-only: {0}")]
    ReadOnly(String),
    #[error(transparent)]
    Sql(rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<rusqlite::Error> for TableFileError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::QueryReturnedNoRows => TableFileError::NotFound,
            other => TableFileError::Sql(other),
        }
    }
}

impl From<kition_kdm::KdmError> for TableFileError {
    fn from(error: kition_kdm::KdmError) -> Self {
        TableFileError::Kdm(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, TableFileError>;

impl TableFileError {
    pub fn invalid(message: impl Into<String>) -> Self {
        TableFileError::Invalid(message.into())
    }
}
