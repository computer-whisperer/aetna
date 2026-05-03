//! Wgpu renderer — the production paint path.
//!
//! v0.1 scope: paints `stock::rounded_rect` quads + `stock::text_sdf`
//! glyph runs, plus user-registered custom shaders that share the
//! rounded_rect vertex layout. Focus rings reuse the rounded-rect
//! pipeline as an outline-only instance; shadow is still reserved until
//! the SDF quad can widen beyond the element rect.
//!
//! # Insert-into-pass integration
//!
//! The renderer does not own the device, queue, swapchain, or render
//! pass. The host creates all of those, configures the surface, begins
//! the encoder + pass, and calls [`UiRenderer::draw`] to record draws
//! into the pass. The host then ends the pass, submits, and presents.
//!
//! ```ignore
//! let mut ui = UiRenderer::new(&device, &queue, surface_format);
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
//! Call [`UiRenderer::register_shader`] with a name and WGSL source.
//! The shader's vertex/fragment must use the shared instance layout —
//! see `shaders/rounded_rect.wgsl` for the canonical example. Bind the
//! shader at a node via `El::shader(ShaderBinding::custom(name).with(...))`.
//! Per-instance uniforms map to three generic `vec4` slots:
//!
//! | Uniform key | Slot (`@location`) | Accepted types |
//! |---|---|---|
//! | `vec_a` | 2 | `Color` (rgba 0..1) or `Vec4` |
//! | `vec_b` | 3 | `Color` or `Vec4` |
//! | `vec_c` | 4 | `Vec4` (or fall back to scalar `f32` packed in `.x`) |
//!
//! Stock `rounded_rect` reuses the same layout but reads its own named
//! uniforms (`fill`, `stroke`, `stroke_width`, `radius`, `shadow`).
//!
//! # Text rendering
//!
//! `stock::text_sdf` is implemented with [glyphon](https://github.com/grovesNL/glyphon),
//! which wraps cosmic-text for shaping/layout and rasterizes glyphs into
//! a wgpu texture atlas. We rebuild glyphon `Buffer`s per frame from
//! [`DrawOp::GlyphRun`]s — fine for v0.1 (no caching), and matches our
//! current "tree is rebuilt each frame" loop.
//!
//! Paint order follows the draw-op stream. Consecutive quads with the
//! same shader and scissor are batched, but text is rendered exactly at
//! its position in the stream so overlays and modal layers compose
//! correctly.

use std::borrow::Cow;
use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use glyphon::cosmic_text::Align;
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
};
use wgpu::util::DeviceExt;

use crate::draw_ops;
use crate::event::{self, KeyChord, KeyModifiers, UiEvent, UiEventKind, UiKey, UiState};
use crate::ir::{DrawOp, TextAnchor};
use crate::layout;
use crate::shader::{ShaderHandle, StockShader, UniformValue};
use crate::tree::{Color, El, FontWeight, Rect, TextWrap};

/// Per-frame globals bound at @group(0).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct FrameUniforms {
    viewport: [f32; 2],
    _pad: [f32; 2],
}

/// One instance of a rect-shaped shader. Layout is shared between
/// `stock::rounded_rect` and any custom shader registered via
/// [`UiRenderer::register_shader`]. The fragment shader interprets the
/// three vec4 slots however it wants; the vertex shader needs `rect` to
/// place the unit quad in pixel space.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct QuadInstance {
    /// xy = top-left px, zw = size px.
    rect: [f32; 4],
    /// `vec_a` slot — for stock::rounded_rect, this is `fill`.
    slot_a: [f32; 4],
    /// `vec_b` slot — for stock::rounded_rect, this is `stroke`.
    slot_b: [f32; 4],
    /// `vec_c` slot — for stock::rounded_rect, this is
    /// `(stroke_width, radius, shadow, _)`.
    slot_c: [f32; 4],
}

const ROUNDED_RECT_WGSL: &str = include_str!("../shaders/rounded_rect.wgsl");

