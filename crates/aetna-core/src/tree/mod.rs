//! The [`El`] tree — the central data structure.
//!
//! An `El` is an HTML-DOM-shaped node: it has a [`Kind`] (semantic role),
//! styling, layout properties, optional text content, and zero or more
//! child `El`s. Build trees with the component constructors (`text`,
//! `button`, `card`, …) and the layout primitives (`column`, `row`,
//! `spacer`, `divider`).
//!
//! # Tree shape
//!
//! - Visual properties (`fill`, `stroke`, `radius`, `shadow`) live on
//!   `El` for the user-facing modifier API; at render time they resolve
//!   into [`crate::ir::DrawOp`]s bound to a stock shader
//!   ([`crate::shader::StockShader::RoundedRect`] for surfaces,
//!   [`crate::shader::StockShader::Text`] for text).
//! - [`El::shader_override`] lets a custom component bind its own shader
//!   instead of `rounded_rect` for the surface paint. The escape hatch
//!   the substrate must support — see `docs/SHADER_VISION.md`.
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
    Align, Axis, FontWeight, IconName, InteractionState, Justify, Kind, Rect, Sides, Size, Source,
    SurfaceRole, TextAlign, TextOverflow, TextRole, TextWrap,
};

use crate::anim::Timing;
use crate::layout::{LayoutCtx, LayoutFn, VirtualItems};
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
    /// When true, all key events (other than registered hotkeys) route
    /// to this node as raw `KeyDown` instead of being interpreted by
    /// the library's defaults (Tab traversal, Enter/Space activation,
    /// Escape escape). Used by text-input widgets that need to consume
    /// Tab/Enter/etc. as text or editing actions. Implies `focusable`
    /// at the runner — the flag only takes effect when the node is
    /// also the focused target.
    pub capture_keys: bool,
    /// When true, this node's paint opacity is multiplied by the
    /// nearest focusable ancestor's focus envelope (0..1). The library
    /// already animates that envelope on focus / blur; flagged nodes
    /// fade in and out with the same easing without any app-side
    /// focus tracking.
    ///
    /// Used by `text_input`'s caret bar — the caret only paints when
    /// the input is focused, fading via the standard focus animation.
    /// Documented in `widget_kit.md` as part of the public surface.
    pub alpha_follows_focused_ancestor: bool,
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
    pub surface_role: SurfaceRole,
    /// Permit this element to paint outside its layout bounds. The
    /// outset enlarges the quad geometry handed to the shader (and
    /// any focus / shadow / glow visuals are positioned in the
    /// overflow band) while leaving the layout rect — and therefore
    /// sibling positions and hit-testing — unchanged. Subject to
    /// ancestor clip rects: a focused widget inside a `clip()`ped
    /// parent has its overflow clipped, same as any other paint.
    pub paint_overflow: Sides,
    /// Clip this element's own paint and descendants to its computed rect.
    /// Used by scroll panes, host-painted regions, overlays, and any region
    /// where overflow should not leak visually or receive events.
    pub clip: bool,

    /// This element is a vertical scroll viewport. The layout pass reads
    /// the offset from `UiState`'s scroll-offset side map keyed by
    /// `computed_id`, clamps it to `[0, content_h - viewport_h]`, and
    /// writes the clamped value back. Set automatically by [`scroll`].
    pub scrollable: bool,

    /// Treat this element's focusable children as a single arrow-navigable
    /// group: while a focused element is one of the direct children,
    /// `Up` / `Down` / `Home` / `End` move focus among the group's
    /// focusable siblings instead of being routed as a `KeyDown`. Tab
    /// traversal is unchanged.
    ///
    /// Used by `popover_panel` so menu items in a dropdown are
    /// keyboard-navigable; available to any user widget that wants the
    /// same semantics.
    pub arrow_nav_siblings: bool,

    /// Tooltip text. When set, the runtime synthesizes a hover-driven
    /// tooltip layer anchored to this node — appearing after the
    /// hover delay elapses, fading in with the standard envelope, and
    /// dismissed when the pointer leaves or presses the node. The
    /// trigger doesn't have to be focusable or keyed; the runtime
    /// anchors the tooltip via the trigger's `computed_id`.
    pub tooltip: Option<String>,

    /// Override the implicit `stock::rounded_rect` binding for this
    /// node's surface. The escape hatch a user crate uses to bind a
    /// custom shader (e.g. `liquid_glass`).
    pub shader_override: Option<ShaderBinding>,

    /// Second escape hatch: author-supplied layout function that
    /// positions this node's direct children. When set, the layout
    /// pass calls the function instead of running its column/row/
    /// overlay distribution. The library still recurses into each
    /// child and still drives hit-test / focus / animation / scroll
    /// off the rects the function returns. See [`LayoutFn`] for the
    /// contract.
    pub layout_override: Option<LayoutFn>,

    /// Virtualized list state. Set by [`crate::virtual_list`] (and only
    /// on `Kind::VirtualList` nodes). The layout pass uses this to
    /// realize only the rows whose rect intersects the viewport. The
    /// node is automatically `scrollable` + `clip`.
    pub virtual_items: Option<VirtualItems>,

    // Text
    pub text: Option<String>,
    pub text_color: Option<Color>,
    pub text_align: TextAlign,
    pub text_wrap: TextWrap,
    pub text_overflow: TextOverflow,
    pub text_role: TextRole,
    pub text_max_lines: Option<usize>,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub font_mono: bool,
    /// Italic styling. Author-set via [`Self::italic`]; honoured when
    /// this El is a styled text leaf inside an [`Kind::Inlines`] parent
    /// and (best-effort) on standalone text Els.
    pub text_italic: bool,
    /// Underline styling. Author-set via [`Self::underline`].
    pub text_underline: bool,
    /// Strikethrough styling. Author-set via [`Self::strikethrough`].
    pub text_strikethrough: bool,
    /// Link target URL. When set on a text leaf inside [`Kind::Inlines`],
    /// the run renders as a link (themed) and runs sharing a URL group
    /// together for hit-test. Author-set via [`Self::link`].
    pub text_link: Option<String>,

    // Icon
    pub icon: Option<IconName>,
    pub icon_stroke_width: f32,

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
            capture_keys: false,
            alpha_follows_focused_ancestor: false,
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
            surface_role: SurfaceRole::None,
            paint_overflow: Sides::zero(),
            clip: false,
            scrollable: false,
            arrow_nav_siblings: false,
            tooltip: None,
            shader_override: None,
            layout_override: None,
            virtual_items: None,
            text: None,
            text_color: None,
            text_align: TextAlign::Start,
            text_wrap: TextWrap::NoWrap,
            text_overflow: TextOverflow::Clip,
            text_role: TextRole::Body,
            text_max_lines: None,
            font_size: crate::tokens::FONT_BASE,
            font_weight: FontWeight::Regular,
            font_mono: false,
            text_italic: false,
            text_underline: false,
            text_strikethrough: false,
            text_link: None,
            icon: None,
            icon_stroke_width: 2.0,
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
    /// Opt this node into raw key capture when focused. While this
    /// node is the focused target, the library's Tab/Enter/Escape
    /// defaults are bypassed (registered hotkeys still match first)
    /// and the raw `KeyDown` is delivered for the widget to interpret.
    /// Use for text inputs and other editors that want full keyboard
    /// control. Implies `focusable`.
    pub fn capture_keys(mut self) -> Self {
        self.capture_keys = true;
        self.focusable = true;
        self
    }
    /// Multiply this element's paint opacity by the nearest focusable
    /// ancestor's focus envelope (0..1). The library writes that
    /// envelope on every frame as focus enters / leaves the ancestor;
    /// flagged elements fade in and out with the same animation
    /// without any app-side focus tracking. The flag is layout-neutral
    /// and propagates to descendants via the standard opacity chain.
    ///
    /// Used by `text_input`'s caret bar so the caret is only visible
    /// while the input is focused. Any custom widget can use this for
    /// the same kind of "this child only renders when my container is
    /// the focused element" behavior.
    pub fn alpha_follows_focused_ancestor(mut self) -> Self {
        self.alpha_follows_focused_ancestor = true;
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
    pub fn surface_role(mut self, role: SurfaceRole) -> Self {
        self.surface_role = role;
        self
    }
    /// Permit paint to extend beyond this element's layout bounds by
    /// `outset` on each side. Layout-neutral — siblings don't move and
    /// hit-testing still uses the layout rect — but the shader receives
    /// a quad inflated by `outset`. Use for focus rings, drop shadows,
    /// glow halos, or any visual that should escape the box without
    /// affecting flow. Clipped by ancestor `clip()` rects.
    pub fn paint_overflow(mut self, outset: impl Into<Sides>) -> Self {
        self.paint_overflow = outset.into();
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
    /// Treat this element's focusable children as a single arrow-navigable
    /// group: `Up` / `Down` / `Home` / `End` move focus among siblings
    /// while one of them is focused. See the field doc on
    /// [`Self::arrow_nav_siblings`].
    pub fn arrow_nav_siblings(mut self) -> Self {
        self.arrow_nav_siblings = true;
        self
    }

    /// Attach a hover tooltip to this element. The runtime synthesizes
    /// a floating tooltip layer when the pointer rests on the node for
    /// the configured delay, anchors it below (or above, on viewport
    /// collision) the trigger, and removes it on pointer-leave or
    /// press. Layout-neutral — the trigger isn't resized.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
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

    /// Replace the column/row/overlay distribution for this node with
    /// `f`. The function receives a [`LayoutCtx`] (container rect,
    /// children, intrinsic-measure callback) and returns one [`Rect`]
    /// per child in source order. The node itself must size with
    /// `Fixed` or `Fill` on both axes — `Hug` is not supported for
    /// custom-layout nodes.
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
    pub fn text_overflow(mut self, overflow: TextOverflow) -> Self {
        self.text_overflow = overflow;
        self
    }
    pub fn ellipsis(self) -> Self {
        self.text_overflow(TextOverflow::Ellipsis)
    }
    pub fn max_lines(mut self, lines: usize) -> Self {
        self.text_max_lines = Some(lines.max(1));
        self
    }
    pub fn font_size(mut self, s: f32) -> Self {
        self.font_size = s;
        self
    }
    pub fn font_weight(mut self, w: FontWeight) -> Self {
        self.font_weight = w;
        self
    }
    pub fn icon_name(mut self, name: IconName) -> Self {
        self.icon = Some(name);
        self
    }
    pub fn icon_stroke_width(mut self, width: f32) -> Self {
        self.icon_stroke_width = width.max(0.25);
        self
    }
    pub fn icon_size(mut self, size: f32) -> Self {
        let size = size.max(1.0);
        self.font_size = size;
        self.width = Size::Fixed(size);
        self.height = Size::Fixed(size);
        self
    }
    pub fn mono(mut self) -> Self {
        self.font_mono = true;
        self
    }

    /// Italic styling for a text run. Honoured by the
    /// [`Kind::Inlines`] layout pass and (best-effort) on standalone
    /// text Els.
    pub fn italic(mut self) -> Self {
        self.text_italic = true;
        self
    }

    /// Underline styling for a text run.
    pub fn underline(mut self) -> Self {
        self.text_underline = true;
        self
    }

    /// Strikethrough styling for a text run.
    pub fn strikethrough(mut self) -> Self {
        self.text_strikethrough = true;
        self
    }

    /// Markdown-flavoured inline-code styling. Currently `mono`-styled;
    /// a tinted background per the theme is a future addition. Authors
    /// who want raw mono without code chrome should use [`Self::mono`]
    /// instead.
    pub fn code(mut self) -> Self {
        self.text_role = TextRole::Code;
        self.font_size = crate::tokens::FONT_SM;
        self.font_weight = FontWeight::Regular;
        self.font_mono = true;
        self.text_color = Some(crate::tokens::TEXT_FOREGROUND);
        self
    }

    /// Mark this run as a link to `url`. Inside an [`Kind::Inlines`]
    /// parent the run paints with a link-themed color; runs sharing
    /// the same URL group together for hit-test.
    pub fn link(mut self, url: impl Into<String>) -> Self {
        self.text_link = Some(url.into());
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

    /// Set the layout axis directly. The `column` / `row` / `stack`
    /// constructors set this for you; widget builders that compose a
    /// `Kind::Custom` container use this to declare row vs. column vs.
    /// overlay flow without hijacking a stock kind. Documented in
    /// `widget_kit.md` as part of the public author surface.
    pub fn axis(mut self, a: Axis) -> Self {
        self.axis = a;
        self
    }
}

// ---------- Layout primitives (plain functions) ----------

/// A vertical container with a comfortable default gap.
///
/// Defaults: `axis = Column`, `align = Stretch`, `width = Fill(1.0)`,
/// `height = Hug`. The `Hug` height means the column is exactly as
/// tall as its stacked children — the natural shape for most use
/// (sidebar nav, card body, info pair). To make the column claim its
/// parent's full height, set `.height(Size::Fill(1.0))`.
///
/// **Why `height = Hug` and not `Fill`:** when a column with `Fill`
/// height sits inside a row that uses `align(Center)`, the column
/// claims the row's full height and its content top-aligns inside
/// the box (column's main-axis justify defaults to `Start`). The
/// row's centering becomes a no-op for that child — a common surprise.
/// Hugging the height keeps `align()` working the way it reads.
///
/// Hug-width children are stretched to the column's width; Fill-width
/// children always fill regardless of `align`. Override `.align(...)`
/// to position narrower (Hug/Fixed) children: `Start` (left), `Center`,
/// `End` (right).
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
        .height(Size::Hug)
        .axis(Axis::Column)
}

/// A horizontal container with a comfortable default gap.
///
/// Defaults: `axis = Row`, `align = Center`, `height = Hug`. The
/// `Center` default vertically centers the typical row content
/// (icon + text + button) within the row's hug height.
///
/// To make the row span the full available height, set
/// `.height(Size::Fill(1.0))`. `Fill`-height children always claim
/// the row's full extent regardless of `align`; `align` only positions
/// Hug/Fixed-height children that are shorter than the row.
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

/// A vertical scroll viewport. Children stack as in [`column()`]; the
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

/// Block whose direct children flow inline (text leaves + embeds +
/// hard breaks). Models HTML's `<p>` shape: heterogeneous children,
/// attributed runs, optional inline embeds. Children are styled via
/// the existing modifier chain (`.bold()`, `.italic()`, `.color(c)`,
/// `.code()`, `.link(url)`, etc.) — there is no parallel
/// `RichText`/`TextRun` type.
///
/// ```ignore
/// text_runs([
///     text("Aetna — "),
///     text("rich text").bold(),
///     text(" composition."),
///     hard_break(),
///     text("Custom shaders, custom layouts, "),
///     text("virtual_list").code(),
///     text(" — and inline runs."),
/// ])
/// ```
#[track_caller]
pub fn text_runs<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Inlines)
        .at_loc(Location::caller())
        .axis(Axis::Column)
        .align(Align::Start)
        .children(children)
}

