//! App-side scroll request types.
//!
//! Apps push [`ScrollRequest`]s via [`crate::App::drain_scroll_requests`];
//! the layout pass resolves each one against the matching virtual list's
//! row geometry once viewport height and the row-height cache are
//! available, then writes the resulting offset into the scroll state so
//! the same frame's render reflects the new position.
//!
//! Mirrors [`crate::toast`]: the App produces fire-and-forget descriptors
//! and the runtime resolves them with state that's only fully known
//! mid-frame.

/// Where in the viewport a [`ScrollRequest`] should land its target row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollAlignment {
    /// Top of the row aligns with the top of the viewport.
    Start,
    /// Centre of the row aligns with the centre of the viewport.
    Center,
    /// Bottom of the row aligns with the bottom of the viewport.
    End,
    /// No-op if the row already fits entirely inside the viewport;
    /// otherwise scroll the minimum amount that brings it into view
    /// (i.e., align to the nearer edge).
    Visible,
}

/// What the app produces from [`crate::App::drain_scroll_requests`]:
/// "scroll the virtual list keyed `list_key` so row `row` lands per
/// `align`." The runtime resolves the target offset during layout of the
/// matching list using the live viewport rect and row-height cache.
#[derive(Clone, Debug)]
pub struct ScrollRequest {
    pub list_key: String,
    pub row: usize,
    pub align: ScrollAlignment,
}

impl ScrollRequest {
    pub fn new(list_key: impl Into<String>, row: usize, align: ScrollAlignment) -> Self {
        Self {
            list_key: list_key.into(),
            row,
            align,
        }
    }
}
