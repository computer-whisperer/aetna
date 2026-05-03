//! Tree → [`DrawOp`] resolution.
//!
//! Walks the laid-out [`El`] tree and emits a flat [`Vec<DrawOp>`] in
//! paint order. Each visual fact resolves to a `Quad` (bound to a stock
//! or custom shader, with uniforms packed) or a `GlyphRun`.
//!
//! State styling lands here on the CPU side. Hover lightens / press
//! darkens / focus-ring fade are pre-eased into `n.fill`, `n.text_color`,
//! `n.stroke`, and `n.focus_ring_alpha` by
//! [`crate::event::UiState::tick_visual_animations`] *before* this
//! pass runs. What remains here are the deltas that don't ease — alpha
//! multiplication for `Disabled`, and the `Loading` text suffix.

use crate::ir::*;
use crate::shader::*;
use crate::tokens;
use crate::tree::*;

/// Walk the laid-out tree and emit draw ops in paint order.
pub fn draw_ops(root: &El) -> Vec<DrawOp> {
    let mut out = Vec::new();
    push_node(root, &mut out, None);
    out
}

fn push_node(n: &El, out: &mut Vec<DrawOp>, inherited_scissor: Option<Rect>) {
    let (fill, stroke, text_color, weight, suffix) = apply_state(n);
    let own_scissor = if n.clip {
        intersect_scissor(inherited_scissor, n.computed)
    } else {
        inherited_scissor
    };

    // Surface paint. Either a custom shader override, or the implicit
    // `stock::rounded_rect` driven by the El's fill/stroke/radius/shadow.
    if let Some(custom) = &n.shader_override {
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: n.computed,
            scissor: own_scissor,
            shader: custom.handle,
            uniforms: custom.uniforms.clone(),
        });
    } else if fill.is_some() || stroke.is_some() {
        let mut uniforms = UniformBlock::new();
        if let Some(c) = fill {
            uniforms.insert("fill", UniformValue::Color(c));
        }
        if let Some(c) = stroke {
            uniforms.insert("stroke", UniformValue::Color(c));
            uniforms.insert("stroke_width", UniformValue::F32(n.stroke_width));
        }
        uniforms.insert("radius", UniformValue::F32(n.radius));
        if n.shadow > 0.0 {
            uniforms.insert("shadow", UniformValue::F32(n.shadow));
        }
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: n.computed,
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
        let ring_rect = inset_rect(n.computed, -tokens::FOCUS_RING_WIDTH * 0.5);
        let mut uniforms = UniformBlock::new();
        let base = tokens::FOCUS_RING;
        let eased_alpha = (base.a as f32 * n.focus_ring_alpha)
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
        out.push(DrawOp::GlyphRun {
            id: n.computed_id.clone(),
            rect: n.computed,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::TextSdf),
            color: text_color.unwrap_or(tokens::TEXT_FOREGROUND),
            text: display,
            size: n.font_size,
            weight,
            mono: n.font_mono,
            wrap: n.text_wrap,
            anchor,
        });
    }

    for c in &n.children {
        push_node(c, out, own_scissor);
    }
}

/// Apply the residual non-eased state deltas, returning the effective
/// `(fill, stroke, text_color, font_weight, optional text suffix)`.
///
/// Hover/press/focus-ring transitions are pre-eased into `n.fill`,
/// `n.text_color`, `n.stroke`, and `n.focus_ring_alpha` by the
/// animation tracker before draw_ops runs. Disabled (alpha multiply)
/// and Loading (text suffix) don't ease and are still applied here.
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
}
