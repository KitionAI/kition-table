//! # kition-table-kit
//!
//! The Table engine for Kition's in-page editor: a Kition table document (`.ktable`, a
//! self-contained SQLite file holding its own KDM replica) opened in memory, edited, and saved,
//! from JavaScript (WebAssembly) on the Kition SDK.
//!
//! The engine is Kition's own: `kition-table-file` (the file format, SQLite compiled into the
//! module) and `kition-table-api` (the data-document requests, answered with exactly the JSON
//! Kition's runtime routes answer). [`TableDocument`] implements `KitDocument` (contract 1) over
//! them:
//!
//! - `call(method, params)` (an extra reached with `KitEngine.invoke('call', …)`) and the
//!   commands `table.<method>` run one `kition-table-api` request and answer
//!   `{ "result", "change" }`: the route's answer and the `TableChange` the request made (null
//!   when it changed nothing), in the shape `table/changed` carries.
//! - `documents.get` answers the one document, as the runtime's `GET /data-documents/{id}`
//!   does, with its tables and dashboard summaries.
//! - `edit.undo` and `edit.redo` are `undo` and `redo`.
//! - The replica members (`version`, `exportChanges`, `importChanges`, `docId`, `hasReplica`) are
//!   the file's own replica, so the document keeps its KDM identity across saves without the
//!   host keeping a replica. Imported changes queue their `TableChange` for `takeImported`.
//! - `render` is the document's picture (what the editor shows), and `table.file.render` (the
//!   manifest's `agent.render`) the same of a diagnostic's target, a table, a view or a range of
//!   its rows as PNG at a scale and size budget (KHP A39); the editor never draws with them.
//! - `save` answers the whole database, its change log compacted first when it is long, and
//!   vacuumed when more than a quarter of it is free pages.
//! - Attachments are blobs held in the file (`kition_table_file::blobs`): `attachments.put` stores a
//!   file's bytes and answers the attachment value naming them, `attachments.read` answers a
//!   blob's bytes, so an embedding application shows and adds pictures with nothing but the file.
//!   The replica's blob members (`blobs`, `blobsSince`, `addBlobs`) carry them to co-editors
//!   beside the changes that name them.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use base64::Engine as _;
pub use kition_kdm::Origin;
use kition_kdm::{OriginKind, UndoScope, Version};
use kition_kit::KitCommand;
/// The document contract used by this SDK, for native hosts without a separate SDK pin.
pub use kition_kit::KitDocument;
use kition_table_api::respond::{change_json, dashboards_of, tables_of};
use kition_table_api::tables::{default_table_seed, seed_to_spec};
use kition_table_api::{DocRef, METHODS};
use kition_table_file::{CreateOptions, TableChange, TableFile};
use serde_json::{Value, json};

pub mod acts;
mod address;
pub mod agent;
mod check;
mod context;
mod dependencies;
pub mod export;
mod files;
pub mod inspect;
#[cfg(feature = "headless")]
mod model;
mod params;
mod place;
mod preview;
#[cfg(target_arch = "wasm32")]
mod web;

pub use context::EDITOR_STATE;

/// The `kit.table` KDM schema (`model/schema.json`): the schema of the replica every `.ktable`
/// file holds, as Kition's `kition-table-file` embeds it, for the object model and its typings.
pub const MODEL_SCHEMA: &str = include_str!("../../../model/schema.json");

/// The document id requests and answers name: an embedded document is the only one there is.
pub const DOCUMENT_ID: i64 = 1;

/// The methods beyond `kition-table-api`'s [`METHODS`] that [`TableDocument::call`] answers.
pub const DOCUMENT_METHODS: &[(&str, &str)] = &[("documents.get", "GET /data-documents/{documentId}")];

/// The engine's own methods (`docs/features/engine-commands.md`): file import (`kition-table-import`), AI
/// fields without the model call (`kition-table-ai`), and the records changed since a version.
/// Each names the runtime route it replaces, or `-` when it replaces none.
pub const ENGINE_METHODS: &[(&str, &str)] = &[
    ("import.preview", "POST /data-imports/preview"),
    ("import.run", "POST /data-imports"),
    ("ai.prepare", "POST /tables/{tableId}/fields/{fieldId}/ai/run"),
    ("ai.apply", "POST /tables/{tableId}/fields/{fieldId}/ai/run"),
    ("changes.since", "-"),
    ("read", "-"),
    ("preview.show", "-"),
];

/// What the commands of [`ENGINE_METHODS`] take, for `commands()`. No word that names a file or
/// folder (see [`command`]).
const ENGINE_PARAMS: &[(&str, &str)] = &[
    ("import.preview", "{fileName, content (base64 of the bytes), source?, sheet?}: the data-import preview request"),
    (
        "import.run",
        "the data-import execute request (its destination table, write_mode, schema_mode, unique_by?, field_overrides?, import_token?) with fileName, content (base64 of the bytes), and sheet?",
    ),
    ("ai.prepare", "{tableId, fieldId, recordId? | record_ids?, limit?, action?, config?}: the prompts of an AI field run, for the host to generate"),
    (
        "ai.apply",
        "{tableId, fieldId, recordId?, action?, config?, results: [{recordId, text? | image? | images? | error?}]}: what the host generated",
    ),
    (
        "changes.since",
        "{version (base64 of a version; empty for every change)}: the records created, updated, and deleted since it, with their origins",
    ),
    (
        "read",
        "{tableId | viewId, limit?, cursor?}: the records of one table, or of one view (filtered and in its order), under their field names, a page at a time; an answer's next is the step for the rest",
    ),
    ("preview.show", "{tableId?, viewId?, dashboardId?}: the table, view or dashboard the document's picture shows; {} shows what the editor opens"),
];

/// The param that runs a mutating command as a `system` origin (a host feature's write, outside
/// the person's undo): `"system:<name>"`.
pub const AS_ORIGIN_PARAM: &str = "asOrigin";

