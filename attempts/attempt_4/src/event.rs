//! Event routing, hit-testing, and the [`App`] trait.
//!
//! The v0.2 application layer: state-driven rebuild + click events +
//! automatic hover/press visuals. See `LIBRARY_VISION.md` for the
//! shape this fits into.
//!
//! # The model
//!
//! ```ignore
//! struct Counter { value: i32 }
//!
//! impl App for Counter {
//!     fn build(&self) -> El {
//!         column([
//!             h1(format!("{}", self.value)),
//!             row([
//!                 button("-").key("dec"),
//!                 button("+").key("inc"),
//!             ]),
//!         ])
//!     }
//!     fn on_event(&mut self, e: UiEvent) {
//!         match (e.kind, e.key.as_deref()) {
//!             (UiEventKind::Click, Some("inc")) => self.value += 1,
//!             (UiEventKind::Click, Some("dec")) => self.value -= 1,
//!             _ => {}
//!         }
//!     }
//! }
//! ```
//!
//! - **Identity** is `El::key`. Tag a node with `.key("...")` and it's
//!   hit-testable (and gets automatic hover/press visuals).
//! - **The build closure is pure.** It reads `&self`, returns a fresh
//!   tree. The library tracks pointer state, hovered key, pressed key
//!   internally and applies visual deltas after build but before layout
//!   completes.
//! - **Events flow back via `on_event`.** The library hit-tests pointer
//!   events against the most-recently-laid-out tree and emits
//!   [`UiEvent`]s when something is clicked. The host's `App::on_event`
//!   updates state; the library schedules a redraw.
//!
//! # What about hover, press, focus state?
//!
//! Author never writes `.hovered()` or `.pressed()` in `build`. The
//! library walks the tree after `build` and sets `state = Hover` on the
//! node whose key matches the pointer's current hit, then `Press` if
//! the pointer is also down. The visual deltas (lighten on hover,
//! darken on press) flow through the existing `draw_ops::apply_state`
//! path.
//!
//! # Limits in v0.2
//!
//! - Click only — no double-click, drag, scroll, key events. `UiEventKind`
//!   is an enum so adding more variants is non-breaking.
//! - Hit-testing returns the topmost keyed node. Nodes without a key
//!   are transparent to events.
//! - No focus traversal yet (Tab/Shift-Tab). Click can become a focus
//!   source later.

use crate::tree::{El, InteractionState, Rect};

/// User-facing event. The host's [`App::on_event`] receives one of these
/// per discrete user action (click, key press, scroll wheel tick, …).
#[derive(Clone, Debug)]
pub struct UiEvent {
    /// The `key` of the node the event was routed to, if any. `None`
    /// for events with no specific target (e.g., a window-level
    /// keyboard event).
    pub key: Option<String>,
    pub kind: UiEventKind,
}

/// What kind of event happened. Open enum — start with click, grow
/// non-breakingly as the library does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiEventKind {
    /// Pointer down + up landed on the same node.
    Click,
}

/// The application contract. Implement this on your state struct and
/// pass it to the host runner (e.g., `attempt_4_demo::run`).
pub trait App {
    /// Project current state into a scene tree. Called whenever the
    /// host requests a redraw. Pure — no I/O, no mutation.
    fn build(&self) -> El;

    /// Update state in response to a routed event. Default: no-op.
    fn on_event(&mut self, _event: UiEvent) {}
}

/// Internal UI state — pointer position, hovered key, pressed key.
/// Owned by [`crate::wgpu_render::UiRenderer`]; the host doesn't
/// interact with this directly.
///
/// Visual delta application: if `pressed_key` is set, that node renders
/// with `state = Press`. Otherwise, if `hovered_key` is set, that node
/// renders with `state = Hover`. Press takes precedence so clicking a
/// button that's also hovered shows the press visual, not the hover
/// visual.
#[derive(Default, Debug)]
pub struct UiState {
    /// Last known pointer position in **logical** pixels. `None` until
    /// the pointer enters the window.
    pub pointer_pos: Option<(f32, f32)>,
    pub hovered_key: Option<String>,
    pub pressed_key: Option<String>,
}

