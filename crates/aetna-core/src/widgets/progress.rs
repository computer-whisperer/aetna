//! Progress — a non-interactive horizontal bar showing how full a
//! `0.0..=1.0` value is. Shaped like the shadcn / Radix Progress
//! primitive, scaled down to a single `progress(value)` builder
//! because Aetna progress bars don't need to advertise their
//! indeterminate or label-bearing state — apps compose those around
//! the bar.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! struct Storage { used_pct: u32 }
//!
//! impl App for Storage {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         column([
//!             row([
//!                 text("Storage").label(),
//!                 spacer(),
//!                 text(format!("{}%", self.used_pct)).muted(),
//!             ]),
//!             progress(self.used_pct as f32 / 100.0),
//!         ])
//!     }
//! }
//! ```
//!
//! Progress paints the same way as the slider track + fill, minus the
//! thumb. There's no `apply_event` because the widget is read-only —
//! apps update the underlying value through whatever channel they
//! own (timer tick, async snapshot, computed metric, ...).

use std::panic::Location;

use crate::layout::LayoutCtx;
use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;

/// Default bar height in logical pixels.
pub const DEFAULT_HEIGHT: f32 = 8.0;

/// A horizontal progress bar. `value` is clamped to `0.0..=1.0`; the
/// returned `El` defaults to filling its container's width and a
/// fixed [`DEFAULT_HEIGHT`]. Override with `.height(...)` /
/// `.width(...)` like any El.
///
/// Pass `tokens::PRIMARY`, `tokens::SUCCESS`, etc. via `fill_color`
/// to vary the visible portion's color (e.g. switch to
/// `tokens::DESTRUCTIVE` when the value crosses a "near full"
/// threshold).
#[track_caller]
pub fn progress(value: f32, fill_color: Color) -> El {
    let value = value.clamp(0.0, 1.0);
    let layout = move |ctx: LayoutCtx| {
        let r = ctx.container;
        vec![
            // Track spans the full container.
            Rect::new(r.x, r.y, r.w, r.h),
            // Fill spans the portion proportional to value.
            Rect::new(r.x, r.y, r.w * value, r.h),
        ]
    };

    stack([
        El::new(Kind::Custom("progress-track"))
            .fill(tokens::BG_MUTED)
            .radius(tokens::RADIUS_PILL),
        El::new(Kind::Custom("progress-fill"))
            .fill(fill_color)
            .radius(tokens::RADIUS_PILL),
    ])
    .at_loc(Location::caller())
    .metrics_role(MetricsRole::Progress)
    .layout(layout)
    .width(Size::Fill(1.0))
    .default_height(Size::Fixed(DEFAULT_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_and_fill_use_expected_tokens() {
        let p = progress(0.5, tokens::PRIMARY);
        assert_eq!(p.children.len(), 2);
        assert_eq!(p.children[0].fill, Some(tokens::BG_MUTED), "track is muted");
        assert_eq!(
            p.children[1].fill,
            Some(tokens::PRIMARY),
            "fill uses caller's color"
        );
        // Both rounded pills so the bar reads as one piece.
        assert_eq!(p.children[0].radius, tokens::RADIUS_PILL);
        assert_eq!(p.children[1].radius, tokens::RADIUS_PILL);
    }

    #[test]
    fn layout_clamps_value_below_zero() {
        // The visible result of a clamped value is the fill rect's
        // width, so verify the layout closure end-to-end.
        use crate::layout::layout;
        use crate::state::UiState;

        let mut tree = progress(-0.5, tokens::PRIMARY);
        let mut state = UiState::new();
        let viewport = Rect::new(0.0, 0.0, 200.0, DEFAULT_HEIGHT);
        layout(&mut tree, &mut state, viewport);
        let fill_rect = state.rect(&tree.children[1].computed_id);
        assert_eq!(fill_rect.w, 0.0, "negative values clamp to empty fill");
    }

    #[test]
    fn layout_clamps_value_above_one() {
        use crate::layout::layout;
        use crate::state::UiState;

        let mut tree = progress(1.5, tokens::PRIMARY);
        let mut state = UiState::new();
        let viewport = Rect::new(0.0, 0.0, 200.0, DEFAULT_HEIGHT);
        layout(&mut tree, &mut state, viewport);
        let fill_rect = state.rect(&tree.children[1].computed_id);
        assert_eq!(fill_rect.w, 200.0, "values above 1.0 clamp to full track");
    }

    #[test]
    fn layout_fills_proportionally_to_value() {
        use crate::layout::layout;
        use crate::state::UiState;

        let mut tree = progress(0.25, tokens::PRIMARY);
        let mut state = UiState::new();
        let viewport = Rect::new(0.0, 0.0, 200.0, DEFAULT_HEIGHT);
        layout(&mut tree, &mut state, viewport);
        let fill_rect = state.rect(&tree.children[1].computed_id);
        assert!(
            (fill_rect.w - 50.0).abs() < 1e-3,
            "0.25 * 200 = 50; got {}",
            fill_rect.w
        );
    }
}