/// The methods that only read: their commands do not mutate, so a read-only session runs them.
pub const READ_METHODS: &[&str] = &[
    "documents.get",
    "tables.list",
    "fields.list",
    "fields.linkCandidates",
    "views.list",
    "views.getFilterTree",
    "views.getSorts",
    "views.getGroups",
    "views.listFields",
    "records.list",
    "records.history",
    "links.list",
    "csv.export",
    "markdown.export",
    "historyState",
    "dashboards.list",
    "attachments.read",
    "import.preview",
    "ai.prepare",
    "changes.since",
    "read",
    "preview.show",
];

/// The command prefix of the data-document methods.
pub const COMMAND_PREFIX: &str = "table.";

/// `save` vacuums the database first when more than this share (in percent) of its pages are
/// free. Compacting the replica's change log rewrites its snapshot and frees the old one, so a
/// document edited for a while carries a quarter to a third of free pages; a whole-file save
/// would write them every time. Measured natively: `VACUUM` takes about 20 ms at 10,000 records
/// and 85 ms at 50,000, and shrinks such files by 25 to 35%.
pub const VACUUM_FREE_PERCENT: i64 = 25;

/// Vacuums the file's database when more than [`VACUUM_FREE_PERCENT`] of its pages are free.
/// Answers whether it did. Only free space goes: every row, and every row id the format relies
/// on (every table has an explicit `INTEGER PRIMARY KEY`), stays as it was.
pub fn vacuum_if_sparse(file: &TableFile) -> Result<bool, String> {
    let conn = file.conn();
    let pages: i64 = conn.query_row("PRAGMA page_count", [], |row| row.get(0)).map_err(error)?;
    let free: i64 = conn.query_row("PRAGMA freelist_count", [], |row| row.get(0)).map_err(error)?;
    if pages == 0 || free * 100 <= pages * VACUUM_FREE_PERCENT {
        return Ok(false);
    }
    conn.execute_batch("VACUUM").map_err(error)?;
    Ok(true)
}

/// Where a document is in its edit history, so that undoing back to the saved state makes it
/// saved again. Every state the edits reach is a step: an edit makes a new one and keeps the one
/// it left for undo; undo and redo go back to the steps they left. Each request is one undo step
/// of the replica (see [`TableDocument::call`]), so these steps follow the replica's undo stack.
/// A change undo does not revert (another replica's, or a `system` write) leaves the history:
/// no step before it holds the document any more, so none of them is saved.
#[derive(Debug, Clone, Default)]
struct History {
    step: u64,
    last: u64,
    undo: Vec<u64>,
    redo: Vec<u64>,
    /// The step the file holds, or `None` when no step holds what the file holds.
    saved: Option<u64>,
}

impl History {
    fn fresh(&mut self) -> u64 {
        self.last += 1;
        self.last
    }

    /// An edit: a new step, after the one it left, which undo goes back to.
    fn edited(&mut self) {
        self.undo.push(self.step);
        self.redo.clear();
        self.step = self.fresh();
    }

    /// A change undo does not revert.
    fn left(&mut self) {
        self.step = self.fresh();
        self.saved = None;
    }

    fn undone(&mut self) {
        self.redo.push(self.step);
        self.step = self.undo.pop().unwrap_or_else(|| self.fresh());
    }

    fn redone(&mut self) {
        self.undo.push(self.step);
        self.step = self.redo.pop().unwrap_or_else(|| self.fresh());
    }

    fn unsaved(&self) -> bool {
        self.saved != Some(self.step)
    }
}

/// One open table document.
pub struct TableDocument {
    file: TableFile,
    doc: DocRef,
    name: String,
    revision: u64,
    /// Where the document is in its edit history ([`History`]), and so whether it is unsaved.
    history: History,
    /// The history step the last `save` wrote, until `mark_saved` records it (`save` takes `&self`).
    save_pending: Cell<Option<u64>>,
    /// `TableChange`s of imported replica changes, as `table/changed` carries them, until taken.
    imported: Vec<Value>,
    /// What the document's picture shows (`preview.show`); not saved.
    shown: kition_table_picture::Shown,
    /// The picture of the last revision and `shown` drawn.
    picture: RefCell<Option<(u64, kition_table_picture::Shown, Arc<kition_table_picture::Picture>)>>,
}

fn error(err: impl std::fmt::Display) -> String {
    err.to_string()
}

/// A request's error as `execute` answers it: wrong parameters the engine found (a record
/// `values` key that names no field) as a [`kition_kit::schema::refusal`], which the host answers
/// as `invalid_params` with `data.errors`; any other as its message.
fn api_error(err: kition_table_api::ApiError) -> String {
    match err {
        kition_table_api::ApiError::InvalidParams(issues) => kition_kit::schema::refusal(
            &issues
                .into_iter()
                .map(|issue| kition_kit::schema::ParamError { path: issue.path, message: issue.message, expected: issue.expected })
                .collect::<Vec<_>>(),
        ),
        other => other.to_string(),
    }
}

/// Every method [`TableDocument::call`] answers, with the runtime route it mirrors.
fn all_methods() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    DOCUMENT_METHODS.iter().chain(METHODS).chain(ENGINE_METHODS)
}

/// Whether `method` is one [`TableDocument::call`] answers.
pub fn known_method(method: &str) -> bool {
    all_methods().any(|(name, _)| *name == method)
}

/// Every method [`TableDocument::call`] answers, with the runtime route it mirrors (`-` for
/// none), as a JSON array of `[method, route]`.
pub fn methods_json() -> String {
    let all: Vec<[&str; 2]> = all_methods().map(|(method, route)| [*method, *route]).collect();
    serde_json::to_string(&all).unwrap_or_else(|_| "[]".to_owned())
}

/// Takes [`AS_ORIGIN_PARAM`] out of `params`: `None` when absent, the `system` origin it names,
/// or an error for anything else.
fn take_as_origin(params: &mut Value) -> Result<Option<Origin>, String> {
    let Some(value) = params.as_object_mut().and_then(|map| map.remove(AS_ORIGIN_PARAM)) else {
        return Ok(None);
    };
    let refused = || format!("{AS_ORIGIN_PARAM} must be a system origin: \"system:<name>\"");
    let name = value.as_str().and_then(|text| text.strip_prefix("system:")).ok_or_else(refused)?;
    let valid = !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !valid {
        return Err(refused());
    }
    Ok(Some(Origin::system(name)))
}

