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

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::anim::{AnimProp, AnimValue, Animation, Timing};
use crate::tokens;
use crate::tree::{Color, El, InteractionState, Rect};

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

/// A keyboard chord for app-level hotkey registration. Match a key with
/// an exact modifier mask: `KeyChord::ctrl('f')` does not also match
/// `Ctrl+Shift+F`, and `KeyChord::vim('j')` does not match if any
/// modifier is held.
///
/// Register chords from [`App::hotkeys`]; the library matches them
/// against incoming key presses ahead of focus activation routing and
/// emits a [`UiEvent`] with `kind = UiEventKind::Hotkey` and `key`
/// equal to the registered name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyChord {
    pub key: UiKey,
    pub modifiers: KeyModifiers,
}

impl KeyChord {
    /// A bare key with no modifiers (vim-style). `KeyChord::vim('j')`
    /// matches the `j` key with no Ctrl/Shift/Alt/Logo held.
    pub fn vim(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers::default(),
        }
    }

    /// `Ctrl+<char>`.
    pub fn ctrl(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
        }
    }

    /// `Ctrl+Shift+<char>`.
    pub fn ctrl_shift(c: char) -> Self {
        Self {
            key: UiKey::Character(c.to_string()),
            modifiers: KeyModifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        }
    }

    /// A named key with no modifiers (e.g. `KeyChord::named(UiKey::Escape)`).
    pub fn named(key: UiKey) -> Self {
        Self {
            key,
            modifiers: KeyModifiers::default(),
        }
    }

    pub fn with_modifiers(mut self, modifiers: KeyModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// Strict match: keys equal AND modifier mask is identical. Holding
    /// extra modifiers does not match a chord that didn't request them.
    pub fn matches(&self, key: &UiKey, modifiers: KeyModifiers) -> bool {
        key_eq(&self.key, key) && self.modifiers == modifiers
    }
}

