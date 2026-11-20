//! `kition-table`: the Table kit served headless over the Kit Protocol by `kition-headless`, as the
//! kit's `kit.json` declares it: the same document the in-page editor runs (`kition-table-kit`,
//! Kition's table engine), with the object model (`model.describe`, `model.batch`) over the
//! document's own replica, and the identity a file carries (`document.identity`, `document.fork`:
//! a `.ktable` embeds its replica, [`TableFiles`]). Kition's agent edits tables through Kition's
//! runtime instead; this executable is for other hosts, scripts, and the conformance suite.

use std::sync::Arc;

use kition_headless::{EmbeddedReplica, FileIdentity, Headless, KitInfo};
use kition_kit_server::{ErrorReason, RpcError};

/// Commands whose params name no file of the host's: `import.run`'s `target` is a table, and its
/// bytes come in the params.
const SANDBOXED: &[&str] = &["table.import.preview", "table.import.run"];

/// Where a `.ktable` file keeps the table document's replica (`document.identity` and
/// `document.fork`, Kit Process Protocol section 5.6): `kition_kdm`'s snapshot and the change rows
/// after it. A fork's file is the new document's file with the source's blobs its records name, and
/// none of the source's history (`kition_table_file::identity::replace_replica`).
struct TableFiles;

fn unsupported(error: impl std::fmt::Display) -> RpcError {
    RpcError::kit_detail(ErrorReason::UnsupportedFormat, error.to_string())
}

impl EmbeddedReplica for TableFiles {
    fn schema(&self) -> Arc<kition_kdm::Schema> {
        kition_table_file::kdm::schema()
    }

    fn replica(&self, file: &[u8]) -> Result<Option<Vec<u8>>, RpcError> {
        kition_table_file::identity::embedded_replica(file).map_err(unsupported)
    }

    fn replace(&self, file: &[u8], snapshot: &[u8]) -> Result<Vec<u8>, RpcError> {
        kition_table_file::identity::replace_replica(file, snapshot).map_err(unsupported)
    }
}

fn main() -> std::process::ExitCode {
    // The picture's faces (renders and `table.file.render`), embedded at build time from a
    // kition-fonts checkout (feature `embed`), so previews have text wherever the executable runs.
    // Never the machine's own fonts.
    kition_table_fonts::register_embedded();
    let kit = match KitInfo::from_manifest(include_str!("../../../kit.json"), SANDBOXED, kition_table_kit::EDITOR_STATE) {
        Ok(kit) => kit,
        Err(error) => {
            eprintln!("kit: {error}");
            return std::process::ExitCode::from(2);
        }
    };
    let backend =
        Headless::<kition_table_kit::TableDocument>::with_replica_port(kit.clone()).with_file_identity(FileIdentity::Embedded(Arc::new(TableFiles)));
    kition_headless::serve_backend(&kit, backend)
}
