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
//! # Limits in v0.4
//!
//! - Click, focus traversal, activation, and key-down routing only — no
//!   double-click, drag, or scroll events yet. `UiEventKind` is an enum
//!   so adding more variants is non-breaking.
//! - Hit-testing returns the topmost keyed node. Nodes without a key
//!   are transparent to events.
//! - Focus traversal is linear through focusable keyed nodes. Rich
//!   composites can layer roving focus on top later.

use std::collections::HashMap;

use crate::tree::{El, InteractionState, Rect};

/// Hit-test target metadata. `key` is the author-facing route, while
/// `node_id` is the stable laid-out tree path used by artifacts.
#[derive(Clone, Debug, PartialEq)]
pub struct UiTarget {
    pub key: String,
    pub node_id: String,
    pub rect: Rect,
}

/// Keyboard key values normalized by the core library. This keeps
/// `attempt_4` independent from host/windowing crates while covering the
/// navigation and activation keys the library owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiKey {
    Enter,
    Escape,
    Tab,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Character(String),
    Other(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyPress {
    pub key: UiKey,
    pub modifiers: KeyModifiers,
    pub repeat: bool,
}

/// User-facing event. The host's [`App::on_event`] receives one of these
/// per discrete user action (click, key press, scroll wheel tick, …).
#[derive(Clone, Debug)]
pub struct UiEvent {
    /// The `key` of the node the event was routed to, if any. `None`
    /// for events with no specific target (e.g., a window-level
    /// keyboard event).
    pub key: Option<String>,
    /// Full hit-test target for events routed to a concrete element.
    pub target: Option<UiTarget>,
    /// Pointer position in logical pixels when the event was emitted.
    pub pointer: Option<(f32, f32)>,
    /// Keyboard payload for key events.
    pub key_press: Option<KeyPress>,
    pub kind: UiEventKind,
}

/// What kind of event happened. Open enum — start with click, grow
/// non-breakingly as the library does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiEventKind {
    /// Pointer down + up landed on the same node.
    Click,
    /// Focused element was activated by keyboard (Enter/Space).
    Activate,
    /// Escape was pressed. Routed to the focused element when present,
    /// otherwise emitted as a window-level event.
    Escape,
    /// Other keyboard input.
    KeyDown,
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
    pub hovered: Option<UiTarget>,
    pub pressed: Option<UiTarget>,
    pub focused: Option<UiTarget>,
    focus_order: Vec<UiTarget>,
    /// Scroll offset (logical pixels) per scrollable node, keyed by
    /// `El::computed_id`. The library applies these to the tree before
    /// layout; the layout pass clamps them to the available range and
    /// the renderer writes the clamped values back here.
    scroll_offsets: HashMap<String, f32>,
}

impl UiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Walk the tree and set `state` on nodes whose keys match the
    /// current hovered/pressed trackers. Press wins over hover.
    pub fn apply_to_tree(&self, root: &mut El) {
        if let Some(target) = &self.focused {
            set_state_for_target(root, target, InteractionState::Focus);
        }
        if let Some(target) = &self.pressed {
            set_state_for_target(root, target, InteractionState::Press);
        }
        if let Some(target) = &self.hovered {
            // Don't overwrite a press visual if both happen to match.
            if self.pressed.as_ref().map(|p| p.node_id.as_str()) != Some(target.node_id.as_str())
                && self.focused.as_ref().map(|p| p.node_id.as_str())
                    != Some(target.node_id.as_str())
            {
                set_state_for_target(root, target, InteractionState::Hover);
            }
        }
    }

    pub fn sync_focus_order(&mut self, root: &El) {
        self.focus_order = focus_order(root);
        if let Some(focused) = &self.focused {
            if let Some(current) = self
                .focus_order
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
            target.filter(|t| self.focus_order.iter().any(|f| f.node_id == t.node_id))
        {
            self.focused = Some(target);
        }
    }

    pub fn focus_next(&mut self) -> Option<&UiTarget> {
        self.move_focus(1)
    }

    pub fn focus_prev(&mut self) -> Option<&UiTarget> {
        self.move_focus(-1)
    }

    /// Copy stored scroll offsets onto the matching scrollable nodes so
    /// the layout pass can use them. Call after `assign_ids` and before
    /// `layout`. Nodes without a stored offset get `0.0`.
    pub fn apply_scroll_to_tree(&self, root: &mut El) {
        apply_scroll_rec(root, &self.scroll_offsets);
    }

    /// Walk the laid-out tree and read the (now-clamped) `scroll_offset_y`
    /// values back. Call after `layout`. Removes entries for ids that no
    /// longer exist in the tree so stale offsets don't pile up across
    /// rebuilds.
    pub fn read_scroll_from_tree(&mut self, root: &El) {
        let mut next = HashMap::new();
        collect_scroll_rec(root, &mut next);
        self.scroll_offsets = next;
    }

    /// Increment the scroll offset for the deepest scrollable container
    /// containing `point`. Returns `true` if any scrollable was hit and
    /// updated (host can use this to decide whether to request a redraw).
    pub fn pointer_wheel(&mut self, root: &El, point: (f32, f32), dy: f32) -> bool {
        if let Some(id) = scroll_target_at(root, point) {
            *self.scroll_offsets.entry(id).or_insert(0.0) += dy;
            true
        } else {
            false
        }
    }

    pub fn key_down(
        &mut self,
        key: UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        if matches!(key, UiKey::Tab) {
            if modifiers.shift {
                self.focus_prev();
            } else {
                self.focus_next();
            }
            return None;
        }

        let target = self.focused.clone();
        let kind = match (&key, target.is_some()) {
            (UiKey::Enter | UiKey::Space, true) => UiEventKind::Activate,
            (UiKey::Escape, _) => UiEventKind::Escape,
            _ => UiEventKind::KeyDown,
        };
        Some(UiEvent {
            key: target.as_ref().map(|t| t.key.clone()),
            target,
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers,
                repeat,
            }),
            kind,
        })
    }

    fn move_focus(&mut self, delta: isize) -> Option<&UiTarget> {
        if self.focus_order.is_empty() {
            self.focused = None;
            return None;
        }
        let current = self.focused.as_ref().and_then(|target| {
            self.focus_order
                .iter()
                .position(|t| t.node_id == target.node_id)
        });
        let len = self.focus_order.len() as isize;
        let next = match current {
            Some(current) => (current as isize + delta).rem_euclid(len) as usize,
            None if delta < 0 => self.focus_order.len() - 1,
            None => 0,
        };
        self.focused = Some(self.focus_order[next].clone());
        self.focused.as_ref()
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
    hit_test_target(root, point).map(|target| target.key)
}

