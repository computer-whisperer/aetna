//! Backend-neutral draw-op IR.
//!
//! Every visual fact in the laid-out tree resolves to a [`DrawOp`] bound
//! to a [`ShaderHandle`] and a uniform block. The wgpu renderer dispatches
//! by shader handle; the SVG fallback (`crate::bundle::svg`) interprets stock
//! shaders best-effort and emits placeholder rects for custom ones.
//!
//! `BackdropSnapshot` is emitted by [`crate::runtime::RunnerCore`] when
//! the resolved paint stream first needs a backdrop-sampling shader. See
//! `docs/SHADER_VISION.md` for the backend contract.
//!
//! # Why DrawOp over RenderCmd
//!
//! Aetna keeps visual material decisions in shader handles and uniform blocks
//! instead of baking CSS-shaped fields into the IR. Rect colors, gradients,
//! shadows, focus rings, and glass effects resolve to stock shader uniforms or
//! custom shader bindings before a backend records GPU commands.

use crate::shader::{ShaderHandle, UniformBlock};
use crate::text::atlas::RunStyle;
use crate::text::metrics::TextLayout;
use crate::tree::{Color, FontWeight, IconName, Rect, TextWrap};

/// One paint operation in the laid-out frame.
#[derive(Clone, Debug)]
pub enum DrawOp {
    /// A rectangular region painted by a shader (typically
    /// `stock::rounded_rect`, but custom shaders also emit `Quad`).
    Quad {
        id: String,
        rect: Rect,
        scissor: Option<Rect>,
        shader: ShaderHandle,
        uniforms: UniformBlock,
    },
    /// A run of text. The draw op carries the author text and measured layout;
    /// backends shape/rasterize through the shared glyph atlas path.
    GlyphRun {
        id: String,
        rect: Rect,
        scissor: Option<Rect>,
        shader: ShaderHandle,
        /// Carried explicitly on the op for SVG fallback and backend text
        /// shaping.
        color: Color,
        text: String,
        size: f32,
        weight: FontWeight,
        mono: bool,
        wrap: TextWrap,
        anchor: TextAnchor,
        layout: TextLayout,
    },
    /// An attributed paragraph: a sequence of styled runs that flow
    /// together inside one `rect`. The runtime hands `runs` straight to
    /// [`crate::text::atlas::GlyphAtlas::shape_and_rasterize_runs`] so
    /// wrapping decisions cross run boundaries (real prose, not glued
    /// segments). `layout` is an approximate pre-shaping measurement
    /// from `text::metrics` — backends shape for accurate placement;
    /// SVG uses it to lay tspan baselines.
    AttributedText {
        id: String,
        rect: Rect,
        scissor: Option<Rect>,
        shader: ShaderHandle,
        /// Source-order styled spans. Each `String` may contain
        /// embedded `\n` to express in-paragraph hard breaks.
        runs: Vec<(String, RunStyle)>,
        size: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        layout: TextLayout,
    },
    /// A built-in vector icon in a 24x24 coordinate system, scaled into
    /// `rect`. SVG renders the vector path directly; wgpu backends use
    /// tessellated SVG geometry; backends without a native vector icon
    /// painter may fall back to a glyph.
    Icon {
        id: String,
        rect: Rect,
        scissor: Option<Rect>,
        name: IconName,
        color: Color,
        size: f32,
        stroke_width: f32,
    },
    /// Mid-frame snapshot of the current target into a sampled texture,
    /// scheduled before any backdrop-sampling pass.
    BackdropSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAnchor {
    Start,
    Middle,
    End,
}

impl DrawOp {
    pub fn id(&self) -> &str {
        match self {
            DrawOp::Quad { id, .. }
            | DrawOp::GlyphRun { id, .. }
            | DrawOp::AttributedText { id, .. }
            | DrawOp::Icon { id, .. } => id,
            DrawOp::BackdropSnapshot => "<backdrop-snapshot>",
        }
    }
    pub fn shader(&self) -> Option<&ShaderHandle> {
        match self {
            DrawOp::Quad { shader, .. }
            | DrawOp::GlyphRun { shader, .. }
            | DrawOp::AttributedText { shader, .. } => Some(shader),
            DrawOp::Icon { .. } => None,
            DrawOp::BackdropSnapshot => None,
        }
    }
    pub fn scissor(&self) -> Option<Rect> {
        match self {
            DrawOp::Quad { scissor, .. }
            | DrawOp::GlyphRun { scissor, .. }
            | DrawOp::AttributedText { scissor, .. }
            | DrawOp::Icon { scissor, .. } => *scissor,
            DrawOp::BackdropSnapshot => None,
        }
    }
}
