//! Identity, source, and interaction-flag modifiers for [`El`].

use std::panic::Location;

use super::node::El;
use super::semantics::{Kind, Source};

/// Configuration for [`El::hover_alpha`] — the rest and peak alpha
/// endpoints for a node whose opacity binds to the **subtree
/// interaction envelope** (max of hover, focus, and press over the
/// subtree rooted at this node).
///
/// `rest` is the drawn alpha when no descendant of this node is
/// currently the active hover, focus, or press target. `peak` is the
/// drawn alpha at full envelope. Linear interpolation between the two
/// follows the eased subtree envelope (0..1).
///
/// Both fields are clamped to `[0.0, 1.0]` by [`El::hover_alpha`].
/// Typical use is `rest < peak` ("reveal on interaction"), but the
/// representation accepts `rest > peak` ("fade out on interaction") and
/// sub-1.0 peaks for subtle affordances.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverAlpha {
    pub rest: f32,
    pub peak: f32,
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

    /// Attach source-backed copy/hit-test text for this selectable
    /// node. The node still needs `.selectable().key(...)`; this only
    /// changes how selection offsets map to copied text.
    pub fn selection_source(mut self, source: crate::selection::SelectionSource) -> Self {
        self.selection_source = Some(source);
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

    /// Bind this element's paint opacity to the subtree interaction
    /// envelope — the `max` of hover, focus, and press for the subtree
    /// rooted at this element.
    ///
    /// At rest (no descendant is the active hover, focus, or press
    /// target) the element paints at `rest`. At full envelope it paints
    /// at `peak`. Both are clamped to `[0.0, 1.0]`, with linear
    /// interpolation in between following the eased envelope.
    ///
    /// "Subtree" matches CSS `:hover` semantics: hovering, focusing, or
    /// pressing *any descendant* keeps the element revealed. A
    /// hover-revealed close icon stays visible while the cursor moves
    /// across the tab body or while the tab is keyboard-focused; an
    /// action pill stays visible while the cursor moves between
    /// focusable buttons inside it. The trigger isn't strictly
    /// "hover" — focus and press also count — but `hover` is the
    /// dominant case and the name reflects it.
    ///
    /// Layout-neutral — the element keeps its computed rect at all
    /// times. Use for hover-revealed close buttons, secondary actions
    /// on list rows, hover-only validation icons, and other
    /// "show on interaction" patterns where the surrounding layout
    /// shouldn't shift.
    ///
    /// # Beyond alpha
    ///
    /// For the other common hover affordances — Material-style lift
    /// (`translate_y`), button-pop (`scale`), tint shift (`fill`) —
    /// drive the prop from app code using
    /// [`crate::BuildCx::is_hovering_within`] plus
    /// [`Self::animate`]:
    ///
    /// ```ignore
    /// fn build(&self, cx: &BuildCx) -> El {
    ///     let lifted = cx.is_hovering_within("card");
    ///     card([...])
    ///         .key("card")
    ///         .focusable()
    ///         .translate(0.0, if lifted { -2.0 } else { 0.0 })
    ///         .scale(if lifted { 1.02 } else { 1.0 })
    ///         .animate(Timing::SPRING_QUICK)
    /// }
    /// ```
    ///
    /// `is_hovering_within` reads the same subtree predicate
    /// `hover_alpha` consumes (CSS `:hover`-style cascade). `animate`
    /// eases the prop between the two build values across frames, so
    /// the transition is smooth without per-channel declarative API.
    /// `hover_alpha` itself is the alpha-channel shorthand — it skips
    /// the boolean-to-value conversion and the per-node `animate`
    /// allocation, since alpha is the dominant hover affordance.
    pub fn hover_alpha(mut self, rest: f32, peak: f32) -> Self {
        self.hover_alpha = Some(HoverAlpha {
            rest: rest.clamp(0.0, 1.0),
            peak: peak.clamp(0.0, 1.0),
        });
        self
    }

    pub fn at(mut self, file: &'static str, line: u32) -> Self {
        self.source = Source {
            file,
            line,
            from_library: false,
        };
        self
    }

    /// Set source from a `Location` (used internally by
    /// `#[track_caller]` constructors).
    pub fn at_loc(mut self, loc: &'static Location<'static>) -> Self {
        self.source = Source::from_caller(loc);
        self
    }

    /// Mark this El as constructed inside an aetna library closure
    /// where `#[track_caller]` doesn't reach user code (e.g. the
    /// `.map(|item| ...)` body inside `tabs_list`, `radio_group`,
    /// etc.). The lint pass uses this flag to walk blame attribution
    /// upward to the nearest user-source ancestor instead of pointing
    /// findings at aetna-core internals. User code never needs to call
    /// this.
    pub fn from_library(mut self) -> Self {
        self.source.from_library = true;
        self
    }
}
