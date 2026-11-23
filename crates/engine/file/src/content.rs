//! A `.ktable` as a person's file: the document's content, not its history (kition-sdk data model
//! 7.3, rule M16b).
//!
//! The handle a session edits keeps everything a session needs: the replica with its history and
//! undo, the change log, the origin log and the record history the `records.history` command
//! reads. The file it saves holds none of that ([`TableFile::content_bytes`]): its store is the
//! replica's sealed content snapshot (the state, and of the history only the seal: no deleted or
//! overwritten content, no person's or agent's origin), with no change rows and no origin log; the
//! relational projection keeps only live rows (no record history, no deleted tables, fields,
//! views or records); only the blobs the records name
//! stay; and the database is rebuilt (`VACUUM`), so no free page holds anything that was
//! deleted. The history stays with the host's store (the replica's full snapshot), never in the
//! file.
//!
//! A file opens like any `.ktable` ([`TableFile::open_bytes`]): its replica is a shallow one,
//! rooted at the seal, with the document's id and its objects' ids, that edits and merges like
//! any replica.

use std::collections::BTreeSet;

use kition_kdm::{ActorId, Version};
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::blobs::blob_hash_candidates;
use crate::kdm::store;
use crate::{Result, TableFile, file};

impl TableFile {
    /// Whether the replica is sealed: its last change is the seal, so the file it saves is its own
    /// content snapshot ([`TableFile::seal`]).
    pub fn is_sealed(&self) -> bool {
        self.replica.is_sealed()
    }

    /// The file's bytes as a person's file holds them (see the module documentation), from the
    /// sealed replica: [`TableFile::seal`] first, as a host's `prepareSave` does. A replica nobody
    /// sealed is [`crate::TableFileError::Kdm`]. A document that did not change since saves the same
    /// bytes.
    pub fn content_bytes(&self) -> Result<Vec<u8>> {
        let content = self.replica.sealed_content_snapshot()?;
        self.file_with_content(&content)
    }

    /// [`TableFile::content_bytes`] of a replica nobody sealed, for a host that does not call
    /// `prepareSave` yet: the content of a copy sealed under an actor derived from the version
    /// (so the same unsealed version saves the same bytes), and that seal's changes, which the
    /// session takes in once the host stored the bytes ([`TableFile::import_replica_changes`]).
    pub fn content_bytes_of_a_sealed_copy(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let version = self.replica.version();
        let mut copy = self.replica.fork(sealing_actor(&version))?;
        copy.seal()?;
        let seal = copy.export_changes(&version)?;
        let content = copy.sealed_content_snapshot()?;
        Ok((self.file_with_content(&content)?, seal))
    }

    /// The stored snapshot and the number of change rows after it: a saved file holds its content
    /// snapshot and no change row.
    pub fn stored_replica(&self) -> Result<(Vec<u8>, i64)> {
        store::stored(&self.conn)
    }

    /// A copy of this file with `content` (a sealed content snapshot of its replica) as its store
    /// and nothing of its history.
    fn file_with_content(&self, content: &[u8]) -> Result<Vec<u8>> {
        let conn = file::memory_copy(&self.to_bytes()?)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        strip_history(&conn)?;
        store::write_content(&conn, content)?;
        drop_unnamed_blobs(&conn)?;
        conn.execute_batch("VACUUM")?;
        let mut bytes = file::database_bytes(&conn)?;
        settle_header(&mut bytes);
        Ok(bytes)
    }
}

/// The actor a copy of a replica at `version` seals under: the same for the same version.
fn sealing_actor(version: &Version) -> ActorId {
    let digest = Sha256::new().chain_update(b"kition-table-seal").chain_update(version.encode()).finalize();
    let mut high = [0u8; 8];
    high.copy_from_slice(digest.get(..8).unwrap_or(&[1; 8]));
    // The top bit is cleared, as `ActorId::random` does: the engine reserves `u64::MAX`.
    ActorId::from_u64(u64::from_le_bytes(high) >> 1).unwrap_or_else(|_| ActorId::random())
}

/// Deletes from the relational tables what is history: the record history and deleted rows (and
/// what cascades from them). The document's creation time stays: it is set once, when the
/// document is made.
fn strip_history(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DELETE FROM kition_record_history;
         DELETE FROM sqlite_sequence WHERE name = 'kition_record_history';
         DELETE FROM kition_record_links WHERE from_record_id IN (SELECT id FROM kition_records WHERE deleted_at IS NOT NULL)
           OR to_record_id IN (SELECT id FROM kition_records WHERE deleted_at IS NOT NULL);
         DELETE FROM kition_records WHERE deleted_at IS NOT NULL;
         DELETE FROM kition_views WHERE deleted_at IS NOT NULL;
         DELETE FROM kition_fields WHERE deleted_at IS NOT NULL;
         DELETE FROM kition_tables WHERE deleted_at IS NOT NULL;
         DELETE FROM kition_documents WHERE deleted_at IS NOT NULL;
         DELETE FROM kition_attachments WHERE deleted_at IS NOT NULL;",
    )?;
    Ok(())
}

/// Deletes the blobs no record names.
fn drop_unnamed_blobs(conn: &Connection) -> Result<()> {
    let has: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'kition_blobs'", [], |row| row.get(0))?;
    if has == 0 {
        return Ok(());
    }
    let mut named = BTreeSet::new();
    {
        let mut statement = conn.prepare("SELECT values_json FROM kition_records")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            named.extend(blob_hash_candidates(row.get_ref(0)?.as_bytes().unwrap_or_default()));
        }
    }
    let hashes: Vec<String> = {
        let mut statement = conn.prepare("SELECT hash FROM kition_blobs")?;
        statement.query_map([], |row| row.get::<_, String>(0))?.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for hash in hashes.into_iter().filter(|hash| !named.contains(hash)) {
        conn.execute("DELETE FROM kition_blobs WHERE hash = ?1", [hash])?;
    }
    Ok(())
}

/// The database header's counters (file change counter, schema cookie, version-valid-for) set to
/// fixed values: they count the writes the database had, which a saved file leaves out.
fn settle_header(bytes: &mut [u8]) {
    for at in [24usize, 40, 92] {
        if let Some(field) = bytes.get_mut(at..at + 4) {
            field.copy_from_slice(&1u32.to_be_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_sealing_actor_is_the_same_for_the_same_version() {
        let version = kition_kdm::Version::empty();
        assert_eq!(super::sealing_actor(&version), super::sealing_actor(&version));
    }
}
