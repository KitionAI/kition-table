//! Storage for a table's KDM replica inside its `.ktable` file.
//!
//! The replica lives as a compacted snapshot (`kition_kdm`) plus an append-only log of changes
//! since that snapshot (`kition_kdm_changes`). The relational tables (`kition_tables`,
//! `kition_fields`, ...) are a projection of the replica, written separately by a projector; this
//! module only stores and loads the replica itself.
//!
//! Every function here takes a plain `&Connection`. Callers that need crash safety open their own
//! SQL transaction and pass its `rusqlite::Transaction` (which derefs to `Connection`): if the
//! transaction is rolled back, nothing written through it is kept. None of these functions start
//! or end a transaction themselves.

use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, params};

use kition_kdm::{ActorId, Replica, Schema, Version};

use crate::{Result, TableFileError, file};

/// Above this many change rows, or this many bytes of change rows (or [`COMPACT_SNAPSHOT_SHARE`]
/// of the snapshot, when that is more), [`should_compact`] says yes.
const COMPACT_MAX_CHANGES: i64 = 500;
const COMPACT_MAX_BYTES: i64 = 1024 * 1024;

/// The change log may grow to a quarter of the snapshot before [`should_compact`] says yes. A
/// compaction writes the whole snapshot again, so a fixed byte limit made a large document write
/// its snapshot after every few bulk writes (quadratic in the document's size); a share of the
/// snapshot keeps the bytes compactions write in proportion to the bytes the log took in, and
/// what an open replays to a quarter of what it loads.
const COMPACT_SNAPSHOT_SHARE: i64 = 4;

const PROJECTED_GENERATION_KEY: &str = "kdm_projected_generation";
const PROJECTED_SEQ_KEY: &str = "kdm_projected_seq";

/// A random value replaced by every write of the store ([`save_initial`], [`append`],
/// [`compact`]): two copies of one file at the same `(generation, seq)` that were written
/// separately still differ in it. Its projected counterpart is recorded with the projected
/// position.
const EPOCH_KEY: &str = "kdm_epoch";
const PROJECTED_EPOCH_KEY: &str = "kdm_projected_epoch";

/// The document id [`save_initial`] records, so [`append`] and [`compact`] can refuse a replica of
/// another document (one loaded from the wrong file, or a programming mistake).
const DOC_ID_KEY: &str = "kdm_doc_id";

/// Creates the KDM tables and columns if they are missing, and the indexes that keep `kdm_id`
/// unique per live row. Safe to call on a file that already has them.
pub fn ensure_tables(conn: &Connection) -> Result<()> {
    for create in crate::schema::KDM_TABLES {
        conn.execute_batch(create)?;
    }
    for (table, column) in crate::schema::KDM_COLUMNS {
        if !has_column(conn, table, column)? {
            conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"))?;
        }
    }
    for index in crate::schema::KDM_INDEXES {
        conn.execute_batch(index)?;
    }
    Ok(())
}

fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    Ok(file::column_names(conn, table)?.iter().any(|name| name == column))
}

/// Loads the stored replica: the snapshot, then every change row in `seq` order. `None` when
/// `kition_kdm` has no row (the file predates the KDM, or has not been initialized yet).
///
/// [`TableFileError::Kdm`] when, after importing every row, the replica still has changes pending on
/// others that the log does not hold ([`Replica::has_pending_changes`]): the change log is broken,
/// for instance a row was lost between `append` calls, or one was written against a `since` the
/// log never actually reached. The store's invariant is that importing the whole log in `seq`
/// order always leaves nothing pending; this is never silently swallowed into an incomplete
/// replica.
pub fn load(conn: &Connection, schema: Arc<Schema>, actor: ActorId) -> Result<Option<Replica>> {
    let snapshot: Option<Vec<u8>> = conn.query_row("SELECT snapshot FROM kition_kdm WHERE id = 1", [], |row| row.get(0)).optional()?;
    let Some(snapshot) = snapshot else {
        return Ok(None);
    };
    let mut replica = Replica::load(schema, &snapshot, actor)?;
    let mut statement = conn.prepare("SELECT bytes FROM kition_kdm_changes ORDER BY seq ASC")?;
    let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
    for bytes in rows {
        replica.import_changes(&bytes)?;
    }
    if replica.has_pending_changes() {
        return Err(TableFileError::Kdm("the kdm change log is broken: a stored change depends on changes the log does not hold".into()));
    }
    Ok(Some(replica))
}

