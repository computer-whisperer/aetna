//! The [`El`] tree — the central data structure.
//!
//! An `El` is an HTML-DOM-shaped node: it has a [`Kind`] (semantic role),
//! styling, layout properties, optional text content, and zero or more
//! child `El`s. Build trees with the component constructors (`text`,
//! `button`, `card`, …) and the layout primitives (`column`, `row`,
//! `spacer`, `divider`).
//!
//! # What's different from attempt_3
//!
//! - Visual properties (`fill`, `stroke`, `radius`, `shadow`) are still
//!   on `El` for the user-facing modifier API, but at render time they
//!   resolve into [`crate::ir::DrawOp`]s bound to a stock shader
//!   ([`crate::shader::StockShader::RoundedRect`] for surfaces,
//!   [`crate::shader::StockShader::Text`] for text).
//! - [`El::shader_override`] lets a custom component bind its own shader
//!   instead of `rounded_rect` for the surface paint. v0.1 ships no
//!   custom shaders — this is the escape hatch the substrate must support.
//!
//! # Source mapping for free
//!
//! Every constructor in this crate is `#[track_caller]`, so the call site
//! is captured automatically — no `src_here!` macro at every call. The
//! source location lives in [`El::source`] and flows through to the tree
//! dump and lint artifacts the agent loop consumes.

mod color;
mod types;

use std::panic::Location;

pub use color::Color;
pub use types::{
    Align, Axis, FontWeight, InteractionState, Justify, Kind, Rect, Sides, Size, Source, TextAlign,
    TextWrap,
};

use crate::anim::Timing;
use crate::layout::{LayoutCtx, LayoutFn};
use crate::shader::ShaderBinding;
use crate::style::StyleProfile;

/// The core tree node.
///
/// Construct via the component builders (`text`, `button`, `card`,
/// `column`, …) and chain modifiers (`.padding`, `.gap`, `.fill`, …).
/// Avoid building `El` directly — the builders set polished defaults.
#[derive(Clone, Debug)]
pub struct El {
    pub kind: Kind,
    pub style_profile: StyleProfile,
    pub key: Option<String>,
    pub block_pointer: bool,
    pub focusable: bool,
    pub source: Source,

    // Layout
    pub axis: Axis,
    pub gap: f32,
    pub padding: Sides,
    pub align: Align,
    pub justify: Justify,
    pub width: Size,
    pub height: Size,

    // Visual style — these still live on `El` because the modifier API
    // (`.fill(c)`, `.radius(r)`, `.shadow(s)`) is what users type. The
    // renderer translates them into a [`ShaderBinding`] for
    // `stock::rounded_rect` (or whatever `shader_override` specifies)
    // when emitting [`crate::ir::DrawOp`]s.
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
    pub radius: f32,
    pub shadow: f32,
    /// Clip this element's own paint and descendants to its computed rect.
    /// Used by scroll panes, host-painted regions, overlays, and any region
    /// where overflow should not leak visually or receive events.
    pub clip: bool,

    /// This element is a vertical scroll viewport. The layout pass reads
    /// the offset from [`crate::state::UiState::scroll_offsets`] keyed
    /// by `computed_id`, clamps it to `[0, content_h - viewport_h]`, and
    /// writes the clamped value back. Set automatically by [`scroll`].
    pub scrollable: bool,

    /// Override the implicit `stock::rounded_rect` binding for this
    /// node's surface. v0.1 ships no users of this; it's the escape
    /// hatch a user crate uses to bind a custom shader (e.g.
    /// `liquid_glass`).
    pub shader_override: Option<ShaderBinding>,

    /// v0.5 — second escape hatch: author-supplied layout function that
    /// positions this node's direct children. When set, the layout
    /// pass calls the function instead of running its column/row/
    /// overlay distribution. The library still recurses into each
    /// child and still drives hit-test / focus / animation / scroll
    /// off the rects the function returns. See [`LayoutFn`] for the
    /// contract.
    pub layout_override: Option<LayoutFn>,

