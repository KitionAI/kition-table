//! Faces given late (the editor's lazy fonts, Kition platform E9 and E9a): with only Inter, a
//! Chinese, Korean or emoji character asks for a face (`take_wanted`) and shapes as tofu; once its
//! face is registered the generation moves and it shapes in that face. Its own process: the faces
//! are the process's.

#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use kition_table_fonts::{CHECKOUT_FILES, Family, OpticalSizing, REGULAR, add_font, generation, role, shape, take_wanted};

fn checkout() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("KITION_FONTS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../kition-fonts"));
    dir.join(CHECKOUT_FILES[0]).exists().then_some(dir)
}

fn add(dir: &std::path::Path, file: &str) {
    add_font(std::fs::read(dir.join(file)).expect("a font of the checkout")).expect("a font");
}

fn tofu(text: &str) -> bool {
    shape(text, Family::Sans, REGULAR, 13.0, OpticalSizing::Auto).iter().any(|run| run.tofu)
}

#[test]
fn text_wants_the_faces_it_lacks_and_shapes_in_each_once_it_is_registered() {
    let Some(dir) = checkout() else {
        eprintln!("skipped: no kition-fonts checkout (set KITION_FONTS_DIR)");
        return;
    };
    let [inter, droid, nanum, emoji] = CHECKOUT_FILES else { panic!("four faces") };
    add(&dir, inter);
    let first = generation();
    assert!(!take_wanted(), "nothing laid out yet");
    assert!(!tofu("Hello again"));
    assert!(!take_wanted(), "Latin needs only Inter");

    assert!(tofu("Hello 世界"));
    assert!(take_wanted(), "Chinese without its face");
    assert!(!take_wanted(), "the call clears it");
    add(&dir, droid);
    assert!(generation() > first);
    assert!(!tofu("Hello 世界"));
    let face = shape("世界", Family::Sans, REGULAR, 13.0, OpticalSizing::Auto)[0].face;
    assert_eq!(role(face), Some(kition_table_fonts::Role::Fallback));

    assert!(tofu("안녕하세요"), "Droid Sans Fallback has no Hangul");
    assert!(take_wanted(), "Korean without its face");
    add(&dir, nanum);
    assert_eq!(role(shape("안녕하세요", Family::Sans, REGULAR, 13.0, OpticalSizing::Auto)[0].face), Some(kition_table_fonts::Role::Hangul));

    let _ = shape("Launch \u{1F680}", Family::Sans, REGULAR, 13.0, OpticalSizing::Auto);
    assert!(take_wanted(), "an emoji without the emoji face");
    let before = generation();
    add(&dir, emoji);
    assert!(generation() > before);
    let runs = shape("Launch \u{1F680}", Family::Sans, REGULAR, 13.0, OpticalSizing::Auto);
    assert_eq!(runs.last().and_then(|run| role(run.face)), Some(kition_table_fonts::Role::Emoji));
    assert!(!take_wanted(), "every character has its face now");

    // The same face again changes nothing.
    let now = generation();
    add(&dir, droid);
    assert_eq!(generation(), now);
}