fn key_eq(a: &UiKey, b: &UiKey) -> bool {
    match (a, b) {
        (UiKey::Character(x), UiKey::Character(y)) => x.eq_ignore_ascii_case(y),
        _ => a == b,
    }
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
    /// A registered hotkey chord matched. `event.key` is the registered
    /// name (the second element of the `(KeyChord, String)` pair).
    Hotkey,
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

    /// App-level hotkey registry. The library matches incoming key
    /// presses against this list before its own focus-activation
    /// routing; a match emits a [`UiEvent`] with `kind =
    /// UiEventKind::Hotkey` and `key = Some(name)`.
    ///
    /// Called once per build cycle; the host runner snapshots the list
    /// alongside `build()` so the chords stay in sync with state.
    /// Default: no hotkeys.
    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        Vec::new()
    }
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
    /// App-level hotkey registry; the host snapshots `App::hotkeys()`
    /// each frame and stores it here. Matched in `key_down` ahead of
    /// focus activation.
    hotkeys: Vec<(KeyChord, String)>,
    /// In-flight animations keyed by `(computed_id, prop)`. Created
    /// lazily as state transitions happen; trimmed by
    /// [`Self::tick_visual_animations`] when their nodes leave the tree.
    animations: HashMap<(String, AnimProp), Animation>,
    /// Animation pacing mode. Default is `Live`; headless render
    /// binaries switch to `Settled` so single-frame snapshots reflect
    /// the post-animation visual.
    animation_mode: AnimationMode,
}

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

    /// Replace the hotkey registry. Called by the host runner from
    /// `App::hotkeys()` once per build cycle.
    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.hotkeys = hotkeys;
    }

    /// Walk the laid-out tree, retarget per-(node, prop) animations to
    /// the values implied by each node's current `state`, step them
    /// forward to `now`, and write the eased values back into the El's
    /// `fill` / `text_color` / `stroke` / `focus_ring_alpha`.
    ///
    /// The build closure regenerates `n.fill` etc. from the author's
    /// intent each frame, so writing eased values back into the same
    /// fields is safe — the next rebuild restores the originals before
    /// this method runs again.
    ///
    /// Returns `true` if any animation is still in flight; the host
    /// should request another redraw next frame.
    pub fn tick_visual_animations(&mut self, root: &mut El, now: Instant) -> bool {
        let mut visited: HashSet<(String, AnimProp)> = HashSet::new();
        let mut needs_redraw = false;
        let mode = self.animation_mode;
        tick_node(root, &mut self.animations, &mut visited, now, mode, &mut needs_redraw);
        // GC: drop animations whose node left the tree this frame.
        self.animations.retain(|key, _| visited.contains(key));
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
    /// uses this (via [`crate::wgpu_render::PrepareResult`]) to keep
    /// the redraw loop ticking only while there's motion.
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

// ---------------------------------------------------------------------------
// Visual-state animation helpers.
// ---------------------------------------------------------------------------

/// All four animatable visual props. The walker iterates over this list
/// per node so adding a prop is a one-line extension.
const ANIMATED_PROPS: &[AnimProp] = &[
    AnimProp::StateFill,
    AnimProp::StateStroke,
    AnimProp::StateTextColor,
    AnimProp::FocusRingAlpha,
];

fn tick_node(
    node: &mut El,
    anims: &mut HashMap<(String, AnimProp), Animation>,
    visited: &mut HashSet<(String, AnimProp)>,
    now: Instant,
    mode: AnimationMode,
    needs_redraw: &mut bool,
) {
    if !node.computed_id.is_empty() && node.key.is_some() {
        for &prop in ANIMATED_PROPS {
            if let Some(target) = compute_target(node, prop) {
                let key = (node.computed_id.clone(), prop);
                visited.insert(key.clone());
                let anim = anims.entry(key).or_insert_with(|| {
                    // Seed at the target so a node enters the tree
                    // already-settled. The first state change will
                    // retarget and the spring will begin moving.
                    Animation::new(target, target, timing_for(prop), now)
                });
                anim.retarget(target, now);
                let settled = match mode {
                    AnimationMode::Live => anim.step(now),
                    AnimationMode::Settled => {
                        anim.settle();
                        true
                    }
                };
                write_prop(node, prop, anim.current);
                if !settled {
                    *needs_redraw = true;
                }
            }
        }
    }
    for child in &mut node.children {
        tick_node(child, anims, visited, now, mode, needs_redraw);
    }
}

/// Compute the visual target for `prop` based on the node's current
/// interaction state and its build-closure-supplied original value.
/// Returns `None` if the prop doesn't apply (e.g., a node with no fill
/// has no `StateFill` to animate).
fn compute_target(n: &El, prop: AnimProp) -> Option<AnimValue> {
    match prop {
        AnimProp::StateFill => n.fill.map(|fill| AnimValue::Color(state_color(fill, n.state))),
        AnimProp::StateStroke => n.stroke.map(|stroke| AnimValue::Color(state_color(stroke, n.state))),
        AnimProp::StateTextColor => n
            .text_color
            .map(|c| AnimValue::Color(state_text_color(c, n.state))),
        AnimProp::FocusRingAlpha => {
            let target = if matches!(n.state, InteractionState::Focus) {
                1.0
            } else {
                0.0
            };
            Some(AnimValue::Float(target))
        }
    }
}

fn state_color(base: Color, state: InteractionState) -> Color {
    match state {
        InteractionState::Hover => base.lighten(tokens::HOVER_LIGHTEN),
        InteractionState::Press => base.darken(tokens::PRESS_DARKEN),
        _ => base,
    }
}

fn state_text_color(base: Color, state: InteractionState) -> Color {
    match state {
        InteractionState::Hover => base.lighten(tokens::HOVER_LIGHTEN * 0.5),
        _ => base,
    }
}

/// Per-prop timing. Hover/focus settle quickly (overshoot reads as
/// jitter on tiny color deltas); press uses a slightly springier curve
/// so the rebound on release feels responsive.
fn timing_for(prop: AnimProp) -> Timing {
    match prop {
        AnimProp::StateFill | AnimProp::StateStroke | AnimProp::StateTextColor => {
            Timing::SPRING_QUICK
        }
        AnimProp::FocusRingAlpha => Timing::SPRING_QUICK,
    }
}

fn write_prop(n: &mut El, prop: AnimProp, value: AnimValue) {
    match (prop, value) {
        (AnimProp::StateFill, AnimValue::Color(c)) => n.fill = Some(c),
        (AnimProp::StateStroke, AnimValue::Color(c)) => n.stroke = Some(c),
        (AnimProp::StateTextColor, AnimValue::Color(c)) => n.text_color = Some(c),
        (AnimProp::FocusRingAlpha, AnimValue::Float(v)) => {
            n.focus_ring_alpha = v.clamp(0.0, 1.0);
        }
        _ => {}
    }
}

fn is_in_flight(anim: &Animation) -> bool {
    let cur = anim.current.channels();
    let tgt = anim.target.channels();
    if cur.n != tgt.n {
        return true;
    }
    for i in 0..cur.n {
        if (cur.v[i] - tgt.v[i]).abs() > f32::EPSILON {
            return true;
        }
        if anim.velocity.n == cur.n && anim.velocity.v[i].abs() > f32::EPSILON {
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
            hotkeys: Vec::new(),
            animations: HashMap::new(),
            animation_mode: AnimationMode::default(),
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
            hotkeys: Vec::new(),
            animations: HashMap::new(),
            animation_mode: AnimationMode::default(),
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
        let tree = lay_out_counter();
        let mut state = UiState::new();
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

    fn find_fill(node: &El, key: &str) -> Option<Color> {
        if node.key.as_deref() == Some(key) {
            return node.fill;
        }
        node.children.iter().find_map(|c| find_fill(c, key))
    }
    fn find_focus_ring_alpha(node: &El, key: &str) -> Option<f32> {
        if node.key.as_deref() == Some(key) {
            return Some(node.focus_ring_alpha);
        }
        node.children.iter().find_map(|c| find_focus_ring_alpha(c, key))
    }

    #[test]
    fn settled_mode_snaps_hover_to_lightened_fill() {
        // Headless contract: Settled mode must produce the post-hover
        // visual on a single prepare. A windowed runner (Live mode)
        // would ease over many frames; the fixture path can't wait.
        let mut tree = lay_out_counter();
        let mut state = UiState::new();
        state.set_animation_mode(AnimationMode::Settled);
        state.hovered = Some(target(&tree, "inc"));
        state.apply_to_tree(&mut tree);
        let original = find_fill(&tree, "inc").expect("inc fill");

        let needs_redraw = state.tick_visual_animations(&mut tree, Instant::now());
        let after = find_fill(&tree, "inc").expect("inc fill");

        assert!(!needs_redraw, "Settled mode should never report in flight");
        let expected = original.lighten(tokens::HOVER_LIGHTEN);
        assert_eq!(
            (after.r, after.g, after.b),
            (expected.r, expected.g, expected.b),
            "expected lightened fill, got {after:?} (original {original:?})",
        );
    }

    #[test]
    fn live_mode_eases_hover_over_multiple_ticks() {
        // First tick after a state change should be partway between the
        // original and the lightened target — neither snapped to either
        // endpoint. With SPRING_QUICK over an 8 ms tick the spring has
        // moved a small amount.
        let mut tree = lay_out_counter();
        let mut state = UiState::new();
        // Bootstrap: tick once with no hover so the tracker seeds the
        // (button, StateFill) entry at the original fill.
        let t0 = Instant::now();
        state.tick_visual_animations(&mut tree, t0);
        let original = find_fill(&tree, "inc").expect("seed fill");

        // Now hover and tick a single 8 ms step.
        state.hovered = Some(target(&tree, "inc"));
        state.apply_to_tree(&mut tree);
        let needs_redraw = state.tick_visual_animations(
            &mut tree,
            t0 + std::time::Duration::from_millis(8),
        );
        let mid = find_fill(&tree, "inc").expect("mid fill");
        let target_color = original.lighten(tokens::HOVER_LIGHTEN);

        assert!(needs_redraw, "spring should still be in flight after one 8 ms tick");
        // mid is somewhere between original and target — strictly not
        // equal to either, given a non-zero stiffness and a non-zero dt.
        let between = |a: u8, b: u8, c: u8| (a.min(b)..=a.max(b)).contains(&c);
        assert!(
            between(original.r, target_color.r, mid.r)
                && between(original.g, target_color.g, mid.g)
                && between(original.b, target_color.b, mid.b),
            "mid {mid:?} should be between {original:?} and {target_color:?}",
        );
    }

    #[test]
    fn focus_ring_alpha_eases_in_and_out() {
        let mut tree = lay_out_counter();
        let mut state = UiState::new();
        state.set_animation_mode(AnimationMode::Settled);

        // No focus → alpha settled at 0.
        state.tick_visual_animations(&mut tree, Instant::now());
        assert_eq!(find_focus_ring_alpha(&tree, "inc"), Some(0.0));

        // Focus on inc → alpha settles at 1.0.
        let mut tree = lay_out_counter();
        state.focused = Some(target(&tree, "inc"));
        state.apply_to_tree(&mut tree);
        state.tick_visual_animations(&mut tree, Instant::now());
        assert_eq!(find_focus_ring_alpha(&tree, "inc"), Some(1.0));

        // Lose focus → alpha settles back to 0.
        let mut tree = lay_out_counter();
        state.focused = None;
        state.apply_to_tree(&mut tree);
        state.tick_visual_animations(&mut tree, Instant::now());
        assert_eq!(find_focus_ring_alpha(&tree, "inc"), Some(0.0));
    }

    #[test]
    fn animation_entries_gc_when_node_leaves_tree() {
        // Build a tree with two buttons; hover one to seed an entry.
        // Then build a different tree with only one button. The orphan's
        // animation entries should be trimmed.
        let mut tree_a = lay_out_counter();
        let mut state = UiState::new();
        state.hovered = Some(target(&tree_a, "inc"));
        state.apply_to_tree(&mut tree_a);
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
        layout(&mut tree_b, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.hovered = None;
        state.apply_to_tree(&mut tree_b);
        state.tick_visual_animations(&mut tree_b, Instant::now());
        assert!(
            !state
                .animations
                .keys()
                .any(|(id, _)| id == &inc_id_a),
            "stale entries for inc were not GC'd"
        );
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
