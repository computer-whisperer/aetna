//! `aetna-vulkano::Runner` — peer to `aetna_wgpu::Runner`.
//!
//! v5.3 step 4 brings up the GPU-agnostic half: input plumbing, focus
//! and hover tracking, the layout/animation passes, and the public
//! method surface the host calls into. No Vulkan resources are owned
//! yet — `Runner::new` stashes the device / queue / format so step 5
//! can wire up pipelines without changing the constructor signature,
//! `register_shader` compiles WGSL → SPIR-V and caches the words for
//! the same reason, and `prepare()` runs through layout + animation +
//! the snapshot of the laid-out tree (so pointer hit-testing works
//! against real geometry the moment step 5 starts drawing) but stops
//! short of producing GPU buffers. `draw()` is a no-op stub.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use aetna_core::{
    AnimationMode, El, KeyChord, KeyModifiers, Rect, UiEvent, UiEventKind, UiKey, UiState,
    hit_test, layout,
};
use vulkano::{
    command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer},
    device::{Device, Queue},
    format::Format,
};

use crate::naga_compile::wgsl_to_spirv;

/// Mirrors `aetna_wgpu::PrepareResult`. Duplicated rather than shared
/// in v5.3; v5.4 evaluates whether to lift these into `aetna-core` once
/// both backends are mature.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareResult {
    pub needs_redraw: bool,
    pub timings: PrepareTimings,
}

/// Per-stage CPU timing inside [`Runner::prepare`]. Same shape as
/// `aetna_wgpu::PrepareTimings` so the wasm-style frame log can switch
/// between backends without changing format.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareTimings {
    pub layout: std::time::Duration,
    pub draw_ops: std::time::Duration,
    pub paint: std::time::Duration,
    pub gpu_upload: std::time::Duration,
    pub snapshot: std::time::Duration,
}

/// Vulkan runtime owned by the host. One instance per surface/format.
pub struct Runner {
    // GPU handles — stashed for step 5 (pipelines + buffers + atlas
    // mirror) and step 7 (custom-shader pipeline build). Unused at
    // step 4 except as constructor sinks; they're underscored where the
    // compiler would otherwise warn.
    _device: Arc<Device>,
    _queue: Arc<Queue>,
    _target_format: Format,

    viewport_px: (u32, u32),
    surface_size_override: Option<(u32, u32)>,

    /// SPIR-V words cached per registered shader name. Compiled at
    /// `register_shader` time so bad WGSL surfaces close to the call
    /// site, not deferred to a draw-time pipeline build.
    /// Step 5 consumes the entries to build vulkano graphics pipelines.
    registered_shaders: HashMap<&'static str, Vec<u32>>,

    ui_state: UiState,
    /// Last laid-out tree, kept so pointer events arriving between
    /// frames can hit-test against the geometry the user is looking at.
    last_tree: Option<El>,
}

impl Runner {
    /// Create a runner for the given target color format. The host
    /// passes its swapchain format here; pipelines and the atlas
    /// mirror (step 5+) will be built compatible.
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, target_format: Format) -> Self {
        Self {
            _device: device,
            _queue: queue,
            _target_format: target_format,
            viewport_px: (1, 1),
            surface_size_override: None,
            registered_shaders: HashMap::new(),
            ui_state: UiState::new(),
            last_tree: None,
        }
    }

    /// Tell the runner the swapchain image size in physical pixels.
    /// Call once after swapchain create, again on every resize.
    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.surface_size_override = Some((width.max(1), height.max(1)));
    }

    /// Register a custom shader. WGSL is transpiled to SPIR-V via naga
    /// at register time; bad WGSL panics here, not mid-frame. Step 5
    /// builds the actual graphics pipeline lazily from these words.
    ///
    /// Re-registering the same name replaces the previous SPIR-V.
    pub fn register_shader(&mut self, name: &'static str, wgsl: &str) {
        let spirv = wgsl_to_spirv(name, wgsl)
            .unwrap_or_else(|e| panic!("aetna-vulkano: WGSL compile failed for `{name}`: {e}"));
        self.registered_shaders.insert(name, spirv);
    }

    /// Borrow the internal [`UiState`].
    pub fn ui_state(&self) -> &UiState {
        &self.ui_state
    }

    /// One-line diagnostic snapshot of interactive state.
    pub fn debug_summary(&self) -> String {
        self.ui_state.debug_summary()
    }

    /// Most recently laid-out rectangle for a keyed node. Call after
    /// [`Self::prepare`].
    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.last_tree
            .as_ref()
            .and_then(|tree| self.ui_state.rect_of_key(tree, key))
    }

    /// Lay out the tree, run animation tick, and snapshot for next-frame
    /// hit-testing. Returns whether another redraw is needed (springs in
    /// flight, etc.).
    ///
    /// Step 4 stops here — no GPU buffer uploads, no draw-op stream.
    /// Step 5 grows this into the full prepare flow.
    pub fn prepare(&mut self, root: &mut El, viewport: Rect, scale_factor: f32) -> PrepareResult {
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

        // Snapshot for next-frame hit-testing. Step 5 will also drive
        // the GPU paint stream from here.
        self.last_tree = Some(root.clone());

        PrepareResult {
            needs_redraw,
            timings: PrepareTimings::default(),
        }
    }

    /// Update pointer position and recompute the hovered key.
    pub fn pointer_moved(&mut self, x: f32, y: f32) -> Option<&str> {
        self.ui_state.pointer_pos = Some((x, y));
        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        self.ui_state.hovered = hit;
        self.ui_state.hovered.as_ref().map(|t| t.key.as_str())
    }

    /// Pointer left the window — clear hover/press.
    pub fn pointer_left(&mut self) {
        self.ui_state.pointer_pos = None;
        self.ui_state.hovered = None;
        self.ui_state.pressed = None;
    }

    /// Primary mouse button down at `(x, y)` (logical px).
    pub fn pointer_down(&mut self, x: f32, y: f32) {
        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        self.ui_state.set_focus(hit.clone());
        self.ui_state.pressed = hit;
    }

    /// Primary mouse button up. Returns a `Click` event if the release
    /// landed on the same keyed node as the corresponding down.
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

    /// Replace the hotkey registry. Call once per frame, after
    /// `app.build()`, passing `app.hotkeys()`.
    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.ui_state.set_hotkeys(hotkeys);
    }

    /// Switch animation pacing.
    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.ui_state.set_animation_mode(mode);
    }

    /// Apply a wheel delta in **logical** pixels. Routes to the deepest
    /// scrollable container under the cursor.
    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        self.ui_state.pointer_wheel(tree, (x, y), dy)
    }

    /// Record draws into the host-managed primary command-buffer
    /// builder. Call after [`Self::prepare`], inside the host's
    /// render-pass scope.
    ///
    /// Step 4 stub — step 5 wires this up to walk the paint stream.
    pub fn draw(&self, _builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        // No-op until step 5.
        let _ = self.viewport_px;
        let _ = &self.registered_shaders;
    }
}
