//! `addFont` (the picture's faces), and the extras on the exported `TableDocument` class, reached
//! with `KitEngine.invoke(name, …)`:
//! `call` (one data-document request), `takeImported`, `gridOpen`, `gridSync` and `gridStats`
//! (the editor's arrangement, `grid.rs`), and engine contract 2's replica members
//! (`openWithReplica`, `snapshot`, `undo`, `redo` and the rest) in the shapes kition-sdk's
//! `session::web` gives them, as Kition Sheet adds them by hand.

use kition_kit::wasm_bindgen::prelude::*;

use crate::__kition_kit_export::WebDocument;

fn error(message: String) -> JsError {
    JsError::new(&message)
}

/// Registers a font file for the document's picture (Kition platform E9; kit-editor calls it for
/// each of its `fonts` before a table opens); answers its family name. Without fonts the picture
/// has no text.
#[wasm_bindgen(js_name = addFont, wasm_bindgen = kition_kit::wasm_bindgen)]
pub fn add_font(bytes: Vec<u8>) -> Result<String, JsError> {
    kition_table_fonts::add_font(bytes).map_err(error)
}

#[wasm_bindgen(js_class = "TableDocument", wasm_bindgen = kition_kit::wasm_bindgen)]
impl WebDocument {
    /// One `kition-table-api` request: `params` is JSON (an object, or empty for none); answers
    /// `{ "result", "change" }` as JSON.
    pub fn call(&mut self, method: &str, params: &str) -> Result<String, JsError> {
        let params = kition_kit::parse_params(params).map_err(error)?;
        self.0.call(method, &params).map(|value| value.to_string()).map_err(error)
    }

    /// Runs a command as `origin` (the KDM's commit form, the JSON array `[kind, principal,
    /// label]`): the editor's worker runs an agent's step this way, so the step is the agent's and
    /// the agent's undo (KitModel M12a) takes it back.
    #[wasm_bindgen(js_name = executeAs)]
    pub fn execute_as(&mut self, command: &str, params: &str, origin: &str) -> Result<String, JsError> {
        let params = kition_kit::parse_params(params).map_err(error)?;
        let origin = kition_kdm::Origin::decode(origin).ok_or_else(|| error(format!("origin is not [kind, principal, label]: {origin}")))?;
        self.0.execute_as(command, params, &origin).map(|value| value.to_string()).map_err(error)
    }

    /// The `TableChange`s of the replica changes imported since the last call, as a JSON array
    /// of `table/changed` params.
    #[wasm_bindgen(js_name = takeImported)]
    pub fn take_imported(&mut self) -> String {
        serde_json::Value::Array(self.0.take_imported()).to_string()
    }

    /// Arranges a view for the editor: `params` is JSON `{tableId, viewId?, config?, query?,
    /// collapsed?}`; answers the axis (`kition-table-arrange`, format v1) as bytes.
    #[wasm_bindgen(js_name = gridOpen)]
    pub fn grid_open(&mut self, params: &str) -> Result<Vec<u8>, JsError> {
        let params = kition_kit::parse_params(params).map_err(error)?;
        self.0.grid_open(&params).map_err(error)
    }

    /// The editor's arrangement brought to the document's current state: the new axis, or no
    /// bytes when the axis is the same.
    #[wasm_bindgen(js_name = gridSync)]
    pub fn grid_sync(&mut self) -> Result<Vec<u8>, JsError> {
        self.0.grid_sync().map_err(error)
    }

    /// How long arranging took, as JSON (for performance tests).
    #[wasm_bindgen(js_name = gridStats)]
    pub fn grid_stats(&self) -> String {
        self.0.grid_stats().to_string()
    }

    pub fn version(&mut self) -> Vec<u8> {
        self.0.version()
    }

    #[wasm_bindgen(js_name = exportChanges)]
    pub fn export_changes(&mut self, since: &[u8]) -> Result<Vec<u8>, JsError> {
        self.0.export_changes(since).map_err(error)
    }

    #[wasm_bindgen(js_name = importChanges)]
    pub fn import_changes(&mut self, bytes: &[u8]) -> Result<String, JsError> {
        self.0.import_changes(bytes).map(|damage| damage.to_string()).map_err(error)
    }

    /// Every blob (an attachment held in the file) the document holds, for a peer's `addBlobs`.
    pub fn blobs(&mut self) -> Result<Vec<u8>, JsError> {
        self.0.blobs().map_err(error)
    }

    /// The blobs the changes since `since` name, encoded like `blobs()`: for live sync, so a
    /// change ships only the blobs it needs.
    #[wasm_bindgen(js_name = blobsSince)]
    pub fn blobs_since(&mut self, since: &[u8]) -> Result<Vec<u8>, JsError> {
        self.0.blobs_since(since).map_err(error)
    }

    /// Stores a peer's blobs (from their `blobs()` or `blobsSince`) before their changes import.
    #[wasm_bindgen(js_name = addBlobs)]
    pub fn add_blobs(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.0.add_blobs(bytes).map(|_| ()).map_err(error)
    }

    /// The replica as a snapshot, for a peer's `openWithReplica` (engine contract 2).
    pub fn snapshot(&mut self) -> Result<Vec<u8>, JsError> {
        self.0.snapshot().map_err(error)
    }

    /// Undoes the person's last step; answers `{"done", "damage"}` as JSON.
    pub fn undo(&mut self) -> Result<String, JsError> {
        self.0.undo().map(|(done, damage)| serde_json::json!({ "done": done, "damage": damage }).to_string()).map_err(error)
    }

    /// Redoes the person's last undone step; answers `{"done", "damage"}` as JSON.
    pub fn redo(&mut self) -> Result<String, JsError> {
        self.0.redo().map(|(done, damage)| serde_json::json!({ "done": done, "damage": damage }).to_string()).map_err(error)
    }

    /// Opens file bytes with the replica a host stored for them (engine contract 2): a snapshot of
    /// the file's own document (a co-editing room's) is merged into it, and empty `bytes` (no file
    /// yet) open the replica alone.
    #[wasm_bindgen(js_name = openWithReplica)]
    pub fn open_with_replica(name: &str, bytes: &[u8], snapshot: &[u8], blobs: &[u8]) -> Result<WebDocument, JsError> {
        crate::TableDocument::open_with_replica(name, bytes, snapshot, blobs).map(Self).map_err(error)
    }

    #[wasm_bindgen(js_name = canUndo)]
    pub fn can_undo(&self) -> bool {
        self.0.can_undo()
    }

    #[wasm_bindgen(js_name = canRedo)]
    pub fn can_redo(&self) -> bool {
        self.0.can_redo()
    }

    /// The file holds its replica: there always is one.
    #[wasm_bindgen(getter, js_name = hasReplica)]
    pub fn has_replica(&self) -> bool {
        true
    }

    #[wasm_bindgen(getter, js_name = docId)]
    pub fn doc_id(&self) -> String {
        self.0.doc_id()
    }
}

/// Every method `call` answers, with the Kition runtime route it mirrors (relative to
/// `/v1/data-documents/{documentId}`), as a JSON array of `[method, route]`: what a host needs to
/// answer Kition's data-document HTTP API from the engine.
#[wasm_bindgen(js_name = tableMethods, wasm_bindgen = kition_kit::wasm_bindgen)]
pub fn table_methods() -> String {
    crate::methods_json()
}
