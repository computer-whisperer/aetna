//! The [`El`] tree — the central data structure.
//!
//! An `El` is loosely modeled after an HTML element: it has a [`Kind`]
//! (semantic role), styling, layout properties, optional text content,
//! and zero or more child `El`s.
//!
//! Build trees with the component constructors (`text`, `button`, `card`, ...)
//! and the layout primitives in this module (`column`, `row`, `spacer`,
//! `divider`). Then pass the tree to [`crate::layout::layout`] and
//! [`crate::render`] to produce a backend artifact.
//!
//! Most fields are public so renderers and inspection tools can read them.
//! Mutate via the chainable builder methods (`.padding`, `.gap`, `.fill`, etc.)
//! rather than touching fields directly.

use crate::theme::theme;

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
    pub fn inset(self, p: Sides) -> Self {
        Self::new(
            self.x + p.left,
            self.y + p.top,
            (self.w - p.left - p.right).max(0.0),
            (self.h - p.top - p.bottom).max(0.0),
        )
    }
}

/// Per-side padding/inset values. `From<f32>` gives uniform sides.
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

/// A sizing intent along one axis. Layout uses these to allocate space —
/// users never write pixel arithmetic.
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
    /// No layout — children all share the parent's rect (overlay).
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
    /// Stretch fill-sized children to the container's cross-axis extent
    /// (the typical default for column/row).
    #[default]
    Stretch,
}

/// Main-axis distribution when children don't fill the container.
///
/// In typical use, prefer [`spacer`] for ad-hoc gaps; reach for `Justify`
/// when you want children placed without inserting structural nodes.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

/// Font weight. Keep in sync with the renderer's font-loading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Semibold,
    Bold,
}

/// A color (RGBA8) optionally tagged with the theme token it came from.
///
/// The token name has no effect on rendering — it's metadata for inspection
/// and lint passes. Construct via [`Color::rgba`] / [`Color::rgb`] / theme
/// fields, or [`Color::token`] when you want to label a fresh literal.
#[derive(Clone, Copy, Debug)]
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
    /// Same color, alpha overridden. Useful for tinted fills.
    pub fn with_alpha(self, a: u8) -> Self { Self { a, ..self } }
}

/// Semantic identity of an element. Roughly an HTML tag.
///
/// The [`Kind::Custom`] escape hatch lets app code add element kinds
/// without changing this enum.
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
    Custom(&'static str),
}

/// The core tree node.
///
/// Construct via the component builders — `text`, `button`, `card`, `column`,
/// etc. — and then chain modifiers (`.padding`, `.gap`, `.fill`, ...). Avoid
/// constructing `El` directly; the builders set polished defaults.
#[derive(Clone, Debug)]
pub struct El {
    pub kind: Kind,
    /// Optional stable key for list identity / inspection.
    pub key: Option<String>,

    // Layout
    pub axis: Axis,
    pub gap: f32,
    pub padding: Sides,
    pub align: Align,
    pub justify: Justify,
    pub width: Size,
    pub height: Size,

    // Visual style
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
    pub radius: f32,
    pub shadow: f32,

    // Text
    pub text: Option<String>,
    pub text_color: Option<Color>,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub font_mono: bool,

    pub children: Vec<El>,

    /// Filled in by [`crate::layout::layout`].
    pub computed: Rect,
}

impl Default for El {
    fn default() -> Self {
        Self {
            kind: Kind::Group,
            key: None,
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
            text: None,
            text_color: None,
            font_size: 14.0,
            font_weight: FontWeight::Regular,
            font_mono: false,
            children: Vec::new(),
            computed: Rect::default(),
        }
    }
}

impl El {
    pub fn new(kind: Kind) -> Self { Self { kind, ..Default::default() } }

    // --- Identity ---
    pub fn key(mut self, k: impl Into<String>) -> Self { self.key = Some(k.into()); self }