/// Writes `replica`'s snapshot as generation 1 and records its document id. Fails if `kition_kdm`
/// already has a row (its primary key is fixed at 1).
pub fn save_initial(conn: &Connection, replica: &Replica, now: i64) -> Result<()> {
    let snapshot = replica.snapshot()?;
    conn.execute(
        "INSERT INTO kition_kdm (id, snapshot, generation, updated_at) VALUES (1, zeroblob(?1), 1, ?2)",
        params![blob_len(&snapshot)?, now],
    )?;
    put_snapshot(conn, &snapshot)?;
    file::set_meta(conn, DOC_ID_KEY, &replica.doc_id().to_string())?;
    new_epoch(conn)
}

/// Writes `snapshot` into the stored snapshot, which holds as many zero bytes, through SQLite's
/// incremental blob I/O: bound as a parameter, it would be copied twice more in memory (the bound
/// value and the record made of it).
fn put_snapshot(conn: &Connection, snapshot: &[u8]) -> Result<()> {
    let mut blob = conn.blob_open(rusqlite::MAIN_DB, "kition_kdm", "snapshot", 1, false)?;
    blob.write_at(snapshot, 0)?;
    Ok(())
}

fn blob_len(bytes: &[u8]) -> Result<i64> {
    i64::try_from(bytes.len()).map_err(|_| TableFileError::invalid("the kdm snapshot is too large"))
}

fn new_epoch(conn: &Connection) -> Result<()> {
    file::set_meta(conn, EPOCH_KEY, &crate::ids::uuid().to_string())
}

/// The store's identity as of its last write: its position and its epoch. Equal heads mean
/// nothing was stored in between, even by another copy of the file written over this one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Head {
    pub position: (i64, i64),
    pub epoch: String,
}

/// The stored replica's [`Head`]. The epoch is empty for a store that predates epochs.
pub fn head(conn: &Connection) -> Result<Head> {
    Ok(Head { position: stored_position(conn)?, epoch: file::get_meta(conn, EPOCH_KEY)?.unwrap_or_default() })
}

/// Whether the projection was last recorded at the store's current [`Head`].
pub fn projection_is_current(conn: &Connection) -> Result<bool> {
    let stored = head(conn)?;
    Ok(projected_position(conn)? == stored.position && file::get_meta(conn, PROJECTED_EPOCH_KEY)?.unwrap_or_default() == stored.epoch)
}

/// Refuses `replica` if a document id was recorded (by [`save_initial`]) and it does not match.
/// Does nothing when none was recorded yet, so this stays usable before `save_initial` runs.
fn check_doc_id(conn: &Connection, replica: &Replica) -> Result<()> {
    let Some(stored) = file::get_meta(conn, DOC_ID_KEY)? else {
        return Ok(());
    };
    let found = replica.doc_id().to_string();
    if stored != found {
        return Err(TableFileError::invalid(format!("kdm replica is document {found}, but this file holds the KDM replica of document {stored}")));
    }
    Ok(())
}

/// Appends one change row holding every change `replica` has beyond `since`. Returns its `seq`.
/// A no-op (returns the last existing `seq`, or 0 when the log has never held a row) when `since`
/// already includes everything `replica` has: not only when the two versions are equal, but also
/// when `since` is strictly ahead of `replica` (another writer already stored more than `replica`
/// knows about) or holds concurrent changes `replica` lacks. There is nothing to export either
/// way: [`Version::includes`] is the right comparison, not equality.
///
/// `since` must be the version the store last accepted: the stored replica's own version right
/// after the last successful [`save_initial`] or `append` (for instance, [`load`]'s version, or an
/// in-memory replica's version taken immediately after its own last successful `append`). Taking
/// `since` from an in-memory replica that kept transacting after one of its `append` calls was
/// rolled back (its SQL transaction failed or was abandoned) would skip the changes made in
/// between: the engine already reflects them, but they never reached storage.
pub fn append(conn: &Connection, replica: &Replica, since: &Version, origin: &str, now: i64) -> Result<i64> {
    check_doc_id(conn, replica)?;
    if since.includes(&replica.version()) {
        return last_seq(conn);
    }
    let bytes = replica.export_changes(since)?;
    conn.execute("INSERT INTO kition_kdm_changes (bytes, origin, created_at) VALUES (?1, ?2, ?3)", params![bytes, origin, now])?;
    let seq = conn.last_insert_rowid();
    record_origin(conn, &replica.version(), origin, now)?;
    new_epoch(conn)?;
    Ok(seq)
}

