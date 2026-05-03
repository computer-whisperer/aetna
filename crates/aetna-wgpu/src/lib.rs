//! Aetna — wgpu backend.
//!
//! v5.0 scope: paints `stock::rounded_rect` quads + `stock::text_sdf`
//! glyph runs + `stock::focus_ring` outlines, plus user-registered
//! custom shaders that share the rounded_rect vertex layout. This is the
//! verbatim port of v0.4's `wgpu_render.rs`, refactored into the
//! `aetna-wgpu` crate with the file split called out in `V5.md`.
//!
//! The single public entry point today is [`Runner`], which owns:
//! - GPU resources (pipelines, buffers, glyph atlas)
//! - [`UiState`](aetna_core::state::UiState) (hover/press/focus/scroll
//!   trackers, hotkey registry, animation state)
//! - The last laid-out tree (so events arriving between frames hit-test
//!   against current geometry)
//!
//! A later commit splits `Runner` into a paint half (`WgpuPainter`) and
//! a state half (still called `Runner`) that composes a painter — the
//! split mirrors the eventual `aetna-vulkano` shape.
//!
//! # Insert-into-pass integration
//!
//! The runner does not own the device, queue, swapchain, or render
//! pass. The host creates all of those, configures the surface, begins
//! the encoder + pass, and calls [`Runner::draw`] to record draws into
//! the pass. The host then ends the pass, submits, and presents.
//!
//! ```ignore
//! let mut ui = Runner::new(&device, &queue, surface_format);
//! ui.register_shader(&device, "gradient", include_str!("gradient.wgsl"));
//! // per frame:
//! ui.prepare(&device, &queue, &mut tree, viewport, scale_factor);
//! ui.draw(&mut pass);
//! ```
//!
//! `prepare` is split from `draw` so all `queue.write_buffer` calls and
//! glyphon atlas updates happen before the render pass begins, matching
//! wgpu's expected order.
//!
//! # Custom shaders
//!
//! Call [`Runner::register_shader`] with a name and WGSL source. The
//! shader's vertex/fragment must use the shared instance layout — see
//! `shaders/rounded_rect.wgsl` (in aetna-core) for the canonical
//! example. Bind the shader at a node via
//! `El::shader(ShaderBinding::custom(name).with(...))`. Per-instance
//! uniforms map to three generic `vec4` slots:
//!
//! | Uniform key | Slot (`@location`) | Accepted types |
//! |---|---|---|
//! | `vec_a` | 2 | `Color` (rgba 0..1) or `Vec4` |
//! | `vec_b` | 3 | `Color` or `Vec4` |
//! | `vec_c` | 4 | `Vec4` (or fall back to scalar `f32` packed in `.x`) |
//!
//! Stock `rounded_rect` reuses the same layout but reads its own named
//! uniforms (`fill`, `stroke`, `stroke_width`, `radius`, `shadow`).

mod instance;
mod pipeline;
mod text;

use std::collections::HashMap;
use std::time::Instant;

use glyphon::{
    Cache, FontSystem, Resolution, SwashCache, TextArea, TextAtlas, TextRenderer, Viewport,
};
use wgpu::util::DeviceExt;

use aetna_core::draw_ops;
use aetna_core::event::{KeyChord, KeyModifiers, UiEvent, UiEventKind, UiKey};
use aetna_core::hit_test;
use aetna_core::ir::DrawOp;
use aetna_core::layout;
use aetna_core::shader::{ShaderHandle, StockShader, stock_wgsl};
use aetna_core::state::{AnimationMode, UiState};
use aetna_core::tree::{El, Rect};

use crate::instance::{
    InstanceRun, PaintItem, PhysicalScissor, QuadInstance, close_run, pack_instance,
    physical_scissor, set_scissor,
};
use crate::pipeline::{FrameUniforms, build_quad_pipeline};
use crate::text::{TextLayer, build_text_buffer};

