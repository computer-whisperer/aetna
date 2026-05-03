//! Tree → [`DrawOp`] resolution.
//!
//! Walks the laid-out [`El`] tree and emits a flat [`Vec<DrawOp>`] in
//! paint order. Each visual fact resolves to a `Quad` (bound to a stock
//! or custom shader, with uniforms packed) or a `GlyphRun`.
//!
//! State styling lands here on the CPU side. Hover lightens / press
//! darkens / focus-ring fade are pre-eased into `n.fill`, `n.text_color`,
//! `n.stroke`, and `n.focus_ring_alpha` by
//! [`crate::state::UiState::tick_visual_animations`] *before* this
//! pass runs. What remains here are the deltas that don't ease — alpha
//! multiplication for `Disabled`, and the `Loading` text suffix.

use crate::ir::*;
use crate::shader::*;
use crate::tokens;
use crate::tree::*;

/// Walk the laid-out tree and emit draw ops in paint order.
pub fn draw_ops(root: &El) -> Vec<DrawOp> {
    let mut out = Vec::new();
    push_node(root, &mut out, None, (0.0, 0.0), 1.0);
    out
}

fn push_node(
    n: &El,
    out: &mut Vec<DrawOp>,
    inherited_scissor: Option<Rect>,
    inherited_translate: (f32, f32),
    inherited_opacity: f32,
) {
    let (fill, stroke, text_color, weight, suffix) = apply_state(n);

    // `translate` is subtree-inheriting: descendants paint at their
    // computed rect plus all ancestor `translate` accumulated through
    // the recursion. `scale` and `opacity` apply to this node only —
    // a parent fading to 0.5 multiplies through to descendants via
    // `inherited_opacity`, but `scale` doesn't propagate (descendants
    // keep their own paint metrics).
    let total_translate = (
        inherited_translate.0 + n.translate.0,
        inherited_translate.1 + n.translate.1,
    );
    let opacity = inherited_opacity * n.opacity;

    let translated_rect = translated(n.computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, n.scale);
    let painted_font_size = n.font_size * n.scale;

    // Clip is computed in the (already-translated) paint space so
    // descendants below this clip rect get scissored consistently with
    // where the surface visually lands.
    let own_scissor = if n.clip {
        intersect_scissor(inherited_scissor, painted_rect)
    } else {
        inherited_scissor
    };

    // Surface paint. Either a custom shader override, or the implicit
    // `stock::rounded_rect` driven by the El's fill/stroke/radius/shadow.
    if let Some(custom) = &n.shader_override {
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: painted_rect,
            scissor: own_scissor,
            shader: custom.handle,
            uniforms: custom.uniforms.clone(),
        });
    } else if fill.is_some() || stroke.is_some() {
        let mut uniforms = UniformBlock::new();
        if let Some(c) = fill {
            uniforms.insert("fill", UniformValue::Color(opaque(c, opacity)));
        }
        if let Some(c) = stroke {
            uniforms.insert("stroke", UniformValue::Color(opaque(c, opacity)));
            uniforms.insert("stroke_width", UniformValue::F32(n.stroke_width));
        }
        uniforms.insert("radius", UniformValue::F32(n.radius));
        if n.shadow > 0.0 {
            uniforms.insert("shadow", UniformValue::F32(n.shadow));
        }
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: painted_rect,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::RoundedRect),
            uniforms,
        });
    }

    // Focus ring: emit while the per-node alpha (eased by the
    // animation tracker on focus enter / leave) is non-zero. The ring
    // colour multiplies `tokens::FOCUS_RING.a` by the alpha so the ring
    // fades in on focus and fades out after focus moves elsewhere.
    if n.focus_ring_alpha > 0.0
        && (matches!(
            n.kind,
            Kind::Button | Kind::Card | Kind::Badge | Kind::Custom(_)
        ) || stroke.is_some())
    {
        let ring_rect = inset_rect(painted_rect, -tokens::FOCUS_RING_WIDTH * 0.5);
        let mut uniforms = UniformBlock::new();
        let base = tokens::FOCUS_RING;
        let eased_alpha = (base.a as f32 * n.focus_ring_alpha * opacity)
            .round()
            .clamp(0.0, 255.0) as u8;
        uniforms.insert("color", UniformValue::Color(base.with_alpha(eased_alpha)));
        uniforms.insert("width", UniformValue::F32(tokens::FOCUS_RING_WIDTH));
        uniforms.insert(
            "radius",
            UniformValue::F32(n.radius + tokens::FOCUS_RING_WIDTH * 0.5),
        );
        out.push(DrawOp::Quad {
            id: format!("{}.focus-ring", n.computed_id),
            rect: ring_rect,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::FocusRing),
            uniforms,
        });
    }

    if let Some(text) = &n.text {
        let display = match suffix {
            Some(s) => format!("{text}{s}"),
            None => text.clone(),
        };
        let anchor = match n.kind {
            Kind::Button | Kind::Badge => TextAnchor::Middle,
            _ => match n.text_align {
                TextAlign::Start => TextAnchor::Start,
                TextAlign::Center => TextAnchor::Middle,
                TextAlign::End => TextAnchor::End,
            },
        };
        let text_color =
            opaque(text_color.unwrap_or(tokens::TEXT_FOREGROUND), opacity);
        out.push(DrawOp::GlyphRun {
            id: n.computed_id.clone(),
            rect: painted_rect,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::TextSdf),
            color: text_color,
            text: display,
            size: painted_font_size,
            weight,
            mono: n.font_mono,
            wrap: n.text_wrap,
            anchor,
        });
    }

    for c in &n.children {
        push_node(c, out, own_scissor, total_translate, opacity);
    }
}

