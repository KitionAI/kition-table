//! `kition-table`: the Table kit served headless over the Kit Protocol by `kition-headless`, as the
//! kit's `kit.json` declares it: the same document the in-page editor runs (`kition-table-kit`,
//! Kition's table engine), with the object model (`model.describe`, `model.batch`) over the
//! document's own replica. Kition's agent edits tables through Kition's runtime instead; this
//! executable is for other hosts, scripts, and the conformance suite.

/// Commands whose params name no file of the host's: `import.run`'s `target` is a table, and its
/// bytes come in the params.
const SANDBOXED: &[&str] = &["table.import.preview", "table.import.run"];

fn main() -> std::process::ExitCode {
    // The picture's faces (renders and `table.file.render`), embedded at build time from a
    // kition-fonts checkout (feature `embed`), so previews have text wherever the executable runs.
    // Never the machine's own fonts.
    kition_table_fonts::register_embedded();
    kition_headless::run_manifest_model::<kition_table_kit::TableDocument>(
        include_str!("../../../kit.json"),
        SANDBOXED,
        kition_table_kit::EDITOR_STATE,
    )
}
