//! Geometry, sizing intents, and enum-shaped tree fields.
//!
//! Split out of `tree/mod.rs` so the `El` builder file is just `El` and
//! the layout primitives. Everything here is `Copy` (or near-Copy) and
//! has no inter-module dependencies.

use std::panic::Location;

/// A rectangle in **logical pixels** — the host's `scale_factor` is
/// applied at paint time, so layout, hit-testing, and `Rect`-shaped
/// API arguments all speak the same un-scaled coordinate space
/// regardless of HiDPI / scaling factors.
///
/// Origin top-left, +y down (the same convention CSS uses for the
/// viewport box).
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
    /// Inverse of [`Self::inset`]: extend the rect outward by `p` on each
    /// side. Used by `draw_ops` to produce the painted rect from the
    /// layout rect when an element opts into `paint_overflow`.
    pub fn outset(self, p: Sides) -> Self {
        Self::new(
            self.x - p.left,
            self.y - p.top,
            self.w + p.left + p.right,
            self.h + p.top + p.bottom,
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
///
/// On a container's **main axis** (the axis its children flow along),
/// `Fill` siblings split the leftover space proportional to their weights;
/// `Hug` siblings take their content size and pack toward the start.
///
/// On the **cross axis**, sizing is governed by the parent's [`Align`]:
/// `Align::Stretch` (the column / scroll default) stretches both `Hug`
/// and `Fill` children to the container's full extent, while
/// `Align::Center | Start | End` shrinks them to their intrinsic size
/// so the alignment can actually position them. `Fixed` is honored
/// regardless. This mirrors CSS flex's `align-items` semantics.
///
/// The default is `Hug`, matching the CSS flex item default
/// (`flex: 0 1 auto`) — content-sized on main axis, deferred to
/// `align-items` on cross axis. Use `.width(Size::Fill(1.0))` to
/// claim leftover space (the analog of `flex: 1` or `width: 100%`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Size {
    Fixed(f32),
    Fill(f32),
    #[default]
    Hug,
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

/// Cross-axis sizing + alignment of children, mirroring CSS
/// `align-items`. `Align` governs both how non-`Fixed` children are
/// **sized** on the cross axis and where smaller children are
/// **positioned** within the container's cross extent.
///
/// - `Stretch` — non-`Fixed` children claim the container's full
///   cross extent (CSS `align-items: stretch`). Default for `row`,
///   `column`, and `scroll`.
/// - `Start` — non-`Fixed` children shrink to intrinsic and pin to
///   the start of the cross axis (top for rows, left for columns).
/// - `Center` — non-`Fixed` children shrink to intrinsic and center
///   in the cross extent.
/// - `End` — non-`Fixed` children shrink to intrinsic and pin to the
///   end (bottom for rows, right for columns).
///
/// `Size::Fixed` is always honored exactly; `Align` only positions
/// Fixed children within the cross extent. Under non-`Stretch`
/// alignments, `Hug` and `Fill` collapse to intrinsic — the same way
/// CSS flex doesn't distinguish between them on the cross axis.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Align {
    /// Pin to the start of the cross axis (top for rows, left for columns).
    Start,
    /// Center in the cross extent.
    Center,
    /// Pin to the end of the cross axis (bottom for rows, right for columns).
    End,
    /// Stretch non-`Fixed` children to the container's cross extent
    /// (CSS `align-items: stretch`). Default.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

/// Semantic typography role for text-bearing nodes.
///
/// The role is inspectable design intent first, and default styling
/// second. Builders and modifiers use it so app code can ask for
/// familiar text primitives (`caption`, `label`, `title`, …) instead
/// of scattering raw font sizes through product surfaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextRole {
    #[default]
    Body,
    Caption,
    Label,
    Title,
    Heading,
    Display,
    Code,
}

/// Built-in icon names. The string forms intentionally mirror common
/// lucide/shadcn names so agents can reach for familiar labels.
///
/// `#[non_exhaustive]` because we expect to grow the icon set; new
/// variants must not break exhaustive matches in downstream code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum IconName {
    Activity,
    AlertCircle,
    BarChart,
    Bell,
    Check,
    ChevronDown,
    ChevronRight,
    Command,
    Download,
    FileText,
    Folder,
    GitBranch,
    GitCommit,
    Info,
    LayoutDashboard,
    Menu,
    MoreHorizontal,
    Plus,
    RefreshCw,
    Search,
    Settings,
    Upload,
    Users,
    X,
}