    // --- Sizing ---
    pub fn width(mut self, w: Size) -> Self { self.width = w; self }
    pub fn height(mut self, h: Size) -> Self { self.height = h; self }
    /// Convenience: width = Hug, height = Hug. Sizes the element to its content.
    pub fn hug(mut self) -> Self { self.width = Size::Hug; self.height = Size::Hug; self }
    /// Convenience: width = Fill(1), height = Fill(1).
    pub fn fill_size(mut self) -> Self { self.width = Size::Fill(1.0); self.height = Size::Fill(1.0); self }

    // --- Layout (container) ---
    pub fn padding(mut self, p: impl Into<Sides>) -> Self { self.padding = p.into(); self }
    pub fn gap(mut self, g: f32) -> Self { self.gap = g; self }
    pub fn align(mut self, a: Align) -> Self { self.align = a; self }
    pub fn justify(mut self, j: Justify) -> Self { self.justify = j; self }

    // --- Visual ---
    pub fn fill(mut self, c: Color) -> Self { self.fill = Some(c); self }
    pub fn stroke(mut self, c: Color) -> Self {
        self.stroke = Some(c);
        if self.stroke_width == 0.0 { self.stroke_width = 1.0; }
        self
    }
    pub fn stroke_width(mut self, w: f32) -> Self { self.stroke_width = w; self }
    pub fn radius(mut self, r: f32) -> Self { self.radius = r; self }
    pub fn shadow(mut self, s: f32) -> Self { self.shadow = s; self }

    // --- Text-bearing ---
    pub fn text(mut self, t: impl Into<String>) -> Self { self.text = Some(t.into()); self }
    pub fn text_color(mut self, c: Color) -> Self { self.text_color = Some(c); self }
    pub fn font_size(mut self, s: f32) -> Self { self.font_size = s; self }
    pub fn font_weight(mut self, w: FontWeight) -> Self { self.font_weight = w; self }
    pub fn mono(mut self) -> Self { self.font_mono = true; self }

    // --- Children ---
    pub fn child(mut self, c: impl Into<El>) -> Self { self.children.push(c.into()); self }
    pub fn children<I, E>(mut self, cs: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        self.children.extend(cs.into_iter().map(Into::into));
        self
    }
}

// ---------- Layout primitives ----------
//
// These are plain functions, not types, so the user-facing surface stays
// flat: `column([...])` reads like a tag, not a builder pattern.

/// A vertical container with sensible default gap.
pub fn column<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .children(children)
        .gap(theme().space.md)
        .align(Align::Stretch)
        .clone_with_axis(Axis::Column)
}

/// A horizontal container with sensible default gap, vertically centered.
pub fn row<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .children(children)
        .gap(theme().space.sm)
        .align(Align::Center)
        .clone_with_axis(Axis::Row)
}

/// An overlay stack — children share the parent's rect.
pub fn stack<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .children(children)
        .clone_with_axis(Axis::Overlay)
}

/// A `Fill(1)` filler. Use inside a `row` to push siblings to the right,
/// or inside a `column` to push siblings to the bottom.
pub fn spacer() -> El {
    El::new(Kind::Spacer)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}

/// A 1-pixel separator line.
pub fn divider() -> El {
    let t = theme();
    El::new(Kind::Divider)
        .height(Size::Fixed(1.0))
        .width(Size::Fill(1.0))
        .fill(t.border.default)
}

// Internal helper used by the primitives above to avoid an extra builder call.
impl El {
    fn clone_with_axis(mut self, axis: Axis) -> Self { self.axis = axis; self }
}

// ---------- &str → El convenience for ergonomic children ----------
//
// Lets callers write `card("Title", ["a body line"])` without wrapping the
// string in `text(...)`. Strings render as default body text.

impl From<&str> for El {
    fn from(s: &str) -> Self { crate::text::text(s) }
}

impl From<String> for El {
    fn from(s: String) -> Self { crate::text::text(s) }
}

impl From<&String> for El {
    fn from(s: &String) -> Self { crate::text::text(s.as_str()) }
}
