//! SVG fallback renderer — approximate visual output for the agent loop.
//!
//! In attempt_4 the production renderer is wgpu (landing v0.1+); SVG is
//! demoted to an explicitly-approximate fixture renderer. It exists so
//! the agent feedback loop (edit Rust → render → look at PNG + lint)
//! keeps working while the wgpu renderer is brought up.
//!
//! Stock shaders are interpreted best-effort:
//!
//! - `stock::rounded_rect` → `<rect>` with `fill`, `stroke`, `rx` from
//!   uniforms and an SVG drop-shadow filter.
//! - `stock::text_sdf` → `<text>` element with font + color from the op.
//! - `stock::focus_ring` → unfilled `<rect>` with stroke from uniforms.
//! - Custom shaders → labeled placeholder rect with metadata in
//!   `<title>` and a translucent fill so they're visible during fixture
//!   rendering. The wgpu renderer is the source of truth for custom
//!   shaders; SVG just lets you see the layout.

use std::fmt::Write as _;

use crate::ir::*;
use crate::shader::*;
use crate::text_metrics;
use crate::tokens;
use crate::tree::*;

/// Render a `Vec<DrawOp>` to an SVG string.
pub fn svg_from_ops(width: f32, height: f32, ops: &[DrawOp], bg: Color) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    s.push_str(SHADOW_DEFS);
    let _ = writeln!(
        s,
        r#"<rect width="100%" height="100%" fill="{}"/>"#,
        color_svg(bg)
    );

    for (i, op) in ops.iter().enumerate() {
        if let Some(scissor) = op.scissor() {
            let clip_id = format!("clip-{i}");
            let _ = writeln!(
                s,
                r#"<clipPath id="{clip_id}"><rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}"/></clipPath><g clip-path="url(#{clip_id})">"#,
                scissor.x, scissor.y, scissor.w, scissor.h,
            );
            emit_op(&mut s, op);
            s.push_str("</g>\n");
        } else {
            emit_op(&mut s, op);
        }
    }
    s.push_str("</svg>\n");
    s
}

fn emit_op(s: &mut String, op: &DrawOp) {
    match op {
        DrawOp::Quad {
            id,
            rect,
            shader,
            uniforms,
            ..
        } => emit_quad(s, id, *rect, shader, uniforms),
        DrawOp::GlyphRun {
            id,
            rect,
            color,
            size,
            weight,
            mono,
            wrap,
            anchor,
            layout,
            ..
        } => {
            emit_glyph_run(
                s, id, *rect, *color, *size, *weight, *mono, *wrap, *anchor, layout,
            );
        }
        DrawOp::AttributedText {
            id,
            rect,
            runs,
            size,
            wrap,
            anchor,
            layout,
            ..
        } => {
            emit_attributed_text(s, id, *rect, *size, *wrap, *anchor, runs, layout);
        }
        DrawOp::BackdropSnapshot => {} // v2 — no SVG analogue.
    }
}