impl IconName {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "activity" => Some(Self::Activity),
            "alert-circle" | "alert" => Some(Self::AlertCircle),
            "bar-chart" | "chart-bar" => Some(Self::BarChart),
            "bell" => Some(Self::Bell),
            "check" => Some(Self::Check),
            "chevron-down" => Some(Self::ChevronDown),
            "chevron-right" => Some(Self::ChevronRight),
            "command" => Some(Self::Command),
            "download" => Some(Self::Download),
            "file-text" | "file" => Some(Self::FileText),
            "folder" => Some(Self::Folder),
            "git-branch" => Some(Self::GitBranch),
            "git-commit" => Some(Self::GitCommit),
            "info" => Some(Self::Info),
            "layout-dashboard" | "dashboard" => Some(Self::LayoutDashboard),
            "menu" => Some(Self::Menu),
            "more-horizontal" | "more" => Some(Self::MoreHorizontal),
            "plus" => Some(Self::Plus),
            "refresh-cw" | "refresh" => Some(Self::RefreshCw),
            "search" => Some(Self::Search),
            "settings" => Some(Self::Settings),
            "upload" => Some(Self::Upload),
            "users" => Some(Self::Users),
            "x" | "close" => Some(Self::X),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Activity => "activity",
            Self::AlertCircle => "alert-circle",
            Self::BarChart => "bar-chart",
            Self::Bell => "bell",
            Self::Check => "check",
            Self::ChevronDown => "chevron-down",
            Self::ChevronRight => "chevron-right",
            Self::Command => "command",
            Self::Download => "download",
            Self::FileText => "file-text",
            Self::Folder => "folder",
            Self::GitBranch => "git-branch",
            Self::GitCommit => "git-commit",
            Self::Info => "info",
            Self::LayoutDashboard => "layout-dashboard",
            Self::Menu => "menu",
            Self::MoreHorizontal => "more-horizontal",
            Self::Plus => "plus",
            Self::RefreshCw => "refresh-cw",
            Self::Search => "search",
            Self::Settings => "settings",
            Self::Upload => "upload",
            Self::Users => "users",
            Self::X => "x",
        }
    }

    pub fn fallback_glyph(self) -> &'static str {
        match self {
            Self::Activity => "~",
            Self::AlertCircle => "!",
            Self::BarChart => "▮",
            Self::Bell => "•",
            Self::Check => "✓",
            Self::ChevronDown => "⌄",
            Self::ChevronRight => "›",
            Self::Command => "⌘",
            Self::Download => "↓",
            Self::FileText => "□",
            Self::Folder => "▱",
            Self::GitBranch => "⑂",
            Self::GitCommit => "⊙",
            Self::Info => "i",
            Self::LayoutDashboard => "▦",
            Self::Menu => "☰",
            Self::MoreHorizontal => "…",
            Self::Plus => "+",
            Self::RefreshCw => "↻",
            Self::Search => "⌕",
            Self::Settings => "⚙",
            Self::Upload => "↑",
            Self::Users => "●",
            Self::X => "×",
        }
    }
}

impl TextRole {
    pub fn name(self) -> &'static str {
        match self {
            TextRole::Body => "body",
            TextRole::Caption => "caption",
            TextRole::Label => "label",
            TextRole::Title => "title",
            TextRole::Heading => "heading",
            TextRole::Display => "display",
            TextRole::Code => "code",
        }
    }
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
    /// Vertically scrollable region whose children are produced lazily
    /// by an author-supplied closure. Only rows whose rect intersects
    /// the viewport are realized and laid out each frame. Always
    /// clipping + scrollable.
    VirtualList,
    /// Block whose direct children flow inline (text leaves + embeds +
    /// hard breaks). Mirrors HTML's `<p>` / `<span>` shape: children
    /// are heterogeneous Els and the layout pass shapes them as one
    /// attributed line/wrap-block via cosmic-text. A child El with
    /// `text` set contributes a styled text run; a child El without
    /// text contributes an inline embed; a `Kind::HardBreak` forces a
    /// line break.
    Inlines,
    /// Forced line break inside a `Kind::Inlines` block. Mirrors HTML's
    /// `<br>`. Outside an `Inlines` parent, lays out as a zero-size
    /// leaf.
    HardBreak,
    /// Escape hatch for app-defined components.
    Custom(&'static str),
}

/// Semantic paint role for rect-shaped surfaces.
///
/// `Kind` says what a node is structurally; `SurfaceRole` says how a
/// visual surface should be themed. This lets stock widgets keep a
/// familiar author API while themes map panels, popovers, inputs, and
/// selected rows to different shader recipes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum SurfaceRole {
    /// No special semantic role. Theme fallback applies.
    #[default]
    None,
    Panel,
    Raised,
    Sunken,
    Popover,
    Selected,
    Current,
    Input,
    Danger,
}

impl SurfaceRole {
    pub fn name(self) -> &'static str {
        match self {
            SurfaceRole::None => "none",
            SurfaceRole::Panel => "panel",
            SurfaceRole::Raised => "raised",
            SurfaceRole::Sunken => "sunken",
            SurfaceRole::Popover => "popover",
            SurfaceRole::Selected => "selected",
            SurfaceRole::Current => "current",
            SurfaceRole::Input => "input",
            SurfaceRole::Danger => "danger",
        }
    }

    pub fn uniform_id(self) -> f32 {
        match self {
            SurfaceRole::None => 0.0,
            SurfaceRole::Panel => 1.0,
            SurfaceRole::Raised => 2.0,
            SurfaceRole::Sunken => 3.0,
            SurfaceRole::Popover => 4.0,
            SurfaceRole::Selected => 5.0,
            SurfaceRole::Current => 6.0,
            SurfaceRole::Input => 7.0,
            SurfaceRole::Danger => 8.0,
        }
    }
}

/// Interaction state, applied as a render-time visual delta.
///
/// Written by the runtime's interaction tracker. State styling lives in
/// draw-op resolution; the tree carries the resolved state flag and the
/// renderer applies the appropriate transformation when emitting draw ops.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
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
#[non_exhaustive]
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