fn translated(r: Rect, offset: (f32, f32)) -> Rect {
    if offset.0 == 0.0 && offset.1 == 0.0 {
        return r;
    }
    Rect::new(r.x + offset.0, r.y + offset.1, r.w, r.h)
}

/// Scale `r` uniformly by `s` around its centre. `s == 1.0` short-circuits
/// to identity so the common case is allocation-free of float drift.
fn scaled_around_center(r: Rect, s: f32) -> Rect {
    if (s - 1.0).abs() < f32::EPSILON {
        return r;
    }
    let cx = r.center_x();
    let cy = r.center_y();
    let w = r.w * s;
    let h = r.h * s;
    Rect::new(cx - w * 0.5, cy - h * 0.5, w, h)
}

fn opaque(c: Color, opacity: f32) -> Color {
    if (opacity - 1.0).abs() < f32::EPSILON {
        return c;
    }
    let a = (c.a as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
    c.with_alpha(a)
}

/// Resolve the effective `(fill, stroke, text_color, font_weight,
/// optional text suffix)` for paint.
///
/// Hover and press are applied as **envelope mixes**: `apply_state`
/// reads `n.hover_amount` / `n.press_amount` (both 0..1, eased by the
/// animation tracker) and lerps the build-time colour toward the
/// state-modulated colour. This composition keeps state easing
/// independent of mid-flight changes to `n.fill` — the author can swap
/// a button's colour during a hover and the new colour appears with
/// the same eased lighten amount, no fighting between trackers.
///
/// Disabled (alpha multiply) and Loading (text suffix) aren't eased
/// and are still applied here.
fn apply_state(
    n: &El,
) -> (
    Option<Color>,
    Option<Color>,
    Option<Color>,
    FontWeight,
    Option<&'static str>,
) {
    let mut fill = n.fill;
    let mut stroke = n.stroke;
    let mut text_color = n.text_color;
    let weight = n.font_weight;
    let mut suffix = None;

    if n.hover_amount > 0.0 {
        let h = n.hover_amount;
        fill = fill.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN), h));
        stroke = stroke.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN), h));
        text_color = text_color.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN * 0.5), h));
    }
    if n.press_amount > 0.0 {
        let p = n.press_amount;
        fill = fill.map(|c| c.mix(c.darken(tokens::PRESS_DARKEN), p));
        stroke = stroke.map(|c| c.mix(c.darken(tokens::PRESS_DARKEN), p));
    }

    match n.state {
        InteractionState::Default
        | InteractionState::Focus
        | InteractionState::Hover
        | InteractionState::Press => {}
        InteractionState::Disabled => {
            let alpha = (255.0 * tokens::DISABLED_ALPHA) as u8;
            fill = fill.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
            stroke = stroke.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
            text_color =
                text_color.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
        }
        InteractionState::Loading => {
            text_color = text_color.map(|c| c.with_alpha(((c.a as u32 * 200) / 255) as u8));
            suffix = Some(" ⋯");
        }
    }
    (fill, stroke, text_color, weight, suffix)
}

fn inset_rect(r: Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + by * 2.0, r.h + by * 2.0)
}