    // Text
    pub text: Option<String>,
    pub text_color: Option<Color>,
    pub text_align: TextAlign,
    pub text_wrap: TextWrap,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub font_mono: bool,

    pub children: Vec<El>,

    /// Paint-time alpha multiplier in `[0, 1]`. Default `1.0`. Multiplies
    /// the alpha channel of `fill`, `stroke`, and text colour at draw
    /// time. Layout-neutral. App-driven changes are eased when
    /// [`Self::animate`] is set.
    pub opacity: f32,
    /// Paint-time offset in logical pixels. Default `(0.0, 0.0)`.
    /// **Subtree-inheriting**: descendants paint at their computed rect
    /// plus all ancestor `translate` accumulated through the paint
    /// recursion. Use this to slide a sidebar / drawer / list-item
    /// without re-running layout. App-driven changes are eased when
    /// [`Self::animate`] is set.
    pub translate: (f32, f32),
    /// Per-node uniform scale around the computed-rect centre. Default
    /// `1.0`. Scales this node's surface quad and (if it carries text)
    /// its glyph run together. **Not** subtree-inheriting — descendants
    /// keep their own scale. Use this for tap-bounce on a button. App-
    /// driven changes are eased when [`Self::animate`] is set.
    pub scale: f32,
    /// Opt-in app-driven prop interpolation. When `Some(timing)`, the
    /// animation tracker eases `fill` / `text_color` / `stroke` /
    /// `opacity` / `translate` / `scale` between rebuilds — the value
    /// the build closure produces becomes the spring/tween target;
    /// `current` carries over from last frame. State visuals (hover /
    /// press / focus ring) keep their own library defaults regardless.
    pub animate: Option<Timing>,

    /// Stable path-based ID, filled by the layout pass. Used as the
    /// key for every side map that holds per-node bookkeeping in
    /// [`crate::state::UiState`] — computed rects, interaction state,
    /// state-envelope amounts, scroll offsets, in-flight animations.
    pub computed_id: String,
}

impl Default for El {
    fn default() -> Self {
        Self {
            kind: Kind::Group,
            style_profile: StyleProfile::TextOnly,
            key: None,
            block_pointer: false,
            focusable: false,
            source: Source::default(),
            axis: Axis::Overlay,
            gap: 0.0,
            padding: Sides::zero(),
            align: Align::Stretch,
            justify: Justify::Start,
            width: Size::Fill(1.0),
            height: Size::Fill(1.0),
            fill: None,
            stroke: None,
            stroke_width: 0.0,
            radius: 0.0,
            shadow: 0.0,
            clip: false,
            scrollable: false,
            shader_override: None,
            layout_override: None,
            text: None,
            text_color: None,
            text_align: TextAlign::Start,
            text_wrap: TextWrap::NoWrap,
            font_size: crate::tokens::FONT_BASE,
            font_weight: FontWeight::Regular,
            font_mono: false,
            children: Vec::new(),
            opacity: 1.0,
            translate: (0.0, 0.0),
            scale: 1.0,
            animate: None,
            computed_id: String::new(),
        }
    }
}

