//! The document a `.ktable` file carries (`document.identity` and `document.fork`, Kit Process
//! Protocol section 5.6, KitModel M7a).
//!
//! A `.ktable` embeds its document's KDM replica (`kition_kdm`'s snapshot and the change rows of
//! `kition_kdm_changes` after it, [`crate::kdm::store`]), so a byte copy of a file is the same
//! document. [`embedded_replica`] reads that replica without opening the file for editing;
//! [`replace_replica`] writes the file again around another replica (the SDK's copy of it under a
//! new docId), keeping none of the source's history.

use rusqlite::{Connection, OptionalExtension, params};

use crate::blobs::{self, blob_hash_candidates};
use crate::kdm::{schema, store};
use crate::{Result, TableFile, TableFileError, file};

/// The replica `bytes` (a `.ktable` file) embeds, as one KDM snapshot: the stored snapshot with
/// the change rows after it imported, or `None` for an empty file (one made for the kit before it
/// saved a table there). Refuses bytes that are no `.ktable` file this build reads (another
/// schema version), a broken change log, and a replica of another document than the one the file
/// records. Never writes.
pub fn embedded_replica(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let conn = read_only(bytes)?;
    file::check_version(&conn)?;
    let replica =
        store::load(&conn, schema(), crate::ids::actor())?.ok_or_else(|| TableFileError::Kdm("the ktable file holds no kdm replica".into()))?;
    if let Some(recorded) = file::get_meta(&conn, store::DOC_ID_KEY)?
        && recorded != replica.doc_id().to_string()
    {
        return Err(TableFileError::invalid(format!("the file records document {recorded}, its replica is document {}", replica.doc_id())));
    }
    let changes: i64 = conn.query_row("SELECT COUNT(*) FROM kition_kdm_changes", [], |row| row.get(0))?;
    if changes == 0 {
        return Ok(Some(conn.query_row("SELECT snapshot FROM kition_kdm WHERE id = 1", [], |row| row.get(0))?));
    }
    Ok(Some(replica.snapshot()?))
}

/// `bytes` (a `.ktable` file) holding the document `snapshot` instead of its own (`document.fork`):
/// the file a new document made of `snapshot` is ([`TableFile::create_from_replica_in_memory`]:
/// the snapshot as the stored replica with no change rows, the relational tables projected from
/// it, `document_id` and the recorded docId the snapshot's), with the blobs of `bytes` its records
/// name. Nothing else of the source is carried over: no change rows, origin log, record history or
/// change log, no deleted rows of the projection, no blob no record names, and no free page
/// holding any of them (the database is vacuumed), so no earlier identity or history stays in the
/// file. Refuses bytes that are no `.ktable` file and a snapshot that is no table document.
pub fn replace_replica(bytes: &[u8], snapshot: &[u8]) -> Result<Vec<u8>> {
    let source = read_only(bytes)?;
    file::check_version(&source)?;
    let fork = TableFile::create_from_replica_in_memory(snapshot)?;
    if has_table(&source, "kition_blobs")? {
        let named = {
            let mut statement = fork.conn.prepare("SELECT values_json FROM kition_records WHERE deleted_at IS NULL")?;
            let mut rows = statement.query([])?;
            let mut named = std::collections::BTreeSet::new();
            while let Some(row) = rows.next()? {
                named.extend(blob_hash_candidates(row.get_ref(0)?.as_bytes().unwrap_or_default()));
            }
            named
        };
        let mut read = source.prepare("SELECT hash, mime, name, size, bytes, created_at FROM kition_blobs WHERE hash = ?1")?;
        for hash in &named {
            let Some((mime, name, size, content, created)) = read
                .query_row([hash], |row| {
                    Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, Vec<u8>>(4)?, row.get::<_, i64>(5)?))
                })
                .optional()?
            else {
                continue;
            };
            fork.conn.execute_batch(blobs::CREATE_TABLE)?;
            fork.conn.execute(
                "INSERT INTO kition_blobs (hash, mime, name, size, bytes, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![hash, mime, name, size, content, created],
            )?;
        }
    }
    fork.conn.execute_batch("VACUUM")?;
    fork.to_bytes()
}

/// `bytes` as a read-only database in memory.
fn read_only(bytes: &[u8]) -> Result<Connection> {
    let mut conn = Connection::open_in_memory()?;
    conn.deserialize_read_exact(rusqlite::MAIN_DB, bytes, bytes.len(), true)?;
    Ok(conn)
}

fn has_table(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(())).optional()?.is_some())
}
