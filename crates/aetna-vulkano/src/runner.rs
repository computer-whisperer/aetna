//! `aetna-vulkano::Runner` — peer to `aetna_wgpu::Runner`.
//!
//! v5.3 step 5 grows the Runner from the GPU-agnostic skeleton (step 4)
//! to actually rendering rect-shaped surfaces. It now owns:
//!
//! - a single-pass render pass with one color attachment (the host
//!   creates framebuffers against this and exposes its handle so
//!   pipelines can be subpass-pinned at construction time);
//! - one `GraphicsPipeline` per registered shader (stock rounded_rect +
//!   focus_ring up-front, custom shaders added via `register_shader`);
//! - a persistent quad VBO (the unit-quad strip), a persistent frame
//!   uniform buffer (viewport extent), a single descriptor set bound to
//!   it, and a host-visible instance buffer that grows on demand.
//!
//! `prepare()` walks the `DrawOp` stream produced by `aetna-core`,
//! packs `Quad`s into the instance buffer, and groups consecutive ones
//! sharing a pipeline + scissor into `InstanceRun`s. `draw()` walks the
//! resulting paint stream and records vulkano commands into the host's
//! primary command-buffer builder.
//!
//! Text isn't here yet — `DrawOp::GlyphRun` only closes the current
//! quad run for now. Step 6 wires up the atlas-mirror text path.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use aetna_core::{
    AnimationMode, El, KeyChord, KeyModifiers, Rect, UiEvent, UiEventKind, UiKey, UiState,
    draw_ops, hit_test, ir,
    layout,
    shader::{ShaderHandle, StockShader, stock_wgsl},
};
use smallvec::smallvec;
use vulkano::{
    buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer},
    command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer},
    descriptor_set::{
        DescriptorSet, WriteDescriptorSet, allocator::StandardDescriptorSetAllocator,
    },
    device::{Device, Queue},
    format::Format,
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{
        GraphicsPipeline, Pipeline, PipelineBindPoint,
        graphics::viewport::Viewport,
    },
    render_pass::{RenderPass, Subpass},
};

use crate::instance::{
    InstanceRun, PaintItem, PhysicalScissor, QuadInstance, close_run, pack_instance,
    physical_scissor, set_scissor,
};
use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::{FrameUniforms, build_quad_pipeline};

