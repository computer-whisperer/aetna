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
//! per-backend. This is v5.4 step 2 of option B — share what's
//! identical, no trait. Same shape as v5.4 step 1 (`crate::paint`),
//! larger surface.
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
//! A v5.4 review considered extracting a `trait Painter { fn
//! prepare(...); fn draw(...); fn set_scissor(...); }` so backends
//! would share *one* abstraction surface. We declined: the only call
//! sites left after this module + [`crate::paint`] are the two
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
use crate::event::{KeyChord, KeyModifiers, PointerButton, UiEvent, UiEventKind, UiKey};
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
use crate::tree::{Color, El, FontWeight, Rect, TextWrap};

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
        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        self.ui_state.hovered = hit;
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
        self.ui_state.hovered = None;
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
        self.ui_state.key_down(key, modifiers, repeat)
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
        layout::layout(root, &mut self.ui_state, viewport);
        self.ui_state.sync_focus_order(root);
        self.ui_state.apply_to_state();
        let needs_redraw = self.ui_state.tick_visual_animations(root, Instant::now());
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
        // v0.7: at most one snapshot per frame. Auto-inserted before
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
                    name,
                    color,
                    size,
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
                        name.fallback_glyph(),
                        *size,
                        FontWeight::Regular,
                        TextWrap::NoWrap,
                        TextAnchor::Middle,
                        scale_factor,
                    );
                    for index in layers {
                        self.paint_items.push(PaintItem::Text(index));
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
                    // v0.7 caps at one snapshot per frame; an explicit
                    // op only lands if the auto-emitter hasn't fired yet.
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
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind.clone()).collect();
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
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind.clone()).collect();
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
        let kinds: Vec<UiEventKind> = events.iter().map(|e| e.kind.clone()).collect();
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
        assert_eq!(snapshots, 1, "v0.7 caps backdrop depth at 1");
    }
}
