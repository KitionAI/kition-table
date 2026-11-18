//! A face registered after the grid shaped text (the editor's lazy fonts, Kition platform E9): the
//! shaper's remembered lines were made with fewer faces, so it shapes again, and a line it drew as
//! tofu draws in its face. Its own process: the faces are the process's.

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use kition_table_display::Face;
use kition_table_fonts::{CHECKOUT_FILES, add_font};
use kition_table_grid::text::{Kind, Shaper};

fn checkout() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("KITION_FONTS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../kition-fonts"));
    dir.join(CHECKOUT_FILES[0]).exists().then_some(dir)
}

fn kinds(shaper: &mut Shaper, text: &str, max: f32) -> Vec<Kind> {
    shaper.line(text, 400, 13.0, max).pieces.iter().map(|piece| piece.kind).collect()
}

#[test]
fn the_shaper_forgets_lines_shaped_before_a_face_came() {
    let Some(dir) = checkout() else {
        eprintln!("skipped: no kition-fonts checkout (set KITION_FONTS_DIR)");
        return;
    };
    let add = |file: &str| add_font(std::fs::read(dir.join(file)).expect("a font of the checkout")).expect("a font");
    add(CHECKOUT_FILES[0]);
    let mut shaper = Shaper::default();
    let text = "Plan 世界";
    // Shaped twice: the second answer is the remembered line, and so is a cut one.
    for _ in 0..2 {
        assert_eq!(kinds(&mut shaper, text, f32::INFINITY), [Kind::Text(Face::Sans), Kind::Tofu]);
        assert!(kinds(&mut shaper, "Plan 世界世界世界世界", 60.0).contains(&Kind::Tofu));
    }
    add(CHECKOUT_FILES[1]);
    assert_eq!(kinds(&mut shaper, text, f32::INFINITY), [Kind::Text(Face::Sans), Kind::Text(Face::Fallback)]);
    assert!(!kinds(&mut shaper, "Plan 世界世界世界世界", 60.0).contains(&Kind::Tofu));
}
