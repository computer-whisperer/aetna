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
