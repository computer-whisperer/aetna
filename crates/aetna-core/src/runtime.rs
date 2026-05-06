//! `RunnerCore` — the backend-agnostic half of every Aetna runner.
//!
//! Holds interaction state ([`UiState`], `last_tree`) and paint scratch
//! buffers (`quad_scratch`, `runs`, `paint_items`) plus the geometry
//! context (`viewport_px`, `surface_size_override`) needed to project
//! layout's logical-pixel rects into physical-pixel scissors. Exposes
//! the identical interaction methods both backends ship: `pointer_*`,
//! `key_down`, `set_hotkeys`, `set_animation_mode`, `ui_state`,
//! `rect_of_key`, `debug_summary`, `set_surface_size`, plus the layout
//! / paint-stream stages that are pure CPU work.
//!
//! Each backend's `Runner` *contains* a `RunnerCore` and forwards the
//! interaction methods to it; only the GPU resources (pipelines,
//! buffers, atlases) and the actual GPU upload + draw work stay
//! per-backend. The split shares what's identical without a trait —
//! same shape as `crate::paint`, larger surface.
//!
//! ## What this module does NOT own
//!
//! - **Pipeline registration.** Each backend builds its own
//!   `pipelines: HashMap<ShaderHandle, BackendPipeline>` because the
//!   pipeline value type is GPU-specific.
//! - **Text upload.** Glyph atlas pages live on the GPU as backend
//!   images; the `TextPaint` that owns them is per-backend. Core
//!   reaches into it through the [`TextRecorder`] trait during the
//!   paint stream loop, then the backend flushes its atlas separately.
//! - **GPU upload of `quad_scratch` / frame uniforms.** Backend
//!   responsibility — `prepare()` orchestrates the full sequence.
//! - **`draw()`.** Both backends walk `core.paint_items` + `core.runs`
//!   themselves because the encoder type (and lifetime) diverges.
//!
//! ## Why no `Painter` trait
//!
//! Extracting a `trait Painter { fn prepare(...); fn draw(...); fn
//! set_scissor(...); }` was considered so backends would share *one*
//! abstraction surface. We declined: the only call sites left after
//! this module + [`crate::paint`] are the two
//! `prepare()` GPU-upload tails and the two `draw()` walks, and both
//! need backend-typed handles (`wgpu::RenderPass<'_>` /
//! `AutoCommandBufferBuilder<...>`) that no trait can hide without
//! generics that re-fragment the surface. A `Painter` trait would
//! reduce to a 1-method `set_scissor` indirection plus host-side
//! ceremony — dead weight. The duplication that *is* worth abstracting
//! is the host harness (winit init, swapchain management,
//! `aetna-{wgpu,vulkano}-demo::run`) — and that lives a layer above
//! the paint surface, not inside it. Revisit if a third backend lands
//! or if the GPU-upload sequences diverge enough to make a typed-state
//! interface earn its keep.

use std::ops::Range;
use std::time::Duration;

use web_time::Instant;

use crate::draw_ops;
use crate::event::{KeyChord, KeyModifiers, PointerButton, UiEvent, UiEventKind, UiKey, UiTarget};
use crate::focus;
use crate::hit_test;
use crate::ir::{DrawOp, TextAnchor};
use crate::layout;
use crate::paint::{
    InstanceRun, PaintItem, PhysicalScissor, QuadInstance, close_run, pack_instance,
    physical_scissor,
};
use crate::shader::ShaderHandle;
use crate::state::{AnimationMode, UiState};
use crate::text::atlas::RunStyle;
use crate::theme::Theme;
use crate::tooltip;
use crate::tree::{Color, El, FontWeight, Rect, TextWrap};

/// Logical-pixel overlap kept between the pre-page and post-page
/// viewport when the user clicks the scroll track above/below the
/// thumb. Matches browser convention: paging by `viewport_h - overlap`
/// preserves the bottom (resp. top) row across the jump so context
/// isn't lost.
const SCROLL_PAGE_OVERLAP: f32 = 24.0;

/// Reported back from each backend's `prepare(...)` per frame. The
/// host uses `needs_redraw` to keep the redraw loop ticking only
/// while there is in-flight motion (a hover spring still settling, a
/// focus ring still fading out), then idles. `timings` is a per-frame
/// CPU breakdown for diagnostic logging.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareResult {
    pub needs_redraw: bool,
    pub timings: PrepareTimings,
}

/// Per-stage CPU timing inside each backend's `prepare`. Cheap to
/// compute (a handful of `Instant::now()` calls per frame) and useful
/// for finding the dominant cost when frame budget is tight.
///
/// Stages:
/// - `layout`: layout pass + focus order sync + state apply + animation tick.
/// - `draw_ops`: tree → DrawOp[] resolution.
/// - `paint`: paint-stream loop (quad packing + text shaping via cosmic-text).
/// - `gpu_upload`: backend-side instance buffer write + atlas flush + frame uniforms.
/// - `snapshot`: cloning the laid-out tree for next-frame hit-testing.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareTimings {
    pub layout: Duration,
    pub draw_ops: Duration,
    pub paint: Duration,
    pub gpu_upload: Duration,
    pub snapshot: Duration,
}

/// Backend-agnostic runner state.
///
/// Each backend's `Runner` owns one of these as its `core` field and
/// forwards the public interaction surface to it. The fields are `pub`
/// so backends can read them in `draw()` (which has to traverse
/// `paint_items` + `runs` against backend-specific pipeline and
/// instance-buffer objects).
pub struct RunnerCore {
    pub ui_state: UiState,
    /// Snapshot of the last laid-out tree, kept so pointer events
    /// arriving between frames hit-test against the geometry the user
    /// is actually looking at.
    pub last_tree: Option<El>,

    /// Per-frame quad instance scratch — backends `bytemuck::cast_slice`
    /// this into their VBO upload.
    pub quad_scratch: Vec<QuadInstance>,
    pub runs: Vec<InstanceRun>,
    pub paint_items: Vec<PaintItem>,

    /// Physical viewport size in pixels. Backends use this for `draw()`
    /// scissor binding (logical scissors get projected into this space
    /// inside `prepare_paint`).
    pub viewport_px: (u32, u32),
    /// When set, overrides the physical viewport derived from
    /// `viewport.w * scale_factor` so paint-side scissor math matches
    /// the actual swapchain extent. Backends call
    /// [`Self::set_surface_size`] from their host's surface-config /
    /// resize hook to keep this in lockstep.
    pub surface_size_override: Option<(u32, u32)>,

    /// Theme used when resolving implicit widget surfaces to shaders.
    pub theme: Theme,
}

impl Default for RunnerCore {
    fn default() -> Self {
        Self::new()
    }
}

