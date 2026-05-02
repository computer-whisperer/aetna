//! Render commands and the SVG backend.
//!
//! After [`crate::layout::layout`], walk the tree and emit a flat
//! [`Vec<RenderCmd>`]. Backends consume that list:
//!
//! - [`render_svg`] — text-based fixture output (for the agent loop).
//! - (later) wgpu/vulkano — convert to vertex buffers.
//!
//! State styling lives here. The tree carries `state: InteractionState`;
//! when emitting commands we apply visual deltas:
//!
//! - `Hover`    — lighten fill, brighten text.
//! - `Press`    — darken fill, slight shadow reduction.
//! - `Focus`    — add a ring stroke just inside the rect.
//! - `Disabled` — multiply alpha by `tokens::DISABLED_ALPHA`.
//! - `Loading`  — append " ⋯" to text and dim slightly.
//!
//! Keeping state styling in the render path (rather than baked into the
//! tree) means the same `El` tree can be rendered for any state without
//! mutating the authored structure.

use std::fmt::Write as _;

use crate::tokens;
use crate::tree::*;

/// A flat draw operation. Backend-agnostic.
#[derive(Clone, Debug)]
pub enum RenderCmd {
    Rect {
        id: String,
        rect: Rect,
        fill: Option<Color>,
        stroke: Option<Color>,
        stroke_width: f32,
        radius: f32,
        shadow: f32,
    },
    Text {
        id: String,
        rect: Rect,
        text: String,
        color: Color,
        size: f32,
        weight: FontWeight,
        mono: bool,
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
    let (fill, stroke, text_color, font_weight, extra_text) = apply_state(n);

    if fill.is_some() || stroke.is_some() {
        out.push(RenderCmd::Rect {
            id: n.computed_id.clone(),
            rect: n.computed,
            fill,
            stroke,
            stroke_width: n.stroke_width,
            radius: n.radius,
            shadow: n.shadow,
        });
    }

    // Focus ring: an extra stroke just inside the rect when focused.
    if matches!(n.state, InteractionState::Focus)
        && (matches!(n.kind, Kind::Button | Kind::Card | Kind::Badge | Kind::Custom(_))
            || stroke.is_some())
    {
        out.push(RenderCmd::Rect {
            id: format!("{}.focus-ring", n.computed_id),
            rect: inset_rect(n.computed, -tokens::FOCUS_RING_WIDTH * 0.5),
            fill: None,
            stroke: Some(tokens::FOCUS_RING),
            stroke_width: tokens::FOCUS_RING_WIDTH,
            radius: n.radius + tokens::FOCUS_RING_WIDTH * 0.5,
            shadow: 0.0,
        });
    }

    if let Some(text) = &n.text {
        let display = match extra_text {
            Some(suffix) => format!("{text}{suffix}"),
            None => text.clone(),
        };
        let anchor = match n.kind {
            Kind::Button | Kind::Badge => TextAnchor::Middle,
            _ => TextAnchor::Start,
        };
        out.push(RenderCmd::Text {
            id: n.computed_id.clone(),
            rect: n.computed,
            text: display,
            color: text_color.unwrap_or(tokens::TEXT_FOREGROUND),
            size: n.font_size,
            weight: font_weight,
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

// ---------- SVG backend ----------

/// Render the laid-out tree to an SVG string at the given viewport size.
pub fn render_svg(root: &El, width: f32, height: f32) -> String {
    let cmds = render_commands(root);
    svg_from_commands(width, height, &cmds, tokens::BG_APP)
}

/// Lower-level: render an explicit list of commands.
pub fn svg_from_commands(width: f32, height: f32, cmds: &[RenderCmd], bg: Color) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    s.push_str(SHADOW_DEFS);
    let _ = writeln!(s, r#"<rect width="100%" height="100%" fill="{}"/>"#, color_svg(bg));

    for cmd in cmds {
        match cmd {
            RenderCmd::Rect { id, rect, fill, stroke, stroke_width, radius, shadow } => {
                let filter = shadow_filter(*shadow);
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
                    r#"<rect data-node="{}" x="{:.2}" y="{:.2}" width="{:.2}" height="{:.2}" rx="{:.2}"{}{}{} />"#,
                    esc(id), rect.x, rect.y, rect.w, rect.h, r, fill_attr, stroke_attr, filter
                );
            }
            RenderCmd::Text { id, rect, text, color, size, weight, mono, anchor } => {
                let (x, anchor_attr) = match anchor {
                    TextAnchor::Start => (rect.x + 2.0, "start"),
                    TextAnchor::Middle => (rect.center_x(), "middle"),
                    TextAnchor::End => (rect.right() - 2.0, "end"),
                };
                let y = rect.center_y() + size * 0.34;
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
                    r#"<text data-node="{}" x="{:.2}" y="{:.2}" font-family="{}" font-size="{:.2}" font-weight="{}" fill="{}" text-anchor="{}">{}</text>"#,
                    esc(id), x, y, family, size, weight_str, color_svg(*color), anchor_attr, esc(text)
                );
            }
        }
    }
    s.push_str("</svg>\n");
    s
}

const SHADOW_DEFS: &str = r##"<defs>
  <filter id="shadow-sm" x="-20%" y="-20%" width="140%" height="140%">
    <feDropShadow dx="0" dy="2" stdDeviation="2" flood-color="#000" flood-opacity="0.20"/>
  </filter>
  <filter id="shadow-md" x="-20%" y="-20%" width="140%" height="140%">
    <feDropShadow dx="0" dy="6" stdDeviation="6" flood-color="#000" flood-opacity="0.28"/>
  </filter>
  <filter id="shadow-lg" x="-20%" y="-20%" width="140%" height="140%">
    <feDropShadow dx="0" dy="12" stdDeviation="14" flood-color="#000" flood-opacity="0.32"/>
  </filter>
</defs>
"##;

fn shadow_filter(shadow: f32) -> &'static str {
    if shadow >= 16.0 { r#" filter="url(#shadow-lg)""# }
    else if shadow >= 6.0 { r#" filter="url(#shadow-md)""# }
    else if shadow > 0.0 { r#" filter="url(#shadow-sm)""# }
    else { "" }
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
