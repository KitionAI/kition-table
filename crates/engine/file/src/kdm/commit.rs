//! The write path of [`TableFile`]: every mutation is one KDM transaction, stored and projected
//! in one SQL transaction.
//!
//! A write takes the file's write lock (`BEGIN IMMEDIATE`), first catches up with what other
//! handles stored since this one last read the store (imports it, then repairs), runs the
//! transaction on the replica, appends its changes ([`store::append`]), projects every object
//! changed ([`project::project_changes`]), records the projected position, and compacts the log
//! when it has grown. On any failure the SQL transaction rolls back and, when the replica already
//! holds changes the store does not, the replica is reloaded from the store: memory never runs
//! ahead of the file, and an error leaves nothing behind.
//!
//! The `since` of each append is the stored version this handle last read or wrote, never the
//! in-memory replica's own version. The store's [`store::Head`] (position and a random epoch
//! every store write replaces) tells whether anything was stored since, even by another copy of
//! the file written over this one. When the store lacks changes this handle has (an older or
//! different copy replaced the file), they are stored again in a write of their own before the
//! next mutation runs, so a failing mutation never discards them. Undo and redo are repaired
//! before projecting: they can bring back what others changed meanwhile (a name since taken).

use std::collections::BTreeSet;

use kition_kdm::{Changes, ObjectId, Origin, OriginKind, Replica, Tx, UndoScope, Version};
use kition_time::now_millis;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

use super::model;
use super::ops::{CreateAt, TableOp};
use super::project::{self, TableChange};
use super::read::ModelView;
use super::{repair, store};
use crate::file::{self, FileIdentity};
use crate::{Result, TableFile, TableFileError};

/// What [`TableFile::reopen_merging`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// Nothing differed: the handle is on the file's current content.
    Unchanged,
    /// The file held the same document; both sides' changes are now in the file and the handle.
    Merged(TableChange),
    /// The file now holds another document; the handle switched to it wholesale.
    DifferentDocument,
}

/// How a write's catch-up with the store went.
enum CatchUp {
    /// The store held nothing this handle lacked.
    Current,
    /// Changes other handles stored were imported.
    Imported,
    /// The store lacks changes this handle has (the file was replaced by an older copy, or by
    /// another copy): [`TableFile::resync`] appends them again and rebuilds the projection.
    Diverged,
}