/// Initial size for the dynamic instance buffer (grows as needed).
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// A contiguous run of instances drawn with the same pipeline. Built in
/// tree order so a custom shader sandwiched between two stock surfaces
/// is drawn at the right z-position.
#[derive(Clone, Copy)]
struct InstanceRun {
    handle: ShaderHandle,
    scissor: Option<PhysicalScissor>,
    first: u32,
    count: u32,
}

/// Renderer state owned by the host. One instance per surface/format.
pub struct UiRenderer {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PhysicalScissor {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

#[derive(Clone, Copy)]
enum PaintItem {
    QuadRun(usize),
    Text(usize),
}

struct TextLayer {
    renderer: TextRenderer,
    scissor: Option<PhysicalScissor>,
}

#[derive(Clone, Copy)]
struct TextMeta {
    left: f32,
    top: f32,
    color: GlyphColor,
    bounds: TextBounds,
}

impl UiRenderer {
    /// Create a renderer for the given target color format. The host
    /// passes its swapchain/render-target format here so pipelines and
    /// the glyph atlas are built compatible.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        // ---- Shared resources ----
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("attempt_4::frame_uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("attempt_4::bind_layout"),
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
            label: Some("attempt_4::bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buf.as_entire_binding(),
            }],
        });

        let quad_vbo = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("attempt_4::quad_vbo"),
            // Triangle strip: 4 corners, uv 0..1.
            contents: bytemuck::cast_slice::<f32, u8>(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("attempt_4::instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<QuadInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("attempt_4::pipeline_layout"),
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
            ROUNDED_RECT_WGSL,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::RoundedRect), rr_pipeline);
        let focus_pipeline = build_quad_pipeline(
            device,
            &pipeline_layout,
            target_format,
            "stock::focus_ring",
            ROUNDED_RECT_WGSL,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::FocusRing), focus_pipeline);

        // ---- Text pipeline (glyphon) ----
        let mut font_system = FontSystem::new();
        // Bundle Roboto with the crate so typography is consistent
        // across machines (no fontconfig surprises). FontSystem still
        // sees system fonts as a fallback, but our explicit Family::Name
        // request below picks the bundled face.
        let db = font_system.db_mut();
        db.load_font_data(include_bytes!("../fonts/Roboto-Regular.ttf").to_vec());
        db.load_font_data(include_bytes!("../fonts/Roboto-Medium.ttf").to_vec());
        db.load_font_data(include_bytes!("../fonts/Roboto-Bold.ttf").to_vec());
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
    /// [`crate::shader::ShaderBinding::custom`]; nodes bound to it via
    /// [`crate::tree::El::shader`] paint through this pipeline.
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

    /// Lay out the tree, resolve to draw ops, and upload per-frame
    /// buffers (quad instances + glyph atlas). Must be called before
    /// [`Self::draw`] and outside of any render pass.
    ///
    /// `viewport` is in **logical** pixels — the units the layout pass
    /// works in. `scale_factor` is the HiDPI multiplier (1.0 on a
    /// regular display, 2.0 on most modern HiDPI, can be fractional).
    /// The host's render-pass target should be sized at physical pixels
    /// (`viewport × scale_factor`); the renderer maps logical → physical
    /// internally so layout, fonts, and SDF math stay device-independent.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        root: &mut El,
        viewport: Rect,
        scale_factor: f32,
    ) {
        // Pre-pass: assign IDs so scroll offsets (keyed by id) can be
        // applied before layout positions anything.
        layout::assign_ids(root);
        self.ui_state.apply_scroll_to_tree(root);

        layout::layout(root, viewport);
        // Layout has clamped any out-of-range scroll offsets; persist
        // the clamped values so the next frame starts from a valid state.
        self.ui_state.read_scroll_from_tree(root);

        self.ui_state.sync_focus_order(root);
        // Apply UI-state visual deltas after layout, so focus targets can
        // survive rebuilds by node id and update their current rect.
        self.ui_state.apply_to_tree(root);
        let ops = draw_ops::draw_ops(root);

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
                    // (e.g. FocusRing until its pipeline lands, custom
                    // shaders the host forgot to register). The lint
                    // pass surfaces this elsewhere.
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
                label: Some("attempt_4::instance_buf (resized)"),
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
            .and_then(|t| event::hit_test_target(t, (x, y)));
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
            .and_then(|t| event::hit_test_target(t, (x, y)));
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
            .and_then(|t| event::hit_test_target(t, (x, y)));
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

/// Per-instance vertex attributes — must match the shared
/// `InstanceInput` struct in `shaders/rounded_rect.wgsl` and any
/// registered custom shader.
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect (xy=topleft px, zw=size px)
    2 => Float32x4,  // vec_a (stock::rounded_rect: fill)
    3 => Float32x4,  // vec_b (stock::rounded_rect: stroke)
    4 => Float32x4,  // vec_c (stock::rounded_rect: stroke_width, radius, shadow, _)
];

