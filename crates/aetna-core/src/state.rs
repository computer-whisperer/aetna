//! [`UiState`] — the renderer's interaction-state side store.
//!
//! Holds pointer position, hovered/pressed/focused targets, per-node
//! scroll offsets, the app-supplied hotkey registry, and the per-(node,
//! prop) animation map. The host doesn't touch this directly; the
//! renderer ([`crate::Runner`] in `aetna-wgpu`) owns one and routes
//! input events through it.
//!
//! Visual delta application: if `pressed` is set, that node renders with
//! `state = Press`. Otherwise, if `hovered` is set, that node renders
//! with `state = Hover`. Press takes precedence so clicking a button
//! that's also hovered shows the press visual, not the hover visual.
//! Focus is independent of both — the focus ring is its own envelope.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::anim::{AnimProp, Animation};
use crate::anim::tick::{is_in_flight, tick_node};
use crate::event::{KeyChord, KeyModifiers, KeyPress, UiEvent, UiEventKind, UiKey, UiTarget};
use crate::focus::focus_order;
use crate::hit_test::scroll_target_at;
use crate::tree::{El, InteractionState, Rect};

/// Animation pacing.
///
/// `Live` steps springs by wall-clock time, used by the windowed runner.
/// `Settled` snaps every in-flight animation to its target each tick,
/// used by headless paths so single-frame snapshots are deterministic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnimationMode {
    #[default]
    Live,
    Settled,
}

/// State-driven visual envelope kind. Each is a 0..1 amount written by
/// the animation tick and consumed by [`crate::draw_ops::draw_ops`] to
/// modulate a node's surface visuals (lighten on hover, darken on press,
/// fade in/out the focus ring).
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum EnvelopeKind {
    Hover,
    Press,
    FocusRing,
}

/// Internal UI state — interaction trackers + the side maps the library
/// writes during layout / state-apply / animation-tick passes. Owned by
/// the renderer; the host doesn't interact with this directly.
///
/// The side maps replace the per-node bookkeeping fields that used to
/// live on `El` (computed rect, interaction state, envelope amounts).
/// Keying is by `El::computed_id`, the path-shaped string assigned by
/// the layout pass.
#[derive(Default, Debug)]
pub struct UiState {
    /// Last known pointer position in **logical** pixels. `None` until
    /// the pointer enters the window.
    pub pointer_pos: Option<(f32, f32)>,
    pub hovered: Option<UiTarget>,
    pub pressed: Option<UiTarget>,
    pub focused: Option<UiTarget>,
    pub(crate) focus_order: Vec<UiTarget>,
    /// Scroll offset (logical pixels) per scrollable node, keyed by
    /// `El::computed_id`. The layout pass reads this when positioning a
    /// scrollable's children and writes back the clamped value.
    pub(crate) scroll_offsets: HashMap<String, f32>,
    /// App-level hotkey registry; the host snapshots `App::hotkeys()`
    /// each frame and stores it here. Matched in `key_down` ahead of
    /// focus activation.
    pub(crate) hotkeys: Vec<(KeyChord, String)>,
    /// In-flight animations keyed by `(computed_id, prop)`. Created
    /// lazily as state transitions happen; trimmed by
    /// [`Self::tick_visual_animations`] when their nodes leave the tree.
    pub(crate) animations: HashMap<(String, AnimProp), Animation>,
    /// Animation pacing mode. Default is `Live`; headless render
    /// binaries switch to `Settled` so single-frame snapshots reflect
    /// the post-animation visual.
    animation_mode: AnimationMode,

    // ---- side maps (formerly El bookkeeping) ----
    /// Computed rect per node, written by the layout pass.
    pub(crate) computed_rects: HashMap<String, Rect>,
    /// Library-resolved interaction state per node, derived each frame
    /// from the focused/pressed/hovered trackers by [`Self::apply_to_state`].
    pub(crate) node_states: HashMap<String, InteractionState>,
    /// State-envelope amounts (0..1) per (node, kind), written by the
    /// animation tick. `draw_ops` reads these to modulate the surface
    /// visuals; missing entries read as `0.0`.
    pub(crate) envelopes: HashMap<(String, EnvelopeKind), f32>,
}

