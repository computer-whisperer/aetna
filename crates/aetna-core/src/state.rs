//! [`UiState`] — the renderer's interaction-state side store.
//!
//! Holds pointer position, hovered/pressed/focused targets, per-node
//! scroll offsets, the app-supplied hotkey registry, and the per-(node,
//! prop) animation map. The host doesn't touch this directly; backend
//! runners such as `aetna_wgpu::Runner` own one and route input events
//! through it.
//!
//! Visual delta application: if `pressed` is set, that node renders with
//! `state = Press`. Otherwise, if `hovered` is set, that node renders
//! with `state = Hover`. Press takes precedence so clicking a button
//! that's also hovered shows the press visual, not the hover visual.
//! Focus is independent of both — the focus ring is its own envelope.

mod cursor;
mod scroll;
mod types;
mod widget_state;

use std::collections::HashSet;
use std::fmt::Debug;
// `web_time::Instant` is API-identical to `std::time::Instant` on
// native and uses `performance.now()` on wasm32 — std's `Instant::now()`
// panics in the browser because there is no monotonic clock there.
use web_time::Instant;

use crate::anim::AnimProp;
use crate::anim::tick::{is_in_flight, tick_node};
use crate::event::{
    KeyChord, KeyModifiers, KeyPress, PointerButton, UiEvent, UiEventKind, UiKey, UiTarget,
};
use crate::focus::focus_order;
use crate::tree::{El, InteractionState, Rect};

pub use types::{AnimationMode, EnvelopeKind, ScrollMetrics, ThumbDrag, WidgetState};
pub(crate) use types::{SelectionDrag, caret_blink_alpha_for};

use types::{
    AnimationState, CaretState, ClickSequence, ClickState, FocusState, HotkeyState, LayoutState,
    MULTI_CLICK_DIST, MULTI_CLICK_TIME, NodeInteractionState, PopoverFocusState, ScrollState,
    SelectionState, ToastState, TooltipState, WidgetStateStore,
};

/// Internal UI state — interaction trackers + the side maps the library
/// writes during layout / state-apply / animation-tick passes. Owned by
/// the renderer; the host doesn't interact with this directly.
///
/// The side maps replace the per-node bookkeeping fields that used to
/// live on `El` (computed rect, interaction state, envelope amounts).
/// Keying is by `El::computed_id`, the path-shaped string assigned by
/// the layout pass.
#[derive(Default)]
pub struct UiState {
    /// Last known pointer position in **logical** pixels. `None` until
    /// the pointer enters the window.
    pub pointer_pos: Option<(f32, f32)>,
    pub hovered: Option<UiTarget>,
    pub pressed: Option<UiTarget>,
    /// Secondary / middle button down-target, kept on a separate
    /// channel so it doesn't fight the primary `pressed` envelope or
    /// move focus. Carries the button kind so `pointer_up` knows which
    /// click variant to emit. Cleared by `pointer_up` matching the
    /// same button.
    pub(crate) pressed_secondary: Option<(UiTarget, PointerButton)>,
    pub focused: Option<UiTarget>,
    pub(crate) focus: FocusState,
    /// Mirror of the application's current
    /// [`crate::selection::Selection`]. Set by the host runner once
    /// per frame from [`crate::event::App::selection`]; read by the
    /// painter to draw highlight bands and by the selection manager
    /// to know what's currently active when extending a drag.
    pub current_selection: crate::selection::Selection,
    /// Internal selection traversal and drag state.
    pub(crate) selection: SelectionState,
    pub(crate) click: ClickState,
    pub(crate) caret: CaretState,
    pub(crate) popover_focus: PopoverFocusState,
    pub(crate) tooltip: TooltipState,
    pub(crate) scroll: ScrollState,
    /// Runtime-managed toast notification queue and id allocator.
    pub(crate) toast: ToastState,
    /// App-declared keyboard shortcuts and their action names.
    pub(crate) hotkeys: HotkeyState,
    /// Visual prop animations, state envelopes, and animation pacing.
    pub(crate) animation: AnimationState,

