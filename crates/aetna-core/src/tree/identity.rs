//! Identity, source, and interaction-flag modifiers for [`El`].

use std::panic::Location;

use super::node::El;
use super::semantics::{Kind, Source};

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

    /// Show the focus ring on this node even when focus arrived via
    /// pointer click. Default focus-ring behavior follows the web
    /// platform's `:focus-visible` rule — ring on Tab, no ring on
    /// click. Widgets where the ring is meaningful regardless of
    /// source — text input, text area — opt in here so clicking into
    /// the field still raises the "now active" affordance. Implies
    /// nothing about focusability; pair with `.focusable()`.
    pub fn always_show_focus_ring(mut self) -> Self {
        self.always_show_focus_ring = true;
        self
    }

    /// Opt this node into the library's text-selection system. The
    /// node must also carry an explicit `.key(...)`; selection requires
    /// stable identity across rebuilds the same way focus does.
    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }

    /// Opt this node into raw key capture when focused. While this
    /// node is the focused target, the library's Tab/Enter/Escape
    /// defaults are bypassed and raw `KeyDown` events are delivered for
    /// the widget to interpret. Implies `focusable`.
    pub fn capture_keys(mut self) -> Self {
        self.capture_keys = true;
        self.focusable = true;
        self
    }

    /// Multiply this element's paint opacity by the nearest focusable
    /// ancestor's focus envelope.
    pub fn alpha_follows_focused_ancestor(mut self) -> Self {
        self.alpha_follows_focused_ancestor = true;
        self
    }

    /// Multiply this node's paint opacity by the runtime's caret blink
    /// alpha.
    pub fn blink_when_focused(mut self) -> Self {
        self.blink_when_focused = true;
        self
    }

    /// Borrow hover and press visual envelopes from the nearest
    /// focusable ancestor.
    pub fn state_follows_interactive_ancestor(mut self) -> Self {
        self.state_follows_interactive_ancestor = true;
        self
    }

    /// Reveal this element on hover of a focusable ancestor (or the
    /// element itself).
    ///
    /// `rest_opacity` is the drawn alpha when no hover signal is
    /// present (clamped to `[0, 1]`); the element fades up to full
    /// opacity as the ancestor's hover envelope animates toward `1.0`.
    /// Self-hover composes via `max`, so mousing directly onto the
    /// element keeps it visible after the ancestor loses hover.
    ///
    /// Layout-neutral — the element keeps its computed rect at all
    /// times. Use this for hover-revealed close buttons, secondary
    /// actions on list rows, hover-only validation icons, and other
    /// "show on hover" patterns where the surrounding layout
    /// shouldn't shift on hover.
    pub fn reveal_on_hover(mut self, rest_opacity: f32) -> Self {
        self.reveal_on_hover = Some(rest_opacity.clamp(0.0, 1.0));
        self
    }

    pub fn at(mut self, file: &'static str, line: u32) -> Self {
        self.source = Source { file, line };
        self
    }

    /// Set source from a `Location` (used internally by
    /// `#[track_caller]` constructors).
    pub fn at_loc(mut self, loc: &'static Location<'static>) -> Self {
        self.source = Source::from_caller(loc);
        self
    }
}
