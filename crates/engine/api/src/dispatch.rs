//! [`handle`]: one entry point for every request, by method name.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::coerce::{list_window, parse_id};
use crate::{ApiError, ApiResult, DocRef, TableFile, attachments, dashboards, names, records, respond, tables, transact, values, view_config};

/// Every method [`handle`] answers, with the runtime route it mirrors (relative to
/// `/v1/data-documents/{documentId}`).
pub const METHODS: &[(&str, &str)] = &[
    ("table.list", "GET /tables"),
    ("table.add", "POST /tables"),
    ("table.set", "PATCH /tables/{tableId}"),
    ("table.delete", "DELETE /tables/{tableId}"),
    ("field.list", "GET /tables/{tableId}/fields"),
    ("field.add", "POST /tables/{tableId}/fields"),
    ("field.set", "PATCH /tables/{tableId}/fields/{fieldId}"),
    ("field.delete", "DELETE /tables/{tableId}/fields/{fieldId}"),
    ("field.duplicate", "POST /tables/{tableId}/fields/{fieldId}/duplicate"),
    ("field.linkCandidates", "GET /tables/{tableId}/fields/{fieldId}/link-candidates"),
    ("tableView.list", "GET /tables/{tableId}/views"),
    ("tableView.add", "POST /tables/{tableId}/views"),
    ("tableView.set", "PATCH /tables/{tableId}/views/{viewId}"),
    ("tableView.delete", "DELETE /tables/{tableId}/views/{viewId}"),
    ("tableView.getFilterTree", "GET /tables/{tableId}/views/{viewId}/filter-tree"),
    ("tableView.setFilterTree", "PUT /tables/{tableId}/views/{viewId}/filter-tree"),
    ("tableView.getSorts", "GET /tables/{tableId}/views/{viewId}/sorts"),
    ("tableView.setSorts", "PUT /tables/{tableId}/views/{viewId}/sorts"),
    ("tableView.getGroups", "GET /tables/{tableId}/views/{viewId}/groups"),
    ("tableView.setGroups", "PUT /tables/{tableId}/views/{viewId}/groups"),
    ("tableView.setConfig", "PATCH /tables/{tableId}/views/{viewId}/config"),
    ("tableView.listFields", "GET /tables/{tableId}/views/{viewId}/view-fields"),
    ("tableView.setField", "PATCH /tables/{tableId}/views/{viewId}/view-fields/{fieldId}"),
    ("record.list", "GET /tables/{tableId}/records"),
    ("record.add", "POST /tables/{tableId}/records"),
    ("record.set", "PATCH /tables/{tableId}/records/{recordId}"),
    ("record.delete", "DELETE /tables/{tableId}/records/{recordId}"),
    ("record.history", "GET /tables/{tableId}/records/{recordId}/history"),
    ("record.bulkDelete", "POST /tables/{tableId}/records/bulk-delete"),
    ("record.bulkCopy", "POST /tables/{tableId}/records/bulk-copy"),
    ("recordLink.list", "GET /tables/{tableId}/records/{recordId}/links"),
    ("recordLink.replace", "POST /tables/{tableId}/records/{recordId}/links"),
    ("recordLink.delete", "DELETE /tables/{tableId}/records/{recordId}/links"),
    ("file.importCsv", "POST /tables/{tableId}/import/csv"),
    ("file.exportCsv", "GET /tables/{tableId}/export/csv"),
    ("file.exportMarkdown", "GET /tables/{tableId}/export/markdown"),
    ("edit.transact", "POST /transact"),
    ("history.undo", "POST /undo"),
    ("history.redo", "POST /redo"),
    ("history.state", "GET /history-state"),
    ("dashboard.list", "GET /dashboards"),
    ("dashboard.set", "PUT /dashboards/{dashboardId}"),
    ("dashboard.delete", "DELETE /dashboards/{dashboardId}"),
    ("attachment.add", "POST /tables/{tableId}/attachments"),
    ("attachment.get", "GET /blobs/{hash}"),
];