/// Initial size for the dynamic instance buffer (grows as needed).
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// Reported back from [`Runner::prepare`] each frame. The host uses
/// `needs_redraw` to keep the redraw loop ticking only while there is
/// in-flight motion (a hover spring still settling, a focus ring still
/// fading out), then idles. This lets animation drive frames without a
/// continuous tick when nothing is changing.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareResult {
    pub needs_redraw: bool,
}

/// Wgpu runtime owned by the host. One instance per surface/format.
///
/// In v5.0 this is a single struct holding both GPU resources and
/// interaction state. A follow-up commit splits the GPU half into a
/// `WgpuPainter` value held inside the runner — same fields, cleaner
/// boundary, and the same shape we'll want when `aetna-vulkano` lands.
pub struct Runner {
    target_format: wgpu::TextureFormat,

    // Shared resources.
    pipeline_layout: wgpu::PipelineLayout,
    quad_bind_group: wgpu::BindGroup,
    frame_buf: wgpu::Buffer,
    quad_vbo: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    quad_scratch: Vec<QuadInstance>,
    runs: Vec<InstanceRun>,

    // One pipeline per registered shader (stock + custom).
    pipelines: HashMap<ShaderHandle, wgpu::RenderPipeline>,

    // Text pipeline resources (stock::text_sdf, via glyphon).
    font_system: FontSystem,
    swash_cache: SwashCache,
    glyph_atlas: TextAtlas,
    glyph_viewport: Viewport,
    text_layers: Vec<TextLayer>,

    // Replayed by draw() in exact paint order.
    paint_items: Vec<PaintItem>,
    viewport_px: (u32, u32),

    // Interaction state (v0.2).
    ui_state: UiState,
    /// Last laid-out tree, kept so events arriving between frames can
    /// hit-test against the geometry the user is actually looking at.
    /// Stored by clone (cheap at fixture sizes; revisit when trees grow).
    last_tree: Option<El>,
}