impl El {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            ..Default::default()
        }
    }

    // ---- Identity / source ----
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = Some(k.into());
        self
    }
    pub fn block_pointer(mut self) -> Self {
        self.block_pointer = true;
        self
    }
    pub fn focusable(mut self) -> Self {
        self.focusable = true;
        self
    }
    pub fn at(mut self, file: &'static str, line: u32) -> Self {
        self.source = Source { file, line };
        self
    }
    /// Set source from a `Location` (used internally by `#[track_caller]` constructors).
    pub fn at_loc(mut self, loc: &'static Location<'static>) -> Self {
        self.source = Source::from_caller(loc);
        self
    }

    // ---- Sizing ----
    pub fn width(mut self, w: Size) -> Self {
        self.width = w;
        self
    }
    pub fn height(mut self, h: Size) -> Self {
        self.height = h;
        self
    }
    pub fn hug(mut self) -> Self {
        self.width = Size::Hug;
        self.height = Size::Hug;
        self
    }
    pub fn fill_size(mut self) -> Self {
        self.width = Size::Fill(1.0);
        self.height = Size::Fill(1.0);
        self
    }

    // ---- Layout (container) ----
    pub fn padding(mut self, p: impl Into<Sides>) -> Self {
        self.padding = p.into();
        self
    }
    pub fn gap(mut self, g: f32) -> Self {
        self.gap = g;
        self
    }
    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }
    pub fn justify(mut self, j: Justify) -> Self {
        self.justify = j;
        self
    }

    // ---- Visual ----
    pub fn fill(mut self, c: Color) -> Self {
        self.fill = Some(c);
        self
    }
    pub fn stroke(mut self, c: Color) -> Self {
        self.stroke = Some(c);
        if self.stroke_width == 0.0 {
            self.stroke_width = 1.0;
        }
        self
    }
    pub fn stroke_width(mut self, w: f32) -> Self {
        self.stroke_width = w;
        self
    }
    pub fn radius(mut self, r: f32) -> Self {
        self.radius = r;
        self
    }
    pub fn shadow(mut self, s: f32) -> Self {
        self.shadow = s;
        self
    }
    pub fn clip(mut self) -> Self {
        self.clip = true;
        self
    }
    pub fn scrollable(mut self) -> Self {
        self.scrollable = true;
        self
    }

    // ---- Paint-time transforms (animatable via `.animate()`) ----
    /// Multiply this element's paint alpha by `v` (clamped to `[0, 1]`).
    /// Layout-neutral. Multiplies onto `fill`, `stroke`, and text colour
    /// at paint time.
    pub fn opacity(mut self, v: f32) -> Self {
        self.opacity = v.clamp(0.0, 1.0);
        self
    }
    /// Offset this element's paint and its descendants by `(x, y)` in
    /// logical pixels. Layout-neutral; descendants inherit the offset.
    pub fn translate(mut self, x: f32, y: f32) -> Self {
        self.translate = (x, y);
        self
    }
    /// Uniformly scale this element's paint around its rect centre.
    /// Affects the surface quad and (if it carries text) the glyph
    /// run together. Not subtree-inheriting.
    pub fn scale(mut self, v: f32) -> Self {
        self.scale = v.max(0.0);
        self
    }
    /// Opt this element into app-driven prop interpolation. When the
    /// build closure produces a different value for `fill` /
    /// `text_color` / `stroke` / `opacity` / `translate` / `scale`
    /// between rebuilds, the library eases from the prior frame's
    /// value to the new value using `timing`. State visuals (hover /
    /// press / focus) remain on the library's own timing.
    pub fn animate(mut self, timing: Timing) -> Self {
        self.animate = Some(timing);
        self
    }

    /// Bind a shader for the surface paint, replacing the implicit
    /// `stock::rounded_rect`. The element's `fill`/`stroke`/`radius`/
    /// `shadow` fields are ignored when this is set; the shader receives
    /// only the uniforms in the binding.
    pub fn shader(mut self, binding: ShaderBinding) -> Self {
        self.shader_override = Some(binding);
        self
    }

    /// v0.5 — replace the column/row/overlay distribution for this
    /// node with `f`. The function receives a [`LayoutCtx`] (container
    /// rect, children, intrinsic-measure callback) and returns one
    /// [`Rect`] per child in source order. The node itself must size
    /// with `Fixed` or `Fill` on both axes — `Hug` is not supported
    /// for custom-layout nodes in this slice.
    pub fn layout<F>(mut self, f: F) -> Self
    where
        F: Fn(LayoutCtx) -> Vec<Rect> + Send + Sync + 'static,
    {
        self.layout_override = Some(LayoutFn::new(f));
        self
    }

    // ---- Text-bearing ----
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = Some(t.into());
        self
    }
    pub fn text_color(mut self, c: Color) -> Self {
        self.text_color = Some(c);
        self
    }
    pub fn text_align(mut self, align: TextAlign) -> Self {
        self.text_align = align;
        self
    }
    pub fn center_text(self) -> Self {
        self.text_align(TextAlign::Center)
    }
    pub fn end_text(self) -> Self {
        self.text_align(TextAlign::End)
    }
    pub fn text_wrap(mut self, wrap: TextWrap) -> Self {
        self.text_wrap = wrap;
        self
    }
    pub fn wrap_text(self) -> Self {
        self.text_wrap(TextWrap::Wrap)
    }
    pub fn nowrap_text(self) -> Self {
        self.text_wrap(TextWrap::NoWrap)
    }
    pub fn font_size(mut self, s: f32) -> Self {
        self.font_size = s;
        self
    }
    pub fn font_weight(mut self, w: FontWeight) -> Self {
        self.font_weight = w;
        self
    }
    pub fn mono(mut self) -> Self {
        self.font_mono = true;
        self
    }

    // ---- Children ----
    pub fn child(mut self, c: impl Into<El>) -> Self {
        self.children.push(c.into());
        self
    }
    pub fn children<I, E>(mut self, cs: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        self.children.extend(cs.into_iter().map(Into::into));
        self
    }

    // ---- Internal: style profile ----
    pub fn style_profile(mut self, p: StyleProfile) -> Self {
        self.style_profile = p;
        self
    }

    // ---- Internal: axis (used by layout primitives below) ----
    pub(crate) fn axis(mut self, a: Axis) -> Self {
        self.axis = a;
        self
    }
}