/// Find the topmost keyed node and return full target metadata.
pub fn hit_test_target(root: &El, point: (f32, f32)) -> Option<UiTarget> {
    match hit_test_rec(root, point, None) {
        Hit::Target(target) => Some(target),
        Hit::Blocked | Hit::Miss => None,
    }
}

pub fn focus_order(root: &El) -> Vec<UiTarget> {
    let mut out = Vec::new();
    collect_focus(root, None, &mut out);
    out
}

fn collect_focus(node: &El, inherited_clip: Option<Rect>, out: &mut Vec<UiTarget>) {
    let clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(node.computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(node.computed),
        }
    } else {
        inherited_clip
    };
    if node.focusable {
        if let Some(key) = &node.key {
            if clip
                .map(|c| c.intersect(node.computed).is_some())
                .unwrap_or(true)
            {
                out.push(UiTarget {
                    key: key.clone(),
                    node_id: node.computed_id.clone(),
                    rect: node.computed,
                });
            }
        }
    }
    for child in &node.children {
        collect_focus(child, clip, out);
    }
}

enum Hit {
    Target(UiTarget),
    Blocked,
    Miss,
}

fn hit_test_rec(node: &El, point: (f32, f32), inherited_clip: Option<Rect>) -> Hit {
    if let Some(clip) = inherited_clip {
        if !clip.contains(point.0, point.1) {
            return Hit::Miss;
        }
    }
    if !node.computed.contains(point.0, point.1) {
        return Hit::Miss;
    }
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(node.computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(node.computed),
        }
    } else {
        inherited_clip
    };
    // Children paint last → are on top → check first.
    for child in node.children.iter().rev() {
        match hit_test_rec(child, point, child_clip) {
            Hit::Target(target) => return Hit::Target(target),
            Hit::Blocked => return Hit::Blocked,
            Hit::Miss => {}
        }
    }
    // No child hit. Self counts only if it has a key.
    if let Some(key) = &node.key {
        return Hit::Target(UiTarget {
            key: key.clone(),
            node_id: node.computed_id.clone(),
            rect: node.computed,
        });
    }
    if node.block_pointer {
        return Hit::Blocked;
    }
    Hit::Miss
}

fn apply_scroll_rec(node: &mut El, offsets: &HashMap<String, f32>) {
    if node.scrollable {
        node.scroll_offset_y = offsets.get(&node.computed_id).copied().unwrap_or(0.0);
    }
    for c in &mut node.children {
        apply_scroll_rec(c, offsets);
    }
}

fn collect_scroll_rec(node: &El, out: &mut HashMap<String, f32>) {
    if node.scrollable && node.scroll_offset_y != 0.0 {
        out.insert(node.computed_id.clone(), node.scroll_offset_y);
    }
    for c in &node.children {
        collect_scroll_rec(c, out);
    }
}

