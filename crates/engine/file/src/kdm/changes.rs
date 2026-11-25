//! Which records changed since a version of the replica, and who changed them.
//!
//! Every stored write notes the version it reached and its origin in the origin log
//! ([`store::record_origin`]), which survives compaction. [`TableFile::record_changes_since`]
//! walks the log from a version within the session's change log: for each write after it, the
//! replica as it was before the write (`Replica::fork_at` the previous version, which takes back
//! the later changes by the old values their operations carry) takes the write's changes
//! (exported from a fork at the version the write reached), and the records the import changed are
//! compared before and after.
//!
//! Before the session ([`TableFile::stored_record_history`]) nothing holds changes: the host keeps
//! a snapshot per checkpoint, and a record's history is what differs between consecutive ones.
//! A record no table listed before and one lists after was created; one listed before and no
//! longer listed was deleted; one listed on both sides whose cells (or links) differ was updated,
//! in those fields.
//!
//! Changes the log cannot attribute (writes older than the log keeps, or writes made before the
//! log existed) are reported with the origin [`UNKNOWN_ORIGIN`].

use std::collections::{BTreeSet, HashMap};

use kition_kdm::{ObjectId, Origin, OriginKind, Replica, Version};
use rusqlite::OptionalExtension;
use serde::Serialize;
use serde_json::Value;

use super::model::{CELL_TEXT, LINK, RECORD};
use super::read::ModelView;
use super::store;
use crate::{Result, TableFile, TableFileError};

/// The origin of changes the origin log cannot attribute.
pub const UNKNOWN_ORIGIN: &str = "system:unknown";

/// What happened to a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordChangeKind {
    Created,
    Updated,
    Deleted,
}

/// One record a write created, updated, or deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordChange {
    pub table_id: i64,
    pub record_id: i64,
    /// The fields whose values changed (updates only), sorted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field_ids: Option<Vec<i64>>,
    /// The write's origin: `person:<principal>`, `agent:<principal>`, `extension:<principal>`,
    /// `import:<principal>`, or `system:<label>`.
    pub origin: String,
}

/// The records created, updated, and deleted since a version, in the order the writes made them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordChanges {
    pub created: Vec<RecordChange>,
    pub updated: Vec<RecordChange>,
    pub deleted: Vec<RecordChange>,
}

/// An encoded origin (`Origin::encode`, the origin log's form) as `kind:principal`, or
/// `system:<label>` for a system write.
pub fn render_origin(origin: &Origin) -> String {
    match origin.kind {
        OriginKind::System => format!("system:{}", origin.label.clone().unwrap_or_else(|| origin.principal.clone())),
        kind => format!("{}:{}", kind.as_str(), origin.principal),
    }
}

fn render_encoded(encoded: &str) -> String {
    Origin::decode(encoded).map_or_else(|| UNKNOWN_ORIGIN.to_owned(), |origin| render_origin(&origin))
}

impl TableFile {
    /// The records created, updated, and deleted since `since` (a version of this file's replica;
    /// [`Version::empty`] for every change), each with the origin of the write that made it.
    /// [`TableFileError::Invalid`] when `since` names changes the replica does not have.
    pub fn record_changes_since(&self, since: &Version) -> Result<RecordChanges> {
        let current = self.replica.version();
        if !current.includes(since) {
            return Err(TableFileError::invalid("the version names changes this document does not have"));
        }
        // The replica holds the changes of its own session only: what came before the version it
        // was loaded at is the content it opened, not changes since `since`.
        let since = if since.includes(&self.floor) { since } else { &self.floor };
        let mut walk = Walk { file: self, prev: fork(&self.replica, since)?, out: Collected::default() };
        if since.includes(&current) {
            return Ok(walk.out.finish());
        }
        // The log's floor: where the rows it dropped had brought the replica.
        if let Some(floor) = store::origin_log_floor(&self.conn)?
            && current.includes(&floor)
            && !walk.prev.version().includes(&floor)
        {
            walk.step(Some(&floor), UNKNOWN_ORIGIN)?;
        }
        for (version, encoded) in store::origin_log(&self.conn)? {
            // A row the replica never reached (the file was replaced by another copy) is skipped;
            // whatever it holds is reported at the end.
            if walk.prev.version().includes(&version) || !current.includes(&version) {
                continue;
            }
            let target = if version == current { None } else { Some(&version) };
            walk.step(target, &render_encoded(&encoded))?;
        }
        if !walk.prev.version().includes(&current) {
            walk.step(None, UNKNOWN_ORIGIN)?;
        }
        Ok(walk.out.finish())
    }
}

fn fork(replica: &Replica, at: &Version) -> Result<Replica> {
    Ok(replica.fork_at(at, crate::ids::actor())?)
}