impl UiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk the tree and set `state` on nodes whose keys match the
    /// current hovered/pressed trackers. Press wins over hover.
    pub fn apply_to_tree(&self, root: &mut El) {
        if let Some(k) = self.pressed_key.as_deref() {
            set_state_for_key(root, k, InteractionState::Press);
        }
        if let Some(k) = self.hovered_key.as_deref() {
            // Don't overwrite a press visual if both happen to match.
            if Some(k) != self.pressed_key.as_deref() {
                set_state_for_key(root, k, InteractionState::Hover);
            }
        }
    }
}

/// Find the topmost keyed node whose laid-out rect contains `point`
/// (logical pixels). Returns `None` if the point hits no keyed node.
///
/// Walks children in reverse paint order so a button on top of a card
/// is preferred over the card. Only nodes with `key.is_some()` are
/// hit-test targets — author intent is "I tagged it with a key, it's
/// interactive."
pub fn hit_test(root: &El, point: (f32, f32)) -> Option<String> {
    hit_test_rec(root, point)
}

fn hit_test_rec(node: &El, point: (f32, f32)) -> Option<String> {
    if !rect_contains(node.computed, point) {
        return None;
    }
    // Children paint last → are on top → check first.
    for child in node.children.iter().rev() {
        if let Some(hit) = hit_test_rec(child, point) {
            return Some(hit);
        }
    }
    // No child hit. Self counts only if it has a key.
    node.key.clone()
}

fn rect_contains(r: Rect, (x, y): (f32, f32)) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

fn set_state_for_key(node: &mut El, key: &str, state: InteractionState) -> bool {
    if node.key.as_deref() == Some(key) {
        node.state = state;
        return true;
    }
    for child in &mut node.children {
        if set_state_for_key(child, key, state) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::tree::*;
    use crate::{button, column, row};

    fn lay_out_counter() -> El {
        let mut tree = column([
            crate::text("0"),
            row([
                button("-").key("dec"),
                button("+").key("inc"),
            ]),
        ])
        .padding(20.0);
        layout(&mut tree, Rect::new(0.0, 0.0, 400.0, 200.0));
        tree
    }

    #[test]
    fn hit_test_finds_keyed_button() {
        let tree = lay_out_counter();
        // Walk the tree to find each keyed node's center, hit-test it.
        for key in &["dec", "inc"] {
            let r = find_rect(&tree, key).expect("button rect");
            let center = (r.x + r.w * 0.5, r.y + r.h * 0.5);
            let hit = hit_test(&tree, center);
            assert_eq!(hit.as_deref(), Some(*key), "hit-test {key} returned {hit:?}");
        }
    }

    #[test]
    fn hit_test_misses_unkeyed_text() {
        let tree = lay_out_counter();
        // The "0" heading has no key — clicking it should hit nothing.
        let r = find_text_rect(&tree).expect("text rect");
        let center = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        assert!(hit_test(&tree, center).is_none());
    }

    #[test]
    fn hit_test_outside_returns_none() {
        let tree = lay_out_counter();
        assert!(hit_test(&tree, (-10.0, -10.0)).is_none());
        assert!(hit_test(&tree, (9999.0, 9999.0)).is_none());
    }

    #[test]
    fn ui_state_applies_hover() {
        let mut tree = lay_out_counter();
        let state = UiState {
            pointer_pos: None,
            hovered_key: Some("inc".into()),
            pressed_key: None,
        };
        state.apply_to_tree(&mut tree);
        assert_eq!(node_state(&tree, "inc"), Some(InteractionState::Hover));
        assert_eq!(node_state(&tree, "dec"), Some(InteractionState::Default));
    }

    #[test]
    fn ui_state_press_wins_over_hover_on_same_key() {
        let mut tree = lay_out_counter();
        let state = UiState {
            pointer_pos: None,
            hovered_key: Some("inc".into()),
            pressed_key: Some("inc".into()),
        };
        state.apply_to_tree(&mut tree);
        assert_eq!(node_state(&tree, "inc"), Some(InteractionState::Press));
    }

    fn find_rect(node: &El, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) { return Some(node.computed); }
        node.children.iter().find_map(|c| find_rect(c, key))
    }
    fn find_text_rect(node: &El) -> Option<Rect> {
        if matches!(node.kind, Kind::Text) { return Some(node.computed); }
        node.children.iter().find_map(find_text_rect)
    }
    fn node_state(node: &El, key: &str) -> Option<InteractionState> {
        if node.key.as_deref() == Some(key) { return Some(node.state); }
        node.children.iter().find_map(|c| node_state(c, key))
    }
}