/// Forced line break inside a [`text_runs`] block. Mirrors HTML's
/// `<br>`. Outside an `Inlines` parent, lays out as a zero-size leaf.
#[track_caller]
pub fn hard_break() -> El {
    El::new(Kind::HardBreak)
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
}

/// Virtualized vertical list of `count` rows of fixed height
/// `row_height`. The library calls `build_row(i)` only for indices
/// whose rect intersects the visible viewport, then lays them out at
/// the scroll-shifted Y. Authors typically key rows with a stable
/// identifier (`button("foo").key("msg-abc")`) so hover/press/focus
/// state survives scrolling.
///
/// The returned El defaults to `Size::Fill(1.0)` on both axes (it's a
/// viewport — its size is decided by the parent). `Size::Hug` would
/// defeat virtualization and panics at layout time.
#[track_caller]
pub fn virtual_list<F>(count: usize, row_height: f32, build_row: F) -> El
where
    F: Fn(usize) -> El + Send + Sync + 'static,
{
    let mut el = El::new(Kind::VirtualList)
        .at_loc(Location::caller())
        .axis(Axis::Column)
        .align(Align::Stretch)
        .clip()
        .scrollable();
    el.virtual_items = Some(VirtualItems::new(count, row_height, build_row));
    el
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
        crate::widgets::text::text(s)
    }
}
impl From<String> for El {
    fn from(s: String) -> Self {
        crate::widgets::text::text(s)
    }
}
impl From<&String> for El {
    fn from(s: &String) -> Self {
        crate::widgets::text::text(s.as_str())
    }
}