struct Walk<'a> {
    file: &'a TableFile,
    /// The replica as it was after the last step.
    prev: Replica,
    out: Collected,
}

impl Walk<'_> {
    /// Moves `prev` to `to` (`None`: the current version), attributing what changed to `origin`.
    fn step(&mut self, to: Option<&Version>, origin: &str) -> Result<()> {
        let changes = match to {
            Some(version) => fork(&self.file.replica, version)?.export_changes(&self.prev.version())?,
            None => self.file.replica.export_changes(&self.prev.version())?,
        };
        let mut next = self.prev.fork(crate::ids::actor())?;
        let changed = next.import_changes(&changes)?;
        diff(self.file, &self.prev, &next, &changed, origin, &mut self.out)?;
        self.prev = next;
        Ok(())
    }
}

#[derive(Default)]
struct Collected {
    changes: Vec<(RecordChangeKind, RecordChange)>,
    /// Where each `(kind, record, origin)` already is in `changes`, to merge repeats.
    index: HashMap<(RecordChangeKind, i64, String), usize>,
}

impl Collected {
    fn push(&mut self, kind: RecordChangeKind, change: RecordChange) {
        let key = (kind, change.record_id, change.origin.clone());
        if let Some(at) = self.index.get(&key) {
            if let (Some((_, existing)), Some(fields)) = (self.changes.get_mut(*at), change.field_ids) {
                let merged = existing.field_ids.get_or_insert_with(Vec::new);
                merged.extend(fields);
                merged.sort_unstable();
                merged.dedup();
            }
            return;
        }
        self.index.insert(key, self.changes.len());
        self.changes.push((kind, change));
    }

    fn finish(self) -> RecordChanges {
        let mut out = RecordChanges::default();
        for (kind, change) in self.changes {
            match kind {
                RecordChangeKind::Created => out.created.push(change),
                RecordChangeKind::Updated => out.updated.push(change),
                RecordChangeKind::Deleted => out.deleted.push(change),
            }
        }
        out
    }
}

fn is_type(replica: &Replica, id: &ObjectId, ty: &str) -> bool {
    replica.object_type(id).is_ok_and(|found| found == ty)
}

/// A record's cells as plain values ([`super::cells::decode`]).
fn cells(replica: &Replica, id: &ObjectId) -> Result<HashMap<String, Value>> {
    if !replica.exists(id) {
        return Ok(HashMap::new());
    }
    Ok(replica.map_entries(id, "cells")?.into_iter().map(|(key, raw)| (key, super::cells::decode(replica, &raw))).collect())
}

