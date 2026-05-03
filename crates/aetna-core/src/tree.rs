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
//!   [`crate::shader::StockShader::TextSdf`] for text).
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

use std::panic::Location;

use crate::anim::Timing;
use crate::shader::ShaderBinding;
use crate::style::StyleProfile;

/// A rectangle in pixel coordinates. Origin top-left, +y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self { Self { x, y, w, h } }
    pub fn right(self) -> f32 { self.x + self.w }
    pub fn bottom(self) -> f32 { self.y + self.h }
    pub fn center_x(self) -> f32 { self.x + self.w * 0.5 }
    pub fn center_y(self) -> f32 { self.y + self.h * 0.5 }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x2 <= x1 {
            return None;
        }
        if y2 <= y1 {
            return None;
        }
        Some(Rect::new(x1, y1, x2 - x1, y2 - y1))
    }
    pub fn inset(self, p: Sides) -> Self {
        Self::new(
            self.x + p.left,
            self.y + p.top,
            (self.w - p.left - p.right).max(0.0),
            (self.h - p.top - p.bottom).max(0.0),
        )
    }
}

/// Per-side padding/inset values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sides {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Sides {
    pub const fn all(v: f32) -> Self { Self { left: v, right: v, top: v, bottom: v } }
    pub const fn xy(x: f32, y: f32) -> Self { Self { left: x, right: x, top: y, bottom: y } }
    pub const fn zero() -> Self { Self::all(0.0) }
}

impl From<f32> for Sides {
    fn from(v: f32) -> Self { Sides::all(v) }
}

/// Sizing intent along one axis. Layout uses these to allocate space
/// — pixel arithmetic should never appear in user code.
///
/// - `Fixed(px)` — exact size.
/// - `Fill(weight)` — claim a share of leftover space; weights are relative.
/// - `Hug` — intrinsic size of contents.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Fixed(f32),
    Fill(f32),
    Hug,
}

impl Default for Size {
    fn default() -> Self { Size::Fill(1.0) }
}

/// Layout direction for a container's children.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Axis {
    /// No layout — children share the parent's rect (overlay).
    #[default]
    Overlay,
    /// Stack children top-to-bottom.
    Column,
    /// Stack children left-to-right.
    Row,
}

/// Cross-axis alignment of children within a container.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Align {
    Start,
    Center,
    End,
    /// Stretch fill-sized children to the container's cross-axis extent.
    #[default]
    Stretch,
}

/// Main-axis distribution when children don't fill the container.
/// Prefer [`spacer`] for ad-hoc gaps.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

/// Font weight — the renderer maps these to font-loading or to font-weight
/// CSS / SVG attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Semibold,
    Bold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextWrap {
    #[default]
    NoWrap,
    Wrap,
}

/// A color (RGBA8) optionally tagged with the theme token it came from.
///
/// Token name has no effect on rendering — it's metadata for inspection,
/// lint, and shader-manifest output. Future render-time theme substitution
/// would key off this name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    pub token: Option<&'static str>,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a, token: None }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Self::rgba(r, g, b, 255) }
    pub const fn token(name: &'static str, r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a, token: Some(name) }
    }
    pub fn with_alpha(self, a: u8) -> Self { Self { a, ..self } }

    /// Lighten by a 0..1 factor (mix toward white).
    pub fn lighten(self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: lerp_u8(self.r, 255, t),
            g: lerp_u8(self.g, 255, t),
            b: lerp_u8(self.b, 255, t),
            ..self
        }
    }
    /// Darken by a 0..1 factor (mix toward black).
    pub fn darken(self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: lerp_u8(self.r, 0, t),
            g: lerp_u8(self.g, 0, t),
            b: lerp_u8(self.b, 0, t),
            ..self
        }
    }

    /// Linearly interpolate between two colours by `t` in `[0, 1]`.
    /// `t = 0` returns `self`, `t = 1` returns `other`. Token metadata
    /// is preserved from `self` so an interpolated token stays named.
    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: lerp_u8(self.r, other.r, t),
            g: lerp_u8(self.g, other.g, t),
            b: lerp_u8(self.b, other.b, t),
            a: lerp_u8(self.a, other.a, t),
            token: self.token,
        }
    }
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}

/// Semantic identity of an element. Roughly an HTML tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A bare layout container with no inherent visuals.
    Group,
    Card,
    Button,
    Badge,
    Text,
    Heading,
    Spacer,
    Divider,
    Overlay,
    Scrim,
    Modal,
    /// A vertically scrollable region. Renders its content with an
    /// applied scroll offset, clips overflow, and routes wheel events
    /// to update the offset.
    Scroll,
    /// Escape hatch for app-defined components.
    Custom(&'static str),
}

