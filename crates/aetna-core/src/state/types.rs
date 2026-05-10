//! Data buckets owned by [`UiState`](super::UiState).
//!
//! This module keeps the side-store data shapes separate from the
//! runtime behavior implemented on `UiState`.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use rustc_hash::FxHashMap;
use web_time::Instant;

use crate::anim::{AnimProp, Animation};
use crate::event::{KeyChord, UiTarget};
use crate::tree::{InteractionState, Rect};

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
///
/// Two flavours:
///
/// - **Per-node envelopes** (`Hover`, `Press`, `FocusRing`) track whether
///   *this exact node* is the active hover / press / focus target. Drive
///   per-element visuals — hover-lighten, press-darken, focus-ring fade.
///   Exactly one node owns each at a time, mirroring the single-target
///   `apply_to_state` semantics.
/// - **Subtree envelopes** (`SubtreeHover`, `SubtreePress`,
///   `SubtreeFocus`) track whether the active hover / press / focus
///   target is *this node or any descendant*. Drive
///   region-shaped affordances — hover-revealed close icons, action
///   pills that should stay visible while the cursor moves to a
///   focusable child, hover-driven translate / scale / tint. Multiple
///   nodes can be "hot" simultaneously (every ancestor of the leaf
///   target). CSS `:hover` semantics, lifted onto our id-keyed tree.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum EnvelopeKind {
    Hover,
    Press,
    FocusRing,
    SubtreeHover,
    SubtreePress,
    SubtreeFocus,
}

/// Runtime visual animation state: app-authored prop animations plus
/// library-owned hover/press/focus envelopes and their pacing mode.
#[derive(Default)]
pub(crate) struct AnimationState {
    /// In-flight animations keyed by `(computed_id, prop)`. Created
    /// lazily as state transitions happen; trimmed by
    /// [`UiState::tick_visual_animations`](super::UiState::tick_visual_animations)
    /// when their nodes leave the tree.
    pub(crate) animations: FxHashMap<(String, AnimProp), Animation>,
    /// State-envelope amounts (0..1) per (node, kind), written by the
    /// animation tick. `draw_ops` reads these to modulate the surface
    /// visuals; missing entries read as `0.0`.
    pub(crate) envelopes: FxHashMap<(String, EnvelopeKind), f32>,
    /// Animation pacing mode. Default is `Live`; headless render
    /// binaries switch to `Settled` so single-frame snapshots reflect
    /// the post-animation visual.
    pub(crate) mode: AnimationMode,
}

impl Debug for AnimationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnimationState")
            .field("animations", &self.animations)
            .field("envelopes", &self.envelopes)
            .field("mode", &self.mode)
            .finish()
    }
}

/// App-declared keyboard shortcuts captured by the host each frame and
/// matched before focused-widget key handling.
#[derive(Default)]
pub(crate) struct HotkeyState {
    /// App-level hotkey registry; the host snapshots `App::hotkeys()`
    /// each frame and stores it here. Matched in `key_down` ahead of
    /// focus activation.
    pub(crate) registry: Vec<(KeyChord, String)>,
}

impl Debug for HotkeyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HotkeyState")
            .field("registry", &self.registry)
            .finish()
    }
}

/// Per-instance state owned by a widget. Widget authors define their own
/// state types (e.g. text-input caret + selection, virtual list scroll
/// offset, dropdown open/closed) and stash them on [`UiState`](super::UiState)
/// keyed by node id via [`UiState::widget_state`](super::UiState::widget_state)
/// / [`UiState::widget_state_mut`](super::UiState::widget_state_mut).
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
pub(super) trait AnyWidgetState: WidgetState {
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

/// Type-erased per-node widget storage owned by [`UiState`](super::UiState).
/// Public access stays through `UiState::widget_state*`; this store just
/// keeps the raw buckets and their debug summaries together.
#[derive(Default)]
pub(super) struct WidgetStateStore {
    pub(super) entries: HashMap<(String, TypeId), Box<dyn AnyWidgetState>>,
}

impl Debug for WidgetStateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(
                self.entries
                    .iter()
                    .map(|((id, _), b)| (id.as_str(), b.type_name(), b.debug_summary())),
            )
            .finish()
    }
}

/// Side maps written by the layout pass and read by hit-testing,
/// drawing, custom layout callbacks, and keyed overlay placement.
#[derive(Default)]
pub(crate) struct LayoutState {
    /// Computed rect per node, written by the layout pass.
    pub(crate) computed_rects: FxHashMap<String, Rect>,
    /// `key -> computed_id` map, refreshed at the top of every layout
    /// pass. Populated only for nodes that carry an author-set `key`;
    /// duplicate keys keep the first entry seen in tree order.
    pub(crate) key_index: FxHashMap<String, String>,
}

impl Debug for LayoutState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutState")
            .field("computed_rects", &self.computed_rects)
            .field("key_index", &self.key_index)
            .finish()
    }
}

/// Resolved per-node interaction state written after input processing
/// and read by animation/drawing passes.
#[derive(Default)]
pub(crate) struct NodeInteractionState {
    pub(crate) nodes: FxHashMap<String, InteractionState>,
}