impl RunnerCore {
    pub fn new() -> Self {
        Self {
            ui_state: UiState::default(),
            last_tree: None,
            quad_scratch: Vec::new(),
            runs: Vec::new(),
            paint_items: Vec::new(),
            viewport_px: (1, 1),
            surface_size_override: None,
            theme: Theme::default(),
        }
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Override the physical viewport size. Call after the host's
    /// surface configure or resize so scissor math sees the swapchain's
    /// real extent (fractional `scale_factor` round-trips can otherwise
    /// land `viewport_px` one pixel off and trip
    /// `set_scissor_rect` validation).
    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.surface_size_override = Some((width.max(1), height.max(1)));
    }

    pub fn ui_state(&self) -> &UiState {
        &self.ui_state
    }

    pub fn debug_summary(&self) -> String {
        self.ui_state.debug_summary()
    }

    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.last_tree
            .as_ref()
            .and_then(|t| self.ui_state.rect_of_key(t, key))
    }

    // ---- Input plumbing ----

    /// Pointer moved to `(x, y)` (logical px). Updates the hovered
    /// node (readable via `ui_state().hovered`) and, if the primary
    /// button is currently held, returns a `Drag` event routed to the
    /// originally pressed target. The event's `modifiers` field
    /// reflects the mask currently tracked on `UiState` (set by the
    /// host via `set_modifiers`).
    pub fn pointer_moved(&mut self, x: f32, y: f32) -> Option<UiEvent> {
        self.ui_state.pointer_pos = Some((x, y));

        // Active scrollbar drag: translate cursor delta into
        // `scroll_offsets` updates. The drag is captured at
        // `pointer_down` so we can map directly onto the scroll
        // container without going through hit-test, and we suppress
        // the normal hover/Drag event emission while it's in flight.
        if let Some(drag) = self.ui_state.thumb_drag.clone() {
            let dy = y - drag.start_pointer_y;
            let new_offset = if drag.track_remaining > 0.0 {
                drag.start_offset + dy * (drag.max_offset / drag.track_remaining)
            } else {
                drag.start_offset
            };
            let clamped = new_offset.clamp(0.0, drag.max_offset);
            self.ui_state
                .scroll_offsets
                .insert(drag.scroll_id, clamped);
            return None;
        }

        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        self.ui_state.set_hovered(hit, Instant::now());
        // Drag: pointer moved while primary button is down → emit Drag
        // to the originally pressed target. Cursor escape from the
        // pressed node is the *normal* drag-extend case (e.g. text
        // selection); we keep emitting until pointer_up clears `pressed`.
        let modifiers = self.ui_state.modifiers;
        self.ui_state.pressed.clone().map(|p| UiEvent {
            key: Some(p.key.clone()),
            target: Some(p),
            pointer: Some((x, y)),
            key_press: None,
            text: None,
            modifiers,
            kind: UiEventKind::Drag,
        })
    }

    pub fn pointer_left(&mut self) {
        self.ui_state.pointer_pos = None;
        self.ui_state.set_hovered(None, Instant::now());
        self.ui_state.pressed = None;
        self.ui_state.pressed_secondary = None;
    }

    /// Primary/secondary/middle pointer button pressed at `(x, y)`.
    /// For the primary button, focuses the hit target and stashes it
    /// as the pressed target; returns a `PointerDown` event so widgets
    /// like text_input can react at down-time (e.g., set the selection
    /// anchor before any drag extends it). Secondary/middle store on a
    /// separate channel and never emit a `PointerDown`.
    pub fn pointer_down(&mut self, x: f32, y: f32, button: PointerButton) -> Option<UiEvent> {
        // Scrollbar track pre-empts normal hit-test: a primary press
        // inside a scrollable's track column either captures a thumb
        // drag (when the press lands inside the visible thumb rect)
        // or pages the scroll offset by a viewport (when it lands
        // above or below the thumb). Both branches suppress focus /
        // press / event chains for the press itself; `pointer_moved`
        // then drives the drag (no-op for paged clicks) and
        // `pointer_up` clears the drag.
        if matches!(button, PointerButton::Primary)
            && let Some((scroll_id, _track, thumb_rect)) = self.ui_state.thumb_at(x, y)
        {
            let metrics = self
                .ui_state
                .scroll_metrics
                .get(&scroll_id)
                .copied()
                .unwrap_or_default();
            let start_offset = self
                .ui_state
                .scroll_offsets
                .get(&scroll_id)
                .copied()
                .unwrap_or(0.0);

            // Grab when the press lands inside the visible thumb;
            // page otherwise. The track is wider than the thumb
            // horizontally, so this branch is decided by `y` alone.
            let grabbed = y >= thumb_rect.y && y <= thumb_rect.y + thumb_rect.h;
            if grabbed {
                let track_remaining = (metrics.viewport_h - thumb_rect.h).max(0.0);
                self.ui_state.thumb_drag = Some(crate::state::ThumbDrag {
                    scroll_id,
                    start_pointer_y: y,
                    start_offset,
                    track_remaining,
                    max_offset: metrics.max_offset,
                });
            } else {
                // Click-to-page. Browser convention: each press
                // shifts the offset by ~one viewport with a small
                // overlap so context isn't lost. Direction is
                // decided by which side of the thumb the press
                // landed on.
                let page = (metrics.viewport_h - SCROLL_PAGE_OVERLAP).max(0.0);
                let delta = if y < thumb_rect.y { -page } else { page };
                let new_offset = (start_offset + delta).clamp(0.0, metrics.max_offset);
                self.ui_state.scroll_offsets.insert(scroll_id, new_offset);
            }
            return None;
        }

        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        // Only the primary button drives focus + the visual press
        // envelope. Secondary/middle clicks shouldn't yank focus from
        // the currently-focused element (matches browser/native behavior
        // where right-clicking a button doesn't take focus).
        if matches!(button, PointerButton::Primary) {
            self.ui_state.set_focus(hit.clone());
            self.ui_state.pressed = hit.clone();
            // A press on the hovered node dismisses any tooltip for
            // the rest of this hover session — matches native UIs.
            self.ui_state.tooltip_dismissed_for_hover = true;
            let modifiers = self.ui_state.modifiers;
            hit.map(|p| UiEvent {
                key: Some(p.key.clone()),
                target: Some(p),
                pointer: Some((x, y)),
                key_press: None,
                text: None,
                modifiers,
                kind: UiEventKind::PointerDown,
            })
        } else {
            // Stash the down-target on the secondary/middle channel so
            // pointer_up can confirm the click landed on the same node.
            self.ui_state.pressed_secondary = hit.map(|h| (h, button));
            None
        }
    }

    /// Pointer released. For the primary button, fires `PointerUp`
    /// (always, with the originally pressed target so drag-aware
    /// widgets see drag-end) and additionally `Click` if the release
    /// landed on the same node as the down. For secondary / middle,
    /// fires the corresponding click variant when the up landed on the
    /// same node; no analogue of `PointerUp` since drag is a primary-
    /// button concept here.
    pub fn pointer_up(&mut self, x: f32, y: f32, button: PointerButton) -> Vec<UiEvent> {
        // Scrollbar drag ends without producing app-level events —
        // the press never went through `pressed` / `pressed_secondary`
        // so there's nothing else to clean up. Released from anywhere;
        // the drag is global once captured, matching native scrollbars.
        if matches!(button, PointerButton::Primary) && self.ui_state.thumb_drag.is_some() {
            self.ui_state.thumb_drag = None;
            return Vec::new();
        }

        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        let modifiers = self.ui_state.modifiers;
        let mut out = Vec::new();
        match button {
            PointerButton::Primary => {
                let pressed = self.ui_state.pressed.take();
                if let Some(p) = pressed.clone() {
                    out.push(UiEvent {
                        key: Some(p.key.clone()),
                        target: Some(p),
                        pointer: Some((x, y)),
                        key_press: None,
                        text: None,
                        modifiers,
                        kind: UiEventKind::PointerUp,
                    });
                }
                if let (Some(p), Some(h)) = (pressed, hit)
                    && p.node_id == h.node_id
                {
                    out.push(UiEvent {
                        key: Some(p.key.clone()),
                        target: Some(p),
                        pointer: Some((x, y)),
                        key_press: None,
                        text: None,
                        modifiers,
                        kind: UiEventKind::Click,
                    });
                }
            }
            PointerButton::Secondary | PointerButton::Middle => {
                let pressed = self.ui_state.pressed_secondary.take();
                if let (Some((p, b)), Some(h)) = (pressed, hit)
                    && b == button
                    && p.node_id == h.node_id
                {
                    let kind = match button {
                        PointerButton::Secondary => UiEventKind::SecondaryClick,
                        PointerButton::Middle => UiEventKind::MiddleClick,
                        PointerButton::Primary => unreachable!(),
                    };
                    out.push(UiEvent {
                        key: Some(p.key.clone()),
                        target: Some(p),
                        pointer: Some((x, y)),
                        key_press: None,
                        text: None,
                        modifiers,
                        kind,
                    });
                }
            }
        }
        out
    }

    pub fn key_down(
        &mut self,
        key: UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        // Capture path: when the focused node opted into raw key
        // capture, the library's Tab/Enter/Escape interpretation is
        // bypassed and the event is delivered as a raw `KeyDown` to
        // the focused target. Hotkeys still match first — an app's
        // global Ctrl+S beats a text input's local consumption of S.
        if self.focused_captures_keys() {
            if let Some(event) = self.ui_state.try_hotkey(&key, modifiers, repeat) {
                return Some(event);
            }
            return self.ui_state.key_down_raw(key, modifiers, repeat);
        }

        // Arrow-nav: if the focused node sits inside an arrow-navigable
        // group (typically a popover_panel of menu items), Up / Down /
        // Home / End move focus among its focusable siblings rather
        // than emitting a `KeyDown` event. Hotkeys are still matched
        // first so a global Ctrl+ArrowUp chord beats menu navigation.
        if matches!(
            key,
            UiKey::ArrowUp | UiKey::ArrowDown | UiKey::Home | UiKey::End
        ) && let Some(siblings) = self.focused_arrow_nav_group()
        {
            if let Some(event) = self.ui_state.try_hotkey(&key, modifiers, repeat) {
                return Some(event);
            }
            self.move_focus_in_group(&key, &siblings);
            return None;
        }

        self.ui_state.key_down(key, modifiers, repeat)
    }

    /// Look up the focused node's nearest [`El::arrow_nav_siblings`]
    /// parent in the last laid-out tree and return the focusable
    /// siblings (the navigation targets for Up / Down / Home / End).
    /// Returns `None` when no node is focused, the tree hasn't been
    /// built yet, or the focused element isn't inside an
    /// arrow-navigable parent.
    fn focused_arrow_nav_group(&self) -> Option<Vec<UiTarget>> {
        let focused = self.ui_state.focused.as_ref()?;
        let tree = self.last_tree.as_ref()?;
        focus::arrow_nav_group(tree, &self.ui_state, &focused.node_id)
    }

    /// Move the focused element to the appropriate sibling for `key`.
    /// `Up` / `Down` step by one (saturating at the ends — no wrap, so
    /// holding the key doesn't loop visually); `Home` / `End` jump to
    /// the first / last sibling.
    fn move_focus_in_group(&mut self, key: &UiKey, siblings: &[UiTarget]) {
        if siblings.is_empty() {
            return;
        }
        let focused_id = match self.ui_state.focused.as_ref() {
            Some(t) => t.node_id.clone(),
            None => return,
        };
        let idx = siblings.iter().position(|t| t.node_id == focused_id);
        let next_idx = match (key, idx) {
            (UiKey::ArrowUp, Some(i)) => i.saturating_sub(1),
            (UiKey::ArrowDown, Some(i)) => (i + 1).min(siblings.len() - 1),
            (UiKey::Home, _) => 0,
            (UiKey::End, _) => siblings.len() - 1,
            _ => return,
        };
        if Some(next_idx) != idx {
            self.ui_state.set_focus(Some(siblings[next_idx].clone()));
        }
    }

    /// Look up the focused node in the last laid-out tree and return
    /// its `capture_keys` flag. False when no node is focused or the
    /// tree hasn't been built yet.
    fn focused_captures_keys(&self) -> bool {
        let Some(focused) = self.ui_state.focused.as_ref() else {
            return false;
        };
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        find_capture_keys(tree, &focused.node_id).unwrap_or(false)
    }

    /// OS-composed text input (printable characters after dead-key /
    /// shift / IME composition). Routed to the focused element as a
    /// `TextInput` event. Returns `None` if no node has focus, or if
    /// `text` is empty (some platforms emit empty composition strings
    /// during IME selection).
    pub fn text_input(&mut self, text: String) -> Option<UiEvent> {
        if text.is_empty() {
            return None;
        }
        let target = self.ui_state.focused.clone()?;
        let modifiers = self.ui_state.modifiers;
        Some(UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: None,
            key_press: None,
            text: Some(text),
            modifiers,
            kind: UiEventKind::TextInput,
        })
    }

    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.ui_state.set_hotkeys(hotkeys);
    }

    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.ui_state.set_animation_mode(mode);
    }

    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        self.ui_state.pointer_wheel(tree, (x, y), dy)
    }

    // ---- Per-frame staging ----

    /// Layout + state apply + animation tick + viewport projection +
    /// `DrawOp` resolution. Returns the resolved op list and whether
    /// visual animations need another frame; writes per-stage timings
    /// into `timings` (`layout` + `draw_ops`).
    pub fn prepare_layout(
        &mut self,
        root: &mut El,
        viewport: Rect,
        scale_factor: f32,
        timings: &mut PrepareTimings,
    ) -> (Vec<DrawOp>, bool) {
        let t0 = Instant::now();
        // Tooltip synthesis runs before the real layout: assign ids
        // first so we can find the hovered node by computed_id, then
        // append a tooltip layer if one is due. The subsequent
        // `layout::layout` call re-assigns (idempotently — same path
        // shapes produce the same ids) and lays out the appended
        // layer alongside everything else.
        layout::assign_ids(root);
        let tooltip_pending = tooltip::synthesize_tooltip(root, &self.ui_state, t0);
        layout::layout(root, &mut self.ui_state, viewport);
        self.ui_state.sync_focus_order(root);
        focus::sync_popover_focus(root, &mut self.ui_state);
        self.ui_state.apply_to_state();
        let needs_redraw =
            self.ui_state.tick_visual_animations(root, Instant::now()) || tooltip_pending;
        self.viewport_px = self.surface_size_override.unwrap_or_else(|| {
            (
                (viewport.w * scale_factor).ceil().max(1.0) as u32,
                (viewport.h * scale_factor).ceil().max(1.0) as u32,
            )
        });
        let t_after_layout = Instant::now();
        let ops = draw_ops::draw_ops_with_theme(root, &self.ui_state, &self.theme);
        let t_after_draw_ops = Instant::now();
        timings.layout = t_after_layout - t0;
        timings.draw_ops = t_after_draw_ops - t_after_layout;
        (ops, needs_redraw)
    }

    /// Walk the resolved `DrawOp` list, packing quads into
    /// `quad_scratch` + grouping them into `runs`, interleaving text
    /// records via the backend-supplied [`TextRecorder`]. Returns the
    /// number of quad instances written (so the backend can size its
    /// instance buffer).
    ///
    /// Callers must call `text.frame_begin()` themselves *before*
    /// invoking this — `prepare_paint` does not call it for them
    /// because backends often want to clear other per-frame text
    /// scratch in the same step.
    pub fn prepare_paint<F1, F2>(
        &mut self,
        ops: &[DrawOp],
        is_registered: F1,
        samples_backdrop: F2,
        text: &mut dyn TextRecorder,
        scale_factor: f32,
        timings: &mut PrepareTimings,
    ) where
        F1: Fn(&ShaderHandle) -> bool,
        F2: Fn(&ShaderHandle) -> bool,
    {
        let t0 = Instant::now();
        self.quad_scratch.clear();
        self.runs.clear();
        self.paint_items.clear();

        let mut current: Option<(ShaderHandle, Option<PhysicalScissor>)> = None;
        let mut run_first: u32 = 0;
        // At most one snapshot per frame. Auto-inserted before
        // the first paint that samples the backdrop.
        let mut snapshot_emitted = false;

        for op in ops {
            match op {
                DrawOp::Quad {
                    rect,
                    scissor,
                    shader,
                    uniforms,
                    ..
                } => {
                    if !is_registered(shader) {
                        continue;
                    }
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        continue;
                    }
                    if !snapshot_emitted && samples_backdrop(shader) {
                        close_run(
                            &mut self.runs,
                            &mut self.paint_items,
                            current,
                            run_first,
                            self.quad_scratch.len() as u32,
                        );
                        current = None;
                        run_first = self.quad_scratch.len() as u32;
                        self.paint_items.push(PaintItem::BackdropSnapshot);
                        snapshot_emitted = true;
                    }
                    let inst = pack_instance(*rect, *shader, uniforms);

                    let key = (*shader, phys);
                    if current != Some(key) {
                        close_run(
                            &mut self.runs,
                            &mut self.paint_items,
                            current,
                            run_first,
                            self.quad_scratch.len() as u32,
                        );
                        current = Some(key);
                        run_first = self.quad_scratch.len() as u32;
                    }
                    self.quad_scratch.push(inst);
                }
                DrawOp::GlyphRun {
                    rect,
                    scissor,
                    color,
                    text: glyph_text,
                    size,
                    weight,
                    wrap,
                    anchor,
                    ..
                } => {
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        continue;
                    }
                    let layers = text.record(
                        *rect,
                        phys,
                        *color,
                        glyph_text,
                        *size,
                        *weight,
                        *wrap,
                        *anchor,
                        scale_factor,
                    );
                    for index in layers {
                        self.paint_items.push(PaintItem::Text(index));
                    }
                }
                DrawOp::AttributedText {
                    rect,
                    scissor,
                    runs,
                    size,
                    wrap,
                    anchor,
                    ..
                } => {
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        continue;
                    }
                    let layers =
                        text.record_runs(*rect, phys, runs, *size, *wrap, *anchor, scale_factor);
                    for index in layers {
                        self.paint_items.push(PaintItem::Text(index));
                    }
                }
                DrawOp::Icon {
                    rect,
                    scissor,
                    source,
                    color,
                    size,
                    stroke_width,
                    ..
                } => {
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;

                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(phys, Some(s) if s.w == 0 || s.h == 0) {
                        continue;
                    }
                    let recorded = text.record_icon(
                        *rect,
                        phys,
                        source,
                        *color,
                        *size,
                        *stroke_width,
                        scale_factor,
                    );
                    match recorded {
                        RecordedPaint::Text(layers) => {
                            for index in layers {
                                self.paint_items.push(PaintItem::Text(index));
                            }
                        }
                        RecordedPaint::Icon(runs) => {
                            for index in runs {
                                self.paint_items.push(PaintItem::IconRun(index));
                            }
                        }
                    }
                }
                DrawOp::BackdropSnapshot => {
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current = None;
                    run_first = self.quad_scratch.len() as u32;
                    // Cap at one snapshot per frame; an explicit op only
                    // lands if the auto-emitter hasn't fired yet.
                    if !snapshot_emitted {
                        self.paint_items.push(PaintItem::BackdropSnapshot);
                        snapshot_emitted = true;
                    }
                }
            }
        }
        close_run(
            &mut self.runs,
            &mut self.paint_items,
            current,
            run_first,
            self.quad_scratch.len() as u32,
        );
        timings.paint = Instant::now() - t0;
    }

    /// Take a clone of the laid-out tree for next-frame hit-testing.
    /// Call after the per-frame work completes (GPU upload, atlas
    /// flush, etc.) so the snapshot reflects final geometry. Writes
    /// `timings.snapshot`.
    pub fn snapshot(&mut self, root: &El, timings: &mut PrepareTimings) {
        let t0 = Instant::now();
        self.last_tree = Some(root.clone());
        timings.snapshot = Instant::now() - t0;
    }
}