/// Interaction state, applied as a render-time visual delta.
///
/// Set with [`El::with_state`]. State styling lives in the renderer; the
/// tree carries the state flag and the renderer applies the appropriate
/// transformation when emitting draw ops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InteractionState {
    #[default]
    Default,
    Hover,
    Press,
    Focus,
    Disabled,
    Loading,
}

/// Recorded source location for an element. Set automatically via
/// `#[track_caller]` on every constructor.
#[derive(Clone, Copy, Debug, Default)]
pub struct Source {
    pub file: &'static str,
    pub line: u32,
}

impl Source {
    pub fn from_caller(loc: &'static Location<'static>) -> Self {
        Self { file: loc.file(), line: loc.line() }
    }
}

/// The core tree node.
///
/// Construct via the component builders (`text`, `button`, `card`,
/// `column`, …) and chain modifiers (`.padding`, `.gap`, `.fill`, …).
/// Avoid building `El` directly — the builders set polished defaults.
#[derive(Clone, Debug)]
pub struct El {
    pub kind: Kind,
    pub style_profile: StyleProfile,
    pub state: InteractionState,
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

    /// This element is a vertical scroll viewport. The layout pass measures
    /// content height ignoring `scroll_offset_y`, then translates child
    /// rects by `-scroll_offset_y` and writes the clamped offset back.
    /// Set automatically by [`scroll`]; the offset itself is owned by
    /// [`crate::event::UiState`] and applied before layout.
    pub scrollable: bool,
    /// Vertical scroll offset in logical pixels. Set by
    /// `UiState::apply_scroll_to_tree` from the per-node tracker; the
    /// layout pass clamps it to `[0, content_h - viewport_h]` and writes
    /// the clamped value back here so the renderer can persist it.
    pub scroll_offset_y: f32,

    /// Override the implicit `stock::rounded_rect` binding for this
    /// node's surface. v0.1 ships no users of this; it's the escape
    /// hatch a user crate uses to bind a custom shader (e.g.
    /// `liquid_glass`).
    pub shader_override: Option<ShaderBinding>,

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

    /// Filled by the layout pass.
    pub computed: Rect,
    /// Stable path-based ID, filled by the layout pass for inspection.
    pub computed_id: String,

    /// Focus-ring alpha for this node, in `[0, 1]`. Written by
    /// [`crate::event::UiState::tick_visual_animations`]; eases 0→1 on
    /// focus enter and 1→0 on focus leave. The renderer emits a focus
    /// ring quad iff this is > 0 and scales the ring's color alpha
    /// by it. Lets the ring fade out after focus moves elsewhere.
    pub focus_ring_alpha: f32,
    /// Hover-state visual envelope in `[0, 1]`. Written by the
    /// animation tracker. `apply_state` in `draw_ops` lerps the display
    /// fill / stroke / text-colour between the build-time value and
    /// `lighten(value, HOVER_LIGHTEN)` based on this. Storing the
    /// *amount* instead of the absolute eased colour keeps state
    /// transitions independent of mid-flight build-value changes.
    pub hover_amount: f32,
    /// Press-state envelope, mirroring [`Self::hover_amount`]. Lerps
    /// toward `darken(value, PRESS_DARKEN)`.
    pub press_amount: f32,
}

impl Default for El {
    fn default() -> Self {
        Self {
            kind: Kind::Group,
            style_profile: StyleProfile::TextOnly,
            state: InteractionState::Default,
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
            scroll_offset_y: 0.0,
            shader_override: None,
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
            computed: Rect::default(),
            computed_id: String::new(),
            focus_ring_alpha: 0.0,
            hover_amount: 0.0,
            press_amount: 0.0,
        }
    }
}

impl El {
    pub fn new(kind: Kind) -> Self { Self { kind, ..Default::default() } }

    // ---- Identity / source ----
    pub fn key(mut self, k: impl Into<String>) -> Self { self.key = Some(k.into()); self }
    pub fn block_pointer(mut self) -> Self { self.block_pointer = true; self }
    pub fn focusable(mut self) -> Self { self.focusable = true; self }
    pub fn at(mut self, file: &'static str, line: u32) -> Self {
        self.source = Source { file, line };
        self
    }
    /// Set source from a `Location` (used internally by `#[track_caller]` constructors).
    pub fn at_loc(mut self, loc: &'static Location<'static>) -> Self {
        self.source = Source::from_caller(loc);
        self
    }

