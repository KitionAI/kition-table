//! Color glyphs (emoji) painted with vello_cpu from the fonts' drawing steps (ported from
//! kition-note's view): layers clipped to glyph outlines, filled with palette colors and linear,
//! radial, or sweep gradients.

use kition_table_fonts::{Brush, CompositeMode, Extend, Paint, Step};
use kurbo::{Affine, BezPath, Rect, Shape};
use vello_cpu::RenderContext;
use vello_cpu::peniko::color::{AlphaColor, DynamicColor, Srgb};
use vello_cpu::peniko::{self, BlendMode, Compose, Gradient, Mix};

/// Paints a color glyph's `steps` (font units, y up) with `base` taking them to the screen;
/// `foreground` is the text's color, for the palette's foreground entries.
pub fn paint(ctx: &mut RenderContext, base: Affine, steps: &[Step], foreground: [u8; 4]) {
    let mut transforms = vec![base];
    // What the next `Fill` fills: the innermost clip's shape, in its own transform.
    let mut clips: Vec<(Affine, BezPath)> = Vec::new();
    let top = |transforms: &[Affine]| transforms.last().copied().unwrap_or(base);
    for step in steps {
        match step {
            Step::PushTransform(m) => transforms.push(top(&transforms) * affine(m)),
            Step::PopTransform => {
                if transforms.len() > 1 {
                    transforms.pop();
                }
            }
            Step::PushClip(path) => {
                let at = top(&transforms);
                ctx.set_transform(at);
                ctx.push_clip_layer(path);
                clips.push((at, BezPath::clone(path)));
            }
            Step::PushClipBox([x0, y0, x1, y1]) => {
                let at = top(&transforms);
                let path = Rect::new(f64::from(*x0), f64::from(*y0), f64::from(*x1), f64::from(*y1)).to_path(0.1);
                ctx.set_transform(at);
                ctx.push_clip_layer(&path);
                clips.push((at, path));
            }
            Step::PopClip => {
                clips.pop();
                ctx.pop_layer();
            }
            Step::FillPath { path, brush_transform, brush } => {
                ctx.set_transform(top(&transforms));
                set_brush(ctx, brush, foreground);
                ctx.set_paint_transform(brush_transform.as_ref().map_or(Affine::IDENTITY, affine));
                ctx.fill_path(path);
                ctx.reset_paint_transform();
            }
            Step::Fill(brush) => {
                // The innermost clip's shape, so the fill covers what the clip lets through.
                let Some((at, path)) = clips.last() else { continue };
                let inverse = at.inverse();
                ctx.set_transform(*at);
                set_brush(ctx, brush, foreground);
                // The brush is in the current transform's space, the shape in the clip's.
                ctx.set_paint_transform(inverse * top(&transforms));
                ctx.fill_path(path);
                ctx.reset_paint_transform();
            }
            Step::PushLayer(mode) => ctx.push_layer(None, Some(blend(*mode)), None, None, None),
            Step::PopLayer => ctx.pop_layer(),
        }
    }
}

fn affine(m: &[f32; 6]) -> Affine {
    Affine::new(m.map(f64::from))
}

fn color(paint: Paint, foreground: [u8; 4]) -> AlphaColor<Srgb> {
    let [r, g, b, a] = match paint {
        Paint::Rgba(rgba) => rgba,
        Paint::Foreground(alpha) => {
            let [r, g, b, a] = foreground;
            [r, g, b, (f32::from(a) * alpha.clamp(0.0, 1.0)).round() as u8]
        }
    };
    AlphaColor::from_rgba8(r, g, b, a)
}

fn stops(stops: &[(f32, Paint)], foreground: [u8; 4]) -> Vec<peniko::ColorStop> {
    stops
        .iter()
        .map(|(offset, paint)| peniko::ColorStop { offset: *offset, color: DynamicColor::from_alpha_color(color(*paint, foreground)) })
        .collect()
}

fn extend(extend: Extend) -> peniko::Extend {
    match extend {
        Extend::Repeat => peniko::Extend::Repeat,
        Extend::Reflect => peniko::Extend::Reflect,
        _ => peniko::Extend::Pad,
    }
}

fn set_brush(ctx: &mut RenderContext, brush: &Brush, foreground: [u8; 4]) {
    let point = |(x, y): (f32, f32)| (f64::from(x), f64::from(y));
    match brush {
        Brush::Solid(paint) => ctx.set_paint(color(*paint, foreground)),
        Brush::Linear { p0, p1, stops: list, extend: how } => {
            ctx.set_paint(Gradient::new_linear(point(*p0), point(*p1)).with_stops(stops(list, foreground).as_slice()).with_extend(extend(*how)));
        }
        Brush::Radial { c0, r0, c1, r1, stops: list, extend: how } => {
            ctx.set_paint(
                Gradient::new_two_point_radial(point(*c0), *r0, point(*c1), *r1)
                    .with_stops(stops(list, foreground).as_slice())
                    .with_extend(extend(*how)),
            );
        }
        Brush::Sweep { center, start, end, stops: list, extend: how } => {
            ctx.set_paint(
                Gradient::new_sweep(point(*center), start.to_radians(), end.to_radians())
                    .with_stops(stops(list, foreground).as_slice())
                    .with_extend(extend(*how)),
            );
        }
    }
}

fn blend(mode: CompositeMode) -> BlendMode {
    let compose = |compose| BlendMode::new(Mix::Normal, compose);
    let mix = |mix| BlendMode::new(mix, Compose::SrcOver);
    match mode {
        CompositeMode::Clear => compose(Compose::Clear),
        CompositeMode::Src => compose(Compose::Copy),
        CompositeMode::Dest => compose(Compose::Dest),
        CompositeMode::DestOver => compose(Compose::DestOver),
        CompositeMode::SrcIn => compose(Compose::SrcIn),
        CompositeMode::DestIn => compose(Compose::DestIn),
        CompositeMode::SrcOut => compose(Compose::SrcOut),
        CompositeMode::DestOut => compose(Compose::DestOut),
        CompositeMode::SrcAtop => compose(Compose::SrcAtop),
        CompositeMode::DestAtop => compose(Compose::DestAtop),
        CompositeMode::Xor => compose(Compose::Xor),
        CompositeMode::Plus => compose(Compose::Plus),
        CompositeMode::Screen => mix(Mix::Screen),
        CompositeMode::Overlay => mix(Mix::Overlay),
        CompositeMode::Darken => mix(Mix::Darken),
        CompositeMode::Lighten => mix(Mix::Lighten),
        CompositeMode::ColorDodge => mix(Mix::ColorDodge),
        CompositeMode::ColorBurn => mix(Mix::ColorBurn),
        CompositeMode::HardLight => mix(Mix::HardLight),
        CompositeMode::SoftLight => mix(Mix::SoftLight),
        CompositeMode::Difference => mix(Mix::Difference),
        CompositeMode::Exclusion => mix(Mix::Exclusion),
        CompositeMode::Multiply => mix(Mix::Multiply),
        CompositeMode::HslHue => mix(Mix::Hue),
        CompositeMode::HslSaturation => mix(Mix::Saturation),
        CompositeMode::HslColor => mix(Mix::Color),
        CompositeMode::HslLuminosity => mix(Mix::Luminosity),
        _ => compose(Compose::SrcOver),
    }
}
