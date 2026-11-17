//! With the `embed` feature (the headless executable's), embeds the table's faces (`FILES`, the
//! `CHECKOUT_FILES` of `src/lib.rs`) from a kition-fonts checkout into the build:
//! `KITION_FONTS_DIR`, else the `kition-fonts` checkout beside this repository. Without one the
//! build warns and embeds nothing (the picture then has no text); `KITION_FONTS_REQUIRED` makes it
//! an error. It only reads the local checkout: no network. The web build (no `embed`) embeds no
//! font: the editor hands the engine its faces.
use std::fmt::Write as _;
use std::path::PathBuf;

/// The same list as `CHECKOUT_FILES` in `src/lib.rs` (a test keeps them equal).
const FILES: &[&str] =
    &["fonts/inter/Inter-Variable.ttf", "fonts/droid-sans-fallback/DroidSansFallbackFull.ttf", "fonts/noto-color-emoji/Noto-COLRv1.ttf"];

fn main() {
    println!("cargo::rerun-if-env-changed=KITION_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=KITION_FONTS_REQUIRED");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("embedded_fonts.rs");
    let mut src = String::from("pub(crate) static EMBEDDED: &[&[u8]] = &[\n");
    if std::env::var_os("CARGO_FEATURE_EMBED").is_some() {
        let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
        let dir = std::env::var_os("KITION_FONTS_DIR").map_or_else(|| manifest.join("../../../../kition-fonts"), PathBuf::from);
        let found: Vec<PathBuf> = FILES.iter().map(|file| dir.join(file)).filter(|path| path.is_file()).collect();
        if found.len() == FILES.len() {
            for path in &found {
                println!("cargo::rerun-if-changed={}", path.display());
                let _ = writeln!(src, "    include_bytes!({:?}),", path.display().to_string());
            }
        } else if std::env::var_os("KITION_FONTS_REQUIRED").is_some() {
            println!("cargo::error=no kition-fonts checkout with the table's faces at {} (set KITION_FONTS_DIR)", dir.display());
        } else {
            println!(
                "cargo::warning=building without the table's faces: no kition-fonts checkout at {} (set KITION_FONTS_DIR); the picture has no text",
                dir.display()
            );
        }
    }
    src.push_str("];\n");
    if std::fs::read_to_string(&out).ok().as_deref() != Some(&src)
        && let Err(e) = std::fs::write(&out, src)
    {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}
