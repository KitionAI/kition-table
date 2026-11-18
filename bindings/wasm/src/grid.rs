//! The editor's arrangement (`kition-table-arrange`) over the open document: the extras
//! `gridOpen`, `gridSync`, `gridHits` and `gridStats`.
//!
//! `gridOpen` arranges a view of a table for the editor (its settings as the editor holds them,
//! its search, its collapsed groups) and answers the axis (`kition_table_arrange::axis`, format
//! v2). `gridHits` answers which cells of a run of its rows hold the search. The document then notes what every request and every imported change touches; `gridSync`
//! brings the arrangement to the document's current state, moving only the records the changes
//! touched, and answers the new axis, or no bytes when the axis is the same. A change to the
//! table's fields, its records' positions, or the view's settings arranges it again; so does any
//! change the document could not attribute.

use std::collections::BTreeSet;

use kition_kdm::Version;
use kition_table_arrange::{Arranged, Arrangement, Stats, Stopwatch};
use kition_table_file::{TableChange, TableFile, View};
use serde_json::{Map, Value, json};

/// What the editor arranges: a table, its view, the view's settings as the editor holds them,
/// its search.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    table_id: i64,
    view_id: Option<i64>,
    config: Option<Map<String, Value>>,
    query: String,
}

struct Session {
    key: Key,
    collapsed: BTreeSet<String>,
    settings: Arrangement,
    arranged: Arranged,
    /// The replica version the arrangement and the noted changes reach.
    seen: Version,
    touched: BTreeSet<String>,
    view_changed: bool,
    full: bool,
}

/// The document's arrangement for the editor, and its timings.
#[derive(Default)]
pub(crate) struct Grid {
    session: Option<Session>,
    epoch: u32,
    stats: Stats,
}

fn same_settings(a: &Arrangement, b: &Arrangement) -> bool {
    let names =
        |items: &[(kition_table_file::Field, bool)]| items.iter().map(|(field, descending)| (field.name.clone(), *descending)).collect::<Vec<_>>();
    a.filter == b.filter && names(&a.groups) == names(&b.groups) && names(&a.sorts) == names(&b.sorts)
}

/// The table's fields and the view, and the view's settings with the editor's `config`.
fn settings(file: &TableFile, key: &Key) -> Result<(Vec<kition_table_file::Field>, Arrangement), String> {
    let table = file.get_table(key.table_id).map_err(|_| format!("not found: table {}", key.table_id))?;
    let view: Option<&View> = match key.view_id {
        Some(id) => Some(table.views.iter().find(|view| view.id == id).ok_or_else(|| format!("not found: view {id} of table {}", key.table_id))?),
        None => None,
    };
    let settings = match (view, &key.config) {
        (Some(view), Some(config)) => Arrangement::of_config(file, &table.fields, view, config)?,
        (view, _) => Arrangement::of(file, &table.fields, view)?,
    };
    Ok((table.fields, settings))
}

impl Grid {
    /// Notes a request or an import that moved the replica from `before` to `after` and changed
    /// `change` (`None` when nothing says what it changed).
    pub(crate) fn note(&mut self, before: &Version, after: &Version, change: Option<&TableChange>) {
        let Some(session) = &mut self.session else { return };
        if &session.seen != before {
            session.full = true;
        }
        if before != after {
            match change {
                Some(change) => session.absorb(change),
                None => session.full = true,
            }
        }
        session.seen = after.clone();
    }

    /// The document was replaced (a failed import put the old one back): arrange again.
    pub(crate) fn replaced(&mut self) {
        if let Some(session) = &mut self.session {
            session.full = true;
        }
    }

    /// `gridOpen`: `{tableId, viewId?, config?, query?, collapsed?}`; answers the axis.
    pub(crate) fn open(&mut self, file: &TableFile, params: &Value) -> Result<Vec<u8>, String> {
        let table_id = params.get("tableId").and_then(Value::as_i64).ok_or("invalid params: tableId is a table's id")?;
        let view_id = match params.get("viewId") {
            None | Some(Value::Null) => None,
            Some(value) => Some(value.as_i64().ok_or("invalid params: viewId is a view's id")?),
        };
        let config = match params.get("config") {
            None | Some(Value::Null) => None,
            Some(Value::Object(config)) => Some(config.clone()),
            Some(_) => return Err("invalid params: config is a view config object".into()),
        };
        let query = match params.get("query") {
            None | Some(Value::Null) => String::new(),
            Some(value) => value.as_str().ok_or("invalid params: query is text")?.to_owned(),
        };
        let collapsed: BTreeSet<String> = match params.get("collapsed") {
            None | Some(Value::Null) => BTreeSet::new(),
            Some(value) => value
                .as_array()
                .and_then(|items| items.iter().map(|item| item.as_str().map(str::to_owned)).collect::<Option<_>>())
                .ok_or("invalid params: collapsed lists group ids")?,
        };
        let key = Key { table_id, view_id, config, query };
        let same = self.session.as_ref().is_some_and(|session| session.key == key);
        if same {
            // Only the collapsed groups differ: the arrangement stands, brought up to date.
            self.bring_up_to_date(file)?;
        } else {
            let (fields, settings) = settings(file, &key)?;
            let watch = Stopwatch::start();
            let arranged = Arranged::new(file, key.table_id, &fields, &settings, &key.query)?;
            self.stats.arrange.record(watch.elapsed_ms(), arranged.total());
            self.session = Some(Session {
                key,
                collapsed: BTreeSet::new(),
                settings,
                arranged,
                seen: file.replica_version(),
                touched: BTreeSet::new(),
                view_changed: false,
                full: false,
            });
        }
        let session = self.session.as_mut().ok_or("no grid is open")?;
        session.collapsed = collapsed;
        self.epoch += 1;
        Ok(self.emit())
    }