impl Debug for NodeInteractionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeInteractionState")
            .field("nodes", &self.nodes)
            .finish()
    }
}

/// Layout snapshot for a scrollable node. Written each frame by
/// `apply_scroll_offset`; read by the scrollbar thumb in `draw_ops`
/// and by `runtime`'s thumb-drag plumbing. `viewport_h` is the
/// scrollable's inner-rect height (post-padding); `content_h` is the
/// total height of its children; `max_offset` is `(content_h -
/// viewport_h).max(0.0)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollMetrics {
    pub viewport_h: f32,
    pub content_h: f32,
    pub max_offset: f32,
}

/// Active text-selection drag, captured at `pointer_down` on a
/// selectable leaf. The anchor stays fixed; `pointer_moved` extends
/// the head and emits `SelectionChanged`.
#[derive(Clone, Debug)]
pub(crate) struct SelectionDrag {
    pub anchor: crate::selection::SelectionPoint,
}

/// Internal selection-manager state derived from the laid-out tree and
/// active pointer drags. The app-visible selection value remains on
/// `UiState::current_selection` for compatibility.
#[derive(Clone, Debug, Default)]
pub(crate) struct SelectionState {
    /// Selectable text leaves in document (tree) order. Built post-
    /// layout by [`UiState::sync_selection_order`](super::UiState::sync_selection_order);
    /// consulted by the selection manager to map pointer hits to a
    /// [`crate::selection::SelectionPoint`] and to walk cross-element
    /// selections.
    pub(crate) order: Vec<UiTarget>,
    /// Active drag, set by `pointer_down` when the press lands on a
    /// selectable leaf and primary button. The anchor stays fixed for
    /// the duration of the drag; head moves as the pointer moves.
    /// Cleared by `pointer_up`.
    pub(crate) drag: Option<SelectionDrag>,
}

/// Internal focus traversal data derived from the laid-out tree. The
/// currently focused target remains on `UiState::focused` for the
/// existing public API.
#[derive(Clone, Debug, Default)]
pub(crate) struct FocusState {
    pub(crate) order: Vec<UiTarget>,
}

/// Tracks the latest primary `pointer_down` so the next press can
/// extend a multi-click sequence. The runtime increments `count` when
/// a fresh press lands within `MULTI_CLICK_TIME` and `MULTI_CLICK_DIST`
/// of the previous press on the same hit-target; otherwise the
/// sequence resets to 1.
#[derive(Clone, Debug)]
pub(crate) struct ClickSequence {
    pub time: Instant,
    pub pos: (f32, f32),
    pub target_node_id: Option<String>,
    pub count: u8,
}

/// Runtime multi-click bookkeeping. Tracks the latest primary
/// `pointer_down` so the next press can decide whether to extend the
/// sequence or reset to a single click.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClickState {
    pub(crate) last: Option<ClickSequence>,
}

/// Multi-click time window. A press within this duration of the
/// previous matching press extends the sequence (count += 1).
pub(crate) const MULTI_CLICK_TIME: Duration = Duration::from_millis(500);
/// Multi-click distance window in logical pixels. Wider than typical
/// pointer jitter, narrower than a deliberate move to a new target.
pub(crate) const MULTI_CLICK_DIST: f32 = 4.0;

/// Caret stays solid for this long after activity (typing, caret
/// motion, focus arriving) before the blink cycle starts. Prevents
/// the caret from disappearing mid-keystroke.
pub(crate) const CARET_BLINK_GRACE: Duration = Duration::from_millis(500);
/// One on / off period of the caret blink. macOS-ish (~530ms each
/// half) but tunable; the painter only ever sees the resolved alpha,
/// not the period itself.
pub(crate) const CARET_BLINK_PERIOD: Duration = Duration::from_millis(1060);

/// Resolve the caret blink alpha for the given activity age. Returns
/// `1.0` while inside the post-activity grace window, then alternates
/// `1.0` (first half of each period) and `0.0` (second half).
pub(crate) fn caret_blink_alpha_for(age: Duration) -> f32 {
    if age < CARET_BLINK_GRACE {
        return 1.0;
    }
    let t = (age - CARET_BLINK_GRACE).as_millis() as u64;
    let half = (CARET_BLINK_PERIOD.as_millis() as u64) / 2;
    if ((t / half) & 1) == 0 { 1.0 } else { 0.0 }
}

/// Runtime blink state for the focused text caret. Text widgets update
/// this through [`UiState::bump_caret_activity`](super::UiState::bump_caret_activity);
/// the animation tick resolves the current alpha for paint.
#[derive(Clone, Debug, Default)]
pub(crate) struct CaretState {
    /// When the focused-input caret last had visible activity (a
    /// selection change or a focus transition). `None` before the
    /// first bump — caret rendering treats that as solid.
    pub(crate) activity_at: Option<Instant>,
    /// Current caret blink alpha in `[0.0, 1.0]`, written by the
    /// animation tick from `activity_at`.
    pub(crate) blink_alpha: f32,
}