fn build_quad_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    label: &str,
    wgsl: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[
                wgpu::VertexBufferLayout {
                    array_stride: (2 * std::mem::size_of::<f32>()) as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 0,
                    }],
                },
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<QuadInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INSTANCE_ATTRS,
                },
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn close_run(
    runs: &mut Vec<InstanceRun>,
    paint_items: &mut Vec<PaintItem>,
    run_key: Option<(ShaderHandle, Option<PhysicalScissor>)>,
    first: u32,
    end: u32,
) {
    if let Some((handle, scissor)) = run_key {
        let count = end - first;
        if count > 0 {
            let index = runs.len();
            runs.push(InstanceRun {
                handle,
                scissor,
                first,
                count,
            });
            paint_items.push(PaintItem::QuadRun(index));
        }
    }
}

fn physical_scissor(
    scissor: Option<Rect>,
    scale: f32,
    viewport_px: (u32, u32),
) -> Option<PhysicalScissor> {
    let r = scissor?;
    let x1 = (r.x * scale).floor().clamp(0.0, viewport_px.0 as f32) as u32;
    let y1 = (r.y * scale).floor().clamp(0.0, viewport_px.1 as f32) as u32;
    let x2 = (r.right() * scale).ceil().clamp(0.0, viewport_px.0 as f32) as u32;
    let y2 = (r.bottom() * scale).ceil().clamp(0.0, viewport_px.1 as f32) as u32;
    Some(PhysicalScissor {
        x: x1,
        y: y1,
        w: x2.saturating_sub(x1),
        h: y2.saturating_sub(y1),
    })
}

fn set_scissor(
    pass: &mut wgpu::RenderPass<'_>,
    scissor: Option<PhysicalScissor>,
    full: PhysicalScissor,
) {
    let s = scissor.unwrap_or(full);
    pass.set_scissor_rect(s.x, s.y, s.w, s.h);
}