    /// `gridSync`: the axis at the document's current state, or no bytes when it is the same.
    pub(crate) fn sync(&mut self, file: &TableFile) -> Result<Vec<u8>, String> {
        if self.bring_up_to_date(file)? {
            self.epoch += 1;
            return Ok(self.emit());
        }
        Ok(Vec::new())
    }

    /// `gridHits`: `{epoch, from, count, fieldIds}`; which of the fields `fieldIds` hold the search
    /// in the `count` rows of the axis `epoch` from the `from`th (hits format v1). An error when
    /// `epoch` is not the latest axis: its rows are not this arrangement's.
    pub(crate) fn hits(&mut self, file: &TableFile, params: &Value) -> Result<Vec<u8>, String> {
        let number = |key: &str| params.get(key).and_then(Value::as_u64).ok_or_else(|| format!("invalid params: {key} is a whole number"));
        let epoch = u32::try_from(number("epoch")?).map_err(|_| "invalid params: epoch is an axis's epoch".to_owned())?;
        let (from, count) = (usize::try_from(number("from")?).unwrap_or(usize::MAX), usize::try_from(number("count")?).unwrap_or(usize::MAX));
        let field_ids: Vec<i64> = params
            .get("fieldIds")
            .and_then(Value::as_array)
            .and_then(|items| items.iter().map(Value::as_i64).collect())
            .ok_or("invalid params: fieldIds lists field ids")?;
        let session = self.session.as_ref().ok_or("no grid is open: call gridOpen first")?;
        if epoch != self.epoch {
            return Err(format!("stale: the axis {epoch} is not the latest ({})", self.epoch));
        }
        let watch = Stopwatch::start();
        let hits = session.arranged.hits(file, epoch, from, count, &field_ids)?;
        self.stats.hits.record(watch.elapsed_ms(), hits.count as usize);
        Ok(hits.encode())
    }

    /// `gridStats`: how long arranging took.
    pub(crate) fn stats(&self) -> Value {
        let mut stats = self.stats.json();
        stats["epoch"] = json!(self.epoch);
        stats["shown"] = json!(self.session.as_ref().map_or(0, |session| session.arranged.ids().count()));
        stats
    }

    fn emit(&mut self) -> Vec<u8> {
        let Some(session) = &self.session else { return Vec::new() };
        let bytes = session.arranged.axis(self.epoch, &session.collapsed).encode();
        self.stats.axis_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        bytes
    }

    /// Applies what was noted since the arrangement; answers whether the axis changed.
    fn bring_up_to_date(&mut self, file: &TableFile) -> Result<bool, String> {
        let session = self.session.as_mut().ok_or("no grid is open: call gridOpen first")?;
        let current = file.replica_version();
        if session.seen != current {
            session.full = true;
            session.seen = current;
        }
        if session.view_changed && !session.full {
            let (_, settings) = settings(file, &session.key)?;
            session.full = !same_settings(&settings, &session.settings);
        }
        let touched = std::mem::take(&mut session.touched);
        session.view_changed = false;
        if std::mem::take(&mut session.full) {
            let (fields, settings) = settings(file, &session.key)?;
            let watch = Stopwatch::start();
            session.arranged = Arranged::new(file, session.key.table_id, &fields, &settings, &session.key.query)?;
            session.settings = settings;
            self.stats.arrange.record(watch.elapsed_ms(), session.arranged.total());
            return Ok(true);
        }
        if touched.is_empty() {
            return Ok(false);
        }
        let watch = Stopwatch::start();
        let changed = session.arranged.relocate(file, &touched)?;
        self.stats.relocate.record(watch.elapsed_ms(), touched.len());
        Ok(changed)
    }
}

impl Session {
    fn absorb(&mut self, change: &TableChange) {
        let table = self.key.table_id;
        if change.tables.contains(&table) || change.schema.contains(&table) || change.records_reordered.contains(&table) {
            self.full = true;
        }
        if self.key.view_id.is_some_and(|view| change.views.contains(&view)) {
            self.view_changed = true;
        }
        for (owner, row_key) in change.records_upserted.iter().chain(&change.records_deleted) {
            if *owner == table {
                self.touched.insert(row_key.clone());
            }
        }
    }
}