/// The origin a method writes as when the caller names none: an import's, and the AI field
/// writes', as Kition's runtime wrote them.
fn method_origin(method: &str) -> Option<Origin> {
    match method {
        "import.run" => Some(kition_table_import::import_origin()),
        "ai.apply" => Some(kition_table_ai::apply_origin()),
        _ => None,
    }
}

/// The scope an `undo` or `redo` request works in: its `scope` (`person` or `agent`), else `own`.
fn undo_scope(params: &Value, own: UndoScope) -> UndoScope {
    params["scope"].as_str().and_then(UndoScope::parse).unwrap_or(own)
}

/// A KDM version from base64 (`""`: the empty version).
fn decode_version(text: &str) -> Result<Version, String> {
    if text.trim().is_empty() {
        return Ok(Version::empty());
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(text.trim()).map_err(|_| "version must be base64".to_owned())?;
    Version::decode(&bytes).map_err(error)
}

/// Whether the replica's undo reverts writes of `origin`: `system` and `import` writes are not
/// undo steps.
fn undoable(origin: &Origin) -> bool {
    !matches!(origin.kind, OriginKind::System | OriginKind::Import)
}

/// The origin imported changes carry (`import:sync`), as the runtime's replica routes name it.
fn import_origin() -> Origin {
    Origin::new(OriginKind::Import, kition_table_file::kdm::commit::IMPORT_PRINCIPAL)
}

impl TableDocument {
    /// Undo (or Redo) is offered while either scope has a step (KitModel M12a).
    fn can_step(&self, undo: bool) -> bool {
        [UndoScope::Person, UndoScope::Agent].into_iter().any(|scope| self.file.can_step_in(scope, undo))
    }
}

impl TableDocument {
    fn from_file(mut file: TableFile, name: &str) -> Self {
        // A person's edits are undo steps; `import:sync` changes from co-editors never are.
        file.set_origin(Origin::person("kit"));
        Self {
            file,
            doc: DocRef { document_id: DOCUMENT_ID, user_id: 0 },
            name: name.to_owned(),
            revision: 0,
            history: History { saved: Some(0), ..History::default() },
            save_pending: Cell::new(None),
            imported: Vec::new(),
            shown: kition_table_picture::Shown::default(),
            picture: RefCell::new(None),
        }
    }

    /// The document's picture: what the editor shows of it, or what `preview.show` picked
    /// ([`kition_table_picture`]); drawn once per revision.
    fn picture(&self) -> Result<Arc<kition_table_picture::Picture>, String> {
        if let Some((revision, shown, picture)) = self.picture.borrow().as_ref()
            && *revision == self.revision
            && *shown == self.shown
        {
            return Ok(picture.clone());
        }
        // A table or view the picture showed may be gone: show what the editor opens then.
        let picture = kition_table_picture::draw(&self.file, &self.shown, None)
            .or_else(|_| kition_table_picture::draw(&self.file, &kition_table_picture::Shown::default(), None))
            .map(Arc::new)?;
        *self.picture.borrow_mut() = Some((self.revision, self.shown.clone(), picture.clone()));
        Ok(picture)
    }

    /// The open file, for reads the requests do not cover.
    pub fn file(&self) -> &TableFile {
        &self.file
    }

    /// Runs one request by method name (`kition-table-api`'s [`METHODS`], `documents.get`, and
    /// [`ENGINE_METHODS`]) and answers `{ "result", "change" }`. A request that changed the
    /// document moves the revision and makes the document unsaved; undo back to the saved state
    /// makes it saved. A request is one undo step, however many writes it takes, unless it writes
    /// as an origin undo does not revert: `import.run` writes as `import:import` and `ai.apply` as
    /// `system:aifield` (as Kition's runtime did), and a request with
    /// `asOrigin: "system:<name>"` as that `system` origin (a host feature's write).
    ///
    /// An `import.run` that fails leaves the document as it was, as Kition's import restored its
    /// backup.
    pub fn call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let mut params = params.clone();
        let as_origin = take_as_origin(&mut params)?;
        if as_origin.is_some() && matches!(method, "undo" | "redo") {
            return Err(format!("{AS_ORIGIN_PARAM} cannot undo or redo"));
        }
        let Some(origin) = as_origin.or_else(|| method_origin(method)) else {
            return self.run(method, &params);
        };
        let backup = if method == "import.run" { Some((self.file.to_bytes().map_err(error)?, self.file.replica_version())) } else { None };
        let history = backup.as_ref().map(|_| self.history.clone());
        let previous = self.file.origin().clone();
        self.file.set_origin(origin);
        let answer = self.run(method, &params);
        self.file.set_origin(previous.clone());
        if let (Err(_), Some((bytes, version)), Some(history)) = (&answer, backup, history)
            && self.file.replica_version() != version
        {
            let mut file = TableFile::open_bytes(&bytes).map_err(error)?;
            file.set_origin(previous);
            self.file = file;
            self.history = history;
        }
        answer
    }

    /// [`TableDocument::call`] as the file's current origin.
    fn run(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let before = self.file.replica_version();
        self.file.take_last_change();
        let undo_or_redo = matches!(method, "undo" | "redo");
        let grouped = !undo_or_redo && !READ_METHODS.contains(&method) && self.file.begin_undo_group().is_ok();
        let result = match method {
            "documents.get" => Ok(self.document()),
            "import.preview" => kition_table_import::preview(params).map_err(error),
            "import.run" => kition_table_import::run(&mut self.file, self.doc.document_id, &self.name, params).map_err(error),
            "ai.prepare" => kition_table_ai::prepare(&self.file, params).map_err(error),
            "ai.apply" => kition_table_ai::apply(&mut self.file, &self.doc, params).map_err(error),
            "changes.since" => self.changes_since(params),
            "read" => inspect::read(&self.file, params),
            "preview.show" => preview::show(&self.file, params).map(|(shown, answer)| {
                self.shown = shown;
                self.revision += 1;
                answer
            }),
            _ => kition_table_api::handle(&mut self.file, &self.doc, method, params).map_err(api_error),
        };
        if grouped {
            self.file.end_undo_group();
        }
        let change = self.file.take_last_change();
        let moved = self.file.replica_version() != before;
        if moved {
            self.revision += 1;
        }
        let done = result.as_ref().is_ok_and(|answer| answer["done"] == Value::Bool(true));
        // The saved state follows the person's steps; an agent's step or undo (KitModel M12a) is a
        // change the person's undo does not revert.
        let persons =
            if undo_or_redo { undo_scope(params, self.file.undo_scope()) == UndoScope::Person } else { self.file.undo_scope() == UndoScope::Person };
        match method {
            "undo" if done && persons => self.history.undone(),
            "redo" if done && persons => self.history.redone(),
            _ if !moved => {}
            _ if !undo_or_redo && persons && undoable(self.file.origin()) => self.history.edited(),
            _ => self.history.left(),
        }
        let mut result = result?;
        address::answer(&self.file, method, &mut result);
        let change = match change {
            Some(change) => change_json(&self.doc.document_id.to_string(), self.file.origin(), &change),
            None => Value::Null,
        };
        Ok(json!({ "result": result, "change": change }))
    }

    /// `changes.since`: the records created, updated, and deleted since `params.version` (base64 of
    /// a version [`TableDocument::version`] answered; empty for every change), each with the origin
    /// of the write that made it, and the current version.
    pub fn changes_since(&self, params: &Value) -> Result<Value, String> {
        let since = decode_version(params["version"].as_str().unwrap_or_default())?;
        let changes = self.file.record_changes_since(&since).map_err(error)?;
        let list = |items: &[kition_table_file::RecordChange]| serde_json::to_value(items).unwrap_or_else(|_| json!([]));
        Ok(json!({
            "version": base64::engine::general_purpose::STANDARD.encode(self.version()),
            "created": list(&changes.created),
            "updated": list(&changes.updated),
            "deleted": list(&changes.deleted),
        }))
    }

    /// The document as the runtime's `GET /data-documents/{id}` answers it.
    pub fn document(&self) -> Value {
        let info = self.file.info().ok();
        let base = self.name.rsplit('/').next().unwrap_or(&self.name);
        let stem = base.strip_suffix(".ktable").unwrap_or(base);
        let title = info.as_ref().map(|info| info.title.clone()).filter(|title| !title.is_empty()).unwrap_or_else(|| stem.to_owned());
        json!({
            "id": self.doc.document_id,
            "user_id": self.doc.user_id,
            "workspace_root": "",
            "path": self.name,
            "title": title,
            "description": "",
            "icon": "",
            "color": "",
            "meta": null,
            "created_at": info.as_ref().map_or(0, |info| info.created_at),
            "updated_at": info.as_ref().map_or(0, |info| info.updated_at),
            "tables": tables_of(&self.doc, &self.file),
            "dashboards": dashboards_of(&self.file),
        })
    }

    /// The replica's version.
    pub fn version(&self) -> Vec<u8> {
        self.file.replica_version().encode()
    }

    /// The changes a replica at `since` lacks (every change when `since` is empty).
    pub fn export_changes(&self, since: &[u8]) -> Result<Vec<u8>, String> {
        let since = if since.is_empty() { Version::empty() } else { Version::decode(since).map_err(error)? };
        self.file.export_replica_changes(&since).map_err(error)
    }

    /// Imports another replica's changes (never an undo step); answers the damage
    /// (`{"kind":"full"}` when the document changed, else `{"kind":"none"}`) and queues the
    /// change for [`TableDocument::take_imported`].
    pub fn import_changes(&mut self, changes: &[u8]) -> Result<Value, String> {
        let before = self.file.replica_version();
        let imported = self.file.import_replica_changes(changes).map_err(error)?;
        if self.file.replica_version() == before {
            return Ok(json!({ "kind": "none" }));
        }
        self.revision += 1;
        self.history.left();
        if imported.change != TableChange::default() {
            let mut value = change_json(&self.doc.document_id.to_string(), &import_origin(), &imported.change);
            value["documentPath"] = Value::from(self.name.clone());
            self.imported.push(value);
        }
        Ok(json!({ "kind": "full" }))
    }

    /// Runs a command ([`KitDocument::execute`]) as `origin` (a person, an agent, an extension)
    /// instead of the editor's person: the replica records the write as theirs.
    pub fn execute_as(&mut self, command: &str, params: Value, origin: &Origin) -> Result<Value, String> {
        let previous = self.file.origin().clone();
        self.file.set_origin(origin.clone());
        let answer = self.execute(command, params);
        self.file.set_origin(previous);
        answer
    }

    /// What a write with `basis` of `command` rests on beyond its premises, as `covered` tells it
    /// (`(object id, fact)`), for the comparison with the independent reading of the handlers.
    #[doc(hidden)]
    pub fn covered_facts(&self, command: &str, params: &Value) -> Result<Vec<(String, String)>, String> {
        let Some(command) = self.commands().into_iter().find(|c| c.id == command) else { return Ok(Vec::new()) };
        place::facts(&self.file, &command, params)
    }

    /// The changes imported since the last call, as `table/changed` params (with `documentPath`).
    pub fn take_imported(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.imported)
    }

    /// Every blob the file holds, as a blob section (`(hash, u64 length, bytes)*`).
    pub fn blobs(&self) -> Result<Vec<u8>, String> {
        self.file.all_blobs_section().map_err(error)
    }

    /// The blobs the changes since `since` name (every blob the changes name when `since` is
    /// empty), as a blob section: what a co-editor needs beside [`TableDocument::export_changes`].
    pub fn blobs_since(&self, since: &[u8]) -> Result<Vec<u8>, String> {
        let changes = self.export_changes(since)?;
        self.file.blobs_for_changes(&changes).map_err(error)
    }

    /// Stores a co-editor's blobs (a blob section), before the changes that name them are
    /// imported. Blobs are file bookkeeping, not edits: the document's revision stays.
    pub fn add_blobs(&mut self, section: &[u8]) -> Result<usize, String> {
        self.file.add_blob_section(section).map_err(error)
    }

    /// The replica's KDM document id.
    pub fn doc_id(&self) -> String {
        self.file.replica().doc_id().to_string()
    }

    pub fn can_undo(&self) -> bool {
        self.file.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.file.can_redo()
    }

    /// The replica as a snapshot (every change, from the empty version): a peer opens it as the
    /// same document with [`TableDocument::open_with_replica`].
    pub fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.export_changes(&[])
    }

    /// Opens a document with the replica a host stored for it (engine contract 2), with `blobs`
    /// (a blob section) the attachments its changes name. Empty `bytes` (no file yet: a shared
    /// table opened from its room) open the replica alone from `snapshot`.
    ///
    /// A `.ktable` holds its own replica, so file `bytes` open as [`KitDocument::open`] opens
    /// them, and a snapshot of the file's own document is merged into it: a co-editing host
    /// stores the room's snapshot as the replica to open (the embed's `adoptRoom`), and the file
    /// then holds every change the room acknowledged, also those not saved into the file yet. A
    /// snapshot the file already holds (a single-user host's, stored with the file) changes
    /// nothing, so the document opens saved; one that adds changes opens it unsaved, since the
    /// file lacks them. A snapshot of another document, or one that is no replica, is not the
    /// file's and is left out.
    pub fn open_with_replica(name: &str, bytes: &[u8], snapshot: &[u8], blobs: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            let mut file = TableFile::create_from_replica_in_memory(snapshot).map_err(error)?;
            if !blobs.is_empty() {
                file.add_blob_section(blobs).map_err(error)?;
            }
            return Ok(Self::from_file(file, name));
        }
        let mut doc = Self::open(name, bytes)?;
        if snapshot.is_empty() {
            return Ok(doc);
        }
        let before = doc.file.replica_version();
        let merged = (|| {
            if !blobs.is_empty() {
                doc.file.add_blob_section(blobs).map_err(error)?;
            }
            doc.file.import_replica_changes(snapshot).map_err(error)
        })();
        match merged {
            Ok(_) => {
                if doc.file.replica_version() != before {
                    // The editor reads the merged document whole when it opens: nothing to queue.
                    doc.history.left();
                }
                Ok(doc)
            }
            // Another document's snapshot, or none: the file as it is.
            Err(_) => Self::open(name, bytes),
        }
    }

    /// Undoes the person's last step: `done` is `false`, not an error, when there is none; the
    /// damage is `{"kind":"full"}` when something changed, else `{"kind":"none"}`.
    pub fn undo(&mut self) -> Result<(bool, Value), String> {
        self.step(true)
    }

    /// Redoes the person's last undone step, answered as [`TableDocument::undo`] answers.
    pub fn redo(&mut self) -> Result<(bool, Value), String> {
        self.step(false)
    }

    fn step(&mut self, undo: bool) -> Result<(bool, Value), String> {
        let done = self.file.can_step_in(UndoScope::Person, undo);
        if done {
            let command = if undo { "edit.undo" } else { "edit.redo" };
            self.execute(command, json!({ "scope": "person" }))?;
        }
        Ok((done, json!({ "kind": if done { "full" } else { "none" } })))
    }
}