const INITIAL_INSTANCE_CAPACITY: u64 = 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareResult {
    pub needs_redraw: bool,
    pub timings: PrepareTimings,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PrepareTimings {
    pub layout: std::time::Duration,
    pub draw_ops: std::time::Duration,
    pub paint: std::time::Duration,
    pub gpu_upload: std::time::Duration,
    pub snapshot: std::time::Duration,
}

pub struct Runner {
    device: Arc<Device>,
    _queue: Arc<Queue>,
    /// Used to build the runner-owned render pass; kept around so step 7's
    /// `register_shader` hot-reload path can rebuild pipelines if needed.
    #[allow(dead_code)]
    target_format: Format,

    memory_alloc: Arc<StandardMemoryAllocator>,
    /// Step 6 will use this to allocate per-page text-atlas descriptor sets.
    #[allow(dead_code)]
    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,

    render_pass: Arc<RenderPass>,

    pipelines: HashMap<ShaderHandle, Arc<GraphicsPipeline>>,

    quad_vbo: Subbuffer<[f32]>,
    frame_uniform_buf: Subbuffer<FrameUniforms>,
    frame_descriptor_set: Arc<DescriptorSet>,

    instance_buf: Subbuffer<[QuadInstance]>,
    instance_capacity: u64,

    quad_scratch: Vec<QuadInstance>,
    runs: Vec<InstanceRun>,
    paint_items: Vec<PaintItem>,

    viewport_px: (u32, u32),
    surface_size_override: Option<(u32, u32)>,

    /// SPIR-V words cached per registered custom shader name. The
    /// pipeline itself is built lazily on first use.
    registered_shaders: HashMap<&'static str, Vec<u32>>,

    ui_state: UiState,
    last_tree: Option<El>,
}

impl Runner {
    /// Create a runner. The host's swapchain must use `target_format`;
    /// stock pipelines are built against a single-pass render pass with
    /// a color attachment in that format. Call [`Self::render_pass`] to
    /// get the pass back so you can build framebuffers against it.
    pub fn new(device: Arc<Device>, queue: Arc<Queue>, target_format: Format) -> Self {
        let memory_alloc = Arc::new(StandardMemoryAllocator::new_default(device.clone()));
        let descriptor_alloc = Arc::new(StandardDescriptorSetAllocator::new(
            device.clone(),
            Default::default(),
        ));

        let render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    // Clear at pass-begin so the host doesn't need a
                    // separate `clear_color_image` step. The host passes
                    // its own clear color via `begin_render_pass`'s
                    // `clear_values`.
                    format: target_format,
                    samples: 1,
                    load_op: Clear,
                    store_op: Store,
                },
            },
            pass: {
                color: [color],
                depth_stencil: {},
            },
        )
        .expect("aetna-vulkano: create render pass");
        let subpass = Subpass::from(render_pass.clone(), 0)
            .expect("aetna-vulkano: subpass 0 of single-pass render pass");

        let mut pipelines = HashMap::new();
        let rr = build_quad_pipeline(
            device.clone(),
            subpass.clone(),
            "stock::rounded_rect",
            stock_wgsl::ROUNDED_RECT,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::RoundedRect), rr.clone());
        let fr = build_quad_pipeline(
            device.clone(),
            subpass,
            "stock::focus_ring",
            stock_wgsl::ROUNDED_RECT,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::FocusRing), fr);

        // Persistent quad VBO — 4 corners of the unit quad as a triangle
        // strip, written once.
        let quad_vbo = Buffer::from_iter(
            memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::VERTEX_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            [0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .expect("aetna-vulkano: quad VBO");

        // Persistent host-visible frame uniforms. Host writes new values
        // each frame in `prepare()`; the descriptor set bound to it
        // doesn't need rebuilding because the buffer handle is stable.
        let frame_uniform_buf = Buffer::new_sized::<FrameUniforms>(
            memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::UNIFORM_BUFFER,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: frame uniform buffer");

        // Bind the frame buffer to set 0 of the rounded_rect pipeline's
        // layout. All rect-shaped pipelines share this set 0 binding,
        // so one descriptor set serves them all.
        let frame_set_layout = rr.layout().set_layouts()[0].clone();
        let frame_descriptor_set = DescriptorSet::new(
            descriptor_alloc.clone(),
            frame_set_layout,
            [WriteDescriptorSet::buffer(0, frame_uniform_buf.clone())],
            [],
        )
        .expect("aetna-vulkano: frame descriptor set");

        let instance_buf = create_instance_buffer(&memory_alloc, INITIAL_INSTANCE_CAPACITY);

        Self {
            device,
            _queue: queue,
            target_format,
            memory_alloc,
            descriptor_alloc,
            render_pass,
            pipelines,
            quad_vbo,
            frame_uniform_buf,
            frame_descriptor_set,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            quad_scratch: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY as usize),
            runs: Vec::new(),
            paint_items: Vec::new(),
            viewport_px: (1, 1),
            surface_size_override: None,
            registered_shaders: HashMap::new(),
            ui_state: UiState::new(),
            last_tree: None,
        }
    }

    /// The render pass pipelines are built against. The host must
    /// construct framebuffers against this pass and begin/end it around
    /// each call to [`Self::draw`].
    pub fn render_pass(&self) -> &Arc<RenderPass> {
        &self.render_pass
    }

    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.surface_size_override = Some((width.max(1), height.max(1)));
    }

    /// Register a custom shader. WGSL → SPIR-V at register time; bad
    /// WGSL panics here, not mid-frame. The graphics pipeline is built
    /// eagerly so a shader registered for a `key` is ready to draw
    /// immediately.
    pub fn register_shader(&mut self, name: &'static str, wgsl: &str) {
        // Cache the SPIR-V words too — useful for diagnostics + future
        // re-registration without re-running naga.
        let spirv = wgsl_to_spirv(name, wgsl)
            .unwrap_or_else(|e| panic!("aetna-vulkano: WGSL compile failed for `{name}`: {e}"));
        self.registered_shaders.insert(name, spirv);

        let subpass = Subpass::from(self.render_pass.clone(), 0)
            .expect("aetna-vulkano: subpass 0");
        let pipeline = build_quad_pipeline(self.device.clone(), subpass, name, wgsl);
        self.pipelines.insert(ShaderHandle::Custom(name), pipeline);
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
            .and_then(|tree| self.ui_state.rect_of_key(tree, key))
    }

    /// Lay out the tree, run animation tick, walk the draw-op stream,
    /// and upload per-frame buffers (instance data + frame uniforms).
    /// Must be called before [`Self::draw`] and outside of any render
    /// pass.
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

        // Walk DrawOps → instance buffer + paint stream.
        let ops = draw_ops::draw_ops(root, &self.ui_state);
        self.quad_scratch.clear();
        self.runs.clear();
        self.paint_items.clear();

        let mut current_run: Option<(ShaderHandle, Option<PhysicalScissor>)> = None;
        let mut run_first: u32 = 0;

        for op in &ops {
            match op {
                ir::DrawOp::Quad {
                    rect,
                    scissor,
                    shader,
                    uniforms,
                    ..
                } => {
                    if !self.pipelines.contains_key(shader) {
                        // Unknown shader handle — skip but break the run
                        // so paint order is preserved across the gap.
                        close_run(
                            &mut self.runs,
                            &mut self.paint_items,
                            current_run,
                            run_first,
                            self.quad_scratch.len() as u32,
                        );
                        current_run = None;
                        run_first = self.quad_scratch.len() as u32;
                        continue;
                    }
                    let phys = physical_scissor(*scissor, scale_factor, self.viewport_px);
                    let key = (*shader, phys);
                    if current_run != Some(key) {
                        close_run(
                            &mut self.runs,
                            &mut self.paint_items,
                            current_run,
                            run_first,
                            self.quad_scratch.len() as u32,
                        );
                        run_first = self.quad_scratch.len() as u32;
                        current_run = Some(key);
                    }
                    self.quad_scratch
                        .push(pack_instance(*rect, *shader, uniforms));
                }
                ir::DrawOp::GlyphRun { .. } | ir::DrawOp::BackdropSnapshot { .. } => {
                    // Step 6 will record glyph runs into a Text PaintItem.
                    // For now: close the current quad run so the absent
                    // text doesn't disrupt batching of surrounding quads.
                    close_run(
                        &mut self.runs,
                        &mut self.paint_items,
                        current_run,
                        run_first,
                        self.quad_scratch.len() as u32,
                    );
                    current_run = None;
                    run_first = self.quad_scratch.len() as u32;
                }
            }
        }
        close_run(
            &mut self.runs,
            &mut self.paint_items,
            current_run,
            run_first,
            self.quad_scratch.len() as u32,
        );

        // Grow the instance buffer if needed. Power-of-two doubling
        // matches aetna-wgpu and keeps reallocation amortised O(1).
        let needed = self.quad_scratch.len() as u64;
        if needed > self.instance_capacity {
            let new_cap = needed.next_power_of_two().max(self.instance_capacity * 2);
            self.instance_buf = create_instance_buffer(&self.memory_alloc, new_cap);
            self.instance_capacity = new_cap;
        }

        if !self.quad_scratch.is_empty() {
            let mut write = self
                .instance_buf
                .write()
                .expect("aetna-vulkano: instance buffer write");
            write[..self.quad_scratch.len()].copy_from_slice(&self.quad_scratch);
        }

        {
            // FrameUniforms.viewport is the **logical** viewport — the
            // vertex shader divides per-instance positions (which layout
            // produced in logical pixels) by it to get clip-space coords.
            // Using physical here would render every quad at scale_factor⁻¹
            // size in the top-left — and silently break hit-testing,
            // because layout's logical rects no longer match what the user
            // sees.
            let mut write = self
                .frame_uniform_buf
                .write()
                .expect("aetna-vulkano: frame uniform write");
            *write = FrameUniforms {
                viewport: [viewport.w, viewport.h],
                _pad: [0.0, 0.0],
            };
        }

        self.last_tree = Some(root.clone());

        PrepareResult {
            needs_redraw,
            timings: PrepareTimings::default(),
        }
    }

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

    /// Record draws into the host-managed primary command-buffer
    /// builder. Call inside the host's `begin_render_pass` /
    /// `end_render_pass` scope, with the runner's `render_pass()`.
    pub fn draw(&self, builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        if self.paint_items.is_empty() {
            return;
        }

        let (px_w, px_h) = self.viewport_px;
        builder
            .set_viewport(
                0,
                smallvec![Viewport {
                    offset: [0.0, 0.0],
                    extent: [px_w as f32, px_h as f32],
                    depth_range: 0.0..=1.0,
                }],
            )
            .expect("set_viewport");

        let full = PhysicalScissor {
            x: 0,
            y: 0,
            w: px_w,
            h: px_h,
        };

        for item in &self.paint_items {
            match *item {
                PaintItem::QuadRun(idx) => {
                    let run = &self.runs[idx];
                    set_scissor(builder, run.scissor, full);
                    let pipeline = self
                        .pipelines
                        .get(&run.handle)
                        .expect("run handle has no pipeline (bug in prepare)");
                    builder
                        .bind_pipeline_graphics(pipeline.clone())
                        .expect("bind_pipeline_graphics");
                    builder
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            pipeline.layout().clone(),
                            0,
                            self.frame_descriptor_set.clone(),
                        )
                        .expect("bind_descriptor_sets");
                    builder
                        .bind_vertex_buffers(
                            0,
                            (self.quad_vbo.clone(), self.instance_buf.clone()),
                        )
                        .expect("bind_vertex_buffers");
                    // SAFETY: the pipeline expects 4 vertices (the unit
                    // quad strip) and `run.count` instances starting at
                    // `run.first`; the instance buffer was sized to fit
                    // every packed instance in `prepare()`.
                    unsafe {
                        builder.draw(4, run.count, 0, run.first).expect("draw");
                    }
                }
            }
        }

    }
}

fn create_instance_buffer(
    allocator: &Arc<StandardMemoryAllocator>,
    capacity: u64,
) -> Subbuffer<[QuadInstance]> {
    Buffer::new_slice::<QuadInstance>(
        allocator.clone(),
        BufferCreateInfo {
            usage: BufferUsage::VERTEX_BUFFER,
            ..Default::default()
        },
        AllocationCreateInfo {
            memory_type_filter: MemoryTypeFilter::PREFER_HOST
                | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
            ..Default::default()
        },
        capacity,
    )
    .expect("aetna-vulkano: instance buffer alloc")
}
