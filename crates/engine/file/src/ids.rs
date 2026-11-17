//! New ids: the document's, its objects' (tables, fields, views, records, links, dashboards), the
//! replica's actor, and the editor's filter nodes. Random, except in a reproducible build
//! (`SOURCE_DATE_EPOCH` set, [`kition_time::source_date_epoch`]): there they come from a counter
//! in the process, hashed with the epoch, so the same commands save the same bytes
//!.

use std::sync::atomic::{AtomicU64, Ordering};

use kition_kdm::{ActorId, DocId, ObjectId};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// The ids handed out so far in a reproducible build.
static NEXT: AtomicU64 = AtomicU64::new(0);

/// The next counted id's 16 bytes, when the build is reproducible.
fn counted() -> Option<[u8; 16]> {
    let epoch = kition_time::source_date_epoch()?;
    let index = NEXT.fetch_add(1, Ordering::Relaxed);
    let digest = Sha256::new().chain_update(b"kition-table-id").chain_update(epoch.to_le_bytes()).chain_update(index.to_le_bytes()).finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(digest.get(..16)?);
    Some(bytes)
}

/// A new UUID (version 4 in form, counted in a reproducible build).
pub fn uuid() -> Uuid {
    counted().map_or_else(Uuid::new_v4, |bytes| uuid::Builder::from_random_bytes(bytes).into_uuid())
}

/// A new object id for the replica: a UUID v7, as the KDM makes them, so ids sort in the order
/// they were made (links are read in that order). In a reproducible build its time is the epoch
/// and the counter follows it.
pub fn object() -> ObjectId {
    let id = match kition_time::source_date_epoch() {
        Some(epoch) => {
            let index = NEXT.fetch_add(1, Ordering::Relaxed);
            let mut counter = [0u8; 10];
            if let Some(tail) = counter.get_mut(2..) {
                tail.copy_from_slice(&index.to_be_bytes());
            }
            uuid::Builder::from_unix_timestamp_millis(u64::try_from(epoch.saturating_mul(1000)).unwrap_or_default(), &counter).into_uuid()
        }
        None => Uuid::now_v7(),
    };
    ObjectId::from_string(id.to_string())
}

/// A new record id (a UUID v4, as records always had).
pub fn record() -> ObjectId {
    ObjectId::from_string(uuid().to_string())
}

/// A new document id.
pub fn document() -> DocId {
    DocId::parse(&uuid().to_string()).unwrap_or_default()
}

/// A new actor for a writable replica session. The top bit is cleared, as `ActorId::random`
/// does: the engine reserves `u64::MAX`.
pub fn actor() -> ActorId {
    match counted() {
        Some(bytes) => {
            let mut high = [0u8; 8];
            high.copy_from_slice(bytes.get(..8).unwrap_or(&[0; 8]));
            ActorId::from_u64(u64::from_le_bytes(high) >> 1).unwrap_or_else(|_| ActorId::random())
        }
        None => ActorId::random(),
    }
}
