//! Scroll offset, scrollbar, and wheel helpers for [`UiState`](super::UiState).

use crate::hit_test::scroll_target_at;
use crate::tree::{El, Rect};

use super::UiState;

impl UiState {
    /// Seed or read the persistent scroll offset for `id`. Use this to
    /// pre-position a scroll viewport before [`crate::layout::layout`]
    /// runs (call [`crate::layout::assign_ids`] first to populate the
    /// node's `computed_id`).
    pub fn set_scroll_offset(&mut self, id: impl Into<String>, value: f32) {
        self.scroll.offsets.insert(id.into(), value);
    }

    /// Read the current scroll offset for `id`. Defaults to `0.0`.
    pub fn scroll_offset(&self, id: &str) -> f32 {
        self.scroll.offsets.get(id).copied().unwrap_or(0.0)
    }

    /// Iterate `(scroll_node_id, track_rect)` for every scrollable
    /// whose visible scrollbar is currently active. Hosts use this to
    /// drive cursor changes (e.g., a vertical-resize cursor over the
    /// thumb), to drive screenshot tools, or to test interaction
    /// flows. The map is rebuilt every layout pass.
    pub fn scrollbar_tracks(&self) -> impl Iterator<Item = (&str, &Rect)> {
        self.scroll
            .thumb_tracks
            .iter()
            .map(|(id, rect)| (id.as_str(), rect))
    }

    /// Look up the scrollable whose track rect contains `(x, y)`,
    /// returning its `computed_id`, the track rect, and the visible
    /// thumb rect. Returns `None` if no track is currently visible at
    /// that point. The track rect is wider than the visible thumb
    /// (Fitts's law) and spans the full viewport height so callers
    /// can branch on whether `y` lands inside the thumb (grab) or
    /// above/below (click-to-page).
    pub fn thumb_at(&self, x: f32, y: f32) -> Option<(String, Rect, Rect)> {
        for (id, track) in &self.scroll.thumb_tracks {
            if track.contains(x, y) {
                let thumb = self
                    .scroll
                    .thumb_rects
                    .get(id)
                    .copied()
                    .unwrap_or_else(|| Rect::new(track.x, track.y, track.w, 0.0));
                return Some((id.clone(), *track, thumb));
            }
        }
        None
    }

    /// Increment the scroll offset for the deepest scrollable container
    /// containing `point`. Returns `true` if any scrollable was hit and
    /// updated (host can use this to decide whether to request a redraw).
    pub fn pointer_wheel(&mut self, root: &El, point: (f32, f32), dy: f32) -> bool {
        if let Some(id) = scroll_target_at(root, self, point) {
            *self.scroll.offsets.entry(id).or_insert(0.0) += dy;
            true
        } else {
            false
        }
    }
}
