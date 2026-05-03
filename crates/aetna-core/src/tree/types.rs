//! Geometry, sizing intents, and enum-shaped tree fields.
//!
//! Split out of `tree/mod.rs` so the `El` builder file is just `El` and
//! the layout primitives. Everything here is `Copy` (or near-Copy) and
//! has no inter-module dependencies.

use std::panic::Location;

/// A rectangle in pixel coordinates. Origin top-left, +y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }
    pub fn center_x(self) -> f32 {
        self.x + self.w * 0.5
    }
    pub fn center_y(self) -> f32 {
        self.y + self.h * 0.5
    }
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
    pub const fn all(v: f32) -> Self {
        Self {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }
    pub const fn xy(x: f32, y: f32) -> Self {
        Self {
            left: x,
            right: x,
            top: y,
            bottom: y,
        }
    }
    pub const fn zero() -> Self {
        Self::all(0.0)
    }
}

impl From<f32> for Sides {
    fn from(v: f32) -> Self {
        Sides::all(v)
    }
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
    fn default() -> Self {
        Size::Fill(1.0)
    }
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
/// Prefer [`super::spacer`] for ad-hoc gaps.
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
/// Set with [`super::El::with_state`]. State styling lives in the renderer;
/// the tree carries the state flag and the renderer applies the appropriate
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
        Self {
            file: loc.file(),
            line: loc.line(),
        }
    }
}
