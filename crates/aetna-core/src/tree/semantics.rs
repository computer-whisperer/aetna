//! Semantic node and paint roles carried by [`El`](crate::El).

use std::panic::Location;

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
    /// A vertically scrollable region.
    Scroll,
    /// Vertically scrollable region whose children are produced lazily.
    VirtualList,
    /// Block whose direct children flow inline.
    Inlines,
    /// Forced line break inside a `Kind::Inlines` block.
    HardBreak,
    /// Raster image element.
    Image,
    /// Escape hatch for app-defined components.
    Custom(&'static str),
}

/// Semantic paint role for rect-shaped surfaces.
///
/// Each variant maps to a theme-applied recipe at paint time. Roles are
/// either *decorative* (set stroke + shadow on top of whatever fill the
/// node already carries) or *fill-providing* (default a fill from the
/// palette when the node has none). The split matters: setting a
/// decorative role on a node with no fill produces an "invisible
/// surface" — only a thin border over the parent's background. For
/// panel-shaped containers, prefer the dedicated widget (`card()`,
/// `sidebar()`, `dialog()`, `popover()`) which bundles role + fill +
/// stroke + radius + shadow correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum SurfaceRole {
    /// No special semantic role. Theme fallback applies.
    #[default]
    None,
    /// **Decorative.** Border + small drop shadow. *Does not paint a
    /// fill* — the node must supply one (e.g. `tokens::CARD`) or sit
    /// inside a widget like `card()` / `sidebar()` that does.
    Panel,
    /// **Decorative.** Border + half-strength shadow, suggesting one
    /// elevation step above its parent. Like `Panel`, no fill.
    Raised,
    /// **Fill-providing.** Slightly darker variant of `MUTED` (palette
    /// `darken(0.08)`) with input-toned border. Use for inset bands —
    /// search wells, segmented-control tracks, recessed list headers.
    Sunken,
    /// **Decorative.** Input-toned border + large drop shadow for
    /// floating panels. Used by `popover()` and friends; bring your
    /// own fill (typically `tokens::POPOVER`).
    Popover,
    /// **Fill-providing.** PRIMARY-tinted alpha 28 fill +
    /// PRIMARY-tinted alpha 110 border. The selected item inside a
    /// collection. Prefer the `.selected()` chainable, which sets this
    /// role plus content color in one call.
    Selected,
    /// **Fill-providing.** Solid `ACCENT` fill + neutral border for
    /// the current page / nav item. Prefer the `.current()` chainable,
    /// which also bumps font weight and content color.
    Current,
    /// **Fill-providing.** Same recipe as `Sunken` — used by text
    /// inputs and other editable surfaces.
    Input,
    /// **Decorative.** Destructive-toned border, no shadow. Pair with
    /// a tint fill (e.g. `tokens::DESTRUCTIVE.with_alpha(40)`) for the
    /// classic "danger" band in a form or section header.
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