/// The record's row (`(id, table id)`), live or deleted, in the file's projection.
fn record_row(file: &TableFile, row_key: &str) -> Result<Option<(i64, i64)>> {
    Ok(file
        .conn
        .prepare_cached("SELECT id, table_id FROM kition_records WHERE row_key = ?1 ORDER BY deleted_at IS NOT NULL, id DESC LIMIT 1")?
        .query_row([row_key], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?)
}

/// The field row's id for a field object, live or deleted.
fn field_id(file: &TableFile, cache: &mut HashMap<String, Option<i64>>, object: &str) -> Result<Option<i64>> {
    if let Some(found) = cache.get(object) {
        return Ok(*found);
    }
    let found = file
        .conn
        .prepare_cached("SELECT id FROM kition_fields WHERE kdm_id = ?1 ORDER BY deleted_at IS NOT NULL, id DESC LIMIT 1")?
        .query_row([object], |row| row.get(0))
        .optional()?;
    cache.insert(object.to_owned(), found);
    Ok(found)
}

/// Collects what one step changed in records: `changed` are the objects the step's import
/// changed, `prev` and `next` the replica before and after it.
fn diff(file: &TableFile, prev: &Replica, next: &Replica, changed: &BTreeSet<ObjectId>, origin: &str, out: &mut Collected) -> Result<()> {
    let before = ModelView::new(prev);
    let after = ModelView::new(next);
    let mut fields = HashMap::new();
    // Updated records and their fields, in object order; links add their record's link field.
    let mut updated: Vec<(ObjectId, BTreeSet<i64>)> = Vec::new();
    let mut updated_at: HashMap<ObjectId, usize> = HashMap::new();
    let mut note = |record: ObjectId, field: Option<i64>, updated: &mut Vec<(ObjectId, BTreeSet<i64>)>| {
        let at = *updated_at.entry(record.clone()).or_insert_with(|| {
            updated.push((record, BTreeSet::new()));
            updated.len() - 1
        });
        if let (Some(field), Some((_, set))) = (field, updated.get_mut(at)) {
            set.insert(field);
        }
    };
    for id in changed {
        if is_type(prev, id, RECORD) || is_type(next, id, RECORD) {
            let was = before.table_of_record(id)?.is_some();
            let is = after.table_of_record(id)?.is_some();
            let kind = match (was, is) {
                (false, true) => RecordChangeKind::Created,
                (true, false) => RecordChangeKind::Deleted,
                (true, true) => {
                    let (old, new) = (cells(prev, id)?, cells(next, id)?);
                    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
                    for key in keys {
                        if old.get(key) != new.get(key) {
                            let field = field_id(file, &mut fields, key)?;
                            note(id.clone(), field, &mut updated);
                        }
                    }
                    continue;
                }
                (false, false) => continue,
            };
            if let Some((record_id, table_id)) = record_row(file, id.as_str())? {
                out.push(kind, RecordChange { table_id, record_id, field_ids: None, origin: origin.to_owned() });
            }
        } else if is_type(prev, id, CELL_TEXT) || is_type(next, id, CELL_TEXT) {
            // A long text's characters changed: its record's cell did.
            let source = if next.exists(id) { next } else { prev };
            let end = |name: &str| -> Result<String> { Ok(source.get(id, name)?.as_str().unwrap_or_default().to_owned()) };
            let (record, field) = (ObjectId::from_string(end("record")?), end("field")?);
            if before.table_of_record(&record)?.is_some() && after.table_of_record(&record)?.is_some() {
                let field = field_id(file, &mut fields, &field)?;
                note(record, field, &mut updated);
            }
        } else if is_type(prev, id, LINK) || is_type(next, id, LINK) {
            let source = if next.exists(id) { next } else { prev };
            let end = |name: &str| -> Result<String> { Ok(source.get(id, name)?.as_str().unwrap_or_default().to_owned()) };
            let (from, field) = (ObjectId::from_string(end("from")?), end("field")?);
            // A link of a record created or deleted in this step is part of that change.
            if before.table_of_record(&from)?.is_some() && after.table_of_record(&from)?.is_some() {
                let field = field_id(file, &mut fields, &field)?;
                note(from, field, &mut updated);
            }
        }
    }
    for (record, field_ids) in updated {
        if let Some((record_id, table_id)) = record_row(file, record.as_str())? {
            out.push(
                RecordChangeKind::Updated,
                RecordChange { table_id, record_id, field_ids: Some(field_ids.into_iter().collect()), origin: origin.to_owned() },
            );
        }
    }
    Ok(())
}

/// A point of the document's stored history the host names (a checkpoint, Kit UI protocol
/// section 7m): the replica version it names, when it was recorded (milliseconds since the Unix
/// epoch), who made it when the host knows, and the snapshot the host keeps for it
/// (`host/checkpointRead`), when the caller read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPoint {
    pub version: Version,
    pub time: i64,
    pub author: Option<String>,
    pub snapshot: Option<Vec<u8>>,
}

/// How many of the newest checkpoints [`TableFile::stored_record_history`] reads: each one is
/// the whole document's snapshot, loaded as a replica, so a record's history costs this many
/// loads of the document at most (the editor reads as many, `web/src/App.tsx`).
pub const STORED_HISTORY_POINTS: usize = 10;

/// Where a stretch of [`TableFile::stored_record_history`] ends: a checkpoint, or the state this
/// session started at.
enum Stop<'a> {
    Point(&'a HistoryPoint),
    SessionStart,
}

impl TableFile {
    /// The replica version this handle's session started at: the record history rows
    /// ([`TableFile::list_record_history`]) hold what happened after it.
    pub fn session_start(&self) -> &Version {
        &self.session_start
    }