/// Return the `computed_id` of the deepest scrollable container whose
/// `computed` rect contains `point`, respecting clipping ancestors.
/// Used to route wheel events.
fn scroll_target_at(root: &El, point: (f32, f32)) -> Option<String> {
    let mut hit = None;
    scroll_target_rec(root, point, None, &mut hit);
    hit
}

fn scroll_target_rec(
    node: &El,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    out: &mut Option<String>,
) {
    if let Some(clip) = inherited_clip {
        if !clip.contains(point.0, point.1) {
            return;
        }
    }
    if !node.computed.contains(point.0, point.1) {
        return;
    }
    if node.scrollable {
        *out = Some(node.computed_id.clone());
    }
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(node.computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(node.computed),
        }
    } else {
        inherited_clip
    };
    for c in &node.children {
        scroll_target_rec(c, point, child_clip, out);
    }
}

fn set_state_for_target(node: &mut El, target: &UiTarget, state: InteractionState) -> bool {
    if node.computed_id == target.node_id {
        node.state = state;
        return true;
    }
    for child in &mut node.children {
        if set_state_for_target(child, target, state) {
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
    use crate::{button, column, row, scroll};

    fn lay_out_counter() -> El {
        let mut tree = column([
            crate::text("0"),
            row([button("-").key("dec"), button("+").key("inc")]),
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
            assert_eq!(
                hit.as_deref(),
                Some(*key),
                "hit-test {key} returned {hit:?}"
            );
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
            hovered: Some(target(&tree, "inc")),
            pressed: None,
            focused: None,
            focus_order: Vec::new(),
            scroll_offsets: HashMap::new(),
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
            hovered: Some(target(&tree, "inc")),
            pressed: Some(target(&tree, "inc")),
            focused: None,
            focus_order: Vec::new(),
            scroll_offsets: HashMap::new(),
        };
        state.apply_to_tree(&mut tree);
        assert_eq!(node_state(&tree, "inc"), Some(InteractionState::Press));
    }

    #[test]
    fn hit_test_respects_clipping_ancestor() {
        let mut tree = column([row([
            button("-").key("visible"),
            button("+").key("clipped").width(Size::Fixed(240.0)),
        ])
        .clip()
        .width(Size::Fixed(80.0))]);
        layout(&mut tree, Rect::new(0.0, 0.0, 400.0, 100.0));

        let clipped = find_rect(&tree, "clipped").expect("clipped button rect");
        assert!(hit_test(&tree, (clipped.center_x(), clipped.center_y())).is_none());
    }

    #[test]
    fn unkeyed_blocking_node_stops_fallthrough() {
        let mut tree = stack([
            El::new(Kind::Scrim)
                .key("dismiss")
                .fill(crate::tokens::OVERLAY_SCRIM),
            El::new(Kind::Modal)
                .block_pointer()
                .width(Size::Fixed(100.0))
                .height(Size::Fixed(100.0)),
        ])
        .align(Align::Center)
        .justify(Justify::Center);
        layout(&mut tree, Rect::new(0.0, 0.0, 300.0, 300.0));

        assert!(hit_test(&tree, (150.0, 150.0)).is_none());
        assert_eq!(hit_test(&tree, (10.0, 10.0)).as_deref(), Some("dismiss"));
    }

    #[test]
    fn focus_order_collects_keyed_focusable_nodes() {
        let tree = lay_out_counter();
        let order = focus_order(&tree);
        let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, vec!["dec", "inc"]);
    }

    #[test]
    fn sync_focus_order_preserves_existing_focus_by_node_id() {
        let tree = lay_out_counter();
        let mut state = UiState::new();
        state.sync_focus_order(&tree);
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
        state.focus_next();
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("dec"));
        state.focus_next();
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));

        let rebuilt = lay_out_counter();
        state.sync_focus_order(&rebuilt);
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
    }

    #[test]
    fn shift_tab_moves_focus_backward() {
        let tree = lay_out_counter();
        let mut state = UiState::new();
        state.sync_focus_order(&tree);
        state.focus_prev();
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
    }

    #[test]
    fn enter_key_activates_focused_target() {
        let tree = lay_out_counter();
        let mut state = UiState::new();
        state.sync_focus_order(&tree);
        state.focus_next();
        state.focus_next();

        let event = state
            .key_down(UiKey::Enter, KeyModifiers::default(), false)
            .expect("activation event");

        assert_eq!(event.kind, UiEventKind::Activate);
        assert_eq!(event.key.as_deref(), Some("inc"));
        assert!(matches!(
            event.key_press.as_ref().map(|p| &p.key),
            Some(UiKey::Enter)
        ));
    }

    #[test]
    fn enter_without_focus_is_key_down() {
        let tree = lay_out_counter();
        let mut state = UiState::new();
        state.sync_focus_order(&tree);

        let event = state
            .key_down(UiKey::Enter, KeyModifiers::default(), false)
            .expect("key event");

        assert_eq!(event.kind, UiEventKind::KeyDown);
        assert_eq!(event.key, None);
    }

    #[test]
    fn tab_changes_focus_without_app_event() {
        let tree = lay_out_counter();
        let mut state = UiState::new();
        state.sync_focus_order(&tree);

        assert!(
            state
                .key_down(UiKey::Tab, KeyModifiers::default(), false)
                .is_none()
        );
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("dec"));
    }

    #[test]
    fn hit_test_through_scrolled_content() {
        // Three 60px buttons in a 100px-tall scroll viewport. The
        // second button is initially below the visible area.
        // After scrolling 60px, button[1] is now at the top.
        let mut tree = scroll([
            button("zero").key("b0").height(Size::Fixed(60.0)),
            button("one").key("b1").height(Size::Fixed(60.0)),
            button("two").key("b2").height(Size::Fixed(60.0)),
        ])
        .key("list")
        .height(Size::Fixed(100.0));
        tree.scroll_offset_y = 60.0;
        layout(&mut tree, Rect::new(0.0, 0.0, 200.0, 100.0));

        // Buttons hug their text width — click at b1's center after the
        // scroll shift to land inside its actual rect.
        let r1 = find_rect(&tree, "b1").expect("b1 rect");
        let hit = hit_test(&tree, (r1.center_x(), r1.center_y()));
        assert_eq!(hit.as_deref(), Some("b1"));

        // b0 has been scrolled above the viewport — clicking where it
        // would now sit (above y=0) misses it.
        let r0 = find_rect(&tree, "b0").expect("b0 rect");
        assert!(r0.bottom() <= 0.0, "b0 should be above the viewport, was {:?}", r0);
    }

    #[test]
    fn pointer_wheel_routes_to_deepest_scrollable() {
        // Outer scroll containing an inner scroll. Wheel events at the
        // inner's center should target the inner.
        let mut tree = scroll([
            button("above").key("above").height(Size::Fixed(40.0)),
            scroll([button("inner-row").key("inner-row").height(Size::Fixed(60.0))])
                .key("inner")
                .height(Size::Fixed(100.0)),
        ])
        .key("outer")
        .height(Size::Fixed(300.0));
        layout(&mut tree, Rect::new(0.0, 0.0, 200.0, 300.0));

        let inner_rect = find_rect(&tree, "inner-row").expect("inner row rect");
        let mut state = UiState::new();
        let routed = state.pointer_wheel(&tree, (inner_rect.center_x(), inner_rect.center_y()), 30.0);
        assert!(routed, "wheel should route to a scrollable");
        // Inner's id includes its key.
        let inner_id = find_id_for_kind(&tree, "inner").expect("inner id");
        assert!(
            state.scroll_offsets.contains_key(&inner_id),
            "expected inner offset, got {:?}",
            state.scroll_offsets.keys().collect::<Vec<_>>()
        );
    }

    fn find_id_for_kind(node: &El, key: &str) -> Option<String> {
        if matches!(node.kind, Kind::Scroll) && node.key.as_deref() == Some(key) {
            return Some(node.computed_id.clone());
        }
        node.children.iter().find_map(|c| find_id_for_kind(c, key))
    }

    #[test]
    fn stale_focus_clears_on_rebuild() {
        let tree = lay_out_counter();
        let mut state = UiState::new();
        state.focused = Some(UiTarget {
            key: "gone".into(),
            node_id: "root.missing".into(),
            rect: Rect::default(),
        });

        state.sync_focus_order(&tree);

        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
    }

    fn find_rect(node: &El, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) {
            return Some(node.computed);
        }
        node.children.iter().find_map(|c| find_rect(c, key))
    }
    fn find_text_rect(node: &El) -> Option<Rect> {
        if matches!(node.kind, Kind::Text) {
            return Some(node.computed);
        }
        node.children.iter().find_map(find_text_rect)
    }
    fn node_state(node: &El, key: &str) -> Option<InteractionState> {
        if node.key.as_deref() == Some(key) {
            return Some(node.state);
        }
        node.children.iter().find_map(|c| node_state(c, key))
    }
    fn target(node: &El, key: &str) -> UiTarget {
        let rect = find_rect(node, key).expect("target rect");
        UiTarget {
            key: key.to_string(),
            node_id: find_id(node, key).expect("target id"),
            rect,
        }
    }
    fn find_id(node: &El, key: &str) -> Option<String> {
        if node.key.as_deref() == Some(key) {
            return Some(node.computed_id.clone());
        }
        node.children.iter().find_map(|c| find_id(c, key))
    }
}
