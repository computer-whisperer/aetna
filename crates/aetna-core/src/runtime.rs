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
use crate::event::{KeyChord, KeyModifiers, UiEvent, UiEventKind, UiKey};
use crate::hit_test;
use crate::ir::{DrawOp, TextAnchor};
use crate::layout;
use crate::paint::{
    InstanceRun, PaintItem, PhysicalScissor, QuadInstance, close_run, pack_instance,
    physical_scissor,
};
use crate::shader::ShaderHandle;
use crate::state::{AnimationMode, UiState};
use crate::text_atlas::RunStyle;
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
        }
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

    pub fn pointer_moved(&mut self, x: f32, y: f32) -> Option<&str> {
        self.ui_state.pointer_pos = Some((x, y));
        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        self.ui_state.hovered = hit;
        self.ui_state.hovered.as_ref().map(|t| t.key.as_str())
    }

    pub fn pointer_left(&mut self) {
        self.ui_state.pointer_pos = None;
        self.ui_state.hovered = None;
        self.ui_state.pressed = None;
    }

    pub fn pointer_down(&mut self, x: f32, y: f32) {
        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        self.ui_state.set_focus(hit.clone());
        self.ui_state.pressed = hit;
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) -> Option<UiEvent> {
        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        let pressed = self.ui_state.pressed.take();
        match (pressed, hit) {
            (Some(p), Some(h)) if p.node_id == h.node_id => Some(UiEvent {
                key: Some(p.key.clone()),
                target: Some(p),
                pointer: Some((x, y)),
                key_press: None,
                kind: UiEventKind::Click,
            }),
            _ => None,
        }
    }

    pub fn key_down(
        &mut self,
        key: UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        self.ui_state.key_down(key, modifiers, repeat)
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
        let ops = draw_ops::draw_ops(root, &self.ui_state);
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
    pub fn prepare_paint<F: Fn(&ShaderHandle) -> bool>(
        &mut self,
        ops: &[DrawOp],
        is_registered: F,
        text: &mut dyn TextRecorder,
        scale_factor: f32,
        timings: &mut PrepareTimings,
    ) {
        let t0 = Instant::now();
        self.quad_scratch.clear();
        self.runs.clear();
        self.paint_items.clear();

        let mut current: Option<(ShaderHandle, Option<PhysicalScissor>)> = None;
        let mut run_first: u32 = 0;

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
