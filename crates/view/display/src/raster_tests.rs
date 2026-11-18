//! Each op drawn by the rasterizer: where its pixels land at device pixel ratios 1, 1.5 and 2.

use super::*;

/// The `kition-fonts` checkout's faces: `KITION_FONTS_DIR`, else the sibling of this repository.
pub(crate) fn fonts() -> bool {
    let dir = std::env::var_os("KITION_FONTS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../kition-fonts"));
    let found = kition_table_fonts::register_dir(&dir);
    assert_eq!(found, kition_table_fonts::CHECKOUT_FILES.len(), "the kition-fonts checkout at {} (set KITION_FONTS_DIR)", dir.display());
    true
}

const INK: [u8; 4] = [20, 40, 200, 255];

struct Canvas {
    rgba: Vec<u8>,
    width: u32,
}

impl Canvas {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * self.width + x) * 4) as usize;
        [self.rgba[at], self.rgba[at + 1], self.rgba[at + 2], self.rgba[at + 3]]
    }
    /// The device pixels with any alpha, as (x0, y0, x1, y1) inclusive.
    fn ink(&self) -> Option<(u32, u32, u32, u32)> {
        let mut found: Option<(u32, u32, u32, u32)> = None;
        for (index, px) in self.rgba.as_chunks::<4>().0.iter().enumerate() {
            if px[3] > 0 {
                let (x, y) = (index as u32 % self.width, index as u32 / self.width);
                found = Some(found.map_or((x, y, x, y), |(a, b, c, d)| (a.min(x), b.min(y), c.max(x), d.max(y))));
            }
        }
        found
    }
}

fn one(op: impl FnOnce(&mut DisplayList, u16) -> Op, resources: &Resources, dpr: f64, size: u32) -> Canvas {
    let mut list = DisplayList::default();
    let ink = list.paint(INK);
    let made = op(&mut list, ink);
    list.push(made);
    Canvas { rgba: raster(&list, resources, 0.0, 0.0, dpr, size, size), width: size }
}

#[test]
fn a_snapped_rect_fills_whole_device_pixels() {
    let none = Resources::default();
    for (dpr, expected) in [(1.0, (3, 3, 12, 7)), (2.0, (5, 5, 24, 14)), (1.5, (4, 4, 18, 10))] {
        let canvas = one(|_, ink| Op::Rect { x: 2.6, y: 2.6, w: 10.0, h: 5.0, paint: ink, snap: true }, &none, dpr, 40);
        assert_eq!(canvas.ink(), Some(expected), "dpr {dpr}");
        let (x0, y0, x1, y1) = expected;
        for (x, y) in [(x0, y0), (x1, y1), ((x0 + x1) / 2, (y0 + y1) / 2)] {
            assert_eq!(canvas.at(x, y), INK, "dpr {dpr}: every covered pixel is the full color");
        }
    }
    // Unsnapped, the edges are antialiased.
    let soft = one(|_, ink| Op::Rect { x: 2.5, y: 2.0, w: 10.0, h: 5.0, paint: ink, snap: false }, &none, 1.0, 20);
    assert!(soft.at(2, 3)[3] > 0 && soft.at(2, 3)[3] < 255);
}

#[test]
fn lines_are_one_device_pixel_at_every_ratio() {
    let none = Resources::default();
    for dpr in [1.0, 1.5, 2.0] {
        let lines = vec![Line { pos: 10.0, from: 0.0, to: 20.0 }, Line { pos: 15.0, from: 2.0, to: 4.0 }];
        let horizontal = one(|_, ink| Op::Lines { axis: Axis::Horizontal, paint: ink, lines }, &none, dpr, 40);
        let row = line_pixel(10.0, 0.0, dpr) as u32;
        let width = snap(20.0, 0.0, dpr) as u32;
        for x in 0..width {
            assert_eq!(horizontal.at(x, row), INK, "dpr {dpr} x {x}");
        }
        assert_eq!(horizontal.at(0, row + 1)[3], 0, "dpr {dpr}: one device pixel thick");
        assert_eq!(horizontal.at(0, row - 1)[3], 0);
        let vertical =
            one(|_, ink| Op::Lines { axis: Axis::Vertical, paint: ink, lines: vec![Line { pos: 7.0, from: 1.0, to: 9.0 }] }, &none, dpr, 40);
        let column = line_pixel(7.0, 0.0, dpr) as u32;
        let (top, bottom) = (snap(1.0, 0.0, dpr) as u32, snap(9.0, 0.0, dpr) as u32 - 1);
        assert_eq!(vertical.ink(), Some((column, top, column, bottom)), "dpr {dpr}");
    }
}

