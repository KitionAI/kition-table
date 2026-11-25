//! A `.ktable` as a person's file: the document's content, not its history (kition-sdk data model
//! 7.3 and 7.4).
//!
//! The handle a session edits keeps everything a session needs: the replica with its change log
//! and undo, the change rows, the origin log and the record history the `records.history` command
//! reads. The file it saves holds none of that ([`TableFile::content_bytes`]): its store is the
//! replica's content (`Replica::content_snapshot`, `content.json`: the state and nothing else, no
//! version, no origin, no deleted or overwritten value) DEFLATE-compressed at a fixed level in the
//! `kition_kdm` row, with no change rows and no origin log; the relational projection keeps only
//! live rows (no record history, no deleted tables, fields, views or records); only the blobs the
//! records name stay; and the database is rebuilt (`VACUUM`) with its header's counters fixed, so
//! no free page holds anything that was deleted and a document saved again unchanged is the same
//! bytes. The history stays with the host's store (the replica's snapshot and its checkpoints),
//! never in the file.
//!
//! A file opens like any `.ktable` ([`TableFile::open_bytes`]): its replica holds the document's
//! id and its objects' ids, on a new line of changes.

use std::collections::BTreeSet;

use rusqlite::Connection;

use crate::blobs::blob_hash_candidates;
use crate::kdm::store;
use crate::{Result, TableFile, file};

impl TableFile {
    /// The file's bytes as a person's file holds them (see the module documentation). A document
    /// that did not change since saves the same bytes.
    pub fn content_bytes(&self) -> Result<Vec<u8>> {
        let content = self.replica.content_snapshot()?;
        self.file_with_content(&content)
    }

    /// The stored snapshot and the number of change rows after it: a saved file holds its content
    /// (`content.json`, decompressed here) and no change row.
    pub fn stored_replica(&self) -> Result<(Vec<u8>, i64)> {
        store::stored(&self.conn)
    }

    /// A copy of this file with `content` (its replica's content) as its store
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
