//! Tree → [`DrawOp`] resolution.
//!
//! Walks the laid-out [`El`] tree and emits a flat [`Vec<DrawOp>`] in
//! paint order. Each visual fact resolves to a `Quad` (bound to a stock
//! or custom shader, with uniforms packed) or a `GlyphRun`.
//!
//! State styling is applied here on the CPU side: hover lightens fills,
//! press darkens, focus emits an extra ring quad, disabled multiplies
//! alpha, loading appends " ⋯". When v0.2 lands, state will likely
//! become a uniform that shaders interpret directly — but for v0.1 the
//! state delta still happens here so stock shaders can stay stateless.

use crate::ir::*;
use crate::shader::*;
use crate::tokens;
use crate::tree::*;

/// Walk the laid-out tree and emit draw ops in paint order.
pub fn draw_ops(root: &El) -> Vec<DrawOp> {
    let mut out = Vec::new();
    push_node(root, &mut out);
    out
}

fn push_node(n: &El, out: &mut Vec<DrawOp>) {
    let (fill, stroke, text_color, weight, suffix) = apply_state(n);

    // Surface paint. Either a custom shader override, or the implicit
    // `stock::rounded_rect` driven by the El's fill/stroke/radius/shadow.
    if let Some(custom) = &n.shader_override {
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: n.computed,
            scissor: None,
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
            scissor: None,
            shader: ShaderHandle::Stock(StockShader::RoundedRect),
            uniforms,
        });
    }

    // Focus ring: an extra inset quad on focus-state nodes, painted by
    // `stock::focus_ring`.
    if matches!(n.state, InteractionState::Focus)
        && (matches!(n.kind, Kind::Button | Kind::Card | Kind::Badge | Kind::Custom(_))
            || stroke.is_some())
    {
        let ring_rect = inset_rect(n.computed, -tokens::FOCUS_RING_WIDTH * 0.5);
        let mut uniforms = UniformBlock::new();
        uniforms.insert("color", UniformValue::Color(tokens::FOCUS_RING));
        uniforms.insert("width", UniformValue::F32(tokens::FOCUS_RING_WIDTH));
        uniforms.insert("radius", UniformValue::F32(n.radius + tokens::FOCUS_RING_WIDTH * 0.5));
        out.push(DrawOp::Quad {
            id: format!("{}.focus-ring", n.computed_id),
            rect: ring_rect,
            scissor: None,
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
            _ => TextAnchor::Start,
        };
        out.push(DrawOp::GlyphRun {
            id: n.computed_id.clone(),
            rect: n.computed,
            scissor: None,
            shader: ShaderHandle::Stock(StockShader::TextSdf),
            color: text_color.unwrap_or(tokens::TEXT_FOREGROUND),
            text: display,
            size: n.font_size,
            weight,
            mono: n.font_mono,
            anchor,
        });
    }

    for c in &n.children {
        push_node(c, out);
    }
}

/// Apply state-specific visual deltas, returning the effective
/// (fill, stroke, text_color, font_weight, optional text suffix).
///
/// In v0.2 this likely moves into the shader via a `state` uniform; for
/// v0.1 the CPU-side delta is simpler and lets stock shaders stay
/// stateless.
fn apply_state(n: &El) -> (Option<Color>, Option<Color>, Option<Color>, FontWeight, Option<&'static str>) {
    let mut fill = n.fill;
    let mut stroke = n.stroke;
    let mut text_color = n.text_color;
    let weight = n.font_weight;
    let mut suffix = None;

    match n.state {
        InteractionState::Default | InteractionState::Focus => {}
        InteractionState::Hover => {
            fill = fill.map(|c| c.lighten(tokens::HOVER_LIGHTEN));
            stroke = stroke.map(|c| c.lighten(tokens::HOVER_LIGHTEN));
            text_color = text_color.map(|c| c.lighten(tokens::HOVER_LIGHTEN * 0.5));
        }
        InteractionState::Press => {
            fill = fill.map(|c| c.darken(tokens::PRESS_DARKEN));
            stroke = stroke.map(|c| c.darken(tokens::PRESS_DARKEN));
        }
        InteractionState::Disabled => {
            let alpha = (255.0 * tokens::DISABLED_ALPHA) as u8;
            fill = fill.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
            stroke = stroke.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
            text_color = text_color.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
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