/// How many rows the origin log ([`record_origin`]) keeps: the newest ones.
pub const ORIGIN_LOG_ROWS: i64 = 1000;

/// The meta key holding the version the newest row the origin log dropped had reached.
pub const ORIGIN_LOG_FLOOR_KEY: &str = "kdm_origins_floor";

const ORIGIN_LOG_TABLE: &str = "CREATE TABLE IF NOT EXISTS kition_kdm_origins (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    version BLOB NOT NULL,
    origin TEXT NOT NULL,
    created_at INTEGER NOT NULL
)";

/// Notes in the origin log (`kition_kdm_origins`) that the stored replica reached `version`
/// with a write of `origin` (an encoded [`kition_kdm::Origin`]). Unlike the change log, the
/// origin log survives [`compact`], so who changed what since a version stays known after the
/// change rows are folded into the snapshot ([`crate::kdm::changes`]). It keeps the newest
/// [`ORIGIN_LOG_ROWS`] rows; the version of the newest row it drops is kept as its floor.
pub fn record_origin(conn: &Connection, version: &Version, origin: &str, now: i64) -> Result<()> {
    conn.execute_batch(ORIGIN_LOG_TABLE)?;
    conn.prepare_cached("INSERT INTO kition_kdm_origins (version, origin, created_at) VALUES (?1, ?2, ?3)")?.execute(params![
        version.encode(),
        origin,
        now
    ])?;
    let newest = conn.last_insert_rowid();
    let cut = newest - ORIGIN_LOG_ROWS;
    if cut > 0 {
        let floor: Option<Vec<u8>> =
            conn.query_row("SELECT version FROM kition_kdm_origins WHERE seq <= ?1 ORDER BY seq DESC LIMIT 1", [cut], |row| row.get(0)).optional()?;
        if let Some(floor) = floor {
            conn.execute("DELETE FROM kition_kdm_origins WHERE seq <= ?1", [cut])?;
            file::set_meta(conn, ORIGIN_LOG_FLOOR_KEY, &hex(&floor))?;
        }
    }
    Ok(())
}

/// The origin log's rows, oldest first: `(version, encoded origin)`. Empty when the file has
/// none (a file no write has touched since the log was added).
pub fn origin_log(conn: &Connection) -> Result<Vec<(Version, String)>> {
    let exists: Option<String> =
        conn.query_row("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'kition_kdm_origins'", [], |row| row.get(0)).optional()?;
    if exists.is_none() {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare("SELECT version, origin FROM kition_kdm_origins ORDER BY seq ASC")?;
    let rows =
        statement.query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)))?.collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter().map(|(bytes, origin)| Ok((Version::decode(&bytes)?, origin))).collect()
}

/// The version of the newest row the origin log dropped, if it dropped any.
pub fn origin_log_floor(conn: &Connection) -> Result<Option<Version>> {
    let Some(text) = file::get_meta(conn, ORIGIN_LOG_FLOOR_KEY)? else {
        return Ok(None);
    };
    let bytes = unhex(&text).ok_or_else(|| TableFileError::invalid(format!("kition_meta.{ORIGIN_LOG_FLOOR_KEY} is not hex")))?;
    Ok(Some(Version::decode(&bytes)?))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok()).collect()
}

/// The highest `seq` ever inserted into `kition_kdm_changes`, including generations [`compact`]
/// has since cleared. SQLite's `AUTOINCREMENT` counter (`sqlite_sequence`) survives the `DELETE`
/// that empties the table on compaction, so seqs keep increasing across generations; reading
/// `MAX(seq)` instead would wrongly read back as 0 right after a compact, and a seq could be
/// reused. 0 when the table has never held a row (the counter is created on its first insert).
fn last_seq(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT seq FROM sqlite_sequence WHERE name = 'kition_kdm_changes'", [], |row| row.get(0)).optional()?.unwrap_or(0))
}