/// A command's title, words a person reads and command search matches (KHP A5c); `method` is the
/// command id without [`COMMAND_PREFIX`].
fn title(method: &str) -> &'static str {
    match method {
        "documents.get" => "Read the document",
        "tables.list" => "List tables",
        "tables.create" => "Create a table",
        "tables.update" => "Update a table",
        "tables.delete" => "Delete a table",
        "fields.list" => "List fields",
        "fields.create" => "Create a field",
        "fields.update" => "Update a field",
        "fields.delete" => "Delete a field",
        "fields.duplicate" => "Duplicate a field",
        "fields.linkCandidates" => "Find records to link",
        "views.list" => "List views",
        "views.create" => "Create a view",
        "views.update" => "Update a view",
        "views.delete" => "Delete a view",
        "views.getFilterTree" => "Read a view's filter",
        "views.putFilterTree" => "Set a view's filter",
        "views.getSorts" => "Read a view's sorts",
        "views.putSorts" => "Set a view's sorts",
        "views.getGroups" => "Read a view's groups",
        "views.putGroups" => "Set a view's groups",
        "views.patchConfig" => "Change a view's row height or frozen columns",
        "views.listFields" => "List a view's field settings",
        "views.patchField" => "Show, hide, resize or move a field in a view",
        "records.list" => "List records",
        "records.create" => "Create a record",
        "records.update" => "Update a record",
        "records.delete" => "Delete a record",
        "records.history" => "Read a record's history",
        "records.bulkDelete" => "Delete many records",
        "records.bulkCopy" => "Copy records",
        "links.list" => "List a record's links",
        "links.replace" => "Set a record's links",
        "links.delete" => "Remove a record's links",
        "csv.import" => "Import CSV text into a table",
        "csv.export" => "Export a table as CSV text",
        "markdown.export" => "Export a table as a Markdown table",
        "transact" => "Run record and field operations as one step",
        "undo" => "Undo a step",
        "redo" => "Redo a step",
        "historyState" => "Read whether undo and redo are possible",
        "dashboards.list" => "List dashboards",
        "dashboards.upsert" => "Create or replace a dashboard",
        "dashboards.delete" => "Delete a dashboard",
        "attachments.put" => "Store an attachment",
        "attachments.read" => "Read an attachment",
        "import.preview" => "Preview a file import",
        "import.run" => "Import a file into a table",
        "ai.prepare" => "Prepare an AI field run",
        "ai.apply" => "Write an AI field's results",
        "changes.since" => "List the records changed since a version",
        "read" => "Read a table's or a view's records",
        "preview.show" => "Show a table, view or dashboard in the picture",
        _ => "",
    }
}