/// Find the `capture_keys` flag of the node whose `computed_id`
/// equals `id`, walking the laid-out tree. Returns `None` when the id
/// isn't found (the focused target outlived its node — a one-frame
/// race after a rebuild).
fn find_capture_keys(node: &El, id: &str) -> Option<bool> {
    if node.computed_id == id {
        return Some(node.capture_keys);
    }
    node.children.iter().find_map(|c| find_capture_keys(c, id))
}

/// Recorded output from an icon draw op. Backends without a vector-icon
/// path use `Text` fallback layers; wgpu can return dedicated icon runs.
pub enum RecordedPaint {
    Text(Range<usize>),
    Icon(Range<usize>),
}

/// Glyph-recording surface implemented by each backend's `TextPaint`.
/// `prepare_paint` calls into it exactly the same way wgpu and vulkano
/// would call their per-backend equivalents.
pub trait TextRecorder {
    /// Append per-glyph instances for `text` and return the range of
    /// indices written into the backend's `TextLayer` storage. Each
    /// returned index lands in `paint_items` as a `PaintItem::Text`.
    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        color: Color,
        text: &str,
        size: f32,
        weight: FontWeight,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize>;

    /// Append per-glyph instances for an attributed paragraph (one
    /// shaped run with per-character RunStyle metadata). Wrapping
    /// decisions cross run boundaries — the result is one ShapedRun
    /// just like a single-style call.
    #[allow(clippy::too_many_arguments)]
    fn record_runs(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        runs: &[(String, RunStyle)],
        size: f32,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize>;

    /// Append a vector icon. Backends with a native vector painter
    /// override this; the default keeps experimental/simple backends on
    /// the previous text-symbol fallback. Built-in icons fall back to
    /// their named glyph; app-supplied SVG icons fall back to a
    /// generic placeholder since they have no canonical glyph.
    #[allow(clippy::too_many_arguments)]
    fn record_icon(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        source: &crate::svg_icon::IconSource,
        color: Color,
        size: f32,
        _stroke_width: f32,
        scale_factor: f32,
    ) -> RecordedPaint {
        let glyph = match source {
            crate::svg_icon::IconSource::Builtin(name) => name.fallback_glyph(),
            crate::svg_icon::IconSource::Custom(_) => "?",
        };
        RecordedPaint::Text(self.record(
            rect,
            scissor,
            color,
            glyph,
            size,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Middle,
            scale_factor,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::{ShaderHandle, StockShader, UniformBlock};

    /// Minimal recorder for tests that don't exercise the text path.
    struct NoText;
    impl TextRecorder for NoText {
        fn record(
            &mut self,
            _rect: Rect,
            _scissor: Option<PhysicalScissor>,
            _color: Color,
            _text: &str,
            _size: f32,
            _weight: FontWeight,
            _wrap: TextWrap,
            _anchor: TextAnchor,
            _scale_factor: f32,
        ) -> Range<usize> {
            0..0
        }
        fn record_runs(
            &mut self,
            _rect: Rect,
            _scissor: Option<PhysicalScissor>,
            _runs: &[(String, RunStyle)],
            _size: f32,
            _wrap: TextWrap,
            _anchor: TextAnchor,
            _scale_factor: f32,
        ) -> Range<usize> {
            0..0
        }
    }

    // ---- input plumbing ----

    /// A tree with one focusable button at (10,10,80,40) keyed "btn",
    /// plus an optional capture_keys text input at (10,60,80,40) keyed
    /// "ti". layout() runs against a 200x200 viewport so the rects
    /// land where we expect.
    fn lay_out_input_tree(capture: bool) -> RunnerCore {
        use crate::tree::*;
        let ti = if capture {
            crate::widgets::text::text("input").key("ti").capture_keys()
        } else {
            crate::widgets::text::text("noop").key("ti").focusable()
        };
        let mut tree =
            crate::column([crate::widgets::button::button("Btn").key("btn"), ti]).padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        core
    }

    #[test]
    fn pointer_up_emits_pointer_up_then_click() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_moved(cx, cy);
        core.pointer_down(cx, cy, PointerButton::Primary);
        let events = core.pointer_up(cx, cy, PointerButton::Primary);
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![UiEventKind::PointerUp, UiEventKind::Click]);
    }

    #[test]
    fn pointer_up_off_target_emits_only_pointer_up() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(cx, cy, PointerButton::Primary);
        // Release off-target (well outside any keyed node).
        let events = core.pointer_up(180.0, 180.0, PointerButton::Primary);
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![UiEventKind::PointerUp],
            "drag-off-target should still surface PointerUp so widgets see drag-end"
        );
    }

