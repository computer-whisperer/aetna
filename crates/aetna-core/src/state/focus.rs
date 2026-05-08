//! Focus traversal helpers for [`UiState`](super::UiState).

use web_time::Instant;

use crate::event::UiTarget;
use crate::focus::focus_order;
use crate::tree::El;

use super::UiState;

impl UiState {
    pub fn sync_focus_order(&mut self, root: &El) {
        let order = focus_order(root, self);
        self.focus.order = order;
        if let Some(focused) = &self.focused {
            if let Some(current) = self
                .focus
                .order
                .iter()
                .find(|t| t.node_id == focused.node_id)
            {
                self.focused = Some(current.clone());
                return;
            }
            self.focused = None;
        }
    }

    pub fn set_focus(&mut self, target: Option<UiTarget>) {
        if let Some(target) =
            target.filter(|t| self.focus.order.iter().any(|f| f.node_id == t.node_id))
        {
            let changed = self.focused.as_ref().map(|f| &f.node_id) != Some(&target.node_id);
            self.focused = Some(target);
            if changed {
                self.bump_caret_activity(Instant::now());
            }
        }
    }

    /// Set whether the current focus should display its focus ring.
    /// The runtime calls this from input-handling paths: pointer-down
    /// clears it (`false`), Tab and arrow-nav raise it (`true`). Apps
    /// that move focus programmatically can also flip it explicitly,
    /// e.g. force the ring on after restoring focus from an off-screen
    /// menu close. See [`UiState::focus_visible`].
    pub fn set_focus_visible(&mut self, visible: bool) {
        self.focus_visible = visible;
    }

    pub fn focus_next(&mut self) -> Option<&UiTarget> {
        self.move_focus(1)
    }

    pub fn focus_prev(&mut self) -> Option<&UiTarget> {
        self.move_focus(-1)
    }

    fn move_focus(&mut self, delta: isize) -> Option<&UiTarget> {
        if self.focus.order.is_empty() {
            self.focused = None;
            return None;
        }
        let current = self.focused.as_ref().and_then(|target| {
            self.focus
                .order
                .iter()
                .position(|t| t.node_id == target.node_id)
        });
        let len = self.focus.order.len() as isize;
        let next = match current {
            Some(current) => (current as isize + delta).rem_euclid(len) as usize,
            None if delta < 0 => self.focus.order.len() - 1,
            None => 0,
        };
        self.focused = Some(self.focus.order[next].clone());
        self.focused.as_ref()
    }
}