/// What a command does, for people and models choosing one (kition-sdk `check_commands`); `method`
/// is the command id without [`COMMAND_PREFIX`].
fn describe(method: &str) -> &'static str {
    match method {
        "edit.undo" => {
            "Takes back the last step of one scope, the person's or the agent's (by default the caller's own); the other scope's later steps stay."
        }
        "edit.redo" => "Puts back the last undone step of one scope (by default the caller's own).",
        "documents.get" => {
            "Reads the document's title, every table with its fields and views, and its dashboards' ids and titles, without changing anything."
        }
        "tables.list" => {
            "Lists every table with its id, name, title, fields (ids, names, types, options) and views; start here to learn the ids and field names other calls need."
        }
        "tables.create" => {
            "Creates a table from a title and optionally its fields (each with a type and options) and views; without fields it gets a primary text field Title, without views a grid view All. Answers the table with its ids."
        }
        "tables.update" => "Changes a table's title, description, metadata or primary field; its fields and records stay as they are.",
        "tables.delete" => "Deletes a table with its fields, records and views, and every link to or from its records.",
        "fields.list" => "Lists a table's fields (columns) in order, with their ids, names, types and options.",
        "fields.create" => {
            "Adds a field (column) to a table: its title, its type (text, number, single_select, date, link_to_record, formula, ai, ...) and the options that type reads; answers the field with its id and name."
        }
        "fields.update" => {
            "Changes a field's title, type, options, formula, AI prompt, flags or primary status; values already stored stay as they are when the type changes."
        }
        "fields.delete" => "Deletes a field and its values from every record.",
        "fields.duplicate" => {
            "Adds a copy of a field's definition (type, options, formula) next to it, titled with \"copy\"; the copy starts with no values."
        }
        "fields.linkCandidates" => {
            "Searches the records a link_to_record field can link to (in its target table) by their display text, for choosing link values; changes nothing."
        }
        "views.list" => "Lists a table's views (grid, kanban, calendar, gallery) with their ids, types and configuration.",
        "views.create" => {
            "Creates a view of a table with a title, a type (grid, kanban, calendar or gallery) and a configuration; answers the view with its id."
        }
        "views.update" => "Renames a view, changes its type, or replaces its configuration.",
        "views.delete" => "Deletes a view; the table's data is not affected.",
        "views.getFilterTree" => "Reads a view's filter as a tree of and/or groups of field conditions (null when it has none).",
        "views.putFilterTree" => "Replaces a view's filter conditions with a tree of and/or groups of field conditions.",
        "views.getSorts" => "Reads a view's sort order: the fields and directions, first to last.",
        "views.putSorts" => "Replaces a view's sort order with a list of fields and directions.",
        "views.getGroups" => "Reads how a view groups its records: the fields and directions.",
        "views.putGroups" => "Replaces how a view groups its records with a list of fields and directions.",
        "views.patchConfig" => {
            "Changes a view's row height or frozen column count, keeping the rest of its configuration; answers the whole configuration."
        }
        "views.listFields" => {
            "Lists the per-field settings a view stores (visibility, width, position, frozen); fields without settings show with the defaults."
        }
        "views.patchField" => "Shows, hides, resizes, moves or freezes one field in one view.",
        "records.list" => "Reads a table's records (rows) with their values keyed by field name, a page at a time (limit, offset), with the total.",
        "records.create" => {
            "Adds one record to a table with values keyed by field name (not title); a key that names no field is refused and nothing is written. Answers the record with its id and row_key."
        }
        "records.update" => {
            "Changes values of one record, keyed by field name (null clears one); fields not named keep their values, and a key that names no field is refused, changing nothing."
        }
        "records.delete" => "Deletes one record and its links.",
        "records.history" => {
            "Reads one record's change history, newest first: its creation and each field update, with the values before and after, who made it and when."
        }
        "records.bulkDelete" => "Deletes many records at once by row_key, as one step; answers how many went and which keys named no record.",
        "records.bulkCopy" => "Copies records by row_key into the same table at a position, as one step; answers the copies' row keys.",
        "links.list" => "Reads the records one record links to through a link_to_record field, as row keys and display text.",
        "links.replace" => "Sets the records one record links to through a link_to_record field, replacing its current links.",
        "links.delete" => "Removes some of one record's links in a link_to_record field, by the linked records' row keys.",
        "csv.import" => {
            "Adds one record per row of CSV text to an existing table; the header row names fields by title or name, and other columns are left out."
        }
        "csv.export" => "Answers a table as CSV text with a file name, without changing anything.",
        "markdown.export" => "Answers a table as a Markdown table, without changing anything.",
        "transact" => {
            "Runs several record and field operations (set cells, create, delete or move records, move fields) as one atomic step that undoes together; a values key that names no field refuses the whole call."
        }
        "undo" => {
            "Takes back the last step of one scope (person or agent; by default the caller's own) and answers what it changed, as edit.undo does."
        }
        "redo" => "Puts back the last undone step of one scope (by default the caller's own) and answers what it changed, as edit.redo does.",
        "historyState" => "Reads whether there is a step to undo and a step to redo.",
        "dashboards.list" => "Lists the dashboards with their ids, titles, layout and widgets.",
        "dashboards.upsert" => {
            "Creates or replaces a dashboard by its id: its title, source table, layout and widgets (charts, numbers, lists over tables)."
        }
        "dashboards.delete" => "Deletes a dashboard; tables are not affected.",
        "attachments.put" => {
            "Stores a file's bytes (base64) in the document; answers the attachment value (url, name, type, size) to put in an attachment field's list."
        }
        "attachments.read" => "Reads an attachment's bytes (base64), name and type by its blob hash.",
        "import.preview" => {
            "Parses a CSV, TSV or spreadsheet file given as base64 and answers its sheets, detected columns and types and sample rows, without changing anything."
        }
        "import.run" => "Imports a file given as base64 into a new or existing table, with write mode, schema mode, unique key and field overrides.",
        "ai.prepare" => "Builds the prompts an AI field's run needs for one record or several, for the host to send to a model; changes nothing.",
        "ai.apply" => "Writes what the host's model generated (text, images or errors per record) into an AI field.",
        "changes.since" => "Reads the records created, updated and deleted since a version, with who made each change.",
        "read" => {
            "Reads the records of one table, or of one view (only those its filter keeps, in its order), with their ids and values under the field names, a page at a time; changes nothing."
        }
        "file.export" => {
            "Answers one table as a CSV or Markdown file's bytes (base64, in data) for the host to write as a new file, without changing anything."
        }
        "file.render" => {
            "Draws the table, a view, some of its rows, a field, one record's cell or a dashboard as the editor shows them, as a PNG at a scale up to 2, such as the target a diagnostic names; changes nothing."
        }
        "preview.show" => {
            "Picks what the document's picture (its renders and previews) shows: a table with its first view, a view, or a dashboard; without parameters, what the editor opens (the first table's first view)."
        }
        _ => "",
    }
}