#[test]
fn rounded_boxes_borders_shadows_and_clips() {
    let none = Resources::default();
    let filled = one(|_, ink| Op::RRect { x: 2.0, y: 2.0, w: 20.0, h: 12.0, r: 6.0, paint: ink }, &none, 1.0, 30);
    assert_eq!(filled.at(12, 8), INK);
    assert_eq!(filled.at(2, 2)[3], 0, "the corner is cut");
    let border = one(|_, ink| Op::RRectStroke { x: 2.0, y: 2.0, w: 20.0, h: 12.0, r: 4.0, width: 2.0, paint: ink }, &none, 1.0, 30);
    assert_eq!(border.at(12, 2), INK, "the border is inside the box");
    assert_eq!(border.at(12, 3), INK);
    assert_eq!(border.at(12, 8)[3], 0, "hollow");
    assert_eq!(border.at(12, 1)[3], 0, "nothing outside");
    let shadow = one(|_, ink| Op::Shadow { x: 10.0, y: 10.0, w: 10.0, h: 10.0, r: 2.0, blur: 3.0, paint: ink }, &none, 1.0, 30);
    let (inside, near, far) = (shadow.at(15, 15)[3], shadow.at(21, 15)[3], shadow.at(29, 15)[3]);
    assert!(inside > near && near > far && near > 0, "{inside} {near} {far}");

    let mut list = DisplayList::default();
    let ink = list.paint(INK);
    list.push(Op::Clip { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
    list.push(Op::Rect { x: 0.0, y: 0.0, w: 20.0, h: 5.0, paint: ink, snap: true });
    list.push(Op::Unclip);
    list.push(Op::Rect { x: 0.0, y: 10.0, w: 20.0, h: 5.0, paint: ink, snap: true });
    let canvas = Canvas { rgba: raster(&list, &none, 0.0, 0.0, 1.0, 30, 30), width: 30 };
    assert_eq!((canvas.at(9, 2), canvas.at(10, 2)[3]), (INK, 0), "clipped to the box");
    assert_eq!(canvas.at(15, 12), INK, "until the unclip");
}

#[test]
fn tofu_is_a_box_outline_on_device_pixels() {
    let none = Resources::default();
    for dpr in [1.0, 2.0] {
        let canvas = one(|_, ink| Op::Tofu { x: 2.0, y: 2.0, w: 6.0, h: 10.0, paint: ink }, &none, dpr, 30);
        let (x0, y0, x1, y1) =
            (snap(2.0, 0.0, dpr) as u32, snap(2.0, 0.0, dpr) as u32, snap(8.0, 0.0, dpr) as u32 - 1, snap(12.0, 0.0, dpr) as u32 - 1);
        assert_eq!(canvas.ink(), Some((x0, y0, x1, y1)));
        assert_eq!((canvas.at(x0, y0 + 3), canvas.at(x1, y1 - 3), canvas.at(x0 + 2, y0)), (INK, INK, INK));
        assert_eq!(canvas.at(x0 + 2, y0 + 3)[3], 0, "hollow");
    }
}

#[test]
fn icons_draw_in_their_box_and_solid_ones_are_filled() {
    let mut resources = Resources::default();
    let star = resources.icon("star").expect("Tessera has a star");
    assert_eq!(resources.icon("star"), Some(star), "one id per icon");
    assert_eq!(resources.icon("no-such-icon"), None);
    let outline = one(|_, ink| Op::Icon { icon: star, x: 4.0, y: 4.0, size: 16.0, stroke: 1.5, paint: ink, solid: false }, &resources, 2.0, 48);
    let solid = one(|_, ink| Op::Icon { icon: star, x: 4.0, y: 4.0, size: 16.0, stroke: 1.5, paint: ink, solid: true }, &resources, 2.0, 48);
    let (x0, y0, x1, y1) = solid.ink().expect("ink");
    assert!(x0 >= 6 && y0 >= 6 && x1 <= 42 && y1 <= 42, "inside its box: {:?}", (x0, y0, x1, y1));
    assert_eq!(outline.at(24, 26)[3], 0, "an outline star is hollow");
    assert_eq!(solid.at(24, 26), INK, "a solid one is filled");
}

#[test]
fn an_icon_is_its_tile_at_a_snapped_box() {
    let mut resources = Resources::default();
    let star = resources.icon("star").expect("Tessera has a star");
    for solid in [false, true] {
        let style = IconStyle { size: 16.0, stroke: 1.5, solid, paint: INK };
        for dpr in [1.0, 1.5, 2.0] {
            let canvas = one(|_, ink| Op::Icon { icon: star, x: 4.3, y: 6.6, size: 16.0, stroke: 1.5, paint: ink, solid }, &resources, dpr, 60);
            let tile = icon_tile(&resources, star, style, dpr).expect("a tile");
            let (left, top) = (snap(4.3, 0.0, dpr) as i32 + tile.left, snap(6.6, 0.0, dpr) as i32 + tile.top);
            let mut worst = 0u8;
            for ty in 0..tile.height {
                for tx in 0..tile.width {
                    let at = ((ty * tile.width + tx) * 4) as usize;
                    let got = canvas.at((left + tx as i32) as u32, (top + ty as i32) as u32);
                    worst = worst.max(tile.rgba[at..at + 4].iter().zip(got).map(|(a, b)| a.abs_diff(b)).max().unwrap_or(0));
                }
            }
            assert!(worst <= 2, "solid {solid}, dpr {dpr}: the canvas differs from the tile by {worst}");
            let (x0, y0, x1, y1) = canvas.ink().expect("ink");
            assert!(x0 as i32 > left && y0 as i32 > top, "solid {solid}, dpr {dpr}: the tile has room around the ink");
            assert!((x1 as i32) < left + tile.width as i32 - 1 && (y1 as i32) < top + tile.height as i32 - 1, "solid {solid}, dpr {dpr}");
        }
    }
    assert_eq!(icon_tile(&resources, star + 1, IconStyle { size: 16.0, stroke: 1.5, solid: false, paint: INK }, 1.0), None);
}

#[test]
fn a_text_baseline_sits_on_a_device_pixel_row() {
    assert!(fonts());
    let none = Resources::default();
    // The same run at baselines a fraction of a device pixel apart draws the same pixels.
    let at = |y: f32, dpr: f64| {
        one(
            |list, ink| {
                let text = list.string("Hamburg");
                Op::Text { x: 4.0, y, align: Align::Left, face: Face::Sans, weight: 400, size: 13.0, paint: ink, text, width: 50.0 }
            },
            &none,
            dpr,
            80,
        )
        .rgba
    };
    assert_eq!(at(20.0, 2.0), at(20.2, 2.0), "dpr 2: 20.2 rounds to the row of 20");
    assert_eq!(at(20.0, 1.0), at(19.6, 1.0), "dpr 1: 19.6 rounds to the row of 20");
    assert_ne!(at(20.0, 2.0), at(20.3, 2.0), "dpr 2: 20.3 is the next row");
}

#[test]
fn images_fill_their_box_or_show_a_placeholder() {
    let mut resources = Resources::default();
    let red = vello_cpu::Pixmap::from_parts(vec![vello_cpu::peniko::color::PremulRgba8 { r: 255, g: 0, b: 0, a: 255 }; 4 * 2], 4, 2);
    resources.set_image(7, red);
    let cover = one(|_, _| Op::Image { key: 7, x: 2.0, y: 2.0, w: 16.0, h: 16.0, fit: Fit::Cover, radius: 0.0 }, &resources, 1.0, 20);
    assert_eq!((cover.at(3, 3), cover.at(16, 16)), ([255, 0, 0, 255], [255, 0, 0, 255]), "cover fills the box");
    let contain = one(|_, _| Op::Image { key: 7, x: 2.0, y: 2.0, w: 16.0, h: 16.0, fit: Fit::Contain, radius: 0.0 }, &resources, 1.0, 20);
    assert_eq!((contain.at(10, 10), contain.at(10, 3)[3]), ([255, 0, 0, 255], 0), "contain letterboxes");
    let missing = one(|_, _| Op::Image { key: 8, x: 2.0, y: 2.0, w: 16.0, h: 16.0, fit: Fit::Cover, radius: 4.0 }, &resources, 1.0, 20);
    assert_eq!(missing.at(10, 10), [240, 240, 240, 255]);
}

#[test]
fn text_lands_on_its_baseline_from_its_anchor() {
    assert!(fonts());
    let none = Resources::default();
    let run = kition_table_fonts::shape("Hamburg", kition_table_fonts::Family::Sans, 400, 13.0);
    let width: f32 = run.iter().map(|run| run.glyphs.iter().map(|glyph| glyph.advance).sum::<f32>() * 13.0 / run.upem).sum();
    for (align, anchor) in [(Align::Left, 10.0), (Align::Center, 10.0 + width / 2.0), (Align::Right, 10.0 + width)] {
        let canvas = one(
            |list, ink| {
                let text = list.string("Hamburg");
                Op::Text { x: anchor, y: 20.0, align, face: Face::Sans, weight: 400, size: 13.0, paint: ink, text, width }
            },
            &none,
            2.0,
            160,
        );
        let (x0, y0, x1, y1) = canvas.ink().expect("ink");
        // Inside the run's advance box (2 device pixels of antialiasing), cap height above the
        // baseline, the g's descender below it.
        assert!(x0 >= 19 && f64::from(x1) <= f64::from(10.0 + width) * 2.0 + 2.0, "{align:?}: {x0}..{x1}, width {width}");
        assert!((40 - 26..40 - 14).contains(&y0) && y1 > 40 && y1 <= 40 + 10, "{align:?}: {y0}..{y1}");
    }
    // A face that is not registered draws nothing; a string outside the section is empty.
    let canvas = one(
        |list, ink| {
            let text = list.string("x");
            Op::Text { x: 0.0, y: 10.0, align: Align::Left, face: Face::Mono, weight: 400, size: 13.0, paint: ink, text, width: 8.0 }
        },
        &none,
        1.0,
        20,
    );
    assert_eq!(canvas.ink(), None);
}

#[test]
fn an_emoji_is_its_tile_at_a_snapped_pen() {
    assert!(fonts());
    let runs = kition_table_fonts::shape("\u{1F600}", kition_table_fonts::Family::Sans, 400, 13.0);
    let (face, glyph) = (runs[0].face, runs[0].glyphs[0].id);
    let mut resources = Resources::default();
    let atlas = resources.emoji(face, glyph);
    assert_eq!(resources.emoji(face, glyph), atlas);
    for dpr in [1.0, 1.5, 2.0] {
        let canvas = one(|_, _| Op::Emoji { atlas, x: 10.3, y: 30.2, size: 13.0 }, &resources, dpr, 120);
        let tile = emoji_tile(&resources, atlas, 13.0, dpr).expect("a tile");
        let (pen_x, pen_y) = (snap(10.3, 0.0, dpr) as i32, snap(30.2, 0.0, dpr) as i32);
        let mut worst = 0u8;
        let mut colored = 0;
        for ty in 0..tile.height {
            for tx in 0..tile.width {
                let (x, y) = (pen_x + tile.left + tx as i32, pen_y + tile.top + ty as i32);
                let at = ((ty * tile.width + tx) * 4) as usize;
                let expected = &tile.rgba[at..at + 4];
                let got = canvas.at(x as u32, y as u32);
                worst = worst.max(expected.iter().zip(got).map(|(a, b)| a.abs_diff(b)).max().unwrap_or(0));
                colored += usize::from(got[3] > 0 && got[0] != got[2]);
            }
        }
        assert!(worst <= 2, "dpr {dpr}: the canvas differs from the tile by {worst}");
        assert!(colored > 20, "dpr {dpr}: the emoji is in color");
        // Nothing outside the tile.
        let (x0, y0, x1, y1) = canvas.ink().expect("ink");
        assert!(x0 as i32 >= pen_x + tile.left && y0 as i32 >= pen_y + tile.top, "dpr {dpr}");
        assert!((x1 as i32) < pen_x + tile.left + tile.width as i32 && (y1 as i32) < pen_y + tile.top + tile.height as i32, "dpr {dpr}");
    }
    assert_eq!(emoji_tile(&resources, atlas + 1, 13.0, 1.0), None);
}

#[test]
fn every_op_of_a_decoded_list_draws_as_the_original() {
    assert!(fonts());
    let mut resources = Resources::default();
    resources.icon("star");
    let list = tests::every_op();
    let decoded = decode(&encode(&list)).expect("decodes");
    let at = |list: &DisplayList| raster(list, &resources, 0.0, 1280.0, 2.0, 600, 120);
    assert_eq!(at(&list), at(&decoded));
    assert!(at(&list).as_chunks::<4>().0.iter().any(|px| px[3] > 0));
}

#[test]
fn an_emoji_over_a_cell_leaves_it_opaque() {
    assert!(fonts());
    let mut resources = Resources::default();
    let mut list = DisplayList::default();
    let paper = list.paint([255, 255, 255, 255]);
    list.push(Op::Rect { x: 0.0, y: 0.0, w: 60.0, h: 40.0, paint: paper, snap: true });
    for (index, text) in ["\u{1F680}", "\u{2728}", "\u{1F600}", "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"].iter().enumerate() {
        let runs = kition_table_fonts::shape(text, kition_table_fonts::Family::Sans, 400, 13.0);
        let atlas = resources.emoji(runs[0].face, runs[0].glyphs[0].id);
        list.push(Op::Emoji { atlas, x: 2.0 + 14.0 * index as f32, y: 25.0, size: 13.0 });
    }
    for dpr in [1.0, 2.0] {
        let pixels = raster(&list, &resources, 0.0, 0.0, dpr, (60.0 * dpr) as u32, (40.0 * dpr) as u32);
        assert!(pixels.as_chunks::<4>().0.iter().all(|px| px[3] == 255), "dpr {dpr}: an emoji's layers cut into the cell");
    }
}
