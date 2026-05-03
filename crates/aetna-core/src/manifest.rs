//! Shader manifest — a textual artifact listing every shader used by a
//! frame's [`DrawOp`] list, with usage counts and resolved uniform values.
//!
//! Part of the agent loop's feedback bundle. The LLM uses this to:
//!
//! - See which stock shader is painting a node it cares about.
//! - See the resolved uniform values per draw (e.g. `fill=primary`,
//!   `radius=12.00`).
//! - Notice when a custom shader is requested but not registered.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::ir::*;

/// Render the shader manifest for an ops list to a string.
pub fn shader_manifest(ops: &[DrawOp]) -> String {
    // group ops by shader handle for the summary; preserve original
    // order under each shader.
    let mut by_shader: BTreeMap<String, Vec<&DrawOp>> = BTreeMap::new();
    for op in ops {
        if let Some(handle) = op.shader() {
            by_shader.entry(handle.name()).or_default().push(op);
        }
    }

    let mut s = String::new();
    if by_shader.is_empty() {
        s.push_str("no shaders\n");
        return s;
    }
    for (name, group) in &by_shader {
        let _ = writeln!(s, "{name}  used {} times", group.len());
        for op in group {
            match op {
                DrawOp::Quad { id, uniforms, .. } => {
                    let _ = write!(s, "  {id}");
                    for (k, v) in uniforms {
                        let _ = write!(s, " {k}={}", v.debug_short());
                    }
                    s.push('\n');
                }
                DrawOp::GlyphRun {
                    id,
                    color,
                    text,
                    size,
                    weight,
                    mono,
                    wrap,
                    anchor,
                    ..
                } => {
                    let preview: String = text.chars().take(28).collect();
                    let suffix = if text.chars().count() > 28 { "…" } else { "" };
                    let _ = write!(
                        s,
                        "  {id} text=\"{preview}{suffix}\" color={} size={size:.1} weight={weight:?} mono={mono} wrap={wrap:?} anchor={anchor:?}",
                        color_label(*color),
                    );
                    s.push('\n');
                }
                DrawOp::BackdropSnapshot => {}
            }
        }
        s.push('\n');
    }
    s
}

/// Render the raw draw-op list for inspection. The wgpu backend consumes
/// the same `Vec<DrawOp>`; this is the textual form for the agent loop.
pub fn draw_ops_text(ops: &[DrawOp]) -> String {
    let mut s = String::new();
    for op in ops {
        match op {
            DrawOp::Quad {
                id,
                rect,
                scissor,
                shader,
                uniforms,
            } => {
                let _ = write!(
                    s,
                    "Quad   shader={:<24} rect=({:.0},{:.0},{:.0},{:.0}) id={id}",
                    shader.name(),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                );
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                for (k, v) in uniforms {
                    let _ = write!(s, " {k}={}", v.debug_short());
                }
                s.push('\n');
            }
            DrawOp::GlyphRun {
                id,
                rect,
                scissor,
                shader,
                color,
                text,
                size,
                weight,
                mono,
                wrap,
                anchor,
            } => {
                let preview: String = text.chars().take(40).collect();
                let suffix = if text.chars().count() > 40 { "…" } else { "" };
                let _ = write!(
                    s,
                    "Glyph  shader={:<24} rect=({:.0},{:.0},{:.0},{:.0}) id={id} text=\"{preview}{suffix}\" color={} size={size:.1} weight={weight:?} mono={mono} wrap={wrap:?} anchor={anchor:?}",
                    shader.name(),
                    rect.x,
                    rect.y,
                    rect.w,
                    rect.h,
                    color_label(*color),
                );
                if let Some(sci) = scissor {
                    write_scissor(&mut s, *sci);
                }
                s.push('\n');
            }
            DrawOp::BackdropSnapshot => {
                let _ = writeln!(s, "BackdropSnapshot");
            }
        }
    }
    s
}

fn color_label(c: crate::tree::Color) -> String {
    match c.token {
        Some(name) => name.to_string(),
        None => format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a),
    }
}

fn write_scissor(s: &mut String, scissor: crate::tree::Rect) {
    let _ = write!(
        s,
        " scissor=({:.0},{:.0},{:.0},{:.0})",
        scissor.x, scissor.y, scissor.w, scissor.h,
    );
}