/// Active scrollbar thumb drag. `start_pointer_y` and `start_offset`
/// are captured at `pointer_down`; `pointer_moved` updates
/// `scroll.offsets[scroll_id]` to `start_offset + (dy *
/// max_offset / track_remaining)` so the cursor-thumb pixel
/// relationship stays 1:1.
#[derive(Clone, Debug)]
pub struct ThumbDrag {
    pub scroll_id: String,
    pub start_pointer_y: f32,
    pub start_offset: f32,
    /// Distance the thumb top can travel — `viewport_h - thumb_h`.
    /// Captured at drag start so a content-resize mid-drag doesn't
    /// retro-actively shift the cursor-thumb correspondence.
    pub track_remaining: f32,
    /// `max_offset` captured at drag start, for the same reason.
    pub max_offset: f32,
}

/// Runtime state for scrollable nodes. Kept as one subsystem inside
/// [`UiState`](super::UiState) so layout, paint, and input code do not
/// each grow their own loose side maps.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScrollState {
    /// Scroll offset (logical pixels) per scrollable node, keyed by
    /// `El::computed_id`. The layout pass reads this when positioning a
    /// scrollable's children and writes back the clamped value.
    pub(crate) offsets: FxHashMap<String, f32>,
    /// Per-scrollable layout metrics — viewport height, content
    /// height, max offset — written by the layout pass and read by
    /// `draw_ops` (to size the scrollbar thumb) and the runtime (to
    /// translate thumb-drag delta into offset delta).
    pub(crate) metrics: FxHashMap<String, ScrollMetrics>,
    /// Per-scrollable thumb rect (logical pixels), populated alongside
    /// `metrics` when the scrollable has `scrollbar` enabled and its
    /// content overflows. Read by `draw_ops` to paint the thumb. An
    /// entry is *absent* when the scrollbar is disabled or the content
    /// fits the viewport.
    pub(crate) thumb_rects: FxHashMap<String, Rect>,
    /// Per-scrollable track rect — the full vertical column that
    /// accepts pointer presses (wider than the visible thumb so the
    /// thumb is easy to grab; full viewport height so a click on the
    /// track above/below the thumb pages by a viewport). Same x-extent
    /// as `thumb_rects` but expanded to `SCROLLBAR_HITBOX_WIDTH` and
    /// the inner-rect height. Populated alongside `thumb_rects`.
    pub(crate) thumb_tracks: FxHashMap<String, Rect>,
    /// Active scrollbar drag, set by `pointer_down` when the press
    /// lands inside a thumb rect, consumed by `pointer_moved` to update
    /// the corresponding `offsets` entry, cleared by `pointer_up`.
    /// Pre-empts normal hit-test so thumb drags don't also fire
    /// app-level pointer events.
    pub(crate) thumb_drag: Option<ThumbDrag>,
    /// Per-virtual-list row-height measurement cache, keyed by the
    /// virtual list node's `computed_id` and then by row index. Filled
    /// by the layout pass for `VirtualMode::Dynamic` lists as rows
    /// enter the viewport and are measured. Subsequent frames read this
    /// instead of falling back to the estimated row height, so scroll
    /// math stabilizes once the visible regions have been seen.
    pub(crate) measured_row_heights: FxHashMap<String, FxHashMap<usize, f32>>,
}

/// Runtime queue for toast notifications. Apps provide fire-and-forget
/// [`crate::toast::ToastSpec`] values; the runtime stamps ids and
/// expiry deadlines here before [`crate::toast::synthesize_toasts`]
/// mirrors the queue into a synthetic overlay layer.
#[derive(Clone, Debug, Default)]
pub(crate) struct ToastState {
    pub(crate) queue: Vec<crate::toast::Toast>,
    pub(crate) next_id: u64,
}

/// Runtime hover timing for tooltips. The hovered target itself stays
/// in the general pointer interaction state; this bucket only tracks
/// tooltip-specific delay and per-hover dismissal.
#[derive(Clone, Debug, Default)]
pub(crate) struct TooltipState {
    /// When the current `hovered` target started being hovered. `None`
    /// when nothing is hovered or the pointer is outside the window.
    /// Used by [`crate::tooltip`] to gate the hover-delay timer.
    pub(crate) hover_started_at: Option<Instant>,
    /// True when the user pressed (or clicked) the hovered node during
    /// the current hover session. Suppresses the tooltip until the
    /// pointer leaves and re-enters, matching native behavior.
    pub(crate) dismissed_for_hover: bool,
}

/// Focus bookkeeping for runtime-managed popover layers. The active
/// focus target and tab order stay on `UiState`; this bucket only
/// tracks layer open/close transitions and saved focus restoration.
#[derive(Clone, Debug, Default)]
pub(crate) struct PopoverFocusState {
    /// LIFO of focus targets pushed when popover layers open. Each new
    /// `Kind::Custom("popover_layer")` snapshots the current focus
    /// here and auto-focuses into the layer; closing the layer pops and
    /// restores. See [`crate::focus::sync_popover_focus`].
    pub(crate) focus_stack: Vec<UiTarget>,
    /// `computed_id`s of every popover-layer node in the last laid-out
    /// tree, in tree order. Diffed against the new tree to detect open
    /// / close transitions.
    pub(crate) layer_ids: Vec<String>,
}