fn intersect_scissor(current: Option<Rect>, next: Rect) -> Option<Rect> {
    match current {
        Some(r) => Some(r.intersect(next).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))),
        None => Some(next),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{button, column, row};

    #[test]
    fn clip_sets_scissor_on_descendant_ops() {
        let mut root = column([row([
            button("Inside").key("inside"),
            button("Too wide").key("outside").width(Size::Fixed(300.0)),
        ])
        .clip()
        .width(Size::Fixed(120.0))]);
        crate::layout::layout(&mut root, Rect::new(0.0, 0.0, 400.0, 100.0));

        let ops = draw_ops(&root);
        let clipped = ops
            .iter()
            .find(|op| op.id().contains("outside"))
            .expect("outside button op");
        let DrawOp::Quad { scissor, .. } = clipped else {
            panic!("expected button surface quad");
        };
        assert_eq!(*scissor, Some(Rect::new(0.0, 0.0, 120.0, 36.0)));
    }

    #[test]
    fn text_align_center_emits_middle_anchor() {
        let mut root = crate::text("Centered").center_text();
        crate::layout::layout(&mut root, Rect::new(0.0, 0.0, 200.0, 80.0));

        let ops = draw_ops(&root);
        let DrawOp::GlyphRun { anchor, .. } = &ops[0] else {
            panic!("expected glyph run");
        };
        assert_eq!(*anchor, TextAnchor::Middle);
    }

    #[test]
    fn paragraph_emits_wrapped_glyph_run() {
        let mut root = crate::paragraph("This sentence should wrap in a narrow box.")
            .width(Size::Fixed(120.0));
        crate::layout::layout(&mut root, Rect::new(0.0, 0.0, 120.0, 120.0));

        let ops = draw_ops(&root);
        let DrawOp::GlyphRun { wrap, .. } = &ops[0] else {
            panic!("expected glyph run");
        };
        assert_eq!(*wrap, TextWrap::Wrap);
    }

    #[test]
    fn opacity_multiplies_alpha_on_quad_uniforms() {
        let mut root = button("X").fill(Color::rgba(200, 100, 50, 200)).opacity(0.5);
        crate::layout::layout(&mut root, Rect::new(0.0, 0.0, 200.0, 100.0));
        let ops = draw_ops(&root);
        let DrawOp::Quad { uniforms, .. } = &ops[0] else {
            panic!("expected quad op");
        };
        let UniformValue::Color(c) = uniforms.get("fill").expect("fill") else {
            panic!("fill should be a colour");
        };
        // 200 * 0.5 = 100
        assert_eq!(c.a, 100, "alpha should be halved by opacity 0.5");
    }

    #[test]
    fn translate_offsets_paint_rect_and_inherits_to_children() {
        // Parent translate of (50, 30) should land child rects at
        // child.computed + (50, 30).
        let mut root = column([button("X").key("x")]).translate(50.0, 30.0);
        crate::layout::layout(&mut root, Rect::new(0.0, 0.0, 400.0, 200.0));
        let computed_x = ops_quad_for(&root, "x").expect("x quad rect");
        // Find the same node's pre-translate computed rect.
        let untranslated = find_computed(&root, "x").expect("x computed");

        assert!((computed_x.x - (untranslated.x + 50.0)).abs() < 0.5);
        assert!((computed_x.y - (untranslated.y + 30.0)).abs() < 0.5);
    }

    #[test]
    fn scale_scales_rect_around_center() {
        let mut root = column([button("X").key("x").scale(2.0).width(Size::Fixed(40.0))]);
        crate::layout::layout(&mut root, Rect::new(0.0, 0.0, 200.0, 100.0));
        let pre = find_computed(&root, "x").expect("computed");
        let post = ops_quad_for(&root, "x").expect("painted");

        // 2x scale around centre: w doubles, x shifts left by w/2.
        assert!((post.w - pre.w * 2.0).abs() < 0.5);
        assert!((post.h - pre.h * 2.0).abs() < 0.5);
        let pre_cx = pre.center_x();
        let post_cx = post.center_x();
        assert!(
            (pre_cx - post_cx).abs() < 0.5,
            "centre should be preserved by scale-around-centre",
        );
    }

    fn ops_quad_for(root: &El, key: &str) -> Option<Rect> {
        let ops = draw_ops(root);
        for op in ops {
            if let DrawOp::Quad { id, rect, .. } = op {
                if id.contains(key) {
                    return Some(rect);
                }
            }
        }
        None
    }
    fn find_computed(node: &El, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) {
            return Some(node.computed);
        }
        node.children.iter().find_map(|c| find_computed(c, key))
    }
}