    /// A record's history before this session, newest first, at the granularity of the host's
    /// checkpoints: `points` are the checkpoints, oldest first, and of them the newest
    /// [`STORED_HISTORY_POINTS`] that carry their snapshot are read, one at a time (two of them
    /// are held at once). Between each two consecutive ones, and from the newest to the state this
    /// session started at, the record's creation, deletion or each field that differs (before and
    /// after values, as the session's rows hold them) is one entry, at the later checkpoint's time
    /// and by its author (time 0 and no author for the stretch up to the session's start, when no
    /// checkpoint names it). What happened before the oldest checkpoint read, several changes
    /// between two checkpoints (only their sum shows) and who made each of them are not known: the
    /// host keeps no changes, and a stored snapshot opens with an empty change log, so the
    /// session's own log starts at its start. A checkpoint taken in this session tells nothing the
    /// session's own rows do not and is skipped, as is one without its snapshot. A record whose
    /// changes all came in this session, or a document with no checkpoints, has none. A snapshot
    /// of another document is [`TableFileError::Invalid`].
    pub fn stored_record_history(&self, table_id: i64, record_id: i64, points: &[HistoryPoint]) -> Result<Vec<crate::HistoryEntry>> {
        let record = self
            .conn
            .prepare_cached("SELECT row_key FROM kition_records WHERE id = ?1 AND table_id = ?2")?
            .query_row(rusqlite::params![record_id, table_id], |row| row.get::<_, String>(0))
            .optional()?;
        let Some(record) = record.map(ObjectId::from_string) else { return Ok(Vec::new()) };
        let end = self.session_start.clone();
        // The checkpoints up to the session's start, each once.
        let mut read: Vec<&HistoryPoint> = Vec::new();
        for point in points.iter().filter(|point| point.snapshot.is_some()) {
            if point.version != end && point.version.includes(&end) {
                continue;
            }
            if read.last().is_some_and(|last| last.version == point.version) {
                continue;
            }
            read.push(point);
        }
        let skip = read.len().saturating_sub(STORED_HISTORY_POINTS);
        let read = read.split_off(skip);
        let Some(first) = read.first() else { return Ok(Vec::new()) };
        let document_id = self.info()?.document_id;
        let mut names = HashMap::new();
        let mut entries = Vec::new();
        // The stops after the first: each later checkpoint, then the session's start.
        let mut stops: Vec<Stop<'_>> = read.iter().skip(1).copied().map(Stop::Point).collect();
        if read.last().is_some_and(|last| last.version != end) {
            stops.push(Stop::SessionStart);
        }
        let mut prev = self.point_replica(first)?;
        for stop in stops {
            // Each checkpoint's replica is loaded when its stretch is told, and dropped after the next.
            let (next, time, actor) = match stop {
                Stop::Point(point) => (self.point_replica(point)?, point.time, point.author.clone().unwrap_or_default()),
                Stop::SessionStart => match fork(&self.replica, &end) {
                    Ok(start) => (start, 0, String::new()),
                    Err(_) => break,
                },
            };
            let was = ModelView::new(&prev).table_of_record(&record)?.is_some();
            let is = ModelView::new(&next).table_of_record(&record)?.is_some();
            let (old, new) = (cells(&prev, &record)?, cells(&next, &record)?);
            let mut entry = |field_id: Option<i64>, action: crate::HistoryAction, before: Value, after: Value| {
                entries.push(crate::HistoryEntry {
                    id: 0,
                    document_id: document_id.clone(),
                    table_id,
                    record_id,
                    field_id,
                    action: action.as_str().to_owned(),
                    before,
                    after,
                    actor: actor.clone(),
                    created_at: time,
                });
            };
            match (was, is) {
                (false, true) => entry(None, crate::HistoryAction::Create, Value::Null, named(self, &mut names, &new)?),
                (true, false) => entry(None, crate::HistoryAction::Delete, named(self, &mut names, &old)?, Value::Null),
                (true, true) => {
                    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
                    for key in keys {
                        let (before, after) = (old.get(key), new.get(key));
                        if before != after {
                            let field = field_row(self, &mut names, key)?.map(|(id, _)| id);
                            entry(field, crate::HistoryAction::Update, before.cloned().unwrap_or(Value::Null), after.cloned().unwrap_or(Value::Null));
                        }
                    }
                }
                (false, false) => {}
            }
            prev = next;
        }
        entries.reverse();
        Ok(entries)
    }

    /// The document as the checkpoint `point` holds it (its snapshot, which [`HistoryPoint`]
    /// carries when the caller read it).
    fn point_replica(&self, point: &HistoryPoint) -> Result<Replica> {
        let snapshot = point.snapshot.as_deref().unwrap_or_default();
        let replica = Replica::load(super::schema(), snapshot, crate::ids::actor())?;
        if replica.doc_id() != self.replica.doc_id() {
            return Err(TableFileError::invalid("a checkpoint holds another document"));
        }
        Ok(replica)
    }
}

/// The field row's `(id, name)` for a field object, live or deleted.
fn field_row(file: &TableFile, cache: &mut HashMap<String, Option<(i64, String)>>, object: &str) -> Result<Option<(i64, String)>> {
    if let Some(found) = cache.get(object) {
        return Ok(found.clone());
    }
    let found = file
        .conn
        .prepare_cached("SELECT id, name FROM kition_fields WHERE kdm_id = ?1 ORDER BY deleted_at IS NOT NULL, id DESC LIMIT 1")?
        .query_row([object], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    cache.insert(object.to_owned(), found.clone());
    Ok(found)
}

/// A record's cells keyed by field name, as a create or delete row holds its values.
fn named(file: &TableFile, cache: &mut HashMap<String, Option<(i64, String)>>, cells: &HashMap<String, Value>) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in cells {
        if let Some((_, name)) = field_row(file, cache, key)? {
            out.insert(name, value.clone());
        }
    }
    Ok(Value::Object(out))
}
