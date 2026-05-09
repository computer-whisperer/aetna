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

use crate::image::{Image, ImageFit};
use crate::shader::{ShaderHandle, UniformBlock};
use crate::svg_icon::IconSource;
use crate::text::atlas::RunStyle;
use crate::text::metrics::TextLayout;
use crate::tree::{Color, FontFamily, FontWeight, Rect, TextWrap};

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
        line_height: f32,
        family: FontFamily,
        /// Monospace face used when `mono` is set. Stamped from the
        /// source El's `mono_font_family` (themed via
        /// `Theme::mono_font_family`).
        mono_family: FontFamily,
        weight: FontWeight,
        mono: bool,
        wrap: TextWrap,
        anchor: TextAnchor,
        layout: TextLayout,
        /// Underline / strikethrough state lifted from the source El's
        /// `text_underline` / `text_strikethrough`. Backends fold them
        /// into the synthesized [`RunStyle`] before shaping so the
        /// decoration pass in [`crate::text::atlas`] runs uniformly
        /// for standalone leaves and attributed paragraphs.
        underline: bool,
        strikethrough: bool,
        /// Optional link URL from the El's `text_link`. Carried for
        /// future hit-test work; today it just pins color + underline
        /// via [`RunStyle::with_link`].
        link: Option<String>,
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
        line_height: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        layout: TextLayout,
    },
    /// A vector icon scaled into `rect`. The `source` is either a
    /// built-in [`crate::tree::IconName`] (24x24 lucide-style) or an
    /// app-supplied [`crate::SvgIcon`]. SVG bundle output renders the
    /// vector paths directly; wgpu/vulkano backends bake an MTSDF (or
    /// tessellate for non-flat materials); backends without a native
    /// vector painter fall back to a glyph for built-ins.
    Icon {
        id: String,
        rect: Rect,
        scissor: Option<Rect>,
        source: IconSource,
        color: Color,
        size: f32,
        stroke_width: f32,
    },
    /// A raster image painted into `rect`. The `image` carries the
    /// pixel data (Arc-shared with the source El) and is keyed by
    /// `image.content_hash()` in backend texture caches. `rect` is the
    /// post-`fit` destination; for `Cover` it can extend past the El's
    /// content area and is clipped via `scissor`. SVG bundle output
    /// emits a placeholder rect labelled with the image's hash.
    Image {
        id: String,
        rect: Rect,
        scissor: Option<Rect>,
        image: Image,
        tint: Option<Color>,
        radius: f32,
        fit: ImageFit,
    },
    /// An app-owned GPU texture composited into the paint stream.
    /// Unlike `DrawOp::Image`, the backend does not upload pixels —
    /// it samples the existing texture identified by `texture` during
    /// paint, keying its bind-group cache on
    /// [`crate::surface::AppTextureId`]. `rect` is the resolved widget
    /// box; the texture fills it 1:1 (no per-rect projection in 0.3.x).
    /// `alpha` selects the blend path. SVG bundle output emits a
    /// placeholder rect labelled with the texture's id.
    AppTexture {
        id: String,
        rect: Rect,
        scissor: Option<Rect>,
        texture: crate::surface::AppTexture,
        alpha: crate::surface::SurfaceAlpha,
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
            | DrawOp::Icon { id, .. }
            | DrawOp::Image { id, .. }
            | DrawOp::AppTexture { id, .. } => id,
            DrawOp::BackdropSnapshot => "<backdrop-snapshot>",
        }
    }
    pub fn shader(&self) -> Option<&ShaderHandle> {
        match self {
            DrawOp::Quad { shader, .. }
            | DrawOp::GlyphRun { shader, .. }
            | DrawOp::AttributedText { shader, .. } => Some(shader),
            DrawOp::Icon { .. } | DrawOp::Image { .. } | DrawOp::AppTexture { .. } => None,
            DrawOp::BackdropSnapshot => None,
        }
    }
    pub fn scissor(&self) -> Option<Rect> {
        match self {
            DrawOp::Quad { scissor, .. }
            | DrawOp::GlyphRun { scissor, .. }
            | DrawOp::AttributedText { scissor, .. }
            | DrawOp::Icon { scissor, .. }
            | DrawOp::Image { scissor, .. }
            | DrawOp::AppTexture { scissor, .. } => *scissor,
            DrawOp::BackdropSnapshot => None,
        }
    }
}