    #[test]
    fn pointer_moved_while_pressed_emits_drag() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(cx, cy, PointerButton::Primary);
        let drag = core
            .pointer_moved(cx + 30.0, cy)
            .expect("drag while pressed");
        assert_eq!(drag.kind, UiEventKind::Drag);
        assert_eq!(drag.target.as_ref().map(|t| t.key.as_str()), Some("btn"));
        assert_eq!(drag.pointer, Some((cx + 30.0, cy)));
    }

    #[test]
    fn pointer_moved_without_press_emits_no_drag() {
        let mut core = lay_out_input_tree(false);
        assert!(core.pointer_moved(50.0, 50.0).is_none());
    }

    /// Build a 200x200 viewport hosting a `scroll([rows...])` whose
    /// content overflows so the thumb is present.
    fn lay_out_scroll_tree() -> (RunnerCore, String) {
        use crate::tree::*;
        let mut tree = crate::scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        let scroll_id = tree.computed_id.clone();
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        (core, scroll_id)
    }

    #[test]
    fn thumb_pointer_down_captures_drag_and_suppresses_events() {
        let (mut core, scroll_id) = lay_out_scroll_tree();
        let thumb = core
            .ui_state
            .thumb_rects
            .get(&scroll_id)
            .copied()
            .expect("scrollable should have a thumb");
        let event = core.pointer_down(
            thumb.x + thumb.w * 0.5,
            thumb.y + thumb.h * 0.5,
            PointerButton::Primary,
        );
        assert!(
            event.is_none(),
            "thumb press should not emit PointerDown to the app"
        );
        let drag = core
            .ui_state
            .thumb_drag
            .as_ref()
            .expect("thumb_drag should be set after pointer_down on thumb");
        assert_eq!(drag.scroll_id, scroll_id);
    }

    #[test]
    fn track_click_above_thumb_pages_up_below_pages_down() {
        let (mut core, scroll_id) = lay_out_scroll_tree();
        let track = core
            .ui_state
            .thumb_tracks
            .get(&scroll_id)
            .copied()
            .expect("scrollable should have a track");
        let thumb = core.ui_state.thumb_rects.get(&scroll_id).copied().unwrap();
        let metrics = core.ui_state.scroll_metrics.get(&scroll_id).copied().unwrap();

        // Press in the track below the thumb at offset 0 → page down.
        let evt = core.pointer_down(
            track.x + track.w * 0.5,
            thumb.y + thumb.h + 10.0,
            PointerButton::Primary,
        );
        assert!(evt.is_none(), "track press should not surface PointerDown");
        assert!(
            core.ui_state.thumb_drag.is_none(),
            "track click outside the thumb should not start a drag",
        );
        let after_down = core.ui_state.scroll_offset(&scroll_id);
        let expected_page = (metrics.viewport_h - SCROLL_PAGE_OVERLAP).max(0.0);
        assert!(
            (after_down - expected_page.min(metrics.max_offset)).abs() < 0.5,
            "page-down offset = {after_down} (expected ~{expected_page})",
        );
        // pointer_up after a track-page is a no-op (no drag to clear).
        let _ = core.pointer_up(0.0, 0.0, PointerButton::Primary);

        // Re-layout to refresh the thumb position at the new offset,
        // then click-to-page up.
        let mut tree = lay_out_scroll_tree_only();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        let track = core
            .ui_state
            .thumb_tracks
            .get(&tree.computed_id)
            .copied()
            .unwrap();
        let thumb = core
            .ui_state
            .thumb_rects
            .get(&tree.computed_id)
            .copied()
            .unwrap();

        core.pointer_down(track.x + track.w * 0.5, thumb.y - 4.0, PointerButton::Primary);
        let after_up = core.ui_state.scroll_offset(&tree.computed_id);
        assert!(
            after_up < after_down,
            "page-up should reduce offset: before={after_down} after={after_up}",
        );
    }

    /// Same fixture as `lay_out_scroll_tree` but doesn't build a
    /// fresh `RunnerCore` — useful when tests want to re-layout
    /// against an existing core to refresh thumb rects after a
    /// scroll offset change.
    fn lay_out_scroll_tree_only() -> El {
        use crate::tree::*;
        crate::scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .gap(12.0)
        .height(Size::Fixed(200.0))
    }

    #[test]
    fn thumb_drag_translates_pointer_delta_into_scroll_offset() {
        let (mut core, scroll_id) = lay_out_scroll_tree();
        let thumb = core.ui_state.thumb_rects.get(&scroll_id).copied().unwrap();
        let metrics = core.ui_state.scroll_metrics.get(&scroll_id).copied().unwrap();
        let track_remaining = (metrics.viewport_h - thumb.h).max(0.0);

        let press_y = thumb.y + thumb.h * 0.5;
        core.pointer_down(thumb.x + thumb.w * 0.5, press_y, PointerButton::Primary);
        // Drag 20 px down — offset should advance by `20 * max_offset / track_remaining`.
        let evt = core.pointer_moved(thumb.x + thumb.w * 0.5, press_y + 20.0);
        assert!(evt.is_none(), "thumb-drag move should suppress Drag event");
        let offset = core.ui_state.scroll_offset(&scroll_id);
        let expected = 20.0 * (metrics.max_offset / track_remaining);
        assert!(
            (offset - expected).abs() < 0.5,
            "offset {offset} (expected {expected})",
        );
        // Overshooting clamps to max_offset.
        core.pointer_moved(thumb.x + thumb.w * 0.5, press_y + 9999.0);
        let offset = core.ui_state.scroll_offset(&scroll_id);
        assert!(
            (offset - metrics.max_offset).abs() < 0.5,
            "overshoot offset {offset} (expected {})",
            metrics.max_offset
        );
        // Release clears the drag without emitting events.
        let events = core.pointer_up(thumb.x, press_y, PointerButton::Primary);
        assert!(events.is_empty(), "thumb release shouldn't emit events");
        assert!(core.ui_state.thumb_drag.is_none());
    }

    #[test]
    fn secondary_click_does_not_steal_focus_or_press() {
        let mut core = lay_out_input_tree(false);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        // Focus elsewhere first via primary click on the input.
        let ti_rect = core.rect_of_key("ti").expect("ti rect");
        let tx = ti_rect.x + ti_rect.w * 0.5;
        let ty = ti_rect.y + ti_rect.h * 0.5;
        core.pointer_down(tx, ty, PointerButton::Primary);
        let _ = core.pointer_up(tx, ty, PointerButton::Primary);
        let focused_before = core.ui_state.focused.as_ref().map(|t| t.key.clone());
        // Right-click on the button.
        core.pointer_down(cx, cy, PointerButton::Secondary);
        let events = core.pointer_up(cx, cy, PointerButton::Secondary);
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![UiEventKind::SecondaryClick]);
        let focused_after = core.ui_state.focused.as_ref().map(|t| t.key.clone());
        assert_eq!(
            focused_before, focused_after,
            "right-click must not steal focus"
        );
        assert!(
            core.ui_state.pressed.is_none(),
            "right-click must not set primary press"
        );
    }

    #[test]
    fn text_input_routes_to_focused_only() {
        let mut core = lay_out_input_tree(false);
        // No focus yet → no event.
        assert!(core.text_input("a".into()).is_none());
        // Focus the button via primary click.
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(cx, cy, PointerButton::Primary);
        let _ = core.pointer_up(cx, cy, PointerButton::Primary);
        let event = core.text_input("hi".into()).expect("focused → event");
        assert_eq!(event.kind, UiEventKind::TextInput);
        assert_eq!(event.text.as_deref(), Some("hi"));
        assert_eq!(event.target.as_ref().map(|t| t.key.as_str()), Some("btn"));
        // Empty text → no event (some IME paths emit empty composition).
        assert!(core.text_input(String::new()).is_none());
    }

    #[test]
    fn capture_keys_bypasses_tab_traversal_for_focused_node() {
        // Focus the capture_keys input. Tab should NOT move focus —
        // it should be delivered as a raw KeyDown to the input.
        let mut core = lay_out_input_tree(true);
        let ti_rect = core.rect_of_key("ti").expect("ti rect");
        let tx = ti_rect.x + ti_rect.w * 0.5;
        let ty = ti_rect.y + ti_rect.h * 0.5;
        core.pointer_down(tx, ty, PointerButton::Primary);
        let _ = core.pointer_up(tx, ty, PointerButton::Primary);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("ti"),
            "primary click on capture_keys node still focuses it"
        );

        let event = core
            .key_down(UiKey::Tab, KeyModifiers::default(), false)
            .expect("Tab → KeyDown to focused");
        assert_eq!(event.kind, UiEventKind::KeyDown);
        assert_eq!(event.target.as_ref().map(|t| t.key.as_str()), Some("ti"));
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("ti"),
            "Tab inside capture_keys must NOT move focus"
        );
    }

    #[test]
    fn capture_keys_falls_back_to_default_when_focus_off_capturing_node() {
        // Tree has both a normal-focusable button and a capture_keys
        // input. Focus the button (normal focusable). Tab should then
        // do library-default focus traversal.
        let mut core = lay_out_input_tree(true);
        let btn_rect = core.rect_of_key("btn").expect("btn rect");
        let cx = btn_rect.x + btn_rect.w * 0.5;
        let cy = btn_rect.y + btn_rect.h * 0.5;
        core.pointer_down(cx, cy, PointerButton::Primary);
        let _ = core.pointer_up(cx, cy, PointerButton::Primary);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("btn"),
            "primary click focuses button"
        );
        // Tab should move focus to the next focusable (the input).
        let _ = core.key_down(UiKey::Tab, KeyModifiers::default(), false);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("ti"),
            "Tab from non-capturing focused does library-default traversal"
        );
    }

    /// A column whose three buttons sit inside an `arrow_nav_siblings`
    /// parent (the shape `popover_panel` produces). Layout runs against
    /// a 200x300 viewport with 10px padding; each button is 80px wide
    /// and 36px tall stacked vertically, plenty inside the clip.
    fn lay_out_arrow_nav_tree() -> RunnerCore {
        use crate::tree::*;
        let mut tree = crate::column([
            crate::widgets::button::button("Red").key("opt-red"),
            crate::widgets::button::button("Green").key("opt-green"),
            crate::widgets::button::button("Blue").key("opt-blue"),
        ])
        .arrow_nav_siblings()
        .padding(10.0);
        let mut core = RunnerCore::new();
        crate::layout::layout(
            &mut tree,
            &mut core.ui_state,
            Rect::new(0.0, 0.0, 200.0, 300.0),
        );
        core.ui_state.sync_focus_order(&tree);
        let mut t = PrepareTimings::default();
        core.snapshot(&tree, &mut t);
        // Pre-focus the middle option (the typical state right after a
        // popover opens — we'll exercise transitions from there).
        let target = core
            .ui_state
            .focus_order
            .iter()
            .find(|t| t.key == "opt-green")
            .cloned();
        core.ui_state.set_focus(target);
        core
    }

    #[test]
    fn arrow_nav_moves_focus_among_siblings() {
        let mut core = lay_out_arrow_nav_tree();

        // ArrowDown moves to next sibling, no event emitted (it was
        // consumed by the navigation path).
        let down = core.key_down(UiKey::ArrowDown, KeyModifiers::default(), false);
        assert!(down.is_none(), "arrow-nav consumes the key event");
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-blue"),
        );

        // ArrowUp moves back.
        core.key_down(UiKey::ArrowUp, KeyModifiers::default(), false);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-green"),
        );

        // Home jumps to first.
        core.key_down(UiKey::Home, KeyModifiers::default(), false);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-red"),
        );

        // End jumps to last.
        core.key_down(UiKey::End, KeyModifiers::default(), false);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-blue"),
        );
    }

    #[test]
    fn arrow_nav_saturates_at_ends() {
        let mut core = lay_out_arrow_nav_tree();
        // Walk to the first option and try to go before it.
        core.key_down(UiKey::Home, KeyModifiers::default(), false);
        core.key_down(UiKey::ArrowUp, KeyModifiers::default(), false);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-red"),
            "ArrowUp at top stays at top — no wrap",
        );
        // Same at the bottom.
        core.key_down(UiKey::End, KeyModifiers::default(), false);
        core.key_down(UiKey::ArrowDown, KeyModifiers::default(), false);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("opt-blue"),
            "ArrowDown at bottom stays at bottom — no wrap",
        );
    }

    /// Build a tree shaped like a real app's `build()` output: a
    /// background row with a "Trigger" button, optionally with a
    /// dropdown popover layered on top.
    fn build_popover_tree(open: bool) -> El {
        use crate::widgets::button::button;
        use crate::widgets::overlay::overlay;
        use crate::widgets::popover::{dropdown, menu_item};
        let mut layers: Vec<El> = vec![button("Trigger").key("trigger")];
        if open {
            layers.push(dropdown(
                "menu",
                "trigger",
                [
                    menu_item("A").key("item-a"),
                    menu_item("B").key("item-b"),
                    menu_item("C").key("item-c"),
                ],
            ));
        }
        overlay(layers).padding(20.0)
    }

    /// Run a full per-frame layout pass against `tree` so all the
    /// post-layout hooks (focus order sync, popover focus stack, etc.)
    /// fire just like a real frame.
    fn run_frame(core: &mut RunnerCore, tree: &mut El) {
        let mut t = PrepareTimings::default();
        core.prepare_layout(tree, Rect::new(0.0, 0.0, 400.0, 300.0), 1.0, &mut t);
        core.snapshot(tree, &mut t);
    }

    #[test]
    fn popover_open_pushes_focus_and_auto_focuses_first_item() {
        let mut core = RunnerCore::new();
        let mut closed = build_popover_tree(false);
        run_frame(&mut core, &mut closed);
        // Pre-focus the trigger as if the user tabbed to it before
        // opening the menu.
        let trigger = core
            .ui_state
            .focus_order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
        );

        // Open the popover. The runtime should snapshot the trigger
        // onto the focus stack and auto-focus the first menu item.
        let mut open = build_popover_tree(true);
        run_frame(&mut core, &mut open);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("item-a"),
            "popover open should auto-focus the first menu item",
        );
        assert_eq!(
            core.ui_state.focus_stack.len(),
            1,
            "trigger should be saved on the focus stack",
        );
        assert_eq!(
            core.ui_state.focus_stack[0].key.as_str(),
            "trigger",
            "saved focus should be the pre-open target",
        );
    }

    #[test]
    fn popover_close_restores_focus_to_trigger() {
        let mut core = RunnerCore::new();
        let mut closed = build_popover_tree(false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus_order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        // Open → focus walks to the menu.
        let mut open = build_popover_tree(true);
        run_frame(&mut core, &mut open);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("item-a"),
        );

        // Close → focus restored to trigger, stack drained.
        let mut closed_again = build_popover_tree(false);
        run_frame(&mut core, &mut closed_again);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
            "closing the popover should pop the saved focus",
        );
        assert!(
            core.ui_state.focus_stack.is_empty(),
            "focus stack should be drained after restore",
        );
    }

    #[test]
    fn popover_close_does_not_override_intentional_focus_move() {
        let mut core = RunnerCore::new();
        // Tree with a second focusable button outside the popover so
        // the user can "click somewhere else" while the menu is open.
        let build = |open: bool| -> El {
            use crate::widgets::button::button;
            use crate::widgets::overlay::overlay;
            use crate::widgets::popover::{dropdown, menu_item};
            let main = crate::row([
                button("Trigger").key("trigger"),
                button("Other").key("other"),
            ]);
            let mut layers: Vec<El> = vec![main];
            if open {
                layers.push(dropdown("menu", "trigger", [menu_item("A").key("item-a")]));
            }
            overlay(layers).padding(20.0)
        };

        let mut closed = build(false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus_order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        let mut open = build(true);
        run_frame(&mut core, &mut open);
        assert_eq!(core.ui_state.focus_stack.len(), 1);

        // Simulate an intentional focus move to a sibling that is
        // outside the popover (e.g. the user re-tabbed somewhere). Do
        // this by setting focus directly while the popover is still in
        // the tree — the existing focus-order contains "other".
        let other = core
            .ui_state
            .focus_order
            .iter()
            .find(|t| t.key == "other")
            .cloned();
        core.ui_state.set_focus(other);

        let mut closed_again = build(false);
        run_frame(&mut core, &mut closed_again);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("other"),
            "focus moved before close should not be overridden by restore",
        );
        assert!(core.ui_state.focus_stack.is_empty());
    }

    #[test]
    fn nested_popovers_stack_and_unwind_focus_correctly() {
        let mut core = RunnerCore::new();
        // Two siblings layered at El root: an outer popover anchored to
        // the trigger, and an inner popover anchored to a button inside
        // the outer panel. Both are real popovers — separate
        // popover_layer ids — so the runtime sees them stack.
        let build = |outer: bool, inner: bool| -> El {
            use crate::widgets::button::button;
            use crate::widgets::overlay::overlay;
            use crate::widgets::popover::{Anchor, popover, popover_panel};
            let main = button("Trigger").key("trigger");
            let mut layers: Vec<El> = vec![main];
            if outer {
                layers.push(popover(
                    "outer",
                    Anchor::below_key("trigger"),
                    popover_panel([button("Open inner").key("inner-trigger")]),
                ));
            }
            if inner {
                layers.push(popover(
                    "inner",
                    Anchor::below_key("inner-trigger"),
                    popover_panel([button("X").key("inner-a"), button("Y").key("inner-b")]),
                ));
            }
            overlay(layers).padding(20.0)
        };

        // Frame 1: nothing open, focus on the trigger.
        let mut closed = build(false, false);
        run_frame(&mut core, &mut closed);
        let trigger = core
            .ui_state
            .focus_order
            .iter()
            .find(|t| t.key == "trigger")
            .cloned();
        core.ui_state.set_focus(trigger);

        // Frame 2: outer opens. Save trigger, focus inner-trigger.
        let mut outer = build(true, false);
        run_frame(&mut core, &mut outer);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("inner-trigger"),
        );
        assert_eq!(core.ui_state.focus_stack.len(), 1);

        // Frame 3: inner also opens. Save inner-trigger, focus inner-a.
        let mut both = build(true, true);
        run_frame(&mut core, &mut both);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("inner-a"),
        );
        assert_eq!(core.ui_state.focus_stack.len(), 2);

        // Frame 4: inner closes. Pop → restore inner-trigger.
        let mut outer_only = build(true, false);
        run_frame(&mut core, &mut outer_only);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("inner-trigger"),
        );
        assert_eq!(core.ui_state.focus_stack.len(), 1);

        // Frame 5: outer closes. Pop → restore trigger.
        let mut none = build(false, false);
        run_frame(&mut core, &mut none);
        assert_eq!(
            core.ui_state.focused.as_ref().map(|t| t.key.as_str()),
            Some("trigger"),
        );
        assert!(core.ui_state.focus_stack.is_empty());
    }

    #[test]
    fn arrow_nav_does_not_intercept_outside_navigable_groups() {
        // Reuse the input tree (no arrow_nav_siblings parent). Arrow
        // keys must produce a regular `KeyDown` event so a
        // capture_keys widget can interpret them as caret motion.
        let mut core = lay_out_input_tree(false);
        let target = core
            .ui_state
            .focus_order
            .iter()
            .find(|t| t.key == "btn")
            .cloned();
        core.ui_state.set_focus(target);
        let event = core
            .key_down(UiKey::ArrowDown, KeyModifiers::default(), false)
            .expect("ArrowDown without navigable parent → event");
        assert_eq!(event.kind, UiEventKind::KeyDown);
    }

    fn quad(shader: ShaderHandle) -> DrawOp {
        DrawOp::Quad {
            id: "q".into(),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            scissor: None,
            shader,
            uniforms: UniformBlock::new(),
        }
    }

    #[test]
    fn samples_backdrop_inserts_snapshot_before_first_glass_quad() {
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        let ops = vec![
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Custom("liquid_glass")),
            quad(ShaderHandle::Custom("liquid_glass")),
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
        ];
        let mut timings = PrepareTimings::default();
        core.prepare_paint(
            &ops,
            |_| true,
            |s| matches!(s, ShaderHandle::Custom(name) if *name == "liquid_glass"),
            &mut NoText,
            1.0,
            &mut timings,
        );

        let kinds: Vec<&'static str> = core
            .paint_items
            .iter()
            .map(|p| match p {
                PaintItem::QuadRun(_) => "Q",
                PaintItem::IconRun(_) => "I",
                PaintItem::Text(_) => "T",
                PaintItem::BackdropSnapshot => "S",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["Q", "S", "Q", "Q"],
            "expected one stock run, snapshot, then a glass run, then a foreground stock run"
        );
    }

    #[test]
    fn no_snapshot_when_no_glass_drawn() {
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        let ops = vec![
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
        ];
        let mut timings = PrepareTimings::default();
        core.prepare_paint(&ops, |_| true, |_| false, &mut NoText, 1.0, &mut timings);
        assert!(
            !core
                .paint_items
                .iter()
                .any(|p| matches!(p, PaintItem::BackdropSnapshot)),
            "no glass shader registered → no snapshot"
        );
    }

    #[test]
    fn at_most_one_snapshot_per_frame() {
        let mut core = RunnerCore::new();
        core.set_surface_size(100, 100);
        let ops = vec![
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Custom("g")),
            quad(ShaderHandle::Stock(StockShader::RoundedRect)),
            quad(ShaderHandle::Custom("g")),
        ];
        let mut timings = PrepareTimings::default();
        core.prepare_paint(
            &ops,
            |_| true,
            |s| matches!(s, ShaderHandle::Custom("g")),
            &mut NoText,
            1.0,
            &mut timings,
        );
        let snapshots = core
            .paint_items
            .iter()
            .filter(|p| matches!(p, PaintItem::BackdropSnapshot))
            .count();
        assert_eq!(snapshots, 1, "backdrop depth is capped at 1");
    }
}