    // ---- State ----
    pub fn with_state(mut self, s: InteractionState) -> Self { self.state = s; self }
    /// Convenience for fixtures that demonstrate hover/press/etc.
    pub fn hovered(self) -> Self { self.with_state(InteractionState::Hover) }
    pub fn pressed(self) -> Self { self.with_state(InteractionState::Press) }
    pub fn focused(self) -> Self { self.with_state(InteractionState::Focus) }
    pub fn disabled(self) -> Self { self.with_state(InteractionState::Disabled) }
    pub fn loading(self) -> Self { self.with_state(InteractionState::Loading) }

    // ---- Sizing ----
    pub fn width(mut self, w: Size) -> Self { self.width = w; self }
    pub fn height(mut self, h: Size) -> Self { self.height = h; self }
    pub fn hug(mut self) -> Self { self.width = Size::Hug; self.height = Size::Hug; self }
    pub fn fill_size(mut self) -> Self { self.width = Size::Fill(1.0); self.height = Size::Fill(1.0); self }

    // ---- Layout (container) ----
    pub fn padding(mut self, p: impl Into<Sides>) -> Self { self.padding = p.into(); self }
    pub fn gap(mut self, g: f32) -> Self { self.gap = g; self }
    pub fn align(mut self, a: Align) -> Self { self.align = a; self }
    pub fn justify(mut self, j: Justify) -> Self { self.justify = j; self }

    // ---- Visual ----
    pub fn fill(mut self, c: Color) -> Self { self.fill = Some(c); self }
    pub fn stroke(mut self, c: Color) -> Self {
        self.stroke = Some(c);
        if self.stroke_width == 0.0 { self.stroke_width = 1.0; }
        self
    }
    pub fn stroke_width(mut self, w: f32) -> Self { self.stroke_width = w; self }
    pub fn radius(mut self, r: f32) -> Self { self.radius = r; self }
    pub fn shadow(mut self, s: f32) -> Self { self.shadow = s; self }
    pub fn clip(mut self) -> Self { self.clip = true; self }
    pub fn scrollable(mut self) -> Self { self.scrollable = true; self }

    // ---- Paint-time transforms (animatable via `.animate()`) ----
    /// Multiply this element's paint alpha by `v` (clamped to `[0, 1]`).
    /// Layout-neutral. Multiplies onto `fill`, `stroke`, and text colour
    /// at paint time.
    pub fn opacity(mut self, v: f32) -> Self { self.opacity = v.clamp(0.0, 1.0); self }
    /// Offset this element's paint and its descendants by `(x, y)` in
    /// logical pixels. Layout-neutral; descendants inherit the offset.
    pub fn translate(mut self, x: f32, y: f32) -> Self { self.translate = (x, y); self }
    /// Uniformly scale this element's paint around its rect centre.
    /// Affects the surface quad and (if it carries text) the glyph
    /// run together. Not subtree-inheriting.
    pub fn scale(mut self, v: f32) -> Self { self.scale = v.max(0.0); self }
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

    // ---- Text-bearing ----
    pub fn text(mut self, t: impl Into<String>) -> Self { self.text = Some(t.into()); self }
    pub fn text_color(mut self, c: Color) -> Self { self.text_color = Some(c); self }
    pub fn text_align(mut self, align: TextAlign) -> Self { self.text_align = align; self }
    pub fn center_text(self) -> Self { self.text_align(TextAlign::Center) }
    pub fn end_text(self) -> Self { self.text_align(TextAlign::End) }
    pub fn text_wrap(mut self, wrap: TextWrap) -> Self { self.text_wrap = wrap; self }
    pub fn wrap_text(self) -> Self { self.text_wrap(TextWrap::Wrap) }
    pub fn nowrap_text(self) -> Self { self.text_wrap(TextWrap::NoWrap) }
    pub fn font_size(mut self, s: f32) -> Self { self.font_size = s; self }
    pub fn font_weight(mut self, w: FontWeight) -> Self { self.font_weight = w; self }
    pub fn mono(mut self) -> Self { self.font_mono = true; self }

    // ---- Children ----
    pub fn child(mut self, c: impl Into<El>) -> Self { self.children.push(c.into()); self }
    pub fn children<I, E>(mut self, cs: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        self.children.extend(cs.into_iter().map(Into::into));
        self
    }

    // ---- Internal: style profile ----
    pub fn style_profile(mut self, p: StyleProfile) -> Self { self.style_profile = p; self }

    // ---- Internal: axis (used by layout primitives below) ----
    pub(crate) fn axis(mut self, a: Axis) -> Self { self.axis = a; self }
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
    fn from(s: &str) -> Self { crate::text::text(s) }
}
impl From<String> for El {
    fn from(s: String) -> Self { crate::text::text(s) }
}
impl From<&String> for El {
    fn from(s: &String) -> Self { crate::text::text(s.as_str()) }
}