fn command(id: &str, title: &str, params: Option<String>, mutates: bool) -> KitCommand {
    KitCommand {
        id: id.to_owned(),
        title: title.to_owned(),
        // No word that names a file or folder (`path`, `file`, …): the headless host would take the
        // command for one that reads files and leave it out.
        params,
        // The parameters as a `ParameterSchemaSpec` (KHP A5, A6), from the request types.
        schema: agent::input_schema(id),
        output_schema: agent::output_schema(id),
        description: describe(id.strip_prefix(COMMAND_PREFIX).unwrap_or(id)).into(),
        mutates,
        enabled: true,
        reads: acts::reads(id),
        targets: acts::targets(id),
        notation_required: acts::REQUIRED_PUBLISHED.contains(&id),
        ..Default::default()
    }
}

impl TableDocument {
    /// Each command's parameter schema, as the catalogue offers it to agents; read once, since a
    /// command's parameters do not change with the document.
    fn parameter_schemas(&self) -> &'static std::collections::HashMap<String, Value> {
        static SCHEMAS: std::sync::OnceLock<std::collections::HashMap<String, Value>> = std::sync::OnceLock::new();
        SCHEMAS.get_or_init(|| self.commands().into_iter().filter_map(|command| Some((command.id, command.schema?))).collect())
    }
}