impl Runner {
    /// Create a runner for the given target color format. The host
    /// passes its swapchain/render-target format here so pipelines and
    /// the glyph atlas are built compatible.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        // ---- Shared resources ----
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::frame_uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aetna_wgpu::bind_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let quad_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aetna_wgpu::bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buf.as_entire_binding(),
            }],
        });

        let quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aetna_wgpu::quad_vbo"),
            // Triangle strip: 4 corners, uv 0..1.
            contents: bytemuck::cast_slice::<f32, u8>(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<QuadInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aetna_wgpu::pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Build stock rect-shaped pipelines up-front; custom shaders are
        // added on demand by the host.
        let mut pipelines = HashMap::new();
        let rr_pipeline = build_quad_pipeline(
            device,
            &pipeline_layout,
            target_format,
            "stock::rounded_rect",
            stock_wgsl::ROUNDED_RECT,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::RoundedRect), rr_pipeline);
        let focus_pipeline = build_quad_pipeline(
            device,
            &pipeline_layout,
            target_format,
            "stock::focus_ring",
            stock_wgsl::ROUNDED_RECT,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::FocusRing), focus_pipeline);

        // ---- Text pipeline (glyphon) ----
        let mut font_system = FontSystem::new();
        // Bundle Roboto with the crate so typography is consistent
        // across machines (no fontconfig surprises). FontSystem still
        // sees system fonts as a fallback, but our explicit Family::Name
        // request below picks the bundled face.
        let db = font_system.db_mut();
        db.load_font_data(include_bytes!("../../aetna-core/fonts/Roboto-Regular.ttf").to_vec());
        db.load_font_data(include_bytes!("../../aetna-core/fonts/Roboto-Medium.ttf").to_vec());
        db.load_font_data(include_bytes!("../../aetna-core/fonts/Roboto-Bold.ttf").to_vec());
        let swash_cache = SwashCache::new();
        let glyph_cache = Cache::new(device);
        let glyph_viewport = Viewport::new(device, &glyph_cache);
        let glyph_atlas = TextAtlas::new(device, queue, &glyph_cache, target_format);

        Self {
            target_format,
            pipeline_layout,
            quad_bind_group,
            frame_buf,
            quad_vbo,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            quad_scratch: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            runs: Vec::new(),
            pipelines,

            font_system,
            swash_cache,
            glyph_atlas,
            glyph_viewport,
            text_layers: Vec::new(),

            paint_items: Vec::new(),
            viewport_px: (1, 1),

            ui_state: UiState::new(),
            last_tree: None,
        }
    }

    /// Register a custom shader. `name` is the same string passed to
    /// `aetna_core::shader::ShaderBinding::custom`; nodes bound to it
    /// via [`El::shader`](aetna_core::tree::El) paint through this
    /// pipeline.
    ///
    /// The WGSL source must use the shared `(rect, vec_a, vec_b, vec_c)`
    /// instance layout and the `FrameUniforms` bind group described in
    /// the module docs. Compilation happens at register time — invalid
    /// WGSL panics here, not mid-frame.
    ///
    /// Re-registering the same name replaces the previous pipeline
    /// (useful for hot-reload during development).
    pub fn register_shader(&mut self, device: &wgpu::Device, name: &'static str, wgsl: &str) {
        let label = format!("custom::{name}");
        let pipeline = build_quad_pipeline(
            device,
            &self.pipeline_layout,
            self.target_format,
            &label,
            wgsl,
        );
        self.pipelines.insert(ShaderHandle::Custom(name), pipeline);
    }

    /// Borrow the internal [`UiState`] — primarily for headless fixtures
    /// that want to look up a node's rect after `prepare` (e.g., to
    /// simulate a pointer at a specific button's center).
    pub fn ui_state(&self) -> &UiState {
        &self.ui_state
    }

    /// Return the most recently laid-out rectangle for a keyed node.
    ///
    /// Call after [`Self::prepare`]. This is the host-composition hook:
    /// reserve a keyed Aetna element in the UI tree, ask for its rect
    /// here, then record host-owned rendering into that region using the
    /// same encoder / render flow that surrounds Aetna's pass.
    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.last_tree
            .as_ref()
            .and_then(|tree| self.ui_state.rect_of_key(tree, key))
    }

    /// Lay out the tree, resolve to draw ops, and upload per-frame
    /// buffers (quad instances + glyph atlas). Must be called before
    /// [`Self::draw`] and outside of any render pass.
    ///
    /// `viewport` is in **logical** pixels — the units the layout pass
    /// works in. `scale_factor` is the HiDPI multiplier (1.0 on a
    /// regular display, 2.0 on most modern HiDPI, can be fractional).
    /// The host's render-pass target should be sized at physical pixels
    /// (`viewport × scale_factor`); the runner maps logical → physical
    /// internally so layout, fonts, and SDF math stay device-independent.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        root: &mut El,
        viewport: Rect,
        scale_factor: f32,
    ) -> PrepareResult {
        // Layout writes computed_id on each El + writes the rect map +
        // reads/clamps/writes scroll offsets, all on UiState's side maps.
        layout::layout(root, &mut self.ui_state, viewport);
        self.ui_state.sync_focus_order(root);
        // Apply UI-state visual deltas after layout, so focus targets can
        // survive rebuilds by node id and update their current rect.
        self.ui_state.apply_to_state();
        // Tick visual animations: retarget springs to the values implied
        // by current state, sample at `now`, write eased values into the
        // envelope side map (state envelopes) and the El's app-driven
        // fields (fill/translate/etc.). Anything in flight forces another
        // redraw next frame.
        let needs_redraw = self
            .ui_state
            .tick_visual_animations(root, Instant::now());
        let ops = draw_ops::draw_ops(root, &self.ui_state);

        self.viewport_px = (
            (viewport.w * scale_factor).ceil().max(1.0) as u32,
            (viewport.h * scale_factor).ceil().max(1.0) as u32,
        );
        self.glyph_viewport.update(
            queue,
            Resolution {
                width: self.viewport_px.0,
                height: self.viewport_px.1,
            },
        );

        // ---- Paint stream: pack quads, prepare text, preserve order ----
        self.quad_scratch.clear();
        self.runs.clear();
        self.text_layers.clear();
        self.paint_items.clear();
        let mut current: Option<(ShaderHandle, Option<PhysicalScissor>)> = None;
        let mut run_first: u32 = 0;

        for op in &ops {
            match op {
                DrawOp::Quad {
                    rect,
                    scissor,
                    shader,
                    uniforms,
                    ..
                } => {
                    // Skip ops whose shader has no pipeline registered
                    // (e.g. custom shaders the host forgot to register).
                    // The lint pass surfaces this elsewhere.
                    if !self.pipelines.contains_key(shader) {
                        continue;
                    }
                    let physical_scissor =
                        physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(physical_scissor, Some(s) if s.w == 0 || s.h == 0) {
                        continue;
                    }
                    let inst = pack_instance(*rect, *shader, uniforms);

                    let run_key = (*shader, physical_scissor);
                    if current != Some(run_key) {
                        close_run(
                            &mut self.runs,
                            &mut self.paint_items,
                            current,
                            run_first,
                            self.quad_scratch.len() as u32,
                        );
                        current = Some(run_key);
                        run_first = self.quad_scratch.len() as u32;
                    }
                    self.quad_scratch.push(inst);
                }
                DrawOp::GlyphRun {
                    rect,
                    scissor,
                    color,
                    text,
                    size,
                    weight,
                    mono,
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

                    let physical_scissor =
                        physical_scissor(*scissor, scale_factor, self.viewport_px);
                    if matches!(physical_scissor, Some(s) if s.w == 0 || s.h == 0) {
                        continue;
                    }
                    let (buffer, meta) = build_text_buffer(
                        &mut self.font_system,
                        *rect,
                        *scissor,
                        text,
                        *size,
                        *weight,
                        *mono,
                        *wrap,
                        *anchor,
                        *color,
                        scale_factor,
                    );
                    let mut renderer = TextRenderer::new(
                        &mut self.glyph_atlas,
                        device,
                        wgpu::MultisampleState::default(),
                        None,
                    );
                    let text_area = TextArea {
                        buffer: &buffer,
                        left: meta.left,
                        top: meta.top,
                        // Positions/sizes are pre-multiplied to physical
                        // pixels already; tell glyphon not to scale further.
                        scale: 1.0,
                        bounds: meta.bounds,
                        default_color: meta.color,
                        custom_glyphs: &[],
                    };
                    renderer
                        .prepare(
                            device,
                            queue,
                            &mut self.font_system,
                            &mut self.glyph_atlas,
                            &self.glyph_viewport,
                            [text_area],
                            &mut self.swash_cache,
                        )
                        .expect("glyphon prepare");
                    let index = self.text_layers.len();
                    self.text_layers.push(TextLayer {
                        renderer,
                        scissor: physical_scissor,
                    });
                    self.paint_items.push(PaintItem::Text(index));
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

        if self.quad_scratch.len() > self.instance_capacity {
            let new_cap = self.quad_scratch.len().next_power_of_two();
            self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aetna_wgpu::instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<QuadInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }

        if !self.quad_scratch.is_empty() {
            queue.write_buffer(
                &self.instance_buf,
                0,
                bytemuck::cast_slice(&self.quad_scratch),
            );
        }

        let frame = FrameUniforms {
            viewport: [viewport.w, viewport.h],
            _pad: [0.0, 0.0],
        };
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&frame));

        // Snapshot the laid-out tree so pointer events arriving before
        // the next prepare can hit-test against current geometry.
        self.last_tree = Some(root.clone());

        PrepareResult { needs_redraw }
    }

    // ---- v0.2 input plumbing ----
    //
    // The host (winit-side) calls these from its event loop.
    // Coordinates are **logical pixels** — divide winit's physical
    // PhysicalPosition by the window scale factor before handing them in.

    /// Update pointer position and recompute the hovered key.
    /// Returns the new hovered key, if any (host can use it for cursor
    /// styling or to decide whether to call `request_redraw`).
    pub fn pointer_moved(&mut self, x: f32, y: f32) -> Option<&str> {
        self.ui_state.pointer_pos = Some((x, y));
        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        self.ui_state.hovered = hit;
        self.ui_state
            .hovered
            .as_ref()
            .map(|target| target.key.as_str())
    }

    /// Pointer left the window — clear hover/press.
    pub fn pointer_left(&mut self) {
        self.ui_state.pointer_pos = None;
        self.ui_state.hovered = None;
        self.ui_state.pressed = None;
    }

    /// Primary mouse button down at `(x, y)` (logical px). Records the
    /// pressed key for press-visual feedback; the actual click event
    /// fires on the matching `pointer_up`.
    pub fn pointer_down(&mut self, x: f32, y: f32) {
        let hit = self
            .last_tree
            .as_ref()
            .and_then(|t| hit_test::hit_test_target(t, &self.ui_state, (x, y)));
        self.ui_state.set_focus(hit.clone());
        self.ui_state.pressed = hit;
    }

    /// Primary mouse button up at `(x, y)`. If the release lands on the
    /// same keyed node as the corresponding `pointer_down`, a `Click`
    /// event is returned for the host to dispatch via `App::on_event`.
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

    /// Replace the hotkey registry. Call once per frame, after `app.build()`,
    /// passing `app.hotkeys()` so chords stay in sync with state.
    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.ui_state.set_hotkeys(hotkeys);
    }

    /// Switch animation pacing. Default is [`AnimationMode::Live`].
    /// Headless render binaries should call this with
    /// [`AnimationMode::Settled`] so a single-frame snapshot reflects
    /// the post-animation visual without depending on integrator timing.
    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.ui_state.set_animation_mode(mode);
    }

    /// Apply a wheel delta in **logical** pixels at `(x, y)`. Routes to
    /// the deepest scrollable container under the cursor in the last
    /// laid-out tree. Returns `true` if the event landed on a scrollable
    /// (host should `request_redraw` so the next frame applies the new
    /// offset).
    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        let Some(tree) = self.last_tree.as_ref() else {
            return false;
        };
        self.ui_state.pointer_wheel(tree, (x, y), dy)
    }

    /// Record draws into the host-managed render pass. Call after
    /// [`Self::prepare`]. Paint order follows the draw-op stream.
    pub fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        let full = PhysicalScissor {
            x: 0,
            y: 0,
            w: self.viewport_px.0,
            h: self.viewport_px.1,
        };
        for item in &self.paint_items {
            match *item {
                PaintItem::QuadRun(index) => {
                    let run = &self.runs[index];
                    set_scissor(pass, run.scissor, full);
                    pass.set_bind_group(0, &self.quad_bind_group, &[]);
                    pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
                    pass.set_vertex_buffer(1, self.instance_buf.slice(..));
                    let pipeline = self
                        .pipelines
                        .get(&run.handle)
                        .expect("run handle has no pipeline (bug in prepare)");
                    pass.set_pipeline(pipeline);
                    pass.draw(0..4, run.first..run.first + run.count);
                }
                PaintItem::Text(index) => {
                    let layer = &self.text_layers[index];
                    set_scissor(pass, layer.scissor, full);
                    layer
                        .renderer
                        .render(&self.glyph_atlas, &self.glyph_viewport, pass)
                        .expect("glyphon render");
                }
            }
        }
    }
}