/// Pack a quad's uniforms into the shared `QuadInstance` layout. Stock
/// `rounded_rect` reads its named uniforms; everything else reads the
/// generic `vec_a`/`vec_b`/`vec_c` slots.
fn pack_instance(
    rect: Rect,
    shader: ShaderHandle,
    uniforms: &crate::shader::UniformBlock,
) -> QuadInstance {
    let rect_arr = [rect.x, rect.y, rect.w, rect.h];

    match shader {
        ShaderHandle::Stock(StockShader::RoundedRect) => QuadInstance {
            rect: rect_arr,
            slot_a: uniforms
                .get("fill")
                .and_then(as_color)
                .map(rgba_f32)
                .unwrap_or([0.0; 4]),
            slot_b: uniforms
                .get("stroke")
                .and_then(as_color)
                .map(rgba_f32)
                .unwrap_or([0.0; 4]),
            slot_c: [
                uniforms.get("stroke_width").and_then(as_f32).unwrap_or(0.0),
                uniforms.get("radius").and_then(as_f32).unwrap_or(0.0),
                uniforms.get("shadow").and_then(as_f32).unwrap_or(0.0),
                0.0,
            ],
        },
        ShaderHandle::Stock(StockShader::FocusRing) => QuadInstance {
            rect: rect_arr,
            slot_a: [0.0; 4],
            slot_b: uniforms
                .get("color")
                .and_then(as_color)
                .map(rgba_f32)
                .unwrap_or([0.0; 4]),
            slot_c: [
                uniforms.get("width").and_then(as_f32).unwrap_or(0.0),
                uniforms.get("radius").and_then(as_f32).unwrap_or(0.0),
                0.0,
                0.0,
            ],
        },
        _ => QuadInstance {
            rect: rect_arr,
            slot_a: uniforms.get("vec_a").map(value_to_vec4).unwrap_or([0.0; 4]),
            slot_b: uniforms.get("vec_b").map(value_to_vec4).unwrap_or([0.0; 4]),
            slot_c: uniforms.get("vec_c").map(value_to_vec4).unwrap_or([0.0; 4]),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn build_text_buffer(
    font_system: &mut FontSystem,
    rect: Rect,
    scissor: Option<Rect>,
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    anchor: TextAnchor,
    color: Color,
    scale: f32,
) -> (Buffer, TextMeta) {
    // All text quantities are pre-multiplied to physical pixels here so
    // glyphon rasterizes at native device DPI (crisp text on HiDPI).
    let physical_size = size * scale;
    let physical_line_height = physical_size * 1.4;
    let metrics = Metrics::new(physical_size, physical_line_height);
    let mut buffer = Buffer::new(font_system, metrics);

    // Buffer width drives cosmic-text wrapping AND alignment. For
    // Middle/End anchors we need a known width so the alignment math
    // works. For Start anchors, constraining width to a too-tight
    // intrinsic rect causes silent wrapping ("Theme" → "Them" + "e"
    // on a hidden second line); leave width unbounded.
    let buffer_width = match (wrap, anchor) {
        (TextWrap::Wrap, _) => Some(rect.w * scale),
        (TextWrap::NoWrap, TextAnchor::Start) => None,
        (TextWrap::NoWrap, TextAnchor::Middle | TextAnchor::End) => Some(rect.w * scale),
    };
    buffer.set_size(
        font_system,
        buffer_width,
        Some((rect.h * scale).max(physical_line_height)),
    );

    // Use bundled Roboto for sans-serif so typography is consistent
    // regardless of what fonts the host has installed. fontdb resolves
    // Name("Roboto") to whichever weight matches the request.
    let family = if mono {
        Family::Monospace
    } else {
        Family::Name("Roboto")
    };
    let attrs = Attrs::new().family(family).weight(map_weight(weight));
    buffer.set_text(font_system, text, attrs, Shaping::Advanced);

    if let Some(align) = match anchor {
        TextAnchor::Start => None,
        TextAnchor::Middle => Some(Align::Center),
        TextAnchor::End => Some(Align::End),
    } {
        for line in buffer.lines.iter_mut() {
            line.set_align(Some(align));
        }
        buffer.shape_until_scroll(font_system, false);
    }

    // Single-line controls center text vertically. Wrapped text boxes
    // are top-aligned so additional lines flow down from the box start.
    let top_logical = match wrap {
        TextWrap::NoWrap => rect.y + ((rect.h - size * 1.4) * 0.5).max(0.0),
        TextWrap::Wrap => rect.y,
    };
    let top = top_logical * scale;
    let left = rect.x * scale;

    // v0.1: don't tightly clip text to its rect bounds — the layout's
    // intrinsic-width estimator is approximate and can be a few pixels
    // narrower than cosmic-text's actual run width. Real overflow shows
    // up in the lint pass and as visible overlap, not silent glyph
    // chopping.
    let bounds = scissor.unwrap_or(Rect::new(0.0, 0.0, 1_000_000_000.0, 1_000_000_000.0));
    let meta = TextMeta {
        left,
        top,
        color: glyphon_color(color),
        bounds: TextBounds {
            left: (bounds.x * scale).floor() as i32 - 2,
            top: (bounds.y * scale).floor() as i32 - 2,
            right: (bounds.right() * scale).ceil() as i32 + 2,
            bottom: (bounds.bottom() * scale).ceil() as i32 + 2,
        },
    };
    (buffer, meta)
}

fn map_weight(w: FontWeight) -> Weight {
    match w {
        FontWeight::Regular => Weight::NORMAL,
        FontWeight::Medium => Weight::MEDIUM,
        FontWeight::Semibold => Weight::SEMIBOLD,
        FontWeight::Bold => Weight::BOLD,
    }
}

fn glyphon_color(c: Color) -> GlyphColor {
    GlyphColor::rgba(c.r, c.g, c.b, c.a)
}

fn as_color(v: &UniformValue) -> Option<Color> {
    match v {
        UniformValue::Color(c) => Some(*c),
        _ => None,
    }
}
fn as_f32(v: &UniformValue) -> Option<f32> {
    match v {
        UniformValue::F32(f) => Some(*f),
        _ => None,
    }
}

/// Coerce any `UniformValue` into the four floats of a vec4 slot.
/// Custom-shader authors typically pass `Color` (rgba) or `Vec4`
/// (arbitrary semantics); `F32` packs into `.x` so a single scalar like
/// `radius` doesn't need a Vec4 wrapper.
fn value_to_vec4(v: &UniformValue) -> [f32; 4] {
    match v {
        UniformValue::Color(c) => rgba_f32(*c),
        UniformValue::Vec4(a) => *a,
        UniformValue::Vec2([x, y]) => [*x, *y, 0.0, 0.0],
        UniformValue::F32(f) => [*f, 0.0, 0.0, 0.0],
        UniformValue::Bool(b) => [if *b { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
    }
}

fn rgba_f32(c: Color) -> [f32; 4] {
    // Tokens are authored in sRGB display space; the surface is an
    // *Srgb format so alpha blending happens in linear space (correct
    // for color blending, slightly fattens light-on-dark text — see
    // the font notes in the module-level docs).
    [
        srgb_to_linear(c.r as f32 / 255.0),
        srgb_to_linear(c.g as f32 / 255.0),
        srgb_to_linear(c.b as f32 / 255.0),
        c.a as f32 / 255.0,
    ]
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::UniformBlock;
    use crate::tokens;

    #[test]
    fn focus_ring_uniforms_pack_into_rounded_rect_layout() {
        let mut uniforms = UniformBlock::new();
        uniforms.insert("color", UniformValue::Color(tokens::FOCUS_RING));
        uniforms.insert("width", UniformValue::F32(2.0));
        uniforms.insert("radius", UniformValue::F32(9.0));

        let inst = pack_instance(
            Rect::new(1.0, 2.0, 30.0, 40.0),
            ShaderHandle::Stock(StockShader::FocusRing),
            &uniforms,
        );

        assert_eq!(inst.rect, [1.0, 2.0, 30.0, 40.0]);
        assert_eq!(inst.slot_a, [0.0; 4]);
        assert!(inst.slot_b[3] > 0.0, "focus ring stroke should be visible");
        assert_eq!(inst.slot_c[0], 2.0);
        assert_eq!(inst.slot_c[1], 9.0);
    }

    #[test]
    fn physical_scissor_converts_logical_to_physical_pixels() {
        let scissor = physical_scissor(Some(Rect::new(10.2, 20.2, 30.2, 40.2)), 2.0, (200, 200))
            .expect("scissor");

        assert_eq!(
            scissor,
            PhysicalScissor {
                x: 20,
                y: 40,
                w: 61,
                h: 81
            }
        );
    }
}