impl TableDocument {
    fn run_command(&mut self, command: &str, params: Value) -> Result<Value, String> {
        // A key the command's schema does not name is refused, naming it (platform rule E7a).
        if let Some(schema) = self.parameter_schemas().get(command) {
            kition_kit::schema::refuse_unknown(command, schema, &params)?;
        }
        let params = if let Some(declared) = self.commands().iter().find(|c| c.id == command) {
            address::normalize(&self.file, declared, &params)?
        } else {
            params
        };
        if command == export::EXPORT_COMMAND {
            return export::export(&self.file, &params);
        }
        if command == preview::RENDER_COMMAND {
            return preview::render(&self.file, &params);
        }
        let method = match command {
            "edit.undo" => "undo",
            "edit.redo" => "redo",
            other => other.strip_prefix(COMMAND_PREFIX).filter(|method| known_method(method)).ok_or_else(|| format!("unknown command: {command}"))?,
        };
        self.call(method, &params)
    }
}

impl KitDocument for TableDocument {
    fn batch_settler(&self) -> Option<kition_kit::BatchSettler> {
        let created_by = self.file.actor();
        Some(Box::new(move |tx| {
            // What a raw create leaves out and a create gives.
            kition_table_file::kdm::model::settle_created(tx, &created_by).map_err(error)?;
            for view in tx.objects_of_type("View") {
                for (field, value) in tx.map_entries(&view, "widths").map_err(error)? {
                    let width = value.as_f64().ok_or("a column width must be a number")?.round().clamp(40.0, 1200.0) as i64;
                    if value != json!(width) {
                        tx.map_set(&view, "widths", &field, json!(width)).map_err(error)?;
                    }
                }
            }
            Ok(())
        }))
    }

    fn open(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let file = TableFile::open_bytes(bytes).map_err(error)?;
        Ok(Self::from_file(file, name))
    }

    /// A new document with one starter table, as Kition's runtime makes one.
    fn create(name: &str, _width: u32, _height: u32) -> Result<Self, String> {
        let base = name.rsplit('/').next().unwrap_or(name);
        let title = base.strip_suffix(".ktable").unwrap_or(base).trim();
        let options = CreateOptions { title: if title.is_empty() { "Untitled table".to_owned() } else { title.to_owned() }, ..Default::default() };
        let mut file = TableFile::create_in_memory(&options).map_err(error)?;
        file.set_origin(Origin::system("create"));
        file.create_table(&seed_to_spec(&default_table_seed("Table"))).map_err(error)?;
        Ok(Self::from_file(file, name))
    }

    fn width(&self) -> u32 {
        self.picture().map_or(1, |picture| picture.width())
    }