    // ---- side maps (formerly El bookkeeping) ----
    /// Layout-owned rect and key-index side maps.
    pub(crate) layout: LayoutState,
    /// Per-node interaction states derived from focused/pressed/hovered
    /// trackers by [`Self::apply_to_state`].
    pub(crate) node_states: NodeInteractionState,
    /// Per-(node, type) widget state buckets. The library owns the
    /// storage but never reads the values — they're for widget authors
    /// to stash text-input carets, dropdown open flags, etc. Entries
    /// are GC'd alongside envelopes/animations when a node leaves the
    /// tree (see [`Self::tick_visual_animations`]).
    widget_states: WidgetStateStore,
    /// Last known keyboard modifier mask. Updated by the host runner
    /// from winit's `ModifiersChanged`; pointer events stamp this
    /// value into their `UiEvent.modifiers` so widgets that need to
    /// detect Shift+click / Ctrl+drag can read it without separate
    /// plumbing.
    pub modifiers: KeyModifiers,
}

impl UiState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up the layout-assigned rect for `id`; returns a zero rect
    /// when `id` is unknown (pre-layout, or not in the laid-out tree).
    pub fn rect(&self, id: &str) -> Rect {
        self.layout
            .computed_rects
            .get(id)
            .copied()
            .unwrap_or_default()
    }

    /// Look up the layout-assigned rect for an app-supplied element
    /// key. Returns `None` when the key is absent from `root` or layout
    /// has not written a rect for that node yet.
    pub fn rect_of_key(&self, root: &El, key: &str) -> Option<Rect> {
        find_target_by_key(root, key)
            .and_then(|target| self.layout.computed_rects.get(&target.node_id).copied())
    }

    /// Build a [`UiTarget`] for an app-supplied element key using the
    /// current layout rect. Useful for hosts that need to anchor native
    /// overlays or forward events into externally painted regions.
    pub fn target_of_key(&self, root: &El, key: &str) -> Option<UiTarget> {
        let target = find_target_by_key(root, key)?;
        let rect = self.layout.computed_rects.get(&target.node_id).copied()?;
        Some(UiTarget { rect, ..target })
    }

    /// Resolved interaction state for `id`. Returns
    /// [`InteractionState::Default`] when no tracker matches.
    pub fn node_state(&self, id: &str) -> InteractionState {
        self.node_states.nodes.get(id).copied().unwrap_or_default()
    }

    /// Current eased state envelope amount in `[0, 1]` for `(id, kind)`.
    /// Missing entries read as `0.0`.
    pub fn envelope(&self, id: &str, kind: EnvelopeKind) -> f32 {
        self.animation
            .envelopes
            .get(&(id.to_string(), kind))
            .copied()
            .unwrap_or(0.0)
    }

    /// Resolve the click count for a fresh primary-button press at
    /// `(x, y)` and update the runtime's last-click record. Increments
    /// the count when this press extends a multi-click sequence (same
    /// target, within `MULTI_CLICK_TIME` and `MULTI_CLICK_DIST` of the
    /// previous press); otherwise resets to 1.
    pub(crate) fn next_click_count(
        &mut self,
        now: Instant,
        pos: (f32, f32),
        target_node_id: Option<&str>,
    ) -> u8 {
        let mut count = 1;
        if let Some(prev) = self.click.last.as_ref() {
            let dt = now.saturating_duration_since(prev.time);
            let dx = pos.0 - prev.pos.0;
            let dy = pos.1 - prev.pos.1;
            let same_target = match (prev.target_node_id.as_deref(), target_node_id) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            if same_target
                && dt < MULTI_CLICK_TIME
                && (dx * dx + dy * dy).sqrt() <= MULTI_CLICK_DIST
            {
                count = prev.count.saturating_add(1);
            }
        }
        self.click.last = Some(ClickSequence {
            time: now,
            pos,
            target_node_id: target_node_id.map(str::to_owned),
            count,
        });
        count
    }

    /// Current click count of the most recent primary press, or 1 if
    /// no press has happened yet. Used by `pointer_up` to stamp the
    /// matching `PointerUp` / `Click` events with the same count their
    /// originating `PointerDown` carried.
    pub(crate) fn current_click_count(&self) -> u8 {
        self.click.last.as_ref().map(|c| c.count).unwrap_or(1)
    }

    /// Rebuild the resolved per-node interaction-state side map from
    /// the current focused/pressed/hovered trackers. Press wins over
    /// Focus on a same-node match; Hover only applies when the node
    /// isn't already pressed or focused.
    ///
    /// Press is gated on the pointer being currently over the
    /// originally-pressed target — drag the cursor off and the press
    /// visual decays, drag back on and it returns. Mirrors the HTML /
    /// Tailwind `:active` behaviour: the visual reflects "would
    /// release-here activate?", not "was pointer_down captured?".
    /// Drag events still route to `pressed` regardless of pointer
    /// position (see `runtime::pointer_moved`); this gating only
    /// affects the visual envelope.
    pub fn apply_to_state(&mut self) {
        self.node_states.nodes.clear();
        if let Some(target) = &self.focused {
            self.node_states
                .nodes
                .insert(target.node_id.clone(), InteractionState::Focus);
        }
        let press_target = match (&self.pressed, &self.hovered) {
            (Some(pressed), Some(hovered)) if pressed.node_id == hovered.node_id => Some(pressed),
            _ => None,
        };
        if let Some(target) = press_target {
            self.node_states
                .nodes
                .insert(target.node_id.clone(), InteractionState::Press);
        }
        if let Some(target) = &self.hovered {
            let already = press_target
                .map(|p| p.node_id == target.node_id)
                .unwrap_or(false)
                || self
                    .focused
                    .as_ref()
                    .map(|f| f.node_id == target.node_id)
                    .unwrap_or(false);
            if !already {
                self.node_states
                    .nodes
                    .insert(target.node_id.clone(), InteractionState::Hover);
            }
        }
    }

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

    /// Walk the laid-out tree and rebuild the selectable-text order.
    /// Same shape as [`Self::sync_focus_order`] but filters for
    /// `selectable` keyed leaves instead of `focusable` ones. Should
    /// run on every frame post-layout, before the selection manager
    /// processes pointer events.
    pub fn sync_selection_order(&mut self, root: &El) {
        let order = crate::focus::selection_order(root, self);
        self.selection.order = order;
    }

    /// Read access to the current document-order list of selectable
    /// leaves. Mainly for tests; the selection manager uses internal
    /// access.
    pub fn selection_order(&self) -> &[UiTarget] {
        &self.selection.order
    }

    /// Update the hovered target. Maintains the hover-stable timer
    /// the tooltip pass reads — resets to `now` whenever the hovered
    /// node changes (or hover is gained), clears when it goes away.
    /// Also clears the per-session "tooltip dismissed by press" flag
    /// so the next hover starts fresh.
    ///
    /// Returns `true` when the hovered identity actually changed —
    /// used by [`crate::runtime::Core::pointer_moved`] to decide
    /// whether the host should redraw (cursor moves *within* the
    /// same hovered node are visual no-ops).
    pub(crate) fn set_hovered(&mut self, new: Option<UiTarget>, now: Instant) -> bool {
        let same = match (&self.hovered, &new) {
            (Some(a), Some(b)) => a.node_id == b.node_id,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.tooltip.hover_started_at = new.as_ref().map(|_| now);
            self.tooltip.dismissed_for_hover = false;
        }
        self.hovered = new;
        !same
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

    /// Reset the caret-blink phase to "fully on": the painter holds
    /// the caret solid for `CARET_BLINK_GRACE` after this call before
    /// resuming the on/off cycle. Called whenever the user does
    /// something the caret should react to — focusing an input,
    /// moving the caret, replacing the selection.
    pub(crate) fn bump_caret_activity(&mut self, now: Instant) {
        self.caret.activity_at = Some(now);
        self.caret.blink_alpha = 1.0;
    }

    pub fn focus_next(&mut self) -> Option<&UiTarget> {
        self.move_focus(1)
    }

    pub fn focus_prev(&mut self) -> Option<&UiTarget> {
        self.move_focus(-1)
    }

    /// Queue a toast for the next frame. Stamps an `id` (monotonic)
    /// and computes the `expires_at` deadline from `now + spec.ttl`.
    /// The runtime re-walks the queue each frame and drops expired
    /// entries before synthesizing the toast layer.
    pub fn push_toast(&mut self, spec: crate::toast::ToastSpec, now: Instant) {
        let id = self.toast.next_id;
        self.toast.next_id = self.toast.next_id.wrapping_add(1);
        self.toast.queue.push(crate::toast::Toast {
            id,
            level: spec.level,
            message: spec.message,
            expires_at: now + spec.ttl,
        });
    }

    /// Remove the toast with the given id. Used by the runtime when
    /// the user clicks a `toast-dismiss-{id}` button; apps that want
    /// to programmatically cancel a toast can call this directly via
    /// the `Runner::dismiss_toast` host accessor.
    pub fn dismiss_toast(&mut self, id: u64) {
        self.toast.queue.retain(|t| t.id != id);
    }

    /// Read-only view of the current toast queue (post-expiry).
    /// Used by hosts that want to drive cursor / accessibility state
    /// from the visible stack.
    pub fn toasts(&self) -> &[crate::toast::Toast] {
        &self.toast.queue
    }

    /// Replace the hotkey registry. Called by the host runner from
    /// `App::hotkeys()` once per build cycle.
    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.hotkeys.registry = hotkeys;
    }

    /// Update the tracked modifier mask. Hosts call this from their
    /// platform's "modifiers changed" hook (e.g. winit's
    /// `WindowEvent::ModifiersChanged`); the value is stamped into
    /// `UiEvent.modifiers` for every subsequent pointer event so
    /// widgets can detect Shift+click / Ctrl+drag without needing a
    /// per-call modifier parameter.
    pub fn set_modifiers(&mut self, modifiers: KeyModifiers) {
        self.modifiers = modifiers;
    }

    /// Walk the laid-out tree, retarget per-(node, prop) animations to
    /// the values implied by each node's current state, step them
    /// forward to `now`, and write back: app-driven props mutate the
    /// El's `fill` / `text_color` / `stroke` / `opacity` / `translate` /
    /// `scale` (so the next rebuild reads the eased value); state
    /// envelopes are written to the envelope side map for `draw_ops` to
    /// modulate visuals from.
    ///
    /// Returns `true` if any animation is still in flight; the host
    /// should request another redraw next frame.
    pub fn tick_visual_animations(&mut self, root: &mut El, now: Instant) -> bool {
        let mut visited: HashSet<(String, AnimProp)> = HashSet::new();
        let mut needs_redraw = false;
        let mode = self.animation.mode;
        tick_node(
            root,
            &mut self.animation.animations,
            &mut self.animation.envelopes,
            &self.node_states.nodes,
            &mut visited,
            now,
            mode,
            &mut needs_redraw,
        );
        // GC: drop animations whose node left the tree this frame.
        self.animation
            .animations
            .retain(|key, _| visited.contains(key));
        // Build a set of live node ids once — used by both envelope and
        // widget_state GC. Cheaper than the previous per-entry linear
        // scan over `visited`, which now matters because widget_state
        // entries can outnumber envelopes.
        let live_ids: HashSet<&str> = visited.iter().map(|(id, _)| id.as_str()).collect();
        self.animation
            .envelopes
            .retain(|(id, _), _| live_ids.contains(id.as_str()));
        self.widget_states
            .entries
            .retain(|(id, _), _| live_ids.contains(id.as_str()));

        // Caret blink. Resolve the new alpha from the activity age,
        // then keep requesting redraws as long as a capture_keys node
        // is focused so the cycle keeps animating in idle frames.
        // `Settled` mode pins the caret to fully on so headless
        // single-frame snapshots don't randomly catch the off phase.
        if let Some(activity_at) = self.caret.activity_at {
            let alpha = match mode {
                AnimationMode::Settled => 1.0,
                AnimationMode::Live => {
                    caret_blink_alpha_for(now.saturating_duration_since(activity_at))
                }
            };
            self.caret.blink_alpha = alpha;
        }
        if mode == AnimationMode::Live && self.focused_node_captures_keys(root) {
            needs_redraw = true;
        }

        needs_redraw
    }

    /// Walk `root` and return whether the currently-focused node has
    /// `capture_keys` set. Used by the animation tick to keep
    /// requesting redraws while a text input is focused (so the caret
    /// blink keeps animating). Returns `false` when no node is focused
    /// or the focused node isn't in the tree.
    fn focused_node_captures_keys(&self, root: &El) -> bool {
        let Some(focused) = self.focused.as_ref() else {
            return false;
        };
        crate::runtime::find_capture_keys(root, &focused.node_id).unwrap_or(false)
    }

    /// Switch animation pacing. The default is [`AnimationMode::Live`];
    /// headless render binaries flip to [`AnimationMode::Settled`] so
    /// a single-frame snapshot reflects the post-animation visual
    /// without depending on integrator timing.
    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.animation.mode = mode;
    }

    /// Whether any visual animation is still moving. The host's runner
    /// uses this (via the renderer's `PrepareResult`) to keep the redraw
    /// loop ticking only while there's motion.
    pub fn has_animations_in_flight(&self) -> bool {
        self.animation.animations.values().any(is_in_flight)
    }

    /// One-line summary of interactive state for diagnostic logging.
    /// Format: `hov=<key|->|press=<key|->|focus=<key|->|env={...}|in_flight=N`.
    /// Keep terse — this is intended for per-frame `console.log`.
    pub fn debug_summary(&self) -> String {
        let key = |t: &Option<UiTarget>| {
            t.as_ref()
                .map(|t| t.key.clone())
                .unwrap_or_else(|| "-".into())
        };
        let mut env: Vec<String> = self
            .animation
            .envelopes
            .iter()
            .map(|((id, kind), v)| format!("{id}/{kind:?}={v:.3}"))
            .collect();
        env.sort();
        let in_flight = self
            .animation
            .animations
            .values()
            .filter(|a| is_in_flight(a))
            .count();
        format!(
            "hov={}|press={}|focus={}|env=[{}]|in_flight={}/{}",
            key(&self.hovered),
            key(&self.pressed),
            key(&self.focused),
            env.join(","),
            in_flight,
            self.animation.animations.len(),
        )
    }

    /// Match `key + modifiers` against the registered hotkey chords.
    /// Returns a `Hotkey` event if any registered chord matches; the
    /// `event.key` is the chord's registered name. Used by both the
    /// library-default path and the capture-keys path (hotkeys always
    /// win over a widget's raw key capture).
    pub fn try_hotkey(
        &self,
        key: &UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        let (_, name) = self
            .hotkeys
            .registry
            .iter()
            .find(|(chord, _)| chord.matches(key, modifiers))?;
        Some(UiEvent {
            key: Some(name.clone()),
            target: None,
            pointer: None,
            key_press: Some(KeyPress {
                key: key.clone(),
                modifiers,
                repeat,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            kind: UiEventKind::Hotkey,
        })
    }

    /// Build a raw `KeyDown` event routed to the focused target,
    /// bypassing the library's Tab/Enter/Escape interpretation. Used
    /// by the runner when the focused node has `capture_keys=true`.
    /// Returns `None` if no node is focused.
    pub fn key_down_raw(
        &self,
        key: UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        let target = self.focused.clone()?;
        Some(UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers,
                repeat,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            kind: UiEventKind::KeyDown,
        })
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
        if let Some(event) = self.try_hotkey(&key, modifiers, repeat) {
            return Some(event);
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
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            kind,
        })
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

impl Debug for UiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiState")
            .field("pointer_pos", &self.pointer_pos)
            .field("hovered", &self.hovered)
            .field("pressed", &self.pressed)
            .field("focused", &self.focused)
            .field("focus", &self.focus)
            .field("popover_focus", &self.popover_focus)
            .field("click", &self.click)
            .field("caret", &self.caret)
            .field("scroll", &self.scroll)
            .field("toast", &self.toast)
            .field("tooltip", &self.tooltip)
            .field("hotkeys", &self.hotkeys)
            .field("animation", &self.animation)
            .field("layout", &self.layout)
            .field("node_states", &self.node_states)
            .field("modifiers", &self.modifiers)
            .field("widget_states", &self.widget_states)
            .finish()
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
    use crate::cursor::Cursor;
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
        assert_eq!(
            target.rect,
            find_rect(&tree, &state, "dec").expect("dec rect")
        );
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
    fn ui_state_press_decays_when_pointer_drags_off_pressed_target() {
        // `:active`-style behaviour: the press visual only renders while
        // the pointer is still over the originally-pressed node. Drag
        // off → pressed target falls back to Default; the newly-hovered
        // node gets its own Hover.
        let (tree, mut state) = lay_out_counter();
        let inc = target(&tree, &state, "inc");
        let dec = target(&tree, &state, "dec");

        // Press on inc, pointer still on inc → Press.
        state.hovered = Some(inc.clone());
        state.pressed = Some(inc.clone());
        state.apply_to_state();
        assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Press);

        // Drag off inc onto dec while still holding the button.
        state.hovered = Some(dec);
        state.apply_to_state();
        assert_eq!(
            node_state(&tree, &state, "inc"),
            InteractionState::Default,
            "press visual cancels when pointer leaves the pressed target",
        );
        assert_eq!(
            node_state(&tree, &state, "dec"),
            InteractionState::Hover,
            "the newly-hovered node still gets its own hover state",
        );

        // Drag back onto inc → Press resumes.
        state.hovered = Some(inc);
        state.apply_to_state();
        assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Press);
    }

    #[test]
    fn ui_state_press_decays_when_pointer_leaves_window() {
        // Same shape as drag-off, but the pointer leaves the window
        // entirely (hovered = None). The press visual should decay so
        // the user sees "release here cancels" feedback even when the
        // cursor is outside the surface.
        let (tree, mut state) = lay_out_counter();
        let inc = target(&tree, &state, "inc");
        state.hovered = Some(inc.clone());
        state.pressed = Some(inc);
        state.apply_to_state();
        assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Press);

        state.hovered = None;
        state.apply_to_state();
        assert_eq!(node_state(&tree, &state, "inc"), InteractionState::Default);
    }

    fn lay_out_cursor_tree() -> (El, UiState) {
        // Panel declares Move; one child has its own `.cursor(Pointer)`
        // (declared); a sibling stack carries no cursor and inherits
        // Move from the panel. Plain stacks (not buttons) so the
        // widget kit's own cursor defaults can't drift the test.
        let mut tree = column([row([
            El::new(Kind::Group).key("undeclared"),
            El::new(Kind::Group).key("declared").cursor(Cursor::Pointer),
        ])])
        .key("panel")
        .cursor(Cursor::Move)
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        (tree, state)
    }

    #[test]
    fn cursor_is_default_when_no_hover_no_press() {
        let (tree, state) = lay_out_cursor_tree();
        assert_eq!(state.cursor(&tree), Cursor::Default);
    }

    #[test]
    fn cursor_returns_hovered_targets_explicit_declaration() {
        let (tree, mut state) = lay_out_cursor_tree();
        state.hovered = Some(target(&tree, &state, "declared"));
        assert_eq!(state.cursor(&tree), Cursor::Pointer);
    }

    #[test]
    fn cursor_inherits_from_ancestor_when_target_undeclared() {
        // The "undeclared" button has no `.cursor(...)`, so the panel's
        // `Move` propagates down — the inheritance rule that lets a
        // pan-surface declare cursor once on the container.
        let (tree, mut state) = lay_out_cursor_tree();
        state.hovered = Some(target(&tree, &state, "undeclared"));
        assert_eq!(state.cursor(&tree), Cursor::Move);
    }

    #[test]
    fn cursor_press_capture_overrides_hovered_target() {
        // Press on the Pointer button, drag onto the Move-inheriting
        // sibling. The cursor stays Pointer for the duration of the
        // press — matches native press-and-hold behaviour.
        let (tree, mut state) = lay_out_cursor_tree();
        let declared = target(&tree, &state, "declared");
        let undeclared = target(&tree, &state, "undeclared");
        state.pressed = Some(declared);
        state.hovered = Some(undeclared);
        assert_eq!(state.cursor(&tree), Cursor::Pointer);
    }

    #[test]
    fn cursor_pressed_overrides_resting_cursor_on_press_target() {
        // `cursor` at rest, `cursor_pressed` while the press anchors
        // here. Mirrors the slider's Grab → Grabbing transition idiom.
        let mut tree = column([El::new(Kind::Group)
            .key("handle")
            .cursor(Cursor::Grab)
            .cursor_pressed(Cursor::Grabbing)]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let handle = target(&tree, &state, "handle");

        // Hover → resting cursor.
        state.hovered = Some(handle.clone());
        assert_eq!(state.cursor(&tree), Cursor::Grab);

        // Press → pressed-cursor wins (and stays once the pointer
        // wanders off — press capture anchors the cursor).
        state.pressed = Some(handle);
        assert_eq!(state.cursor(&tree), Cursor::Grabbing);
        state.hovered = None;
        assert_eq!(
            state.cursor(&tree),
            Cursor::Grabbing,
            "press capture keeps the pressed cursor stable when the pointer drags off",
        );
    }

    #[test]
    fn cursor_pressed_does_not_inherit_from_ancestor_to_descendant() {
        // Only the literal press target's `cursor_pressed` matters.
        // A parent that declared `cursor_pressed` shouldn't re-skin a
        // descendant's press — ancestors should use `cursor` (which
        // does inherit) when they want subtree-wide affordances.
        let mut tree = column([row([El::new(Kind::Group).key("inner")])
            .key("outer")
            .cursor_pressed(Cursor::Grabbing)]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        state.pressed = Some(target(&tree, &state, "inner"));
        // Outer's `cursor_pressed` doesn't leak to the inner press
        // target; with no `cursor` chain at all, falls through to
        // Default.
        assert_eq!(state.cursor(&tree), Cursor::Default);
    }

    #[test]
    fn cursor_pressed_falls_through_to_resting_cursor_when_unset() {
        // Press target without `cursor_pressed` still resolves via
        // the standard walk-up — the new branch is purely additive.
        let mut tree = column([El::new(Kind::Group).key("btn").cursor(Cursor::Pointer)]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        state.pressed = Some(target(&tree, &state, "btn"));
        assert_eq!(state.cursor(&tree), Cursor::Pointer);
    }

    #[test]
    fn cursor_falls_back_to_default_when_target_id_not_in_tree() {
        // Stale tracker (target was removed from the tree mid-frame)
        // shouldn't panic — fall through to Default.
        let (tree, mut state) = lay_out_cursor_tree();
        state.hovered = Some(UiTarget {
            key: "ghost".into(),
            node_id: "no-such-node".into(),
            rect: Rect::default(),
        });
        assert_eq!(state.cursor(&tree), Cursor::Default);
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
    fn sync_selection_order_collects_keyed_selectable_leaves_in_tree_order() {
        let mut tree = column([
            crate::text("Alpha").key("a").selectable(),
            crate::text("Bravo (not selectable)"),
            crate::text("Charlie").key("c").selectable(),
            crate::text("Delta (selectable but unkeyed)").selectable(),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.sync_selection_order(&tree);

        let order = state.selection_order();
        let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
        // Only the keyed-and-selectable leaves should appear, in tree
        // order. The unkeyed selectable leaf is silently excluded —
        // selection requires stable identity.
        assert_eq!(keys, vec!["a", "c"]);
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
        state.scroll.offsets.insert(tree.computed_id.clone(), 60.0);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));

        // Buttons hug their text width — click at b1's center after the
        // scroll shift to land inside its actual rect.
        let r1 = find_rect(&tree, &state, "b1").expect("b1 rect");
        let hit = hit_test(&tree, &state, (r1.center_x(), r1.center_y()));
        assert_eq!(hit.as_deref(), Some("b1"));

        // b0 has been scrolled above the viewport — clicking where it
        // would now sit (above y=0) misses it.
        let r0 = find_rect(&tree, &state, "b0").expect("b0 rect");
        assert!(
            r0.bottom() <= 0.0,
            "b0 should be above the viewport, was {:?}",
            r0
        );
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
            scroll([button("inner-row")
                .key("inner-row")
                .height(Size::Fixed(60.0))])
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
            state.scroll.offsets.contains_key(&inner_id),
            "expected inner offset, got {:?}",
            state.scroll.offsets.keys().collect::<Vec<_>>()
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
        assert_eq!(
            envelope_for(&tree, &state, "inc", EnvelopeKind::Hover),
            Some(1.0)
        );
        assert_eq!(
            envelope_for(&tree, &state, "inc", EnvelopeKind::Press),
            Some(0.0)
        );
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
        let needs_redraw =
            state.tick_visual_animations(&mut tree, t0 + std::time::Duration::from_millis(8));
        let mid = envelope_for(&tree, &state, "inc", EnvelopeKind::Hover).expect("hover envelope");

        assert!(
            needs_redraw,
            "spring should still be in flight after one 8 ms tick"
        );
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
        let mut tree_a =
            column([row([button("X").key("x").fill(Color::rgb(255, 0, 0))])]).padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.set_animation_mode(AnimationMode::Settled);
        state.hovered = Some(target(&tree_a, &state, "x"));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree_a, Instant::now());
        assert_eq!(
            envelope_for(&tree_a, &state, "x", EnvelopeKind::Hover),
            Some(1.0)
        );

        // Rebuild: same button, fill swapped to blue.
        let mut tree_b =
            column([row([button("X").key("x").fill(Color::rgb(0, 0, 255))])]).padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree_b, Instant::now());

        let observed = find_fill(&tree_b, "x").expect("x fill");
        assert_eq!(
            (observed.r, observed.g, observed.b),
            (0, 0, 255),
            "build fill should pass through unchanged — envelope handles state delta separately",
        );
        assert_eq!(
            envelope_for(&tree_b, &state, "x", EnvelopeKind::Hover),
            Some(1.0)
        );
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
        assert_eq!(
            find_fill(&tree_a, "x").map(|c| (c.r, c.g, c.b)),
            Some((255, 0, 0))
        );

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
        let needs_redraw =
            state.tick_visual_animations(&mut tree_b, t0 + std::time::Duration::from_millis(8));
        let mid = find_fill(&tree_b, "x").expect("mid fill");

        assert!(
            needs_redraw,
            "spring should still be in flight after one tick"
        );
        assert!(
            mid.r < 255 && mid.b < 255,
            "expected mid-flight, got {mid:?}",
        );
        assert!(mid.r > 0 || mid.b > 0, "should have moved off the start",);
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
        assert_eq!(
            envelope_for(&tree, &state, "x", EnvelopeKind::Hover),
            Some(1.0)
        );
    }

    #[test]
    fn app_animation_skipped_when_animate_not_set() {
        // Without .animate(), app props are not tracked — the node's
        // fill snaps to whatever the build produces, no easing.
        let mut tree_a = column([row([button("X").key("x").fill(Color::rgb(255, 0, 0))])]) // no .animate()
            .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.tick_visual_animations(&mut tree_a, Instant::now());

        let mut tree_b =
            column([row([button("X").key("x").fill(Color::rgb(0, 0, 255))])]).padding(20.0);
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
                .animation
                .animations
                .keys()
                .any(|(id, _)| id == &inc_id_a),
            "expected at least one entry for inc"
        );

        // Rebuild with only the dec button. inc entries should be gone.
        let mut tree_b = column([crate::text("0"), row([button("-").key("dec")])]).padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.hovered = None;
        state.apply_to_state();
        state.tick_visual_animations(&mut tree_b, Instant::now());
        assert!(
            !state
                .animation
                .animations
                .keys()
                .any(|(id, _)| id == &inc_id_a),
            "stale entries for inc were not GC'd"
        );
    }

    #[derive(Default, Debug)]
    struct TestCaret {
        position: usize,
        blink_phase: f32,
    }
    impl WidgetState for TestCaret {
        fn debug_summary(&self) -> String {
            format!("pos={} blink={:.2}", self.position, self.blink_phase)
        }
    }

    #[test]
    fn widget_state_lazy_inserts_default_and_persists_mutations() {
        let mut state = UiState::new();
        // First call inserts the default.
        let caret = state.widget_state_mut::<TestCaret>("input.0");
        assert_eq!(caret.position, 0);
        caret.position = 7;
        caret.blink_phase = 0.5;
        // Second call returns the same instance.
        let caret = state.widget_state::<TestCaret>("input.0").expect("present");
        assert_eq!(caret.position, 7);
        assert!((caret.blink_phase - 0.5).abs() < f32::EPSILON);
        // Different id → independent storage.
        assert!(state.widget_state::<TestCaret>("input.1").is_none());
    }

    #[test]
    fn widget_state_summary_surfaces_debug_for_tree_dump() {
        let mut state = UiState::new();
        let caret = state.widget_state_mut::<TestCaret>("input.0");
        caret.position = 12;
        caret.blink_phase = 0.25;
        let summary = state.widget_state_summary("input.0");
        assert_eq!(summary.len(), 1);
        let (type_name, debug) = &summary[0];
        assert!(type_name.ends_with("TestCaret"));
        assert_eq!(debug, "pos=12 blink=0.25");
    }

    #[test]
    fn widget_state_gc_when_node_leaves_tree() {
        let (mut tree_a, mut state) = lay_out_counter();
        let inc_id = find_id(&tree_a, "inc").expect("inc id");
        // Seed widget_state on the inc button.
        state.widget_state_mut::<TestCaret>(&inc_id).position = 99;
        state.tick_visual_animations(&mut tree_a, Instant::now());
        assert!(state.widget_state::<TestCaret>(&inc_id).is_some());

        // Rebuild without inc. The GC sweep on the next tick should drop it.
        let mut tree_b = column([crate::text("0"), row([button("-").key("dec")])]).padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.tick_visual_animations(&mut tree_b, Instant::now());
        assert!(
            state.widget_state::<TestCaret>(&inc_id).is_none(),
            "stale widget_state for inc was not GC'd"
        );
    }

    fn find_rect(node: &El, state: &UiState, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) {
            return Some(state.rect(&node.computed_id));
        }
        node.children.iter().find_map(|c| find_rect(c, state, key))
    }
    fn node_state(node: &El, state: &UiState, key: &str) -> InteractionState {
        let mut found = None;
        find_node_state(node, state, key, &mut found);
        found.unwrap_or_default()
    }
    fn find_node_state(node: &El, state: &UiState, key: &str, out: &mut Option<InteractionState>) {
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
