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

use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
// `web_time::Instant` is API-identical to `std::time::Instant` on
// native and uses `performance.now()` on wasm32 — std's `Instant::now()`
// panics in the browser because there is no monotonic clock there.
use web_time::Instant;

use crate::anim::tick::{is_in_flight, tick_node};
use crate::anim::{AnimProp, Animation};
use crate::event::{
    KeyChord, KeyModifiers, KeyPress, PointerButton, UiEvent, UiEventKind, UiKey, UiTarget,
};
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

/// Per-instance state owned by a widget. Widget authors define their own
/// state types (e.g. text-input caret + selection, virtual list scroll
/// offset, dropdown open/closed) and stash them on [`UiState`] keyed by
/// node id via [`UiState::widget_state`] / [`UiState::widget_state_mut`].
///
/// The library never reads the state itself — it just owns the
/// storage, wipes entries when a node leaves the tree, and surfaces
/// `debug_summary()` in the tree dump so the agent loop can see what
/// the widget thinks.
///
/// # Symmetry
///
/// This is the storage contract for stateful widgets. Stock widgets get
/// no privileged shortcuts; everything they do here, an app-defined
/// widget can do too. See `widget_kit.md`.
pub trait WidgetState: 'static + Debug + Send + Sync {
    /// One-line summary for the tree dump. Default empty (the entry's
    /// type name still shows up via the inspector). Override to surface
    /// the most useful per-frame state — e.g. a text input might
    /// return `"caret=12 sel=8..14"`.
    fn debug_summary(&self) -> String {
        String::new()
    }
}

/// Subtrait combining [`WidgetState`] with [`Any`] so the type-erased
/// box can both call trait methods and downcast back to `T`.
trait AnyWidgetState: WidgetState {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn type_name(&self) -> &'static str;
}