impl UiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up the layout-assigned rect for `id`; returns a zero rect
    /// when `id` is unknown (pre-layout, or not in the laid-out tree).
    pub fn rect(&self, id: &str) -> Rect {
        self.computed_rects.get(id).copied().unwrap_or_default()
    }

    /// Look up the layout-assigned rect for an app-supplied element
    /// key. Returns `None` when the key is absent from `root` or layout
    /// has not written a rect for that node yet.
    pub fn rect_of_key(&self, root: &El, key: &str) -> Option<Rect> {
        find_target_by_key(root, key)
            .and_then(|target| self.computed_rects.get(&target.node_id).copied())
    }

    /// Build a [`UiTarget`] for an app-supplied element key using the
    /// current layout rect. Useful for hosts that need to anchor native
    /// overlays or forward events into externally painted regions.
    pub fn target_of_key(&self, root: &El, key: &str) -> Option<UiTarget> {
        let target = find_target_by_key(root, key)?;
        let rect = self.computed_rects.get(&target.node_id).copied()?;
        Some(UiTarget { rect, ..target })
    }

    /// Resolved interaction state for `id`. Returns
    /// [`InteractionState::Default`] when no tracker matches.
    pub fn node_state(&self, id: &str) -> InteractionState {
        self.node_states.get(id).copied().unwrap_or_default()
    }

    /// Current eased state envelope amount in `[0, 1]` for `(id, kind)`.
    /// Missing entries read as `0.0`.
    pub fn envelope(&self, id: &str, kind: EnvelopeKind) -> f32 {
        self.envelopes
            .get(&(id.to_string(), kind))
            .copied()
            .unwrap_or(0.0)
    }

    /// Seed or read the persistent scroll offset for `id`. Use this to
    /// pre-position a scroll viewport before [`crate::layout::layout`]
    /// runs (call [`crate::layout::assign_ids`] first to populate the
    /// node's `computed_id`).
    pub fn set_scroll_offset(&mut self, id: impl Into<String>, value: f32) {
        self.scroll_offsets.insert(id.into(), value);
    }

    /// Read the current scroll offset for `id`. Defaults to `0.0`.
    pub fn scroll_offset(&self, id: &str) -> f32 {
        self.scroll_offsets.get(id).copied().unwrap_or(0.0)
    }

    /// Rebuild [`Self::node_states`] from the current focused/pressed/
    /// hovered trackers. Press wins over Focus on a same-node match;
    /// Hover only applies when the node isn't already pressed or focused.
    pub fn apply_to_state(&mut self) {
        self.node_states.clear();
        if let Some(target) = &self.focused {
            self.node_states
                .insert(target.node_id.clone(), InteractionState::Focus);
        }
        if let Some(target) = &self.pressed {
            self.node_states
                .insert(target.node_id.clone(), InteractionState::Press);
        }
        if let Some(target) = &self.hovered {
            let already = self
                .pressed
                .as_ref()
                .map(|p| p.node_id == target.node_id)
                .unwrap_or(false)
                || self
                    .focused
                    .as_ref()
                    .map(|f| f.node_id == target.node_id)
                    .unwrap_or(false);
            if !already {
                self.node_states
                    .insert(target.node_id.clone(), InteractionState::Hover);
            }
        }
    }

    pub fn sync_focus_order(&mut self, root: &El) {
        let order = focus_order(root, self);
        self.focus_order = order;
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

    /// Increment the scroll offset for the deepest scrollable container
    /// containing `point`. Returns `true` if any scrollable was hit and
    /// updated (host can use this to decide whether to request a redraw).
    pub fn pointer_wheel(&mut self, root: &El, point: (f32, f32), dy: f32) -> bool {
        if let Some(id) = scroll_target_at(root, self, point) {
            *self.scroll_offsets.entry(id).or_insert(0.0) += dy;
            true
        } else {
            false
        }
    }

    /// Replace the hotkey registry. Called by the host runner from
    /// `App::hotkeys()` once per build cycle.
    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.hotkeys = hotkeys;
    }

    /// Walk the laid-out tree, retarget per-(node, prop) animations to
    /// the values implied by each node's current state, step them
    /// forward to `now`, and write back: app-driven props mutate the
    /// El's `fill` / `text_color` / `stroke` / `opacity` / `translate` /
    /// `scale` (so the next rebuild reads the eased value); state
    /// envelopes are written to [`Self::envelopes`] for `draw_ops` to
    /// modulate visuals from.
    ///
    /// Returns `true` if any animation is still in flight; the host
    /// should request another redraw next frame.
    pub fn tick_visual_animations(&mut self, root: &mut El, now: Instant) -> bool {
        let mut visited: HashSet<(String, AnimProp)> = HashSet::new();
        let mut needs_redraw = false;
        let mode = self.animation_mode;
        tick_node(
            root,
            &mut self.animations,
            &mut self.envelopes,
            &self.node_states,
            &mut visited,
            now,
            mode,
            &mut needs_redraw,
        );
        // GC: drop animations whose node left the tree this frame.
        self.animations.retain(|key, _| visited.contains(key));
        // GC envelopes: drop entries for nodes that left the tree.
        self.envelopes.retain(|(id, _), _| {
            visited.iter().any(|(visited_id, _)| visited_id == id)
        });
        needs_redraw
    }

    /// Switch animation pacing. The default is [`AnimationMode::Live`];
    /// headless render binaries flip to [`AnimationMode::Settled`] so
    /// a single-frame snapshot reflects the post-animation visual
    /// without depending on integrator timing.
    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.animation_mode = mode;
    }

    /// Whether any visual animation is still moving. The host's runner
    /// uses this (via the renderer's `PrepareResult`) to keep the redraw
    /// loop ticking only while there's motion.
    pub fn has_animations_in_flight(&self) -> bool {
        self.animations.values().any(is_in_flight)
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

        // Hotkeys win over focused-Enter activation: a focused button
        // with no hotkey on Enter still activates, but Ctrl+Enter (if
        // registered) routes to its hotkey instead. Registration order
        // is precedence — first match wins.
        if let Some((_, name)) = self
            .hotkeys
            .iter()
            .find(|(chord, _)| chord.matches(&key, modifiers))
        {
            return Some(UiEvent {
                key: Some(name.clone()),
                target: None,
                pointer: None,
                key_press: Some(KeyPress {
                    key,
                    modifiers,
                    repeat,
                }),
                kind: UiEventKind::Hotkey,
            });
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

fn find_target_by_key(root: &El, key: &str) -> Option<UiTarget> {
    if root.key.as_deref() == Some(key) {
        return Some(UiTarget {
            key: key.to_string(),
            node_id: root.computed_id.clone(),
            rect: Rect::default(),
        });
    }
    root.children
        .iter()
        .find_map(|child| find_target_by_key(child, key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hit_test::hit_test;
    use crate::layout::{assign_ids, layout};
    use crate::tree::*;
    use crate::{button, column, row, scroll};

    fn lay_out_counter() -> (El, UiState) {
        let mut tree = column([
            crate::text("0"),
            row([button("-").key("dec"), button("+").key("inc")]),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        (tree, state)
    }

    #[test]
    fn rect_of_key_finds_laid_out_node_rect() {
        let (tree, state) = lay_out_counter();
        let inc_by_helper = find_rect(&tree, &state, "inc").expect("inc rect");
        assert_eq!(state.rect_of_key(&tree, "inc"), Some(inc_by_helper));
        assert_eq!(state.rect_of_key(&tree, "missing"), None);
    }

    #[test]
    fn target_of_key_carries_key_id_and_rect() {
        let (tree, state) = lay_out_counter();
        let target = state.target_of_key(&tree, "dec").expect("dec target");
        assert_eq!(target.key, "dec");
        assert_eq!(target.node_id, find_id(&tree, "dec").expect("dec id"));
        assert_eq!(target.rect, find_rect(&tree, &state, "dec").expect("dec rect"));
    }

    #[test]
    fn ui_state_applies_hover() {
        let (tree, mut state) = lay_out_counter();
        state.hovered = Some(target(&tree, &state, "inc"));
        state.apply_to_state();
        assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Hover);
        assert_eq!(node_state(&tree, &state, "dec"), InteractionState::Default);
    }

    #[test]
    fn ui_state_press_wins_over_hover_on_same_key() {
        let (tree, mut state) = lay_out_counter();
        let inc = target(&tree, &state, "inc");
        state.hovered = Some(inc.clone());
        state.pressed = Some(inc);
        state.apply_to_state();
        assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Press);
    }

    #[test]
    fn sync_focus_order_preserves_existing_focus_by_node_id() {
        let (tree, mut state) = lay_out_counter();
        state.sync_focus_order(&tree);
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
        state.focus_next();
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("dec"));
        state.focus_next();
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));

        let (rebuilt, _) = lay_out_counter();
        state.sync_focus_order(&rebuilt);
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
    }

    #[test]
    fn shift_tab_moves_focus_backward() {
        let (tree, mut state) = lay_out_counter();
        state.sync_focus_order(&tree);
        state.focus_prev();
        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
    }

    #[test]
    fn enter_key_activates_focused_target() {
        let (tree, mut state) = lay_out_counter();
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
        let (tree, mut state) = lay_out_counter();
        state.sync_focus_order(&tree);

        let event = state
            .key_down(UiKey::Enter, KeyModifiers::default(), false)
            .expect("key event");

        assert_eq!(event.kind, UiEventKind::KeyDown);
        assert_eq!(event.key, None);
    }

    #[test]
    fn tab_changes_focus_without_app_event() {
        let (tree, mut state) = lay_out_counter();
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
        let mut state = UiState::new();
        assign_ids(&mut tree);
        state.scroll_offsets.insert(tree.computed_id.clone(), 60.0);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));

        // Buttons hug their text width — click at b1's center after the
        // scroll shift to land inside its actual rect.
        let r1 = find_rect(&tree, &state, "b1").expect("b1 rect");
        let hit = hit_test(&tree, &state, (r1.center_x(), r1.center_y()));
        assert_eq!(hit.as_deref(), Some("b1"));

        // b0 has been scrolled above the viewport — clicking where it
        // would now sit (above y=0) misses it.
        let r0 = find_rect(&tree, &state, "b0").expect("b0 rect");
        assert!(r0.bottom() <= 0.0, "b0 should be above the viewport, was {:?}", r0);
    }

    #[test]
    fn hotkey_match_emits_hotkey_event() {
        let mut state = UiState::new();
        state.set_hotkeys(vec![
            (KeyChord::ctrl('f'), "search".to_string()),
            (KeyChord::vim('j'), "down".to_string()),
        ]);

        let event = state
            .key_down(
                UiKey::Character("f".to_string()),
                KeyModifiers {
                    ctrl: true,
                    ..Default::default()
                },
                false,
            )
            .expect("hotkey event");
        assert_eq!(event.kind, UiEventKind::Hotkey);
        assert_eq!(event.key.as_deref(), Some("search"));

        let down = state
            .key_down(
                UiKey::Character("j".to_string()),
                KeyModifiers::default(),
                false,
            )
            .expect("vim event");
        assert_eq!(down.key.as_deref(), Some("down"));
    }

    #[test]
    fn hotkey_misses_when_modifiers_differ() {
        let mut state = UiState::new();
        state.set_hotkeys(vec![(KeyChord::ctrl('f'), "search".to_string())]);

        // Plain `f` (no modifiers) must not match Ctrl+F.
        let plain = state
            .key_down(
                UiKey::Character("f".to_string()),
                KeyModifiers::default(),
                false,
            )
            .expect("event for unhandled key");
        assert_eq!(plain.kind, UiEventKind::KeyDown);
        assert_eq!(plain.key, None);

        // Ctrl+Shift+F also differs from Ctrl+F (strict modifier match).
        let extra = state
            .key_down(
                UiKey::Character("f".to_string()),
                KeyModifiers {
                    ctrl: true,
                    shift: true,
                    ..Default::default()
                },
                false,
            )
            .expect("event");
        assert_eq!(extra.kind, UiEventKind::KeyDown);
    }

    #[test]
    fn hotkey_wins_over_focused_activate() {
        // A hotkey on Ctrl+Enter should not be intercepted by the
        // focused-Enter activation routing.
        let (tree, mut state) = lay_out_counter();
        state.sync_focus_order(&tree);
        state.focus_next();
        state.set_hotkeys(vec![(
            KeyChord {
                key: UiKey::Enter,
                modifiers: KeyModifiers {
                    ctrl: true,
                    ..Default::default()
                },
            },
            "submit".to_string(),
        )]);

        let event = state
            .key_down(
                UiKey::Enter,
                KeyModifiers {
                    ctrl: true,
                    ..Default::default()
                },
                false,
            )
            .expect("event");
        assert_eq!(event.kind, UiEventKind::Hotkey);
        assert_eq!(event.key.as_deref(), Some("submit"));

        // Plain Enter still activates the focused button.
        let activate = state
            .key_down(UiKey::Enter, KeyModifiers::default(), false)
            .expect("event");
        assert_eq!(activate.kind, UiEventKind::Activate);
    }

    #[test]
    fn hotkey_character_match_is_case_insensitive() {
        // Winit reports Shift+a as Character("A"). A `KeyChord::ctrl('a')`
        // with Shift held should still not match (modifier mask differs),
        // but `KeyChord::ctrl_shift('a')` should.
        let mut state = UiState::new();
        state.set_hotkeys(vec![(KeyChord::ctrl_shift('a'), "select-all".to_string())]);

        let event = state
            .key_down(
                UiKey::Character("A".to_string()),
                KeyModifiers {
                    ctrl: true,
                    shift: true,
                    ..Default::default()
                },
                false,
            )
            .expect("event");
        assert_eq!(event.key.as_deref(), Some("select-all"));
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
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 300.0));

        let inner_rect = find_rect(&tree, &state, "inner-row").expect("inner row rect");
        let routed =
            state.pointer_wheel(&tree, (inner_rect.center_x(), inner_rect.center_y()), 30.0);
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
        let (tree, mut state) = lay_out_counter();
        state.focused = Some(UiTarget {
            key: "gone".into(),
            node_id: "root.missing".into(),
            rect: Rect::default(),
        });

        state.sync_focus_order(&tree);

        assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
    }

    fn find_fill(node: &El, key: &str) -> Option<Color> {
        if node.key.as_deref() == Some(key) {
            return node.fill;
        }
        node.children.iter().find_map(|c| find_fill(c, key))
    }
    fn envelope_for(node: &El, state: &UiState, key: &str, kind: EnvelopeKind) -> Option<f32> {
        if node.key.as_deref() == Some(key) {
            return Some(state.envelope(&node.computed_id, kind));
        }
        node.children
            .iter()
            .find_map(|c| envelope_for(c, state, key, kind))
    }

    #[test]
    fn settled_mode_snaps_hover_envelope_to_one() {
        // Headless contract: Settled mode must produce the post-hover
        // envelope on a single prepare. A windowed runner (Live mode)
        // would ease over many frames; the fixture path can't wait.
        let (mut tree, mut state) = lay_out_counter();
        state.set_animation_mode(AnimationMode::Settled);
        state.hovered = Some(target(&tree, &state, "inc"));
        state.apply_to_state();

        let needs_redraw = state.tick_visual_animations(&mut tree, Instant::now());

        assert!(!needs_redraw, "Settled mode should never report in flight");
        assert_eq!(envelope_for(&tree, &state, "inc", EnvelopeKind::Hover), Some(1.0));
        assert_eq!(envelope_for(&tree, &state, "inc", EnvelopeKind::Press), Some(0.0));
        // The build fill stays untouched — the lightening happens in
        // apply_state at draw time, mixing by hover_amount.
    }

    #[test]
    fn live_mode_eases_hover_envelope_over_multiple_ticks() {
        // After a single 8 ms tick the hover envelope should be
        // strictly between 0 and 1 — neither snapped to either end.
        let (mut tree, mut state) = lay_out_counter();
        let t0 = Instant::now();
        state.tick_visual_animations(&mut tree, t0);

        state.hovered = Some(target(&tree, &state, "inc"));
        state.apply_to_state();
        let needs_redraw = state.tick_visual_animations(
            &mut tree,
            t0 + std::time::Duration::from_millis(8),
        );
        let mid = envelope_for(&tree, &state, "inc", EnvelopeKind::Hover).expect("hover envelope");

        assert!(needs_redraw, "spring should still be in flight after one 8 ms tick");
        assert!(
            mid > 0.0 && mid < 1.0,
            "expected envelope mid-flight, got {mid}",
        );
    }

    #[test]
    fn build_value_change_survives_hover_envelope() {
        // The point of envelopes: when the author swaps a button's fill
        // mid-hover, n.fill must reflect the new build value
        // immediately. The envelope keeps easing independently. This is
        // what avoids the AppFill / StateFill fight of an earlier draft.
        let mut tree_a = column([row([button("X")
            .key("x")
            .fill(Color::rgb(255, 0, 0))])])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.set_animation_mode(AnimationMode::Settled);
        state.hovered = Some(target(&tree_a, &state, "x"));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree_a, Instant::now());
        assert_eq!(envelope_for(&tree_a, &state, "x", EnvelopeKind::Hover), Some(1.0));

        // Rebuild: same button, fill swapped to blue.
        let mut tree_b = column([row([button("X")
            .key("x")
            .fill(Color::rgb(0, 0, 255))])])
        .padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree_b, Instant::now());

        let observed = find_fill(&tree_b, "x").expect("x fill");
        assert_eq!(
            (observed.r, observed.g, observed.b),
            (0, 0, 255),
            "build fill should pass through unchanged — envelope handles state delta separately",
        );
        assert_eq!(envelope_for(&tree_b, &state, "x", EnvelopeKind::Hover), Some(1.0));
    }

    #[test]
    fn focus_ring_alpha_eases_in_and_out() {
        let (mut tree, mut state) = lay_out_counter();
        state.set_animation_mode(AnimationMode::Settled);

        // No focus → alpha settled at 0.
        state.tick_visual_animations(&mut tree, Instant::now());
        assert_eq!(
            envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
            Some(0.0)
        );

        // Focus on inc → alpha settles at 1.0.
        let (mut tree, _) = lay_out_counter();
        // Re-layout against the existing state so the rect map is fresh.
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.focused = Some(target(&tree, &state, "inc"));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree, Instant::now());
        assert_eq!(
            envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
            Some(1.0)
        );

        // Lose focus → alpha settles back to 0.
        let (mut tree, _) = lay_out_counter();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.focused = None;
        state.apply_to_state();
        state.tick_visual_animations(&mut tree, Instant::now());
        assert_eq!(
            envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
            Some(0.0)
        );
    }

    #[test]
    fn app_fill_settles_to_new_value_in_settled_mode() {
        // .animate(SPRING_STANDARD) on a node whose fill changes
        // between rebuilds. Settled mode should produce the new fill
        // on the very first tick after the change.
        use crate::anim::Timing;
        let mut tree_a = column([
            crate::text("0"),
            row([button("X")
                .key("x")
                .fill(Color::rgb(255, 0, 0))
                .animate(Timing::SPRING_STANDARD)]),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        state.set_animation_mode(AnimationMode::Settled);
        state.tick_visual_animations(&mut tree_a, Instant::now());
        assert_eq!(find_fill(&tree_a, "x").map(|c| (c.r, c.g, c.b)), Some((255, 0, 0)));

        // Rebuild with a different fill; tracker eases through.
        let mut tree_b = column([
            crate::text("0"),
            row([button("X")
                .key("x")
                .fill(Color::rgb(0, 0, 255))
                .animate(Timing::SPRING_STANDARD)]),
        ])
        .padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.tick_visual_animations(&mut tree_b, Instant::now());

        assert_eq!(
            find_fill(&tree_b, "x").map(|c| (c.r, c.g, c.b)),
            Some((0, 0, 255)),
            "Settled mode should snap to the new build value",
        );
    }

    #[test]
    fn app_fill_eases_in_live_mode() {
        // Same setup as above but in Live mode: after a small dt the
        // colour should be partway between red and blue, not at either.
        use crate::anim::Timing;
        let mut tree_a = column([row([button("X")
            .key("x")
            .fill(Color::rgb(255, 0, 0))
            .animate(Timing::SPRING_STANDARD)])])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let t0 = Instant::now();
        state.tick_visual_animations(&mut tree_a, t0);

        let mut tree_b = column([row([button("X")
            .key("x")
            .fill(Color::rgb(0, 0, 255))
            .animate(Timing::SPRING_STANDARD)])])
        .padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        let needs_redraw = state.tick_visual_animations(
            &mut tree_b,
            t0 + std::time::Duration::from_millis(8),
        );
        let mid = find_fill(&tree_b, "x").expect("mid fill");

        assert!(needs_redraw, "spring should still be in flight after one tick");
        assert!(
            mid.r < 255 && mid.b < 255,
            "expected mid-flight, got {mid:?}",
        );
        assert!(
            mid.r > 0 || mid.b > 0,
            "should have moved off the start",
        );
    }

    #[test]
    fn app_translate_eases_on_rebuild() {
        use crate::anim::Timing;
        let mut tree_a = column([row([button("slide")
            .key("s")
            .translate(0.0, 0.0)
            .animate(Timing::SPRING_STANDARD)])])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.set_animation_mode(AnimationMode::Settled);
        state.tick_visual_animations(&mut tree_a, Instant::now());

        // Rebuild with a different translate.
        let mut tree_b = column([row([button("slide")
            .key("s")
            .translate(100.0, 50.0)
            .animate(Timing::SPRING_STANDARD)])])
        .padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.tick_visual_animations(&mut tree_b, Instant::now());

        let n = find_node(&tree_b, "s").expect("s node");
        assert!((n.translate.0 - 100.0).abs() < 0.5);
        assert!((n.translate.1 - 50.0).abs() < 0.5);
    }

    #[test]
    fn state_envelope_composes_on_app_eased_fill() {
        // A keyed interactive node with .animate() AND being hovered.
        // After Settled tick: n.fill = (eased) build value, hover
        // envelope = 1. draw_ops in apply_state then mixes the build
        // colour toward its lightened version by the envelope amount.
        // Since the envelope is at 1, the emitted Quad's fill should
        // equal lighten(build_fill, HOVER_LIGHTEN).
        use crate::anim::Timing;
        let mut tree = column([row([button("X")
            .key("x")
            .fill(Color::rgb(100, 100, 100))
            .animate(Timing::SPRING_STANDARD)])])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        state.set_animation_mode(AnimationMode::Settled);
        state.hovered = Some(target(&tree, &state, "x"));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree, Instant::now());

        // Build fill survives untouched (envelope handles the delta).
        let n_fill = find_fill(&tree, "x").expect("x fill");
        assert_eq!((n_fill.r, n_fill.g, n_fill.b), (100, 100, 100));
        assert_eq!(envelope_for(&tree, &state, "x", EnvelopeKind::Hover), Some(1.0));
    }

    #[test]
    fn app_animation_skipped_when_animate_not_set() {
        // Without .animate(), app props are not tracked — the node's
        // fill snaps to whatever the build produces, no easing.
        let mut tree_a = column([row([button("X")
            .key("x")
            .fill(Color::rgb(255, 0, 0))])]) // no .animate()
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.tick_visual_animations(&mut tree_a, Instant::now());

        let mut tree_b = column([row([button("X")
            .key("x")
            .fill(Color::rgb(0, 0, 255))])])
        .padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.tick_visual_animations(&mut tree_b, Instant::now());

        let observed = find_fill(&tree_b, "x").expect("x fill");
        assert_eq!(
            (observed.r, observed.g, observed.b),
            (0, 0, 255),
            "no .animate() — value should snap",
        );
    }

    fn find_node<'a>(node: &'a El, key: &str) -> Option<&'a El> {
        if node.key.as_deref() == Some(key) {
            return Some(node);
        }
        node.children.iter().find_map(|c| find_node(c, key))
    }

    #[test]
    fn animation_entries_gc_when_node_leaves_tree() {
        // Build a tree with two buttons; hover one to seed an entry.
        // Then build a different tree with only one button. The orphan's
        // animation entries should be trimmed.
        let (mut tree_a, mut state) = lay_out_counter();
        state.hovered = Some(target(&tree_a, &state, "inc"));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree_a, Instant::now());
        let inc_id_a = find_id(&tree_a, "inc").expect("inc id");
        assert!(
            state
                .animations
                .keys()
                .any(|(id, _)| id == &inc_id_a),
            "expected at least one entry for inc"
        );

        // Rebuild with only the dec button. inc entries should be gone.
        let mut tree_b = column([
            crate::text("0"),
            row([button("-").key("dec")]),
        ])
        .padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.hovered = None;
        state.apply_to_state();
        state.tick_visual_animations(&mut tree_b, Instant::now());
        assert!(
            !state
                .animations
                .keys()
                .any(|(id, _)| id == &inc_id_a),
            "stale entries for inc were not GC'd"
        );
    }

    fn find_rect(node: &El, state: &UiState, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) {
            return Some(state.rect(&node.computed_id));
        }
        node.children
            .iter()
            .find_map(|c| find_rect(c, state, key))
    }
    fn node_state(node: &El, state: &UiState, key: &str) -> InteractionState {
        let mut found = None;
        find_node_state(node, state, key, &mut found);
        found.unwrap_or_default()
    }
    fn find_node_state(
        node: &El,
        state: &UiState,
        key: &str,
        out: &mut Option<InteractionState>,
    ) {
        if node.key.as_deref() == Some(key) {
            *out = Some(state.node_state(&node.computed_id));
            return;
        }
        for c in &node.children {
            find_node_state(c, state, key, out);
            if out.is_some() {
                return;
            }
        }
    }
    fn target(node: &El, state: &UiState, key: &str) -> UiTarget {
        let rect = find_rect(node, state, key).expect("target rect");
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