/// Answers one request against `file`, as the runtime route named in [`METHODS`] answers it
/// (the `data` of its response envelope). `params` is an object (or `null`): the route's path
/// ids (`tableId`, `fieldId`, `viewId`, `recordId`, `dashboardId`; numbers or decimal strings),
/// its query values (`limit`, `offset`, `field_id`, `q`), and its body's fields.
///
/// `file.exportCsv` answers `{ "name", "content" }`, what the route sends as a file download.
/// `attachment.add` takes the file's bytes as `data` (base64) next to `name` and `mimeType`,
/// where the route takes a multipart upload; `attachment.get` answers the blob with its bytes
/// as `data` (base64), where the route sends them as the response body.
/// Writes act as the file's current origin ([`TableFile::set_origin`]).
pub fn handle(file: &mut TableFile, doc: &DocRef, method: &str, params: &Value) -> ApiResult<Value> {
    let empty = Map::new();
    let params = match params {
        Value::Object(map) => map,
        Value::Null => &empty,
        _ => return Err(ApiError::InvalidBody),
    };
    let p = Params(params);
    // A record `values` key that names no field is refused before anything is written, and so is
    // an explicit name the engine would not keep as given.
    values::refuse_unknown_keys(file, method, params)?;
    names::refuse_rewritten_names(file, method, params)?;
    match method {
        "table.list" => tables::list_tables(file, doc),
        "table.add" => tables::create_table(file, doc, &p.body()?),
        "table.set" => {
            let table_id = p.id("tableId")?;
            tables::update_table(file, doc, table_id, params)
        }
        "table.delete" => tables::delete_table(file, p.id("tableId")?),
        "field.list" => tables::list_fields(file, doc, p.id("tableId")?),
        "field.add" => {
            let table_id = p.id("tableId")?;
            tables::create_field(file, doc, table_id, p.body()?)
        }
        "field.set" => {
            let (table_id, field_id) = (p.id("tableId")?, p.id("fieldId")?);
            tables::update_field(file, doc, table_id, field_id, params)
        }
        "field.delete" => tables::delete_field(file, p.id("tableId")?, p.id("fieldId")?),
        "field.duplicate" => {
            let (table_id, field_id) = (p.id("tableId")?, p.id("fieldId")?);
            tables::duplicate_field(file, doc, table_id, field_id)
        }
        "field.linkCandidates" => {
            let field_id = p.id("fieldId")?;
            let limit = records::candidate_limit(p.text("limit").as_deref());
            let query = p.text("q").unwrap_or_default();
            records::link_candidates(file, field_id, &query, limit)
        }
        "tableView.list" => tables::list_views(file, doc, p.id("tableId")?),
        "tableView.add" => {
            let table_id = p.id("tableId")?;
            tables::create_view(file, doc, table_id, p.body()?)
        }
        "tableView.set" => {
            let (table_id, view_id) = (p.id("tableId")?, p.id("viewId")?);
            tables::update_view(file, doc, table_id, view_id, params)
        }
        "tableView.delete" => tables::delete_view(file, p.id("tableId")?, p.id("viewId")?),
        "tableView.getFilterTree" => view_config::get_filter_tree(file, p.view(file)?),
        "tableView.setFilterTree" => {
            let request = p.body()?;
            view_config::put_filter_tree(file, p.view(file)?, request)
        }
        "tableView.getSorts" => view_config::get_sorts(file, p.view(file)?),
        "tableView.setSorts" => {
            let request = p.body()?;
            view_config::put_sorts(file, p.view(file)?, request)
        }
        "tableView.getGroups" => view_config::get_groups(file, p.view(file)?),
        "tableView.setGroups" => {
            let request = p.body()?;
            view_config::put_groups(file, p.view(file)?, request)
        }
        "tableView.setConfig" => {
            let request = p.body()?;
            let (table_id, view_id) = (p.id("tableId")?, p.id("viewId")?);
            view_config::patch_config(file, table_id, view_id, request)
        }
        "tableView.listFields" => view_config::list_view_fields(file, p.view(file)?),
        "tableView.setField" => {
            let request = p.body()?;
            let view_id = p.view(file)?;
            let field_id = p.id("fieldId")?;
            // A column of the view is a field of its table.
            if owner_of(file, "kition_fields", field_id).is_some_and(|table| Some(table) != owner_of(file, "kition_views", view_id)) {
                return Err(ApiError::not_found("View Field not found"));
            }
            view_config::patch_view_field(file, view_id, field_id, request)
        }
        "record.list" => {
            let (limit, offset) = p.window();
            records::list(file, doc, p.id("tableId")?, limit, offset)
        }
        "record.add" => {
            let request = p.body()?;
            let record = records::create(file, p.id("tableId")?, request)?;
            let mut answer = respond::record(doc, &record);
            answer["changed"] = Value::Bool(true);
            Ok(answer)
        }
        "record.set" => {
            let request = p.body()?;
            let (table_id, record_id) = (p.id("tableId")?, p.id("recordId")?);
            let (record, previous) = records::update(file, table_id, record_id, request)?;
            let mut answer = respond::record(doc, &record);
            answer["changed"] = Value::Bool(previous.is_none_or(|(values, order)| values != record.values || order != record.order));
            Ok(answer)
        }
        "record.delete" => records::delete(file, p.id("tableId")?, p.id("recordId")?),
        "record.history" => {
            let (table_id, record_id) = (p.id("tableId")?, p.id("recordId")?);
            let (limit, offset) = p.window();
            let points = records::history_points(p.0.get("checkpoints"))?;
            records::history(file, table_id, record_id, limit, offset, &points)
        }
        "record.bulkDelete" => {
            let request = p.body()?;
            view_config::bulk_delete(file, p.id("tableId")?, request)
        }
        "record.bulkCopy" => {
            let request = p.body()?;
            view_config::bulk_copy(file, p.id("tableId")?, request)
        }
        "recordLink.list" => {
            let (table_id, record_id) = (p.id("tableId")?, p.id("recordId")?);
            let field_id = records::link_field_id(p.text("field_id").as_deref())?;
            records::list_links(file, table_id, record_id, field_id)
        }
        "recordLink.replace" | "recordLink.delete" => {
            let (table_id, record_id) = (p.id("tableId")?, p.id("recordId")?);
            let request = p.body::<records::LinkRequest>().map_err(|_| ApiError::bad_request("Invalid request body"))?.check()?;
            if method == "recordLink.replace" {
                records::replace_links(file, table_id, record_id, request)
            } else {
                records::delete_links(file, table_id, record_id, request)
            }
        }
        "file.importCsv" => {
            let request: records::CsvRequest = p.body()?;
            let rows = records::parse_csv(&request.content.unwrap_or_default())?;
            records::import_csv(file, p.id("tableId")?, &rows)
        }
        "file.exportCsv" => {
            let (name, content) = records::export_csv(file, p.id("tableId")?)?;
            Ok(serde_json::json!({ "name": name, "content": content }))
        }
        "file.exportMarkdown" => records::export_markdown(file, p.id("tableId")?),
        "edit.transact" => {
            let request = p.body::<transact::TransactRequest>()?.check()?;
            Ok(transact::transact(file, doc, &request)?.0)
        }
        "history.undo" => Ok(transact::undo_or_redo(file, doc, false, p.undo_scope()?)?.0),
        "history.redo" => Ok(transact::undo_or_redo(file, doc, true, p.undo_scope()?)?.0),
        "history.state" => Ok(transact::history_state(file)),
        "dashboard.list" => dashboards::list(file),
        "dashboard.set" => dashboards::upsert(file, &p.key("dashboardId"), params),
        "dashboard.delete" => dashboards::delete(file, &p.key("dashboardId")),
        "attachment.add" => {
            let request = p.body()?;
            attachments::put_request(file, p.id("tableId")?, &request)
        }
        "attachment.get" => attachments::read(file, &p.key("hash")),
        other => Err(ApiError::UnknownMethod(other.to_owned())),
    }
}

