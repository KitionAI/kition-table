//! Explicit call context (kition-sdk `KitDocument::editor_state`, `set_context`; Kit Process
//! Protocol section 7a): the editor state, taken and put back without an
//! edit, set for one call from what the call says it acts in, and kept out of an agent's commands.
//!
//! A table file's only editor state is **what its picture shows** (`preview.show`): a
//! table, a view, or a dashboard. No command reads it: every command names its table, view,
//! field and record by id in its parameters, and `file.render` draws the view its call
//! names. There is no selection and no time.
//!
//! A call's **part** is a table or a view, by the id `table_inspect` lists: `{"table": 2}` or
//! `{"view": 3}` (`{"table": 2, "view": 3}` when they agree). It sets what the picture shows for
//! the step.
//!
//! A call's **`on`** names records or fields by their ids. A table file keeps no selection, so `on`
//! reaches a command only through the parameter it marks as its targets: `recordId` (a record of
//! the command's `tableId`) or `fieldId` (a field). The parameter says which, so nothing there is
//! ambiguous. `set_context` itself takes `on` only to check it: each id must be a record or a field
//! of the part's table, and an id that is both is refused as ambiguous; then `on` is refused as not
//! available, since there is no selection to set.

use kition_kit::CallContext;
use kition_table_file::TableFile;
use kition_table_picture::Shown;
use serde_json::{Map, Value, json};

use crate::TableDocument;

/// What [`kition_kit::KitDocument::inspect`] says of the editor rather than of the file, as JSON
/// Pointers: nothing. What the picture shows is not in `inspect()`.
pub const EDITOR_STATE: &[&str] = &[];

/// The command whose work is moving the picture: it keeps its effect for an agent too.
pub const MOVES_THE_EDITOR: &str = "preview.show";

const LISTED: &str = "the tables' and views' ids are in table_inspect's `tables`";

fn tables(file: &TableFile) -> Result<Vec<kition_table_file::Table>, String> {
    file.list_tables().map_err(|error| error.to_string())
}

/// The table and view `fields` (`table`, `view`) name, checked: the table of a view is its own.
pub(crate) fn part_of(file: &TableFile, fields: &Map<String, Value>, what: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let tables = tables(file)?;
    let id = |key: &str| -> Result<Option<i64>, String> {
        match fields.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => crate::address::local(file, if key == "table" { "Table" } else { "View" }, value)
                .ok()
                .filter(|id| *id > 0)
                .map(Some)
                .ok_or_else(|| format!("{what}.{key} is {value}: a {key} is named by its id, a whole number ({LISTED})")),
        }
    };
    let (table, view) = (id("table")?, id("view")?);
    if let Some(table) = table
        && !tables.iter().any(|t| t.id == table)
    {
        return Err(format!("not found: no table {table} ({LISTED})"));
    }
    let owner = match view {
        Some(view) => Some(
            tables
                .iter()
                .find(|t| t.views.iter().any(|v| v.id == view))
                .map(|t| t.id)
                .ok_or_else(|| format!("not found: no view {view} ({LISTED})"))?,
        ),
        None => None,
    };
    if let (Some(table), Some(owner), Some(view)) = (table, owner, view)
        && table != owner
    {
        return Err(format!("view {view} is of table {owner}, and {what}.table names table {table}"));
    }
    Ok((table.or(owner), view))
}

impl TableDocument {
    /// The editor state: what the picture shows.
    pub(crate) fn editor_snapshot(&self) -> Value {
        json!({ "table": self.shown.table, "view": self.shown.view, "dashboard": self.shown.dashboard })
    }

    /// Puts back what [`Self::editor_snapshot`] answered; a table, view or dashboard gone since
    /// shows what the editor opens. No edit.
    pub(crate) fn put_back(&mut self, saved: &Value) -> Result<(), String> {
        let state = saved
            .as_object()
            .filter(|state| state.keys().all(|key| matches!(key.as_str(), "table" | "view" | "dashboard")))
            .ok_or_else(|| format!("{saved} is not the editor state this kit answered"))?;
        let number = |key: &str| state.get(key).and_then(Value::as_i64);
        let dashboard = state.get("dashboard").and_then(Value::as_str).map(str::to_owned);
        let shown = Shown { table: number("table"), view: number("view"), dashboard, ..Shown::default() };
        let exists = kition_table_picture::draw(&self.file, &shown, None).is_ok();
        self.shown = if exists { shown } else { Shown::default() };
        Ok(())
    }

    /// Sets what the picture shows for a call (module docs). Everything is checked before anything
    /// changes; nothing of it is an edit.
    pub(crate) fn set_call_context(&mut self, context: &CallContext) -> Result<(), String> {
        if context.time.is_some() {
            return Err("a table file has no timeline: setting the current time (`at.time`) is not available in this kit".into());
        }
        let part = match &context.part {
            None => None,
            Some(Value::Object(fields)) => {
                if let Some(other) = fields.keys().find(|key| !matches!(key.as_str(), "table" | "view")) {
                    return Err(format!(
                        "the part has `{other}`, which names no part of a table file: a part is `{{\"table\": id}}` or `{{\"view\": id}}`; {LISTED}"
                    ));
                }
                Some(part_of(&self.file, fields, "part")?)
            }
            Some(other) => return Err(format!("the part {other} is not `{{\"table\": id}}` or `{{\"view\": id}}`: {LISTED}")),
        };
        if !context.on.is_empty() {
            let table = part.and_then(|(table, _)| table).ok_or_else(|| {
                "`on` names records or fields of a table, and needs the part's table (`{\"table\": id}` or `{\"view\": id}`) to say which".to_owned()
            })?;
            let found = tables(&self.file)?.into_iter().find(|t| t.id == table).ok_or_else(|| format!("not found: no table {table}"))?;
            let records: Vec<i64> = self.file.list_records(table).map_err(|error| error.to_string())?.iter().map(|record| record.id).collect();
            for item in &context.on {
                let id = item.as_i64().ok_or_else(|| format!("`on` holds {item}: an item is a record's or a field's id, a whole number"))?;
                let (record, field) = (records.contains(&id), found.fields.iter().any(|f| f.id == id));
                match (record, field) {
                    (true, true) => {
                        return Err(format!(
                            "`on` holds {id}, which is both a record and a field of table {table}: give it to a command whose `recordId` or `fieldId` it fills"
                        ));
                    }
                    (false, false) => return Err(format!("`on` holds {id}: table {table} has no record or field {id}")),
                    _ => {}
                }
            }
            return Err(
                "a table file keeps no selection: selecting records or fields from `on` is not available; `on` fills a command's `recordId` or `fieldId`, marked as its targets in table_commands"
                    .into(),
            );
        }
        if let Some((table, view)) = part {
            self.shown = Shown { table, view, ..Shown::default() };
        }
        Ok(())
    }
}