impl TableFile {
    /// The origin of this handle's transactions; the default is `system:unknown`, which is no
    /// undo step.
    pub fn set_origin(&mut self, origin: Origin) {
        self.origin = origin;
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// What the last successful write projected, once.
    pub fn take_last_change(&mut self) -> Option<TableChange> {
        self.last_change.take()
    }

    /// Runs `f` as one transaction with this handle's origin. See the module documentation.
    pub(crate) fn commit<R>(&mut self, f: impl FnOnce(&mut Tx<'_>) -> Result<R>) -> Result<(R, TableChange)> {
        let origin = self.origin.clone();
        self.commit_as(&origin, f)
    }

    /// [`TableFile::commit`] with another origin.
    pub(crate) fn commit_as<R>(&mut self, origin: &Origin, f: impl FnOnce(&mut Tx<'_>) -> Result<R>) -> Result<(R, TableChange)> {
        let label = origin.encode();
        self.write(&label, false, |replica| model::transact(replica, origin, f)).map(|(value, change, _)| (value, change))
    }

    /// The history label of this handle's writes: the principal, or nothing for `system` writes
    /// (as the legacy writers left it).
    pub fn actor(&self) -> String {
        if self.origin.kind == OriginKind::System { String::new() } else { self.origin.principal.clone() }
    }

    /// Runs `step` as one write (see the module documentation); with `repair_after`, what it
    /// changed is repaired before it is stored. Answers the step's value, what the write
    /// projected, and whether the repair wrote anything.
    fn write<R>(&mut self, label: &str, repair_after: bool, step: impl FnOnce(&mut Replica) -> Result<R>) -> Result<(R, TableChange, bool)> {
        self.writable()?;
        if self.stale {
            self.reload()?;
        }
        let actor = self.actor();
        let now = now_millis();
        // A store that lacks changes of this handle (the file was replaced by an older copy) gets
        // them back in a write of their own first, so a failing step can never discard them.
        let resynced = self.resync(now)?;
        let mut since = self.since.clone();
        let mut head = self.head.borrow().clone();
        let outcome = (|| {
            let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
            let start = self.replica.version();
            if matches!(catch_up(&tx, &mut self.replica, &mut since, &mut head)?, CatchUp::Diverged) {
                // Replaced again since the resync. Nothing is written, and `since` stays where
                // the handle left it, so the next write resyncs.
                return Ok(None);
            }
            let written = write_in(&tx, &mut self.replica, &mut self.repair_index, &start, &since, &head, label, &actor, now, repair_after, step)?;
            tx.commit()?;
            Ok(Some(written))
        })();
        match outcome {
            Ok(None) => Err(TableFileError::invalid("the ktable file was replaced during the write; try again")),
            Ok(Some((value, change, repaired, written_head))) => {
                self.since = self.replica.version();
                *self.head.get_mut() = written_head;
                self.identity = FileIdentity::of(&self.path);
                if self.base.is_none() {
                    self.base = ModelView::new(&self.replica).base();
                }
                let change = match resynced {
                    Some(earlier) => merge_changes(earlier, change),
                    None => change,
                };
                self.last_change = Some(change.clone());
                Ok((value, change, repaired))
            }
            Err(err) => {
                // The store holds `since` (the catch-up imported only stored changes); anything
                // beyond it in memory is this failed write's own.
                if !since.includes(&self.replica.version()) {
                    if self.reload().is_err() {
                        self.stale = true;
                    }
                } else {
                    self.since = since;
                    *self.head.get_mut() = head;
                }
                Err(err)
            }
        }
    }

    /// Runs `f` as one transaction of the replica as `origin`, stored and projected like every
    /// write: for writes the model has no function for, such as a file's state written over the
    /// replica (`kition_kit::session::write_state`, when a file read again was saved from changes
    /// this replica lacks).
    pub fn transact_as<R>(&mut self, origin: &Origin, f: impl FnOnce(&mut Tx<'_>) -> Result<R>) -> Result<(R, TableChange)> {
        self.commit_as(origin, f)
    }

    /// Seals the replica for a save (kition-sdk data model 7.3): unless its last change already is
    /// a seal, one `system` change (`content-seal`) that depends on every head, stored like any
    /// write; no edit, no undo step. A saved file then holds the replica's content snapshot
    /// ([`TableFile::content_bytes`]). Answers whether it sealed.
    pub fn seal(&mut self) -> Result<bool> {
        if self.replica.is_sealed() {
            return Ok(false);
        }
        let label = Origin::system("content-seal").encode();
        self.write(&label, false, |replica| Ok(replica.seal()?)).map(|(sealed, _, _)| sealed)
    }

    /// Makes `replica` (the replica a host stored for this document, with its history, the file
    /// taken into it: `kition_kit::session::take_file`) this handle's: stored in place of the
    /// file's, repaired, and projected again in place, so the integer ids of rows that stay are
    /// kept. Undo history starts anew. A replica of another document is
    /// [`TableFileError::Invalid`].
    pub fn adopt_replica(&mut self, mut replica: Replica) -> Result<TableChange> {
        self.writable()?;
        if replica.doc_id() != self.replica.doc_id() {
            return Err(TableFileError::invalid("the replica is of another document"));
        }
        let changed = replica.take_changed();
        repair::run(&mut replica, &changed)?;
        replica.take_changed();
        let now = now_millis();
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        store::compact(&tx, &replica, now)?;
        project::rebuild(&tx, &replica, now)?;
        let change = everything(&tx)?;
        store::set_projected_position(&tx, store::stored_position(&tx)?)?;
        let head = store::head(&tx)?;
        tx.commit()?;
        self.since = replica.version();
        *self.head.get_mut() = head;
        self.base = file::find_base(&self.conn, &replica)?;
        self.repair_index = repair::RepairIndex::default();
        self.floor = file::floor_of(&replica);
        self.session_start = replica.version();
        self.replica = replica;
        self.identity = FileIdentity::of(&self.path);
        self.last_change = Some(change.clone());
        Ok(change)
    }

    /// Refuses writes on a handle opened read-only.
    pub(crate) fn writable(&self) -> Result<()> {
        match &self.read_only {
            Some(reason) => Err(TableFileError::ReadOnly(reason.clone())),
            None => Ok(()),
        }
    }

    /// Brings this handle up to the store before a write. When other handles stored changes,
    /// they are imported (they are already projected). When the store lacks changes this handle
    /// has, they are appended again, repaired, and the projection rebuilt, in a committed write
    /// of their own; on failure nothing in memory is discarded, so the next write retries.
    /// Answers what a resync projected.
    fn resync(&mut self, now: i64) -> Result<Option<TableChange>> {
        if store::head(&self.conn)? == *self.head.borrow() {
            return Ok(None);
        }
        let mut since = self.since.clone();
        let mut head = self.head.borrow().clone();
        let outcome = (|| -> Result<Option<(TableChange, store::Head)>> {
            let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
            let caught = catch_up(&tx, &mut self.replica, &mut since, &mut head)?;
            if !matches!(caught, CatchUp::Diverged) {
                // Imported changes were projected by the handle that stored them.
                self.replica.take_changed();
                tx.commit()?;
                return Ok(None);
            }
            let changed = self.replica.take_changed();
            repair::run(&mut self.replica, &changed)?;
            self.replica.take_changed();
            let label = Origin::system("resync").encode();
            store::append(&tx, &self.replica, &since, &label, now)?;
            project::rebuild(&tx, &self.replica, now)?;
            let change = everything(&tx)?;
            let position = store::stored_position(&tx)?;
            store::set_projected_position(&tx, position)?;
            let written = store::head(&tx)?;
            tx.commit()?;
            Ok(Some((change, written)))
        })();
        match outcome? {
            None => {
                self.since = since;
                *self.head.get_mut() = head;
                Ok(None)
            }
            Some((change, written)) => {
                self.since = self.replica.version();
                *self.head.get_mut() = written;
                Ok(Some(change))
            }
        }
    }

    /// Replaces the replica with the stored one (under a new actor: the discarded replica's
    /// counters must never be reused). Undo history is lost.
    pub(crate) fn reload(&mut self) -> Result<()> {
        let (head, replica) = file::load_consistent(&self.conn)?;
        *self.head.get_mut() = head;
        self.since = replica.version();
        self.base = file::find_base(&self.conn, &replica)?;
        self.repair_index = repair::RepairIndex::default();
        self.floor = file::floor_of(&replica);
        self.replica = replica;
        self.stale = false;
        Ok(())
    }

    /// Applies every op as one transaction (one change row, one undo step). Answers the row keys
    /// `records.create` ops created, in order, and what the write projected.
    pub fn transact(&mut self, ops: &[TableOp]) -> Result<(Vec<String>, TableChange)> {
        self.writable()?;
        let resolved = ops.iter().map(|op| self.resolve_op(op)).collect::<Result<Vec<_>>>()?;
        let now = now_millis();
        let created_by = self.actor();
        self.commit(|tx| {
            let mut created = Vec::new();
            for op in &resolved {
                match op {
                    Resolved::Cells(table, edits, links, texts) => {
                        model::set_cells_known(tx, table, edits, texts)?;
                        let records: Vec<ObjectId> = edits.iter().map(|(record, _)| record.clone()).collect();
                        link_records(tx, &records, links)?;
                    }
                    Resolved::Create(table, at, rows, links) => {
                        let ids = model::create_records(tx, table, at.as_ref(), rows, now, &created_by)?;
                        link_records(tx, &ids, links)?;
                        for id in ids {
                            created.push(id.to_string());
                        }
                    }
                    Resolved::Delete(table, records) => {
                        model::delete_records(tx, table, records)?;
                    }
                    Resolved::Move(table, records, before) => {
                        model::move_records(tx, table, records, before.as_ref())?;
                    }
                    Resolved::MoveFields(table, fields, to_index) => {
                        model::move_fields(tx, table, fields, *to_index)?;
                    }
                }
            }
            Ok(created)
        })
    }

    fn resolve_op(&self, op: &TableOp) -> Result<Resolved> {
        Ok(match op {
            TableOp::CellsSet { table_id, records } => {
                let table = table_object(&self.conn, *table_id)?;
                let names = field_objects_by_name(&self.conn, *table_id)?;
                let link_fields = LinkFields::read(&self.conn, *table_id)?;
                let mut edits = Vec::with_capacity(records.len());
                let mut links = Vec::with_capacity(records.len());
                for edit in records {
                    let (cells, link_cells) = link_fields.split(&self.conn, &edit.values)?;
                    edits.push((ObjectId::from_string(edit.row_key.clone()), by_field_id(&names, &cells)));
                    links.push(link_cells);
                }
                let texts = super::cells::Texts::of_records(&self.replica, edits.iter().map(|(record, _)| record));
                Resolved::Cells(table, edits, links, texts)
            }
            TableOp::RecordsCreate { table_id, at, records } => {
                let table = table_object(&self.conn, *table_id)?;
                let names = field_objects_by_name(&self.conn, *table_id)?;
                let at = at.as_ref().map(|at| match at {
                    CreateAt::Before(key) => model::Anchor::Before(ObjectId::from_string(key)),
                    CreateAt::After(key) => model::Anchor::After(ObjectId::from_string(key)),
                });
                let link_fields = LinkFields::read(&self.conn, *table_id)?;
                let mut rows = Vec::with_capacity(records.len());
                let mut links = Vec::with_capacity(records.len());
                for values in records {
                    let (cells, link_cells) = link_fields.split(&self.conn, values)?;
                    rows.push(by_field_id(&names, &cells));
                    links.push(link_cells);
                }
                Resolved::Create(table, at, rows, links)
            }
            TableOp::RecordsDelete { table_id, row_keys } => {
                Resolved::Delete(table_object(&self.conn, *table_id)?, row_keys.iter().map(ObjectId::from_string).collect())
            }
            TableOp::RecordsMove { table_id, row_keys, before } => Resolved::Move(
                table_object(&self.conn, *table_id)?,
                row_keys.iter().map(ObjectId::from_string).collect(),
                before.as_ref().map(ObjectId::from_string),
            ),
            TableOp::FieldsMove { table_id, field_ids, to_index } => Resolved::MoveFields(
                table_object(&self.conn, *table_id)?,
                field_ids.iter().map(|id| field_object(&self.conn, *table_id, *id)).collect::<Result<Vec<_>>>()?,
                *to_index,
            ),
        })
    }

    /// The undo scope of this handle (KitModel M12a): its origin's, so an agent's handle undoes only
    /// the agent's steps and a person's only the person's; `system` and `import` writes have none
    /// and undo as the person.
    pub fn undo_scope(&self) -> UndoScope {
        UndoScope::of(self.origin.kind).unwrap_or(UndoScope::Person)
    }

    /// Undoes this handle's last undo step; `false` when there was none.
    pub fn undo(&mut self) -> Result<(bool, TableChange)> {
        self.undo_in(self.undo_scope())
    }

    /// Undoes the last step of `scope`, keeping the other scope's later steps (engine rule E14a).
    pub fn undo_in(&mut self, scope: UndoScope) -> Result<(bool, TableChange)> {
        self.writable()?;
        let label = self.origin.clone().with_label("undo").encode();
        // An undo can bring back what others changed meanwhile (a name since taken): repaired.
        self.write(&label, true, |replica| Ok(replica.undo_in(scope)?)).map(|(done, change, _)| (done, change))
    }

    /// Redoes the last undone step; `false` when there was none.
    pub fn redo(&mut self) -> Result<(bool, TableChange)> {
        self.redo_in(self.undo_scope())
    }

    /// Redoes the last undone step of `scope`, like [`TableFile::undo_in`].
    pub fn redo_in(&mut self, scope: UndoScope) -> Result<(bool, TableChange)> {
        self.writable()?;
        let label = self.origin.clone().with_label("redo").encode();
        self.write(&label, true, |replica| Ok(replica.redo_in(scope)?)).map(|(done, change, _)| (done, change))
    }

    /// Whether this handle's scope has a step to undo.
    pub fn can_undo(&self) -> bool {
        self.replica.can_undo_in(self.undo_scope())
    }

    /// Whether this handle's scope has a step to redo.
    pub fn can_redo(&self) -> bool {
        self.replica.can_redo_in(self.undo_scope())
    }

    /// Whether `scope` has a step to undo (`undo` true) or redo.
    pub fn can_step_in(&self, scope: UndoScope, undo: bool) -> bool {
        if undo { self.replica.can_undo_in(scope) } else { self.replica.can_redo_in(scope) }
    }

    /// The writes until [`TableFile::end_undo_group`] undo as one step (each is still its own
    /// change row). Groups do not nest.
    pub fn begin_undo_group(&mut self) -> Result<()> {
        Ok(self.replica.begin_undo_group()?)
    }

    pub fn end_undo_group(&mut self) {
        self.replica.end_undo_group();
    }

    /// Whether the file on disk is no longer the one this handle last opened or wrote: replaced
    /// (another inode), or written by someone else (size or modification time).
    pub fn changed_on_disk(&self) -> bool {
        FileIdentity::of(&self.path) != self.identity
    }

    /// Opens the file at this handle's path afresh and moves the handle onto it. When it holds
    /// the same document, every change this handle has that the file lacks is merged into it
    /// (imported, repaired, projected, appended); when it holds another document, the handle
    /// simply switches to it. Undo history does not survive.
    pub fn reopen_merging(&mut self) -> Result<MergeOutcome> {
        if !self.changed_on_disk() {
            return Ok(MergeOutcome::Unchanged);
        }
        let mut fresh = TableFile::open(&self.path)?;
        fresh.origin = self.origin.clone();
        if fresh.replica.doc_id() != self.replica.doc_id() {
            *self = fresh;
            return Ok(MergeOutcome::DifferentDocument);
        }
        let ours = self.replica.version();
        let theirs = fresh.replica.version();
        let file_lacks = !theirs.includes(&ours);
        let we_lack = !ours.includes(&theirs);
        if !file_lacks && !we_lack {
            *self = fresh;
            return Ok(MergeOutcome::Unchanged);
        }
        let mut change = TableChange::default();
        if file_lacks {
            let bytes = self.replica.export_changes(&theirs)?;
            let import = Origin::new(OriginKind::Import, "import");
            fresh.origin = import.clone();
            let merged = fresh.write(&import.encode(), false, |replica| {
                let changed = replica.import_changes(&bytes)?;
                repair::run(replica, &changed)?;
                Ok(())
            });
            fresh.origin = self.origin.clone();
            change = merged?.1;
        }
        if we_lack {
            // This handle's state is behind the file's: whatever the file holds may differ.
            change = everything(&fresh.conn)?;
        }
        let old_records = live_record_keys(&self.conn)?;
        let new_records = live_record_keys(&fresh.conn)?;
        for key in old_records.difference(&new_records) {
            change.records_deleted.push(key.clone());
        }
        change.records_deleted.sort();
        change.records_deleted.dedup();
        fresh.last_change = Some(change.clone());
        *self = fresh;
        Ok(MergeOutcome::Merged(change))
    }

    /// Compacts the change log ([`store::compact`]) when it is past the fixed limits
    /// ([`store::log_is_large`]), for a document held in memory whose bytes are about to be
    /// saved: writes let the log grow to a share of the snapshot, and the saved file would make
    /// every open replay it. Answers whether it compacted. Does nothing for a file on disk (its
    /// writes compact), a read-only or stale handle, a store another handle wrote since this one,
    /// or a projection behind the store.
    pub fn compact_log(&self) -> Result<bool> {
        if !self.path.as_os_str().is_empty() || self.read_only.is_some() || self.stale {
            return Ok(false);
        }
        if store::head(&self.conn)? != *self.head.borrow() || !store::log_is_large(&self.conn)? {
            return Ok(false);
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !store::projection_is_current(&tx)? {
            return Ok(false);
        }
        store::compact(&tx, &self.replica, now_millis())?;
        store::set_projected_position(&tx, store::stored_position(&tx)?)?;
        let head = store::head(&tx)?;
        tx.commit()?;
        *self.head.borrow_mut() = head;
        Ok(true)
    }

    /// This handle's replica version, the `since` a peer passes to
    /// [`TableFile::export_replica_changes`] to get only what it lacks.
    pub fn replica_version(&self) -> Version {
        self.replica.version()
    }

    /// The replica's changes a replica at `since` lacks; every change from [`Version::empty`].
    pub fn export_replica_changes(&self, since: &Version) -> Result<Vec<u8>> {
        Ok(self.replica.export_changes(since)?)
    }

    /// Merges changes another replica of this document exported (a co-editor's, relayed through
    /// a sync room): imported with an `import` origin, so it is never an undo step and the undo
    /// history of this handle's own edits survives it; then repaired, stored, and projected in
    /// one SQL transaction like any other write. Changes already known change nothing (an empty
    /// change). Changes of another document are refused ([`TableFileError::Kdm`]).
    pub fn import_replica_changes(&mut self, changes: &[u8]) -> Result<ReplicaImport> {
        self.writable()?;
        let import = Origin::new(OriginKind::Import, IMPORT_PRINCIPAL);
        let previous = std::mem::replace(&mut self.origin, import.clone());
        let written = self.write(&import.encode(), true, |replica| {
            replica.import_changes(changes)?;
            Ok(())
        });
        self.origin = previous;
        let ((), change, repaired) = written?;
        Ok(ReplicaImport { change, repaired })
    }
}

/// The principal of [`TableFile::import_replica_changes`]'s `import` origin.
pub const IMPORT_PRINCIPAL: &str = "sync";

/// What [`TableFile::import_replica_changes`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplicaImport {
    /// What the projection changed.
    pub change: TableChange,
    /// Whether the repair after the import wrote anything: changes of this replica's own that
    /// the peers lack, so they must be exported to them like a local edit.
    pub repaired: bool,
}

/// The body of one write, inside its SQL transaction, after the catch-up: `start` is the
/// replica's version before the catch-up, `since` and `head` are what the store holds. Answers
/// the step's value, what the projection changed, whether the repair wrote anything, and the
/// store's head afterwards.
///
/// The repair after the step covers only what the step changed ([`repair::run_since`]): the state
/// before it is repaired, since every stored write is (local model writes keep the rules, and
/// imports, undo, and redo are repaired here).
#[allow(clippy::too_many_arguments)]
fn write_in<R>(
    conn: &Connection,
    replica: &mut Replica,
    index: &mut repair::RepairIndex,
    start: &Version,
    since: &Version,
    head: &store::Head,
    label: &str,
    actor: &str,
    now: i64,
    repair_after: bool,
    step: impl FnOnce(&mut Replica) -> Result<R>,
) -> Result<(R, TableChange, bool, store::Head)> {
    // Changes imported by the catch-up were projected by the handle that stored them; projecting
    // them again is harmless and keeps this write's change complete.
    let mut changed = replica.take_changes();
    index.advance(start, replica, &structural(&changed));
    let before = if repair_after { Some(index.before(replica)?) } else { None };
    let stepped_from = replica.version();
    let value = step(replica)?;
    let stepped = replica.take_changes();
    let stepped_structure = structural(&stepped);
    index.advance(&stepped_from, replica, &stepped_structure);
    let mut repaired = false;
    if let Some(before) = before {
        let (wrote, fixed) = repair::run_since(replica, &stepped_structure, &before, index)?;
        repaired = wrote;
        changed.extend(Changes::from(fixed));
    }
    changed.extend(stepped);
    if since.includes(&replica.version()) && changed.is_empty() {
        return Ok((value, TableChange::default(), repaired, head.clone()));
    }
    store::append(conn, replica, since, label, now)?;
    let change = project::project_changes(conn, replica, &changed, actor, now)?;
    if store::should_compact(conn)? {
        store::compact(conn, replica, now)?;
    }
    let stored = store::stored_position(conn)?;
    store::set_projected_position(conn, stored)?;
    Ok((value, change, repaired, store::head(conn)?))
}

/// The changed objects a repair looks at: those with changes other than writes to keys of map
/// fields. Cells, column widths and hidden columns, and dashboard widgets never break a list,
/// orphan an object, name a base, or decide a link.
fn structural(changes: &Changes) -> BTreeSet<ObjectId> {
    changes.objects().iter().filter(|id| changes.map_writes(id).is_none()).cloned().collect()
}

/// Imports what other handles stored since `head`, and moves `since` and `head` to the store's
/// current state.
fn catch_up(conn: &Connection, replica: &mut Replica, since: &mut Version, head: &mut store::Head) -> Result<CatchUp> {
    let stored = store::head(conn)?;
    if stored == *head {
        return Ok(CatchUp::Current);
    }
    let file = file::load_replica(conn)?;
    if file.doc_id() != replica.doc_id() {
        return Err(TableFileError::invalid("the ktable file now holds another document; reopen it"));
    }
    let theirs = file.version();
    let ours = replica.version();
    if !ours.includes(&theirs) {
        replica.import_changes(&file.export_changes(&ours)?)?;
    }
    let diverged = !theirs.includes(&ours);
    *since = theirs;
    *head = stored;
    Ok(if diverged { CatchUp::Diverged } else { CatchUp::Imported })
}

/// Both changes in one, every list sorted and without repeats.
fn merge_changes(mut a: TableChange, b: TableChange) -> TableChange {
    fn join<T: Ord>(a: &mut Vec<T>, b: Vec<T>) {
        a.extend(b);
        a.sort();
        a.dedup();
    }
    join(&mut a.tables, b.tables);
    join(&mut a.schema, b.schema);
    join(&mut a.views, b.views);
    join(&mut a.records_upserted, b.records_upserted);
    join(&mut a.records_deleted, b.records_deleted);
    join(&mut a.records_reordered, b.records_reordered);
    a.dashboards |= b.dashboards;
    a
}

/// A change naming every live table, view, and record: for when the state a reader holds may
/// differ from the file's anywhere.
fn everything(conn: &Connection) -> Result<TableChange> {
    let ids = |sql: &str| -> Result<Vec<i64>> {
        let mut statement = conn.prepare(sql)?;
        let rows = statement.query_map([], |row| row.get(0))?.collect::<std::result::Result<Vec<i64>, _>>()?;
        Ok(rows)
    };
    let tables = ids("SELECT id FROM kition_tables WHERE deleted_at IS NULL ORDER BY id")?;
    let views = ids("SELECT id FROM kition_views WHERE deleted_at IS NULL ORDER BY id")?;
    let mut records: Vec<(i64, String)> = live_record_keys(conn)?.into_iter().collect();
    records.sort();
    Ok(TableChange {
        tables: tables.clone(),
        schema: tables.clone(),
        views,
        records_upserted: records,
        records_deleted: Vec::new(),
        records_reordered: tables,
        dashboards: true,
    })
}

fn live_record_keys(conn: &Connection) -> Result<BTreeSet<(i64, String)>> {
    let mut statement = conn.prepare(
        "SELECT r.table_id, r.row_key FROM kition_records r JOIN kition_tables t ON t.id = r.table_id \
         WHERE r.deleted_at IS NULL AND t.deleted_at IS NULL",
    )?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<std::result::Result<BTreeSet<_>, _>>()?;
    Ok(rows)
}

/// An op with its integer ids and field names resolved to object ids.
enum Resolved {
    /// With the records' long texts as they read before the write.
    Cells(ObjectId, Vec<(ObjectId, serde_json::Map<String, serde_json::Value>)>, Vec<LinkCells>, super::cells::Texts),
    Create(ObjectId, Option<model::Anchor>, Vec<serde_json::Map<String, serde_json::Value>>, Vec<LinkCells>),
    Delete(ObjectId, Vec<ObjectId>),
    Move(ObjectId, Vec<ObjectId>, Option<ObjectId>),
    MoveFields(ObjectId, Vec<ObjectId>, usize),
}

/// `values` keyed by field name, re-keyed by field object id; unknown names are dropped.
pub(crate) fn by_field_id(
    names: &std::collections::HashMap<String, ObjectId>,
    values: &serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    values.iter().filter_map(|(name, value)| names.get(name).map(|id| (id.to_string(), value.clone()))).collect()
}

/// A record's link cells: each `link_to_record` field written, with the records it names.
pub(crate) type LinkCells = Vec<(ObjectId, Vec<ObjectId>)>;

/// Takes the link cells out of `values` (keyed by field name); see [`LinkFields::split`].
pub(crate) fn split_links(
    conn: &Connection,
    table_id: i64,
    values: &serde_json::Map<String, serde_json::Value>,
) -> Result<(serde_json::Map<String, serde_json::Value>, LinkCells)> {
    LinkFields::read(conn, table_id)?.split(conn, values)
}

/// A table's `link_to_record` fields (name → field object id), read once for a write of many
/// records.
pub(crate) struct LinkFields(std::collections::HashMap<String, String>);

impl LinkFields {
    pub(crate) fn read(conn: &Connection, table_id: i64) -> Result<Self> {
        let mut statement = conn.prepare_cached(
            "SELECT name, kdm_id FROM kition_fields WHERE table_id = ?1 AND type = 'link_to_record' AND deleted_at IS NULL AND kdm_id IS NOT NULL",
        )?;
        let fields = statement.query_map([table_id], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<std::result::Result<_, _>>()?;
        Ok(Self(fields))
    }

    /// Takes the link cells out of `values` (keyed by field name). A link field's value lists
    /// the records it links to as row keys, record ids or the `{row_key, display}` objects
    /// records are read with (one alone is a list of one); `null` names none. An id that names no
    /// live record, or another kind of value, is [`TableFileError::Invalid`].
    pub(crate) fn split(
        &self,
        conn: &Connection,
        values: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(serde_json::Map<String, serde_json::Value>, LinkCells)> {
        use serde_json::Value;
        let fields = &self.0;
        let mut cells = serde_json::Map::new();
        let mut links = Vec::new();
        for (name, value) in values {
            let Some(field) = fields.get(name) else {
                cells.insert(name.clone(), value.clone());
                continue;
            };
            let target = |item: &Value| -> Result<ObjectId> {
                let key = match item {
                    Value::String(key) => Some(key.clone()),
                    Value::Object(map) => map.get("row_key").and_then(Value::as_str).map(str::to_owned),
                    Value::Number(number) => match number.as_i64() {
                        Some(id) => Some(
                            conn.prepare_cached("SELECT row_key FROM kition_records WHERE id = ?1 AND deleted_at IS NULL")?
                                .query_row([id], |row| row.get::<_, String>(0))
                                .optional()?
                                .ok_or_else(|| TableFileError::invalid(format!("{name}: no record {id} to link to")))?,
                        ),
                        None => None,
                    },
                    _ => None,
                };
                key.map(ObjectId::from_string)
                    .ok_or_else(|| TableFileError::invalid(format!("{name}: a link value lists row keys or record ids, not {item}")))
            };
            let targets = match value {
                Value::Null => Vec::new(),
                Value::Array(items) => items.iter().map(target).collect::<Result<_>>()?,
                single => vec![target(single)?],
            };
            links.push((ObjectId::from_string(field.clone()), targets));
        }
        Ok((cells, links))
    }
}

/// Writes the link cells of `records` (one entry per record, in order), one field at a time.
pub(crate) fn link_records(tx: &mut Tx<'_>, records: &[ObjectId], cells: &[LinkCells]) -> Result<()> {
    let mut by_field: std::collections::BTreeMap<ObjectId, Vec<(ObjectId, Vec<ObjectId>)>> = std::collections::BTreeMap::new();
    for (record, links) in records.iter().zip(cells) {
        for (field, targets) in links {
            by_field.entry(field.clone()).or_default().push((record.clone(), targets.clone()));
        }
    }
    for (field, edits) in by_field {
        model::replace_links_many(tx, &field, &edits)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Integer ids and object ids.

fn object_or_not_found(found: Option<Option<String>>) -> Result<ObjectId> {
    match found {
        Some(Some(id)) => Ok(ObjectId::from_string(id)),
        _ => Err(TableFileError::NotFound),
    }
}

/// The object a live table row projects.
pub(crate) fn table_object(conn: &Connection, table_id: i64) -> Result<ObjectId> {
    object_or_not_found(
        conn.prepare_cached("SELECT kdm_id FROM kition_tables WHERE id = ?1 AND deleted_at IS NULL")?
            .query_row([table_id], |row| row.get(0))
            .optional()?,
    )
}

/// The object a live field row of the table projects.
pub(crate) fn field_object(conn: &Connection, table_id: i64, field_id: i64) -> Result<ObjectId> {
    object_or_not_found(
        conn.prepare_cached("SELECT kdm_id FROM kition_fields WHERE id = ?1 AND table_id = ?2 AND deleted_at IS NULL")?
            .query_row([field_id, table_id], |row| row.get(0))
            .optional()?,
    )
}

/// The object a live view row of the table projects.
pub(crate) fn view_object(conn: &Connection, table_id: i64, view_id: i64) -> Result<ObjectId> {
    object_or_not_found(
        conn.prepare_cached("SELECT kdm_id FROM kition_views WHERE id = ?1 AND table_id = ?2 AND deleted_at IS NULL")?
            .query_row([view_id, table_id], |row| row.get(0))
            .optional()?,
    )
}

/// The object (its row key) of a live record row of the table.
pub(crate) fn record_object(conn: &Connection, table_id: i64, record_id: i64) -> Result<ObjectId> {
    object_or_not_found(
        conn.prepare_cached("SELECT row_key FROM kition_records WHERE id = ?1 AND table_id = ?2 AND deleted_at IS NULL")?
            .query_row([record_id, table_id], |row| row.get(0))
            .optional()?,
    )
}

fn live_id(conn: &Connection, sql: &str, key: &str) -> Result<i64> {
    conn.prepare_cached(sql)?.query_row([key], |row| row.get(0)).optional()?.ok_or(TableFileError::NotFound)
}

pub(crate) fn table_id_of(conn: &Connection, object: &ObjectId) -> Result<i64> {
    live_id(conn, "SELECT id FROM kition_tables WHERE kdm_id = ?1 AND deleted_at IS NULL", object.as_str())
}

pub(crate) fn field_id_of(conn: &Connection, object: &ObjectId) -> Result<i64> {
    live_id(conn, "SELECT id FROM kition_fields WHERE kdm_id = ?1 AND deleted_at IS NULL", object.as_str())
}

pub(crate) fn view_id_of(conn: &Connection, object: &ObjectId) -> Result<i64> {
    live_id(conn, "SELECT id FROM kition_views WHERE kdm_id = ?1 AND deleted_at IS NULL", object.as_str())
}

pub(crate) fn record_id_of(conn: &Connection, row_key: &str) -> Result<i64> {
    live_id(conn, "SELECT id FROM kition_records WHERE row_key = ?1 AND deleted_at IS NULL", row_key)
}

/// The table's live fields: object id by name.
pub(crate) fn field_objects_by_name(conn: &Connection, table_id: i64) -> Result<std::collections::HashMap<String, ObjectId>> {
    let mut statement =
        conn.prepare_cached("SELECT name, kdm_id FROM kition_fields WHERE table_id = ?1 AND deleted_at IS NULL AND kdm_id IS NOT NULL")?;
    let rows = statement
        .query_map([table_id], |row| Ok((row.get::<_, String>(0)?, ObjectId::from_string(row.get::<_, String>(1)?))))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

/// The object id of any table or field row, live or deleted (`None`: no such row, or one the
/// replica never knew).
pub(crate) fn any_object(conn: &Connection, table: &str, id: i64) -> Result<Option<ObjectId>> {
    let found: Option<Option<String>> =
        conn.prepare_cached(&format!("SELECT kdm_id FROM {table} WHERE id = ?1"))?.query_row([id], |row| row.get(0)).optional()?;
    Ok(found.flatten().map(ObjectId::from_string))
}