impl<T: WidgetState> AnyWidgetState for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn type_name(&self) -> &'static str {
        std::any::type_name::<T>()
    }
}

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
    pub(crate) focus_order: Vec<UiTarget>,
    /// LIFO of focus targets pushed when popover layers open. Each new
    /// `Kind::Custom("popover_layer")` snapshots the current focus here
    /// and auto-focuses into the layer; closing the layer pops and
    /// restores. See [`crate::focus::sync_popover_focus`].
    pub(crate) focus_stack: Vec<UiTarget>,
    /// `computed_id`s of every popover-layer node in the last laid-out
    /// tree, in tree order. Diffed against the new tree to detect open
    /// / close transitions.
    pub(crate) popover_layer_ids: Vec<String>,
    /// When the current `hovered` target started being hovered. `None`
    /// when nothing is hovered or the pointer is outside the window.
    /// Used by [`crate::tooltip`] to gate the hover-delay timer.
    pub(crate) hover_started_at: Option<Instant>,
    /// True when the user pressed (or clicked) the hovered node
    /// during the current hover session. Suppresses the tooltip until
    /// the pointer leaves and re-enters, matching native behavior.
    pub(crate) tooltip_dismissed_for_hover: bool,
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
    /// Per-(node, type) widget state buckets. The library owns the
    /// storage but never reads the values — they're for widget authors
    /// to stash text-input carets, dropdown open flags, etc. Entries
    /// are GC'd alongside envelopes/animations when a node leaves the
    /// tree (see [`Self::tick_visual_animations`]).
    widget_states: HashMap<(String, TypeId), Box<dyn AnyWidgetState>>,
    /// Last known keyboard modifier mask. Updated by the host runner
    /// from winit's `ModifiersChanged`; pointer events stamp this
    /// value into their `UiEvent.modifiers` so widgets that need to
    /// detect Shift+click / Ctrl+drag can read it without separate
    /// plumbing.
    pub modifiers: KeyModifiers,
    /// `key → computed_id` map, refreshed at the top of every layout
    /// pass. Used by [`crate::layout::LayoutCtx::rect_of_key`] so
    /// custom layouts can position children relative to keyed elements
    /// elsewhere in the tree (e.g. a popover anchored to a button).
    /// Populated only for nodes that carry an author-set `key`; the
    /// duplicate-key case keeps the first entry seen in tree order.
    pub(crate) layout_key_index: HashMap<String, String>,
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

    /// Rebuild the resolved per-node interaction-state side map from
    /// the current focused/pressed/
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

    /// Update the hovered target. Maintains the hover-stable timer
    /// the tooltip pass reads — resets to `now` whenever the hovered
    /// node changes (or hover is gained), clears when it goes away.
    /// Also clears the per-session "tooltip dismissed by press" flag
    /// so the next hover starts fresh.
    pub(crate) fn set_hovered(&mut self, new: Option<UiTarget>, now: Instant) {
        let same = match (&self.hovered, &new) {
            (Some(a), Some(b)) => a.node_id == b.node_id,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.hover_started_at = new.as_ref().map(|_| now);
            self.tooltip_dismissed_for_hover = false;
        }
        self.hovered = new;
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
        // Build a set of live node ids once — used by both envelope and
        // widget_state GC. Cheaper than the previous per-entry linear
        // scan over `visited`, which now matters because widget_state
        // entries can outnumber envelopes.
        let live_ids: HashSet<&str> = visited.iter().map(|(id, _)| id.as_str()).collect();
        self.envelopes
            .retain(|(id, _), _| live_ids.contains(id.as_str()));
        self.widget_states
            .retain(|(id, _), _| live_ids.contains(id.as_str()));
        needs_redraw
    }

    // ---- widget_state typed bucket ----

    /// Look up the widget state of type `T` for `id`. Returns `None` if
    /// no entry exists or the entry was inserted as a different type.
    pub fn widget_state<T: WidgetState>(&self, id: &str) -> Option<&T> {
        self.widget_states
            .get(&(id.to_string(), TypeId::of::<T>()))
            .and_then(|b| b.as_any().downcast_ref::<T>())
    }

    /// Get a mutable reference to the widget state of type `T` for
    /// `id`, inserting `T::default()` if no entry exists. Use this in
    /// the build closure of a stateful widget so the first call after
    /// the node enters the tree produces a fresh state, and every
    /// subsequent call returns the live one.
    pub fn widget_state_mut<T: WidgetState + Default>(&mut self, id: &str) -> &mut T {
        let key = (id.to_string(), TypeId::of::<T>());
        let entry = self
            .widget_states
            .entry(key)
            .or_insert_with(|| Box::new(T::default()));
        entry
            .as_any_mut()
            .downcast_mut::<T>()
            .expect("widget_state TypeId match guarantees downcast succeeds")
    }

    /// Drop the widget state of type `T` for `id`, if any.
    pub fn clear_widget_state<T: WidgetState>(&mut self, id: &str) {
        self.widget_states
            .remove(&(id.to_string(), TypeId::of::<T>()));
    }

    /// Iterate `(id, type_name, debug_summary)` for every live widget
    /// state. Used by the tree dump to surface per-widget state in the
    /// agent loop's view.
    pub fn widget_state_summary(&self, id: &str) -> Vec<(&'static str, String)> {
        self.widget_states
            .iter()
            .filter(|((node_id, _), _)| node_id == id)
            .map(|(_, b)| (b.type_name(), b.debug_summary()))
            .collect()
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
            .envelopes
            .iter()
            .map(|((id, kind), v)| format!("{id}/{kind:?}={v:.3}"))
            .collect();
        env.sort();
        let in_flight = self.animations.values().filter(|a| is_in_flight(a)).count();
        format!(
            "hov={}|press={}|focus={}|env=[{}]|in_flight={}/{}",
            key(&self.hovered),
            key(&self.pressed),
            key(&self.focused),
            env.join(","),
            in_flight,
            self.animations.len(),
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
            modifiers,
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
            modifiers,
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
            modifiers,
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

impl Debug for UiState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiState")
            .field("pointer_pos", &self.pointer_pos)
            .field("hovered", &self.hovered)
            .field("pressed", &self.pressed)
            .field("focused", &self.focused)
            .field("focus_order", &self.focus_order)
            .field("scroll_offsets", &self.scroll_offsets)
            .field("hotkeys", &self.hotkeys)
            .field("animations", &self.animations)
            .field("animation_mode", &self.animation_mode)
            .field("computed_rects", &self.computed_rects)
            .field("node_states", &self.node_states)
            .field("envelopes", &self.envelopes)
            .field("modifiers", &self.modifiers)
            .field(
                "widget_states",
                &self
                    .widget_states
                    .iter()
                    .map(|((id, _), b)| (id.as_str(), b.type_name(), b.debug_summary()))
                    .collect::<Vec<_>>(),
            )
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
            state.animations.keys().any(|(id, _)| id == &inc_id_a),
            "expected at least one entry for inc"
        );

        // Rebuild with only the dec button. inc entries should be gone.
        let mut tree_b = column([crate::text("0"), row([button("-").key("dec")])]).padding(20.0);
        layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.hovered = None;
        state.apply_to_state();
        state.tick_visual_animations(&mut tree_b, Instant::now());
        assert!(
            !state.animations.keys().any(|(id, _)| id == &inc_id_a),
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
