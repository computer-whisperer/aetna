//! Glyphon-backed text rendering for `stock::text_sdf`.
//!
//! This module is the wgpu/glyphon-shaped half of text. The
//! backend-agnostic `cosmic-text` layout half lives in `aetna-core` (or
//! will, after v5.1's text decouple). For v5.0 we still consume glyphon
//! directly here; the structure of this module makes it easy to lift the
//! layout step out later.

use glyphon::cosmic_text::Align;
use glyphon::{
    Attrs, Buffer, Color as GlyphColor, Family, FontSystem, Metrics, Shaping, TextBounds,
    TextRenderer, Weight,
};

use aetna_core::ir::TextAnchor;
use aetna_core::tree::{Color, FontWeight, Rect, TextWrap};

use crate::instance::PhysicalScissor;

pub(crate) struct TextLayer {
    pub renderer: TextRenderer,
    pub scissor: Option<PhysicalScissor>,
}

#[derive(Clone, Copy)]
pub(crate) struct TextMeta {
    pub left: f32,
    pub top: f32,
    pub color: GlyphColor,
    pub bounds: TextBounds,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_text_buffer(
    font_system: &mut FontSystem,
    rect: Rect,
    scissor: Option<Rect>,
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    anchor: TextAnchor,
    color: Color,
    scale: f32,
) -> (Buffer, TextMeta) {
    // All text quantities are pre-multiplied to physical pixels here so
    // glyphon rasterizes at native device DPI (crisp text on HiDPI).
    let physical_size = size * scale;
    let physical_line_height = physical_size * 1.4;
    let metrics = Metrics::new(physical_size, physical_line_height);
    let mut buffer = Buffer::new(font_system, metrics);

    // Buffer width drives cosmic-text wrapping AND alignment. For
    // Middle/End anchors we need a known width so the alignment math
    // works. For Start anchors, constraining width to a too-tight
    // intrinsic rect causes silent wrapping ("Theme" → "Them" + "e"
    // on a hidden second line); leave width unbounded.
    let buffer_width = match (wrap, anchor) {
        (TextWrap::Wrap, _) => Some(rect.w * scale),
        (TextWrap::NoWrap, TextAnchor::Start) => None,
        (TextWrap::NoWrap, TextAnchor::Middle | TextAnchor::End) => Some(rect.w * scale),
    };
    buffer.set_size(
        font_system,
        buffer_width,
        Some((rect.h * scale).max(physical_line_height)),
    );

    // Use bundled Roboto for sans-serif so typography is consistent
    // regardless of what fonts the host has installed. fontdb resolves
    // Name("Roboto") to whichever weight matches the request.
    let family = if mono {
        Family::Monospace
    } else {
        Family::Name("Roboto")
    };
    let attrs = Attrs::new().family(family).weight(map_weight(weight));
    buffer.set_text(font_system, text, attrs, Shaping::Advanced);

    if let Some(align) = match anchor {
        TextAnchor::Start => None,
        TextAnchor::Middle => Some(Align::Center),
        TextAnchor::End => Some(Align::End),
    } {
        for line in buffer.lines.iter_mut() {
            line.set_align(Some(align));
        }
        buffer.shape_until_scroll(font_system, false);
    }

    // Single-line controls center text vertically. Wrapped text boxes
    // are top-aligned so additional lines flow down from the box start.
    let top_logical = match wrap {
        TextWrap::NoWrap => rect.y + ((rect.h - size * 1.4) * 0.5).max(0.0),
        TextWrap::Wrap => rect.y,
    };
    let top = top_logical * scale;
    let left = rect.x * scale;

    // v0.1: don't tightly clip text to its rect bounds — the layout's
    // intrinsic-width estimator is approximate and can be a few pixels
    // narrower than cosmic-text's actual run width. Real overflow shows
    // up in the lint pass and as visible overlap, not silent glyph
    // chopping.
    let bounds = scissor.unwrap_or(Rect::new(0.0, 0.0, 1_000_000_000.0, 1_000_000_000.0));
    let meta = TextMeta {
        left,
        top,
        color: glyphon_color(color),
        bounds: TextBounds {
            left: (bounds.x * scale).floor() as i32 - 2,
            top: (bounds.y * scale).floor() as i32 - 2,
            right: (bounds.right() * scale).ceil() as i32 + 2,
            bottom: (bounds.bottom() * scale).ceil() as i32 + 2,
        },
    };
    (buffer, meta)
}

fn map_weight(w: FontWeight) -> Weight {
    match w {
        FontWeight::Regular => Weight::NORMAL,
        FontWeight::Medium => Weight::MEDIUM,
        FontWeight::Semibold => Weight::SEMIBOLD,
        FontWeight::Bold => Weight::BOLD,
    }
}

fn glyphon_color(c: Color) -> GlyphColor {
    GlyphColor::rgba(c.r, c.g, c.b, c.a)
}