fn emit_quad(s: &mut String, id: &str, rect: Rect, shader: &ShaderHandle, uniforms: &UniformBlock) {
    match shader {
        ShaderHandle::Stock(StockShader::RoundedRect) => {
            let fill = uniforms.get("fill").and_then(as_color);
            let stroke = uniforms.get("stroke").and_then(as_color);
            let stroke_w = uniforms.get("stroke_width").and_then(as_f32).unwrap_or(0.0);
            let radius = uniforms.get("radius").and_then(as_f32).unwrap_or(0.0);
            let shadow = uniforms.get("shadow").and_then(as_f32).unwrap_or(0.0);
            let r = radius.min(rect.w * 0.5).min(rect.h * 0.5).max(0.0);
            let fill_attr = match fill {
                Some(c) => format!(r#" fill="{}""#, color_svg(c)),
                None => r#" fill="none""#.to_string(),
            };
            let stroke_attr = match stroke {
                Some(c) => format!(
                    r#" stroke="{}" stroke-width="{:.2}""#,
                    color_svg(c),
                    stroke_w
                ),
                None => String::new(),
            };
            let filter = shadow_filter(shadow);
            let _ = writeln!(
                s,
                r#"<rect data-node="{}" data-shader="stock::rounded_rect" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}"{}{}{} />"#,
                esc(id),
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                r,
                fill_attr,
                stroke_attr,
                filter
            );
        }
        ShaderHandle::Stock(StockShader::FocusRing) => {
            let color = uniforms
                .get("color")
                .and_then(as_color)
                .unwrap_or(tokens::FOCUS_RING);
            let width = uniforms
                .get("width")
                .and_then(as_f32)
                .unwrap_or(tokens::FOCUS_RING_WIDTH);
            let radius = uniforms.get("radius").and_then(as_f32).unwrap_or(0.0);
            let r = radius.min(rect.w * 0.5).min(rect.h * 0.5).max(0.0);
            let _ = writeln!(
                s,
                r#"<rect data-node="{}" data-shader="stock::focus_ring" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}" fill="none" stroke="{}" stroke-width="{:.2}" />"#,
                esc(id),
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                r,
                color_svg(color),
                width
            );
        }
        ShaderHandle::Stock(StockShader::SolidQuad) => {
            let fill = uniforms
                .get("fill")
                .and_then(as_color)
                .unwrap_or(tokens::BG_MUTED);
            let _ = writeln!(
                s,
                r#"<rect data-node="{}" data-shader="stock::solid_quad" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}" />"#,
                esc(id),
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                color_svg(fill)
            );
        }
        ShaderHandle::Stock(StockShader::DividerLine) => {
            let fill = uniforms
                .get("fill")
                .and_then(as_color)
                .unwrap_or(tokens::BORDER);
            let _ = writeln!(
                s,
                r#"<rect data-node="{}" data-shader="stock::divider_line" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="{}" />"#,
                esc(id),
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                color_svg(fill)
            );
        }
        ShaderHandle::Stock(StockShader::Text) => {
            // text shouldn't appear as a Quad — skip silently.
        }
        ShaderHandle::Custom(name) => {
            // Placeholder rect so layout is visible. Real paint requires
            // wgpu + the registered shader.
            let mut title = format!("custom shader: {name}");
            for (k, v) in uniforms {
                let _ = write!(title, " {k}={}", v.debug_short());
            }
            let _ = writeln!(
                s,
                r#"<g data-node="{}" data-shader="custom::{}"><title>{}</title><rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" fill="rgba(255,0,255,0.18)" stroke="rgba(255,0,255,0.5)" stroke-width="1" stroke-dasharray="3 2" /></g>"#,
                esc(id),
                esc(name),
                esc(&title),
                rect.x,
                rect.y,
                rect.w,
                rect.h
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_glyph_run(
    s: &mut String,
    id: &str,
    rect: Rect,
    color: Color,
    size: f32,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    anchor: TextAnchor,
    layout: &text_metrics::TextLayout,
) {
    let (x, anchor_attr) = match anchor {
        TextAnchor::Start => (rect.x, "start"),
        TextAnchor::Middle => (rect.center_x(), "middle"),
        TextAnchor::End => (rect.right(), "end"),
    };
    let line_top = match wrap {
        TextWrap::NoWrap => rect.y + ((rect.h - layout.line_height) * 0.5).max(0.0),
        TextWrap::Wrap => rect.y,
    };
    let family = if mono {
        "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"
    } else {
        "Inter, ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif"
    };
    let weight_str = match weight {
        FontWeight::Regular => "400",
        FontWeight::Medium => "500",
        FontWeight::Semibold => "600",
        FontWeight::Bold => "700",
    };
    for line in &layout.lines {
        // SVG uses a baseline; glyphon positions by line top. The core
        // text layout carries Roboto's baseline offset so artifacts stay
        // close to the wgpu text path.
        let y = line_top + line.baseline;
        let _ = writeln!(
            s,
            r#"<text data-node="{}" data-shader="stock::text" x="{:.2}" y="{:.2}" font-family="{}" font-size="{:.2}" font-weight="{}" fill="{}" text-anchor="{}">{}</text>"#,
            esc(id),
            x,
            y,
            family,
            size,
            weight_str,
            color_svg(color),
            anchor_attr,
            esc(&line.text)
        );
    }
}

/// Degraded SVG emission for attributed text. The wgpu/vulkano paths
/// shape runs through cosmic-text and produce per-glyph color +
/// run_index, so each run can paint its own fill. SVG can't easily
/// reproduce cosmic-text's wrapping decisions, so we collapse the
/// paragraph onto one `<text>` element with one `<tspan>` per run,
/// carrying that run's color / weight / style attributes. This is
/// honest about the rendering layer's limits — the SVG fallback
/// captures *which* runs flowed where in source order, even if it
/// can't replicate the exact line breaks.
#[allow(clippy::too_many_arguments)]
fn emit_attributed_text(
    s: &mut String,
    id: &str,
    rect: Rect,
    size: f32,
    wrap: TextWrap,
    anchor: TextAnchor,
    runs: &[(String, crate::text_atlas::RunStyle)],
    layout: &text_metrics::TextLayout,
) {
    let (x, anchor_attr) = match anchor {
        TextAnchor::Start => (rect.x, "start"),
        TextAnchor::Middle => (rect.center_x(), "middle"),
        TextAnchor::End => (rect.right(), "end"),
    };
    let line_top = match wrap {
        TextWrap::NoWrap => rect.y + ((rect.h - layout.line_height) * 0.5).max(0.0),
        TextWrap::Wrap => rect.y,
    };
    let baseline = layout
        .lines
        .first()
        .map(|l| l.baseline)
        .unwrap_or(layout.line_height * 0.8);
    let y = line_top + baseline;

    let _ = write!(
        s,
        r#"<text data-node="{}" data-shader="stock::text" x="{:.2}" y="{:.2}" font-size="{:.2}" text-anchor="{}">"#,
        esc(id),
        x,
        y,
        size,
        anchor_attr,
    );
    for (text, style) in runs {
        let family = if style.mono {
            "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"
        } else {
            "Inter, ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif"
        };
        let weight_str = match style.weight {
            FontWeight::Regular => "400",
            FontWeight::Medium => "500",
            FontWeight::Semibold => "600",
            FontWeight::Bold => "700",
        };
        let style_attr = if style.italic { "italic" } else { "normal" };
        let _ = write!(
            s,
            r#"<tspan font-family="{}" font-weight="{}" font-style="{}" fill="{}">{}</tspan>"#,
            family,
            weight_str,
            style_attr,
            color_svg(style.color),
            esc(text),
        );
    }
    s.push_str("</text>\n");
}

fn as_color(v: &UniformValue) -> Option<Color> {
    match v {
        UniformValue::Color(c) => Some(*c),
        _ => None,
    }
}
fn as_f32(v: &UniformValue) -> Option<f32> {
    match v {
        UniformValue::F32(f) => Some(*f),
        _ => None,
    }
}

// Explicit feMerge form instead of the convenient `feDropShadow`. Both
// render the same shadow visually, but rsvg-convert's feDropShadow
// silently round-trips SourceGraphic through linearRGB even with
// color-interpolation-filters="sRGB" on the filter, drifting the
// shape's own interior by 1-2 channel codes (BG_CARD #171a21 → #161c22).
// Routing SourceGraphic through feMergeNode untouched keeps the card
// color exact while the shadow path (SourceAlpha → blur → offset →
// alpha-scale) produces the same dark falloff.
const SHADOW_DEFS: &str = r##"<defs>
  <filter id="shadow-sm" x="-20%" y="-20%" width="140%" height="140%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="SourceAlpha" stdDeviation="2"/>
    <feOffset dx="0" dy="2"/>
    <feComponentTransfer><feFuncA type="linear" slope="0.20"/></feComponentTransfer>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="shadow-md" x="-20%" y="-20%" width="140%" height="140%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="SourceAlpha" stdDeviation="6"/>
    <feOffset dx="0" dy="6"/>
    <feComponentTransfer><feFuncA type="linear" slope="0.28"/></feComponentTransfer>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="shadow-lg" x="-20%" y="-20%" width="140%" height="140%" color-interpolation-filters="sRGB">
    <feGaussianBlur in="SourceAlpha" stdDeviation="14"/>
    <feOffset dx="0" dy="12"/>
    <feComponentTransfer><feFuncA type="linear" slope="0.32"/></feComponentTransfer>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
</defs>
"##;

fn shadow_filter(shadow: f32) -> &'static str {
    if shadow >= 16.0 {
        r#" filter="url(#shadow-lg)""#
    } else if shadow >= 6.0 {
        r#" filter="url(#shadow-md)""#
    } else if shadow > 0.0 {
        r#" filter="url(#shadow-sm)""#
    } else {
        ""
    }
}

pub(crate) fn color_svg(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgba({},{},{},{:.3})", c.r, c.g, c.b, c.a as f32 / 255.0)
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