    fn height(&self) -> u32 {
        self.picture().map_or(1, |picture| picture.height())
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn dirty(&self) -> bool {
        self.history.unsaved()
    }

    fn render(&self, x: f64, y: f64, scale: f64, width: u32, height: u32) -> Result<Vec<u8>, String> {
        Ok(self.picture()?.render(x, y, scale, width, height))
    }

    fn render_area(&self, x0: i32, y0: i32, x1: i32, y1: i32, width: u32, height: u32) -> Result<Vec<u8>, String> {
        Ok(self.picture()?.render_area(x0, y0, x1, y1, width, height))
    }

    /// Runs `command`. An agent's command leaves what the picture shows as it was, also when it
    /// fails; only `table.preview.show`, whose work is moving the picture, keeps its effect. A
    /// person's commands run as they always did.
    fn execute(&mut self, command: &str, params: Value) -> Result<Value, String> {
        let agent = kition_kit::dispatch::role() == Some(kition_kit::dispatch::Role::Agent);
        if !agent || command == context::MOVES_THE_EDITOR {
            return self.run_command(command, params);
        }
        let kept = self.shown.clone();
        let result = self.run_command(command, params);
        self.shown = kept;
        result
    }

    /// What the picture shows (opaque to hosts).
    fn editor_state(&self) -> Value {
        self.editor_snapshot()
    }

    /// Puts back what [`KitDocument::editor_state`] answered; no edit.
    fn restore_editor_state(&mut self, saved: &Value) -> Result<(), String> {
        self.put_back(saved)
    }

    /// Shows the table or view a call acts in (`part`, by id) and checks `on`'s record and field
    /// ids; a table file keeps no selection and no time ([`context`]). No edit.
    fn set_context(&mut self, context: &kition_kit::CallContext) -> Result<(), String> {
        self.set_call_context(context)
    }

    fn under(&self, command: &str, params: &Value) -> Result<Option<kition_kit::Under>, String> {
        if command == place::RAW_CELL {
            return place::raw_cell(&self.file, params).map(Some);
        }
        let Some(command) = self.commands().into_iter().find(|c| c.id == command) else { return Ok(None) };
        place::under(&self.file, &command, params)
    }

    fn derived(&self, ids: &[Value], names: &[String]) -> Result<Vec<kition_kit::DerivedValues>, String> {
        place::derived(&self.file, ids, names)
    }

    fn covered(&self, command: &str, params: &Value, objects: &[kition_kit::CoveredAsk]) -> Result<Option<kition_kit::CoveredAt>, String> {
        let Some(command) = self.commands().into_iter().find(|c| c.id == command) else { return Ok(None) };
        place::covered(&self.file, &command, params, objects)
    }

    /// The document's content for agents and the editor's shell ([`inspect::overview`]): its tables
    /// with their fields, views, record counts and first records, and its dashboards.
    fn inspect(&self) -> Result<Value, String> {
        let mut answer = inspect::overview(&self.file)?;
        answer["canUndo"] = Value::Bool(self.file.can_undo());
        answer["canRedo"] = Value::Bool(self.file.can_redo());
        answer["readOnly"] = json!(self.file.read_only());
        Ok(answer)
    }

    fn summary(&self) -> String {
        inspect::summary(&self.file)
    }

    fn commands(&self) -> Vec<KitCommand> {
        let mut commands = vec![
            KitCommand {
                menu: Some(vec!["Edit".into()]),
                shortcut: Some("Cmd+Z".into()),
                enabled: self.can_step(true),
                ..command("edit.undo", "Undo", None, true)
            },
            KitCommand {
                menu: Some(vec!["Edit".into()]),
                shortcut: Some("Cmd+Shift+Z".into()),
                enabled: self.can_step(false),
                ..command("edit.redo", "Redo", None, true)
            },
        ];
        for (method, route) in DOCUMENT_METHODS.iter().chain(METHODS) {
            let mut params = format!("the params of the data-document route {route}: its ids (tableId, …), query values, and body fields");
            if !READ_METHODS.contains(method) {
                params.push_str("; asOrigin? (system:<name>) writes as a host feature, outside the undo history");
            }
            commands.push(command(&format!("{COMMAND_PREFIX}{method}"), title(method), Some(params), !READ_METHODS.contains(method)));
        }
        for (method, params) in ENGINE_PARAMS {
            let mut params = (*params).to_owned();
            if !READ_METHODS.contains(method) {
                params.push_str("; asOrigin? (system:<name>) writes as a host feature");
            }
            commands.push(command(&format!("{COMMAND_PREFIX}{method}"), title(method), Some(params), !READ_METHODS.contains(method)));
        }
        // Its parameters are in its schema alone: kition-headless once read a `target` named in a
        // command's notes as a file.
        commands.push(command(preview::RENDER_COMMAND, "Render a preview image", None, false));
        commands.push(command(
            export::EXPORT_COMMAND,
            "Export a table as a CSV or Markdown file",
            Some("{format (csv or md), tableId?}: one table as CSV or Markdown, base64, for the host to write".into()),
            false,
        ));
        kition_kit::files::declare(&mut commands, files::FILES);
        params::declare(&mut commands);
        commands
    }

    /// The database's bytes, with a long change log compacted first (`TableFile::compact_log`:
    /// an open replays the log) and vacuumed when it is sparse ([`vacuum_if_sparse`]).
    fn save(&self, _name: Option<&str>) -> Result<Vec<u8>, String> {
        self.file.compact_log().map_err(error)?;
        vacuum_if_sparse(&self.file)?;
        let bytes = self.file.to_bytes().map_err(error)?;
        self.save_pending.set(Some(self.history.step));
        Ok(bytes)
    }

    fn mark_saved(&mut self) {
        self.history.saved = Some(self.save_pending.take().unwrap_or(self.history.step));
    }

    /// A file read again after an outside write: when it is this document and holds every change
    /// of this session, only the changes it adds are imported (with their blobs), as a co-editor's
    /// are, outside both undo histories, so this session keeps its own. A file of another
    /// document, or one that lacks a change of this session, cannot be reconciled: `false`,
    /// nothing changed.
    fn reconcile_reread(&mut self, fresh: &Self) -> Result<bool, String> {
        if fresh.file.replica().doc_id() != self.file.replica().doc_id() {
            return Ok(false);
        }
        let ours = self.file.replica_version();
        if !fresh.file.replica_version().includes(&ours) {
            return Ok(false);
        }
        let changes = fresh.export_changes(&ours.encode())?;
        let blobs = fresh.file.blobs_for_changes(&changes).map_err(error)?;
        if !blobs.is_empty() {
            self.add_blobs(&blobs)?;
        }
        self.import_changes(&changes)?;
        Ok(true)
    }

    /// What is wrong with the document (`check.rs`, catalogued in `agent/checks.json`).
    fn check(&self) -> Vec<kition_kit::diagnostics::Diagnostic> {
        check::check(&self.file)
    }
}

kition_kit::export_kit_document!(TableDocument as "TableDocument");

#[cfg(test)]
mod agent_tests;
#[cfg(test)]
mod engine_tests;
#[cfg(test)]
mod round_trip_tests;
#[cfg(test)]
mod tests;
