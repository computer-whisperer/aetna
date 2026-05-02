//! Render commands and the SVG backend.
//!
//! After [`crate::layout::layout`] has run, walk the tree and emit a flat
//! list of [`RenderCmd`]s. Backends consume that list:
//!
//! - [`render_svg`] in this module — text-based fixture output.
//! - (later) wgpu / native — convert to vertex buffers.
//! - (later) HTML — backends can also render directly from the [`crate::tree::El`]
//!   tree, skipping this IR, since CSS handles layout itself.
//!
//! The IR is intentionally tiny. Anything renderable in attempt_2 today is
//! either a filled/stroked rounded rect or a text run.

use std::fmt::Write as _;

use crate::tree::*;

/// A flat draw operation. Backend-agnostic.
#[derive(Clone, Debug)]
pub enum RenderCmd {
    Rect {
        rect: Rect,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
        radius: f32,
        shadow: f32,
    },
    Text {
        rect: Rect,
        text: String,
        color: Color,
        size: f32,
        weight: FontWeight,
        mono: bool,
        /// Horizontal anchor inside `rect`.
        anchor: TextAnchor,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

/// Walk the laid-out tree and emit render commands in paint order.
pub fn render_commands(root: &El) -> Vec<RenderCmd> {
    let mut out = Vec::new();
    push_node(root, &mut out);
    out
}

fn push_node(n: &El, out: &mut Vec<RenderCmd>) {
    if n.fill.is_some() || n.stroke.is_some() {
        out.push(RenderCmd::Rect {
            rect: n.computed,
            fill: n.fill,
            stroke: n.stroke,
            stroke_width: n.stroke_width,
            radius: n.radius,
            shadow: n.shadow,
        });
    }
    if let Some(text) = &n.text {
        let color = n.text_color.unwrap_or_else(|| crate::theme::theme().text.foreground);
        let anchor = match n.kind {
            // Buttons and badges center their label in the rect; everything
            // else uses left-aligned text. Buttons/badges already size their
            // rect to wrap the label, so centering keeps the text inside
            // even when those elements are stretched (e.g. a Fill-width button).
            Kind::Button | Kind::Badge => TextAnchor::Middle,
            _ => TextAnchor::Start,
        };
        out.push(RenderCmd::Text {
            rect: n.computed,
            text: text.clone(),
            color,
            size: n.font_size,
            weight: n.font_weight,
            mono: n.font_mono,
            anchor,
        });
    }
    for c in &n.children {
        push_node(c, out);
    }
}

// ---------- SVG backend ----------

/// Render the laid-out tree to an SVG string.
///
/// `bg` defaults to the active theme's app background.
pub fn render_svg(root: &El, width: f32, height: f32) -> String {
    let cmds = render_commands(root);
    svg_from_commands(width, height, &cmds, crate::theme::theme().bg.app)
}

/// Lower-level: render an explicit list of commands. Useful for backends
/// that produce render commands by other means.
pub fn svg_from_commands(width: f32, height: f32, cmds: &[RenderCmd], bg: Color) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    let _ = writeln!(s, r##"<defs>
  <filter id="shadow-sm" x="-20%" y="-20%" width="140%" height="140%">
    <feDropShadow dx="0" dy="2" stdDeviation="2" flood-color="#000" flood-opacity="0.20"/>
  </filter>
  <filter id="shadow-md" x="-20%" y="-20%" width="140%" height="140%">
    <feDropShadow dx="0" dy="6" stdDeviation="6" flood-color="#000" flood-opacity="0.28"/>
  </filter>
  <filter id="shadow-lg" x="-20%" y="-20%" width="140%" height="140%">
    <feDropShadow dx="0" dy="12" stdDeviation="14" flood-color="#000" flood-opacity="0.32"/>
  </filter>
</defs>"##);
    let _ = writeln!(s, r#"<rect width="100%" height="100%" fill="{}"/>"#, color_svg(bg));

    for cmd in cmds {
        match cmd {
            RenderCmd::Rect { rect, fill, stroke, stroke_width, radius, shadow } => {
                let filter = if *shadow >= 16.0 {
                    r#" filter="url(#shadow-lg)""#
                } else if *shadow >= 6.0 {
                    r#" filter="url(#shadow-md)""#
                } else if *shadow > 0.0 {
                    r#" filter="url(#shadow-sm)""#
                } else { "" };

                let fill_attr = match fill {
                    Some(c) => format!(r#" fill="{}""#, color_svg(*c)),
                    None => r#" fill="none""#.to_string(),
                };
                let stroke_attr = match stroke {
                    Some(c) => format!(r#" stroke="{}" stroke-width="{:.2}""#, color_svg(*c), stroke_width),
                    None => String::new(),
                };
                let r = radius.min(rect.w * 0.5).min(rect.h * 0.5).max(0.0);
                let _ = writeln!(
                    s,
                    r#"<rect x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}"{}{}{} />"#,
                    rect.x, rect.y, rect.w, rect.h, r, fill_attr, stroke_attr, filter
                );
            }
            RenderCmd::Text { rect, text, color, size, weight, mono, anchor } => {
                let (x, anchor_attr) = match anchor {
                    TextAnchor::Start => (rect.x + 2.0, "start"),
                    TextAnchor::Middle => (rect.center_x(), "middle"),
                    TextAnchor::End => (rect.right() - 2.0, "end"),
                };
                let y = rect.center_y() + size * 0.34; // baseline approximation
                let family = if *mono {
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
                let _ = writeln!(
                    s,
                    r#"<text x="{:.2}" y="{:.2}" font-family="{}" font-size="{:.2}" font-weight="{}" fill="{}" text-anchor="{}">{}</text>"#,
                    x, y, family, size, weight_str, color_svg(*color), anchor_attr, escape_xml(text)
                );
            }
        }
    }
    s.push_str("</svg>\n");
    s
}

fn color_svg(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("rgba({},{},{},{:.3})", c.r, c.g, c.b, c.a as f32 / 255.0)
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
