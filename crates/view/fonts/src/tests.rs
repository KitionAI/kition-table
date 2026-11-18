use super::*;

/// The `kition-fonts` checkout: `KITION_FONTS_DIR`, else the sibling of this repository.
pub fn checkout() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("KITION_FONTS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../kition-fonts"));
    dir.join(CHECKOUT_FILES[0]).exists().then_some(dir)
}

fn registered() -> bool {
    match checkout() {
        Some(dir) => register_dir(&dir) == CHECKOUT_FILES.len(),
        None => {
            eprintln!("skipped: no kition-fonts checkout (set KITION_FONTS_DIR)");
            false
        }
    }
}

#[test]
fn faces_take_their_roles_and_a_mixed_line_shapes_in_runs() {
    if !registered() {
        return;
    }
    let runs = shape("Hello 世界 again", Family::Sans, REGULAR);
    assert_eq!(runs.len(), 3, "Latin, CJK, Latin: {runs:?}");
    let clusters: Vec<usize> = runs.iter().flat_map(|run| run.glyphs.iter().map(|glyph| glyph.cluster)).collect();
    assert!(clusters.windows(2).all(|pair| pair.first() <= pair.get(1)), "clusters in text order");
    assert!(runs.iter().all(|run| run.glyphs.iter().all(|glyph| glyph.id != 0)), "no missing glyphs");
}

#[test]
fn bold_is_heavier_and_outlines_are_cached() {
    if !registered() {
        return;
    }
    let regular = shape("m", Family::Sans, REGULAR);
    let bold = shape("m", Family::Sans, BOLD);
    let (Some(r), Some(b)) = (regular.first().and_then(|run| run.glyphs.first()), bold.first().and_then(|run| run.glyphs.first())) else {
        panic!("no glyphs");
    };
    assert!(b.advance > r.advance, "bold m is wider ({} vs {})", b.advance, r.advance);
    let face = regular.first().map(|run| run.face).unwrap_or(FaceId(0));
    let first = outline(face, BOLD, b.id);
    assert!(!first.elements().is_empty());
    assert!(Arc::ptr_eq(&first, &outline(face, BOLD, b.id)), "cached");
    assert_ne!(outline(face, REGULAR, r.id).elements(), first.elements(), "weights draw differently");
}

#[test]
fn bytes_that_are_not_a_font_are_refused() {
    assert!(add_font(b"not a font".to_vec()).is_err());
}

#[test]
fn emoji_shape_in_the_color_face_and_sequences_are_one_glyph() {
    if !registered() {
        return;
    }
    let runs = shape("a\u{1F600}b", Family::Sans, REGULAR);
    assert_eq!(runs.len(), 3, "text, emoji, text: {runs:?}");
    let spaced = shape("a \u{2728} b", Family::Sans, REGULAR);
    assert_eq!(spaced.len(), 3, "the space after an emoji is the text's: {spaced:?}");
    assert_eq!(spaced[0].face, spaced[2].face);
    let emoji = runs.get(1).unwrap();
    assert!(color_steps(emoji.face, emoji.glyphs[0].id).is_some_and(|steps| !steps.is_empty()), "the emoji has color layers");
    assert!(color_steps(runs[0].face, runs[0].glyphs[0].id).is_none(), "a letter has none");
    // Digits and # are text, unless an emoji variation selector or a keycap mark follows.
    assert_eq!(shape("# 1 2", Family::Sans, REGULAR).len(), 1);
    let keycap = shape("1\u{FE0F}\u{20E3}", Family::Sans, REGULAR);
    assert_eq!((keycap.len(), keycap[0].glyphs.iter().filter(|glyph| glyph.advance > 0.0).count()), (1, 1), "a keycap: {keycap:?}");
    let family = shape("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", Family::Sans, REGULAR);
    assert_eq!(family.len(), 1);
    assert_eq!(family[0].glyphs.iter().filter(|glyph| glyph.advance > 0.0).count(), 1, "a ZWJ family is one glyph: {family:?}");
    // A text variation selector keeps a symbol as text.
    let text = shape("\u{2764}\u{FE0E}", Family::Sans, REGULAR);
    assert!(text.iter().all(|run| color_steps(run.face, run.glyphs[0].id).is_none()));
}

#[test]
fn the_build_embeds_the_checkout_files() {
    // build.rs keeps its own list (a build script cannot read the crate): the same files.
    let build = include_str!("../build.rs");
    for file in CHECKOUT_FILES {
        assert!(build.contains(&format!("{file:?}")), "build.rs embeds {file}");
    }
}
