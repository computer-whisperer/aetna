//! Scroll offset, scrollbar, and wheel helpers for [`UiState`](super::UiState).

use crate::hit_test::scroll_targets_at;
use crate::tree::{El, Rect};

use super::UiState;

const WHEEL_EPSILON: f32 = 0.5;

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

    /// Queue programmatic scroll-to-row requests targeting virtual
    /// lists by key. Each request is consumed during layout of the
    /// matching list — viewport height and row heights are only known
    /// then, especially for `virtual_list_dyn` where unmeasured rows
    /// use the configured estimate. Hosts call this once per frame
    /// from [`crate::event::App::drain_scroll_requests`]; apps that
    /// own a `Runner` can also push directly for tests.
    pub fn push_scroll_requests(&mut self, requests: Vec<crate::scroll::ScrollRequest>) {
        self.scroll.pending_requests.extend(requests);
    }

    /// Drop any scroll requests still queued after the layout pass
    /// completed. Called by `prepare_layout` so requests targeting a
    /// list that wasn't in the tree this frame don't silently fire
    /// against a re-mounted list with the same key on a later frame.
    pub fn clear_pending_scroll_requests(&mut self) {
        self.scroll.pending_requests.clear();
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
    /// under `point` that can move in `dy`'s direction. If the deepest
    /// container is already at that edge (or has no overflow), the wheel
    /// bubbles to the nearest scrollable ancestor that can move.
    ///
    /// Returns `true` if any scrollable consumed the wheel and updated
    /// its stored offset. Hosts use this to decide whether to request a
    /// redraw.
    pub fn pointer_wheel(&mut self, root: &El, point: (f32, f32), dy: f32) -> bool {
        if dy.abs() <= f32::EPSILON {
            return false;
        }
        let targets = scroll_targets_at(root, self, point);
        for id in targets.into_iter().rev() {
            let Some(metrics) = self.scroll.metrics.get(&id).copied() else {
                continue;
            };
            if metrics.max_offset <= WHEEL_EPSILON {
                continue;
            }
            let current = self
                .scroll
                .offsets
                .get(&id)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, metrics.max_offset);
            let can_scroll = if dy > 0.0 {
                current < metrics.max_offset - WHEEL_EPSILON
            } else {
                current > WHEEL_EPSILON
            };
            if !can_scroll {
                continue;
            }
            let next = (current + dy).clamp(0.0, metrics.max_offset);
            if (next - current).abs() <= f32::EPSILON {
                continue;
            }
            self.scroll.offsets.insert(id, next);
            return true;
        }
        false
    }
}
