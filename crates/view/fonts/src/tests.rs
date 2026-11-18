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
    let runs = shape("Hello 世界 again", Family::Sans, REGULAR, 13.0);
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
    let regular = shape("m", Family::Sans, REGULAR, 13.0);
    let bold = shape("m", Family::Sans, BOLD, 13.0);
    let (Some(r), Some(b)) = (regular.first().and_then(|run| run.glyphs.first()), bold.first().and_then(|run| run.glyphs.first())) else {
        panic!("no glyphs");
    };
    assert!(b.advance > r.advance, "bold m is wider ({} vs {})", b.advance, r.advance);
    let face = regular.first().map(|run| run.face).unwrap_or(FaceId(0));
    let first = outline(face, BOLD, 13.0, b.id);
    assert!(!first.elements().is_empty());
    assert!(Arc::ptr_eq(&first, &outline(face, BOLD, 13.0, b.id)), "cached");
    assert_ne!(outline(face, REGULAR, 13.0, r.id).elements(), first.elements(), "weights draw differently");
}

fn width(runs: &[Run], size: f32) -> f32 {
    runs.iter().map(|run| run.glyphs.iter().map(|glyph| glyph.advance).sum::<f32>() * size / run.upem).sum()
}

#[test]
fn shaping_keeps_the_browsers_default_features() {
    if !registered() {
        return;
    }
    // Inter's contextual alternates turn "->" into an arrow: the pair shapes differently from its
    // characters shaped one by one, as a Canvas draws it.
    let pair: Vec<u32> = shape("a->b", Family::Sans, REGULAR, 13.0).iter().flat_map(|run| run.glyphs.iter().map(|glyph| glyph.id)).collect();
    let apart: Vec<u32> = ["a", "-", ">", "b"]
        .iter()
        .flat_map(|part| shape(part, Family::Sans, REGULAR, 13.0).into_iter().flat_map(|run| run.glyphs.into_iter().map(|glyph| glyph.id)))
        .collect();
    assert_ne!(pair, apart, "calt is on");
}

#[test]
fn inter_takes_its_optical_size_from_the_font_size() {
    if !registered() {
        return;
    }
    let text = "Quarterly revenue";
    // Per em, a 20 px line is narrower than a 14 px one (optical sizes tighten); 12 and 14 px
    // both sit at the axis's floor.
    let per_em = |size: f32| width(&shape(text, Family::Sans, REGULAR, size), size) / size;
    assert!((per_em(12.0) - per_em(14.0)).abs() < 1e-4, "{} {}", per_em(12.0), per_em(14.0));
    assert!(per_em(20.0) < per_em(14.0) - 0.05, "{} {}", per_em(20.0), per_em(14.0));
    let face = shape("a", Family::Sans, REGULAR, 14.0)[0].face;
    let id = shape("a", Family::Sans, REGULAR, 14.0)[0].glyphs[0].id;
    assert_ne!(outline(face, REGULAR, 14.0, id).elements(), outline(face, REGULAR, 28.0, id).elements(), "outlines follow opsz");
}

#[test]
fn korean_shapes_in_nanum_and_what_no_face_covers_is_tofu() {
    if !registered() {
        return;
    }
    let runs = shape("Deal 한국어", Family::Sans, REGULAR, 13.0);
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(role(runs[1].face), Some(Role::Hangul));
    assert!(runs.iter().all(|run| !run.tofu && run.glyphs.iter().all(|glyph| glyph.id != 0)));
    // Sinhala is in none of the faces: a tofu run of one box per character, between text runs.
    let runs = shape("a \u{0D9A}\u{0D9A} b", Family::Sans, REGULAR, 13.0);
    let kinds: Vec<bool> = runs.iter().map(|run| run.tofu).collect();
    assert_eq!(kinds, vec![false, true, false], "{runs:?}");
    assert_eq!(runs[1].glyphs.len(), 2);
    assert!(runs[1].glyphs.iter().all(|glyph| glyph.advance == TOFU_ADVANCE * 1000.0 && glyph.id == 0));
    // A zero-width joiner or a variation selector after text is no box.
    assert!(shape("a\u{200D}b\u{FE0E}", Family::Sans, REGULAR, 13.0).iter().all(|run| !run.tofu));
    assert_eq!(face(Role::Sans), Some(runs[0].face));
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
    let runs = shape("a\u{1F600}b", Family::Sans, REGULAR, 13.0);
    assert_eq!(runs.len(), 3, "text, emoji, text: {runs:?}");
    let spaced = shape("a \u{2728} b", Family::Sans, REGULAR, 13.0);
    assert_eq!(spaced.len(), 3, "the space after an emoji is the text's: {spaced:?}");
    assert_eq!(spaced[0].face, spaced[2].face);
    let emoji = runs.get(1).unwrap();
    assert!(color_steps(emoji.face, emoji.glyphs[0].id).is_some_and(|steps| !steps.is_empty()), "the emoji has color layers");
    assert!(color_steps(runs[0].face, runs[0].glyphs[0].id).is_none(), "a letter has none");
    // Digits and # are text, unless an emoji variation selector or a keycap mark follows.
    assert_eq!(shape("# 1 2", Family::Sans, REGULAR, 13.0).len(), 1);
    let keycap = shape("1\u{FE0F}\u{20E3}", Family::Sans, REGULAR, 13.0);
    assert_eq!((keycap.len(), keycap[0].glyphs.iter().filter(|glyph| glyph.advance > 0.0).count()), (1, 1), "a keycap: {keycap:?}");
    let family = shape("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", Family::Sans, REGULAR, 13.0);
    assert_eq!(family.len(), 1);
    assert_eq!(family[0].glyphs.iter().filter(|glyph| glyph.advance > 0.0).count(), 1, "a ZWJ family is one glyph: {family:?}");
    // A text variation selector keeps a symbol as text.
    let text = shape("\u{2764}\u{FE0E}", Family::Sans, REGULAR, 13.0);
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

#[test]
fn a_faces_coverage_of_the_basic_plane_is_its_character_map() {
    if !registered() {
        return;
    }
    for face in faces().iter() {
        let font = face.skrifa().expect("a font");
        let charmap = font.charmap();
        for code in 0..0x1_0000u32 {
            let Some(c) = char::from_u32(code) else { continue };
            assert_eq!(face.covers(c), charmap.map(c).is_some(), "{:?} U+{code:04X}", face.role);
        }
    }
}

#[test]
fn advances_split_as_shaping_does_and_measure_near_it() {
    if !registered() {
        return;
    }
    for text in ["Quarterly planning WAVE", "Hello 世界 again", "서울 지사 회의록", "Typography -> => != office"] {
        let (shaped, drafted) = (shape(text, Family::Sans, REGULAR, 13.0), advances(text, Family::Sans, REGULAR, 13.0));
        let faces = |runs: &[Run]| runs.iter().map(|run| (run.face, run.tofu)).collect::<Vec<_>>();
        assert_eq!(faces(&shaped), faces(&drafted), "{text}: the same runs in the same faces");
        let (exact, near) = (width(&shaped, 13.0), width(&drafted, 13.0));
        assert!((exact - near).abs() <= 0.05 * exact + 6.0, "{text}: {near} near {exact}");
        assert!(drafted.iter().all(|run| run.glyphs.iter().all(|glyph| glyph.id != 0)), "{text}: every character's own glyph");
    }
}