// ---------- Layout primitives (plain functions) ----------

/// A vertical container with a comfortable default gap.
#[track_caller]
pub fn column<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .gap(crate::tokens::SPACE_MD)
        .align(Align::Stretch)
        .axis(Axis::Column)
}

/// A horizontal container with a comfortable default gap, vertically
/// centered. Defaults to hugging height — override with
/// `.height(Size::Fill(1.0))` if you want it to claim leftover space.
#[track_caller]
pub fn row<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .gap(crate::tokens::SPACE_SM)
        .align(Align::Center)
        .height(Size::Hug)
        .axis(Axis::Row)
}

/// An overlay stack — children share the parent's rect.
#[track_caller]
pub fn stack<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Overlay)
}

/// A vertical scroll viewport. Children stack as in [`column`]; the
/// container clips overflow and translates content by the current scroll
/// offset. Wheel events over the viewport update the offset.
///
/// Give it a `.key("...")` so the offset persists by name across
/// rebuilds — without a key, the offset is keyed by sibling index and
/// resets if structure shifts.
#[track_caller]
pub fn scroll<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Scroll)
        .at_loc(Location::caller())
        .children(children)
        .gap(crate::tokens::SPACE_MD)
        .align(Align::Stretch)
        .axis(Axis::Column)
        .clip()
        .scrollable()
}

/// A `Fill(1)` filler. Inside a `row` it pushes siblings to the right;
/// inside a `column` it pushes siblings to the bottom.
#[track_caller]
pub fn spacer() -> El {
    El::new(Kind::Spacer)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}

/// A 1-pixel separator line.
#[track_caller]
pub fn divider() -> El {
    El::new(Kind::Divider)
        .at_loc(Location::caller())
        .height(Size::Fixed(1.0))
        .width(Size::Fill(1.0))
        .fill(crate::tokens::BORDER)
}

// ---------- &str → El convenience ----------
//
// Lets `card("Title", ["a body line"])` work without `text(...)`.

impl From<&str> for El {
    fn from(s: &str) -> Self {
        crate::text::text(s)
    }
}
impl From<String> for El {
    fn from(s: String) -> Self {
        crate::text::text(s)
    }
}
impl From<&String> for El {
    fn from(s: &String) -> Self {
        crate::text::text(s.as_str())
    }
}
