//! Draw-op IR — gpu-shaped, replaces attempt_3's `RenderCmd::Rect/Text`.
//!
//! Every visual fact in the laid-out tree resolves to a [`DrawOp`] bound
//! to a [`ShaderHandle`] and a uniform block. The wgpu renderer dispatches
//! by shader handle; the SVG fallback (`crate::svg`) interprets stock
//! shaders best-effort and emits placeholder rects for custom ones.
//!
//! `BackdropSnapshot` is a v2 placeholder — committed in the architecture
//! (see `SHADER_VISION.md` §"Backdrop sampling architecture") but not
//! emitted by the v0.1 renderer.
//!
//! # Why DrawOp over RenderCmd
//!
//! attempt_3's `RenderCmd::Rect { fill, stroke, radius, shadow }` was a
//! backend-portable least-common-denominator — every visual property had
//! to compile down to *something* on every target. Dropping the
//! abstract-portability constraint (we target wgpu/vulkano only) lets the
//! IR mirror what the GPU actually consumes: a shader handle + a uniform
//! block, dispatched into a render pass. CSS-style concerns (gradients,
//! shadows, frosted glass, custom shapes) become uniforms on stock
//! shaders or full custom shaders.

use crate::shader::{ShaderHandle, UniformBlock};
use crate::text_atlas::RunStyle;
use crate::text_metrics::TextLayout;
use crate::tree::{Color, FontWeight, Rect, TextWrap};

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
    /// A run of text. v0.1 emits the unshaped string + font properties;
    /// glyph shaping happens at render time. v0.2 will replace `text`
    /// with a pre-shaped `Arc<[GlyphInstance]>`.
    GlyphRun {
        id: String,
        rect: Rect,
        scissor: Option<Rect>,
        shader: ShaderHandle,
        /// Carried explicitly on the op for the SVG fallback's
        /// convenience; will move to uniforms when wgpu lands.
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
    /// [`crate::text_atlas::GlyphAtlas::shape_and_rasterize_runs`] so
    /// wrapping decisions cross run boundaries (real prose, not glued
    /// segments). `layout` is an approximate pre-shaping measurement
    /// from `text_metrics` — backends shape for accurate placement;
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
    /// Mid-frame snapshot of the current target into a sampled texture,
    /// scheduled before any backdrop-sampling pass. Reserved for v2.
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
            | DrawOp::AttributedText { id, .. } => id,
            DrawOp::BackdropSnapshot => "<backdrop-snapshot>",
        }
    }
    pub fn shader(&self) -> Option<&ShaderHandle> {
        match self {
            DrawOp::Quad { shader, .. }
            | DrawOp::GlyphRun { shader, .. }
            | DrawOp::AttributedText { shader, .. } => Some(shader),
            DrawOp::BackdropSnapshot => None,
        }
    }
    pub fn scissor(&self) -> Option<Rect> {
        match self {
            DrawOp::Quad { scissor, .. }
            | DrawOp::GlyphRun { scissor, .. }
            | DrawOp::AttributedText { scissor, .. } => *scissor,
            DrawOp::BackdropSnapshot => None,
        }
    }
}