struct Params<'a>(&'a Map<String, Value>);

impl Params<'_> {
    /// A string, or a number in decimal.
    /// `scope` of `undo` and `redo` (KitModel M12a): `person` or `agent`; `None` when left out.
    fn undo_scope(&self) -> ApiResult<Option<kition_kdm::UndoScope>> {
        match self.0.get("scope") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(scope)) => kition_kdm::UndoScope::parse(scope)
                .map(Some)
                .ok_or_else(|| ApiError::bad_request(format!("scope is \"person\" or \"agent\", not {scope:?}"))),
            Some(other) => Err(ApiError::bad_request(format!("scope is \"person\" or \"agent\", not {other}"))),
        }
    }

    fn text(&self, key: &str) -> Option<String> {
        match self.0.get(key)? {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        }
    }

    fn key(&self, key: &str) -> String {
        self.text(key).unwrap_or_default()
    }

    fn id(&self, key: &str) -> ApiResult<i64> {
        parse_id(&self.key(key))
    }

    /// The view `viewId` names, which must be one of the views of the table `tableId` names: a
    /// view of another table is not found, rather than written through a table it is not in.
    fn view(&self, file: &TableFile) -> ApiResult<i64> {
        let (table_id, view_id) = (self.id("tableId")?, self.id("viewId")?);
        if file.get_view(table_id, view_id).is_err() && owner_of(file, "kition_views", view_id).is_some() {
            return Err(ApiError::not_found("View does not exist"));
        }
        Ok(view_id)
    }

    fn window(&self) -> (i64, i64) {
        list_window(self.text("limit").as_deref(), self.text("offset").as_deref())
    }

    fn body<T: DeserializeOwned>(&self) -> ApiResult<T> {
        serde_json::from_value(Value::Object(self.0.clone())).map_err(|_| ApiError::InvalidBody)
    }
}

/// The live table a live view or field (`kind`: its projection table) belongs to.
fn owner_of(file: &TableFile, kind: &str, id: i64) -> Option<i64> {
    file.conn().query_row(&format!("SELECT table_id FROM {kind} WHERE id = ?1 AND deleted_at IS NULL"), [id], |row| row.get(0)).ok()
}