/// Writes `replica`'s snapshot, increments `generation`, and deletes every change row: the
/// snapshot already carries them.
///
/// The change rows and the old snapshot go first, so the new snapshot is written into the pages
/// they leave: SQLite writes a replaced value's new pages before it frees the old ones, and the
/// file would otherwise keep a whole old snapshot of free pages.
pub fn compact(conn: &Connection, replica: &Replica, now: i64) -> Result<()> {
    check_doc_id(conn, replica)?;
    let snapshot = replica.snapshot()?;
    let updated = conn.execute("UPDATE kition_kdm SET snapshot = x'' WHERE id = 1", [])?;
    if updated == 0 {
        return Err(TableFileError::invalid("kdm replica is not initialized; call save_initial first"));
    }
    conn.execute("DELETE FROM kition_kdm_changes", [])?;
    conn.execute(
        "UPDATE kition_kdm SET snapshot = zeroblob(?1), generation = generation + 1, updated_at = ?2 WHERE id = 1",
        params![blob_len(&snapshot)?, now],
    )?;
    put_snapshot(conn, &snapshot)?;
    new_epoch(conn)
}

/// Whether the change log has grown enough that [`compact`] is worth running: more than 500
/// change rows, or more bytes of them than 1 MiB or a quarter of the snapshot, whichever is more.
pub fn should_compact(conn: &Connection) -> Result<bool> {
    let (count, bytes): (i64, i64) =
        conn.query_row("SELECT COUNT(*), COALESCE(SUM(LENGTH(bytes)), 0) FROM kition_kdm_changes", [], |row| Ok((row.get(0)?, row.get(1)?)))?;
    if count > COMPACT_MAX_CHANGES {
        return Ok(true);
    }
    if bytes <= COMPACT_MAX_BYTES {
        return Ok(false);
    }
    let snapshot: i64 = conn.query_row("SELECT COALESCE(MAX(LENGTH(snapshot)), 0) FROM kition_kdm", [], |row| row.get(0))?;
    Ok(bytes > snapshot / COMPACT_SNAPSHOT_SHARE)
}

/// Whether the change log is past the fixed limits (500 rows or 1 MiB), whatever the snapshot's
/// size: an open replays every change row, and replaying is far slower than loading the snapshot.
pub fn log_is_large(conn: &Connection) -> Result<bool> {
    let (count, bytes): (i64, i64) =
        conn.query_row("SELECT COUNT(*), COALESCE(SUM(LENGTH(bytes)), 0) FROM kition_kdm_changes", [], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(count > COMPACT_MAX_CHANGES || bytes > COMPACT_MAX_BYTES)
}

/// The stored replica's position: `(generation, last seq)`. `(0, 0)` when `kition_kdm` has no
/// row; the seq is 0 when the change log has never held a row.
pub fn stored_position(conn: &Connection) -> Result<(i64, i64)> {
    let generation: Option<i64> = conn.query_row("SELECT generation FROM kition_kdm WHERE id = 1", [], |row| row.get(0)).optional()?;
    Ok((generation.unwrap_or(0), last_seq(conn)?))
}

/// The projection's last-applied position, as recorded in `kition_meta`. `(0, 0)` when the
/// projector has not run yet.
pub fn projected_position(conn: &Connection) -> Result<(i64, i64)> {
    Ok((get_meta_i64(conn, PROJECTED_GENERATION_KEY)?.unwrap_or(0), get_meta_i64(conn, PROJECTED_SEQ_KEY)?.unwrap_or(0)))
}

/// Records the projection's last-applied position, with the store's current epoch.
pub fn set_projected_position(conn: &Connection, position: (i64, i64)) -> Result<()> {
    set_meta_i64(conn, PROJECTED_GENERATION_KEY, position.0)?;
    set_meta_i64(conn, PROJECTED_SEQ_KEY, position.1)?;
    let epoch = file::get_meta(conn, EPOCH_KEY)?.unwrap_or_default();
    file::set_meta(conn, PROJECTED_EPOCH_KEY, &epoch)
}

/// `None` when `key` is unset; [`TableFileError::Invalid`] when it is set but is not an integer
/// (corrupt `kition_meta`), rather than silently reading as missing.
fn get_meta_i64(conn: &Connection, key: &str) -> Result<Option<i64>> {
    let Some(value) = file::get_meta(conn, key)? else {
        return Ok(None);
    };
    value.parse().map(Some).map_err(|_| TableFileError::invalid(format!("kition_meta.{key} is not an integer: {value:?}")))
}

fn set_meta_i64(conn: &Connection, key: &str, value: i64) -> Result<()> {
    file::set_meta(conn, key, &value.to_string())
}
