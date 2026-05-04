//! `aetna-vulkano::Runner` — peer to `aetna_wgpu::Runner`.
//!
//! v5.3 step 5 grows the Runner from the GPU-agnostic skeleton (step 4)
//! to actually rendering rect-shaped surfaces. It now owns:
//!
//! - a single-pass render pass with one color attachment (the host
//!   creates framebuffers against this and exposes its handle so
//!   pipelines can be subpass-pinned at construction time);
//! - one `GraphicsPipeline` per registered shader (stock rounded_rect
//!   up-front, custom shaders added via `register_shader`); focus
//!   indicators ride on each focusable node's own quad via uniforms
//!   on `stock::rounded_rect`, no separate ring pipeline;
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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use aetna_core::{
    AnimationMode, El, KeyChord, KeyModifiers, Rect, UiEvent, UiKey, UiState,
    shader::{ShaderHandle, StockShader, stock_wgsl},
};
use smallvec::smallvec;
use vulkano::{
    buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer},
    command_buffer::{
        AutoCommandBufferBuilder, CopyImageInfo, PrimaryAutoCommandBuffer, RenderPassBeginInfo,
        SubpassBeginInfo, SubpassContents, SubpassEndInfo,
        allocator::StandardCommandBufferAllocator,
    },
    descriptor_set::{
        DescriptorSet, DescriptorSetWithOffsets, WriteDescriptorSet,
        allocator::StandardDescriptorSetAllocator, layout::DescriptorSetLayout,
    },
    device::{Device, Queue},
    format::Format,
    image::{
        Image, ImageCreateInfo, ImageType, ImageUsage,
        sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode},
        view::ImageView,
    },
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{GraphicsPipeline, Pipeline, PipelineBindPoint, graphics::viewport::Viewport},
    render_pass::{Framebuffer, RenderPass, Subpass},
};

use aetna_core::paint::{PaintItem, PhysicalScissor, QuadInstance};
use aetna_core::runtime::RunnerCore;

pub use aetna_core::runtime::{PrepareResult, PrepareTimings};

use crate::instance::set_scissor;
use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::{FrameUniforms, build_quad_pipeline};
use crate::text::TextPaint;

const INITIAL_INSTANCE_CAPACITY: u64 = 1024;

pub struct Runner {
    device: Arc<Device>,
    _queue: Arc<Queue>,
    /// Used to build the runner-owned render pass; kept around so step 7's
    /// `register_shader` hot-reload path can rebuild pipelines if needed.
    #[allow(dead_code)]
    target_format: Format,

    memory_alloc: Arc<StandardMemoryAllocator>,
    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,

    /// Render pass with `load_op: Clear` — used for the first (or only)
    /// pass of a frame. Pipelines are built against subpass 0 of this
    /// pass.
    render_pass: Arc<RenderPass>,
    /// Render pass with `load_op: Load` — used for Pass B when the
    /// frame has a `BackdropSnapshot` boundary, so Pass A's pixels
    /// remain underneath. Attachment-compatible with `render_pass` so
    /// the same pipelines work with both.
    load_render_pass: Arc<RenderPass>,

    pipelines: HashMap<ShaderHandle, Arc<GraphicsPipeline>>,

    text_paint: TextPaint,

    quad_vbo: Subbuffer<[f32]>,
    frame_uniform_buf: Subbuffer<FrameUniforms>,
    frame_descriptor_set: Arc<DescriptorSet>,

    instance_buf: Subbuffer<[QuadInstance]>,
    instance_capacity: u64,

    /// SPIR-V words cached per registered custom shader name. The
    /// pipeline itself is built lazily on first use.
    registered_shaders: HashMap<&'static str, Vec<u32>>,

    /// Custom shader names registered with `samples_backdrop=true`.
    /// `prepare_paint` queries this to insert a `BackdropSnapshot`
    /// marker before the first backdrop-sampling draw, and the inner
    /// draw loop binds the snapshot descriptor set at set=1 when the
    /// run's pipeline expects it.
    backdrop_shaders: HashSet<&'static str>,

    /// Linear-filtering sampler bound at `@group(1) @binding(1)` for
    /// every backdrop-sampling pipeline.
    backdrop_sampler: Arc<Sampler>,

    /// Set 1's descriptor-set layout for backdrop-sampling pipelines —
    /// captured from the first backdrop pipeline registered. The same
    /// layout shape (texture_2d + sampler) is shared across all
    /// backdrop shaders, so a single descriptor set built against this
    /// layout binds correctly into any backdrop pipeline.
    backdrop_set_layout: Option<Arc<DescriptorSetLayout>>,

    /// Lazily-allocated snapshot of the color target. Sized to match
    /// the current target on each `render()` call; rebuilt when the
    /// target's extent changes.
    snapshot: Option<SnapshotImage>,
    /// Descriptor set binding the snapshot view + `backdrop_sampler` at
    /// set 1. Rebuilt whenever `snapshot` is recreated.
    backdrop_descriptor_set: Option<Arc<DescriptorSet>>,

    /// Wall-clock origin for the `time` field in `FrameUniforms`.
    start_time: Instant,

    // Backend-agnostic state shared with aetna-wgpu: interaction state,
    // paint-stream scratch (quad_scratch / runs / paint_items),
    // viewport_px, last_tree, the 13 input plumbing methods.
    core: RunnerCore,
}

struct SnapshotImage {
    image: Arc<Image>,
    extent: [u32; 3],
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
        // Used internally for text-atlas dirty-region uploads.
        let cmd_alloc = Arc::new(StandardCommandBufferAllocator::new(
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
        // Pass B (used when there's a BackdropSnapshot boundary)
        // preserves Pass A's contents — `load_op: Load`. Attachment
        // layout matches `render_pass`, so pipelines built against the
        // Clear pass are render-pass-compatible with this Load pass.
        let load_render_pass = vulkano::single_pass_renderpass!(
            device.clone(),
            attachments: {
                color: {
                    format: target_format,
                    samples: 1,
                    load_op: Load,
                    store_op: Store,
                },
            },
            pass: {
                color: [color],
                depth_stencil: {},
            },
        )
        .expect("aetna-vulkano: create load render pass");
        let subpass = Subpass::from(render_pass.clone(), 0)
            .expect("aetna-vulkano: subpass 0 of single-pass render pass");

        let mut pipelines = HashMap::new();
        let rr = build_quad_pipeline(
            device.clone(),
            subpass,
            "stock::rounded_rect",
            stock_wgsl::ROUNDED_RECT,
        );
        pipelines.insert(ShaderHandle::Stock(StockShader::RoundedRect), rr.clone());

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

        let text_subpass =
            Subpass::from(render_pass.clone(), 0).expect("aetna-vulkano: text subpass 0");
        let text_paint = TextPaint::new(
            device.clone(),
            queue.clone(),
            memory_alloc.clone(),
            descriptor_alloc.clone(),
            cmd_alloc,
            text_subpass,
        );

        // Filtering sampler bound at @group(1) @binding(1) for every
        // backdrop-sampling pipeline. Mirrors the wgpu side: linear
        // mag/min, nearest mipmap (we don't generate mips on the
        // snapshot), clamp-to-edge so blur kernels at the rim don't
        // wrap.
        let backdrop_sampler = Sampler::new(
            device.clone(),
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: backdrop sampler");

        Self {
            device,
            _queue: queue,
            target_format,
            memory_alloc,
            descriptor_alloc,
            render_pass,
            load_render_pass,
            pipelines,
            text_paint,
            quad_vbo,
            frame_uniform_buf,
            frame_descriptor_set,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            registered_shaders: HashMap::new(),
            backdrop_shaders: HashSet::new(),
            backdrop_sampler,
            backdrop_set_layout: None,
            snapshot: None,
            backdrop_descriptor_set: None,
            start_time: Instant::now(),
            core: {
                let mut c = RunnerCore::new();
                c.quad_scratch = Vec::with_capacity(INITIAL_INSTANCE_CAPACITY as usize);
                c
            },
        }
    }

    /// The render pass pipelines are built against. The host must
    /// construct framebuffers against this pass and begin/end it around
    /// each call to [`Self::draw`].
    pub fn render_pass(&self) -> &Arc<RenderPass> {
        &self.render_pass
    }

    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.core.set_surface_size(width, height);
    }

    /// Register a custom shader. WGSL → SPIR-V at register time; bad
    /// WGSL panics here, not mid-frame. The graphics pipeline is built
    /// eagerly so a shader registered for a `key` is ready to draw
    /// immediately.
    pub fn register_shader(&mut self, name: &'static str, wgsl: &str) {
        self.register_shader_with(name, wgsl, false);
    }

    /// Register a custom shader with an opt-in backdrop-sampling flag.
    /// When `samples_backdrop` is true, the paint scheduler inserts a
    /// pass boundary before the first draw bound to this shader, and
    /// `Runner::render` arranges Pass A → snapshot copy → Pass B so the
    /// shader can sample the post-Pass-A target through `@group(1)`.
    pub fn register_shader_with(&mut self, name: &'static str, wgsl: &str, samples_backdrop: bool) {
        // Cache the SPIR-V words too — useful for diagnostics + future
        // re-registration without re-running naga.
        let spirv = wgsl_to_spirv(name, wgsl)
            .unwrap_or_else(|e| panic!("aetna-vulkano: WGSL compile failed for `{name}`: {e}"));
        self.registered_shaders.insert(name, spirv);

        let subpass = Subpass::from(self.render_pass.clone(), 0).expect("aetna-vulkano: subpass 0");
        let pipeline = build_quad_pipeline(self.device.clone(), subpass, name, wgsl);
        if samples_backdrop {
            // Capture set 1's layout from the first backdrop pipeline
            // we see. Vulkano builds pipeline layouts via reflection,
            // so any backdrop shader declaring `@group(1) binding(0)
            // texture_2d<f32> + @group(1) binding(1) sampler` produces
            // a structurally-identical layout — one descriptor set
            // built against this layout binds correctly into all
            // backdrop pipelines.
            if self.backdrop_set_layout.is_none() {
                let layouts = pipeline.layout().set_layouts();
                let set1 = layouts.get(1).unwrap_or_else(|| {
                    panic!(
                        "aetna-vulkano: backdrop shader `{name}` has no @group(1) — \
                         expected `backdrop_tex` (binding 0) and `backdrop_smp` (binding 1)"
                    )
                });
                self.backdrop_set_layout = Some(set1.clone());
            }
            self.backdrop_shaders.insert(name);
        } else {
            self.backdrop_shaders.remove(name);
        }
        self.pipelines.insert(ShaderHandle::Custom(name), pipeline);
    }

    pub fn ui_state(&self) -> &UiState {
        self.core.ui_state()
    }

    pub fn debug_summary(&self) -> String {
        self.core.debug_summary()
    }

    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.core.rect_of_key(key)
    }

    /// Lay out the tree, run animation tick, walk the draw-op stream,
    /// and upload per-frame buffers (instance data + frame uniforms).
    /// Must be called before [`Self::draw`] and outside of any render
    /// pass.
    pub fn prepare(&mut self, root: &mut El, viewport: Rect, scale_factor: f32) -> PrepareResult {
        let mut timings = PrepareTimings::default();

        let (ops, needs_redraw) =
            self.core
                .prepare_layout(root, viewport, scale_factor, &mut timings);

        self.text_paint.frame_begin();
        let pipelines = &self.pipelines;
        let backdrop_shaders = &self.backdrop_shaders;
        self.core.prepare_paint(
            &ops,
            |shader| pipelines.contains_key(shader),
            |shader| match shader {
                ShaderHandle::Custom(name) => backdrop_shaders.contains(name),
                ShaderHandle::Stock(_) => false,
            },
            &mut self.text_paint,
            scale_factor,
            &mut timings,
        );

        let t_paint_end = Instant::now();
        // Grow the instance buffer if needed. Power-of-two doubling
        // matches aetna-wgpu and keeps reallocation amortised O(1).
        let needed = self.core.quad_scratch.len() as u64;
        if needed > self.instance_capacity {
            let new_cap = needed.next_power_of_two().max(self.instance_capacity * 2);
            self.instance_buf = create_instance_buffer(&self.memory_alloc, new_cap);
            self.instance_capacity = new_cap;
        }
        if !self.core.quad_scratch.is_empty() {
            let mut write = self
                .instance_buf
                .write()
                .expect("aetna-vulkano: instance buffer write");
            write[..self.core.quad_scratch.len()].copy_from_slice(&self.core.quad_scratch);
        }
        // Sync atlas dirty regions to GPU images + upload glyph instances.
        // Text uploads run through their own one-shot command buffer
        // submitted+waited inside flush().
        self.text_paint.flush();
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
            let time = (Instant::now() - self.start_time).as_secs_f32();
            *write = FrameUniforms {
                viewport: [viewport.w, viewport.h],
                time,
                _pad: 0.0,
            };
        }
        timings.gpu_upload = Instant::now() - t_paint_end;

        self.core.snapshot(root, &mut timings);

        PrepareResult {
            needs_redraw,
            timings,
        }
    }

    pub fn pointer_moved(&mut self, x: f32, y: f32) -> Option<&str> {
        self.core.pointer_moved(x, y)
    }

    pub fn pointer_left(&mut self) {
        self.core.pointer_left();
    }

    pub fn pointer_down(&mut self, x: f32, y: f32) {
        self.core.pointer_down(x, y);
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) -> Option<UiEvent> {
        self.core.pointer_up(x, y)
    }

    pub fn key_down(
        &mut self,
        key: UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        self.core.key_down(key, modifiers, repeat)
    }

    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.core.set_hotkeys(hotkeys);
    }

    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.core.set_animation_mode(mode);
    }

    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        self.core.pointer_wheel(x, y, dy)
    }

    /// Record draws into the host-managed primary command-buffer
    /// builder. Call inside the host's `begin_render_pass` /
    /// `end_render_pass` scope, with the runner's `render_pass()`.
    ///
    /// `BackdropSnapshot` markers in the paint stream are no-ops in
    /// this entry point — backdrop-sampling shaders need the multi-pass
    /// scheduling provided by [`Self::render`]. Hosts that want to use
    /// `liquid_glass`-style shaders should call `render()` instead and
    /// let the runner own pass lifetimes.
    pub fn draw(&self, builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        if self.core.paint_items.is_empty() {
            return;
        }
        self.set_viewport(builder);
        self.draw_items(builder, &self.core.paint_items);
    }

    /// Record draws into a host-supplied command buffer, owning pass
    /// lifetimes ourselves so backdrop-sampling shaders can sample a
    /// snapshot of Pass A's content. Mirrors `aetna_wgpu::Runner::render`.
    ///
    /// The host hands us:
    /// - the command-buffer builder (we record into it),
    /// - the `Framebuffer` matching the current swapchain image,
    /// - the underlying `Image` (used as `copy_src` when we snapshot
    ///   the post-Pass-A content; must be created with
    ///   `ImageUsage::TRANSFER_SRC`),
    /// - the clear color used on the *first* pass (linear sRGB).
    ///
    /// Multi-pass schedule when the paint stream contains a
    /// `BackdropSnapshot`:
    ///
    /// 1. Pass A — every paint item before the snapshot, using the
    ///    runner's Clear render pass with `clear_color`.
    /// 2. `copy_image` — target → snapshot.
    /// 3. Pass B — paint items from the snapshot onward, using the
    ///    runner's Load render pass so Pass A's pixels remain
    ///    underneath.
    ///
    /// Without a snapshot, this collapses to a single Clear pass and is
    /// equivalent to the host wrapping [`Self::draw`] in
    /// `begin_render_pass(Clear) … end_render_pass`.
    pub fn render(
        &mut self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        framebuffer: Arc<Framebuffer>,
        target_image: Arc<Image>,
        clear_color: [f32; 4],
    ) {
        if self.core.paint_items.is_empty() {
            // Even with no draws we begin/end the Clear pass so the
            // attachment is cleared — matches `draw()` semantics when
            // the host wraps it in begin_render_pass(Clear).
            self.begin_pass(builder, framebuffer.clone(), Some(clear_color));
            self.end_pass(builder);
            return;
        }

        let split_at = self
            .core
            .paint_items
            .iter()
            .position(|p| matches!(p, PaintItem::BackdropSnapshot));

        if let Some(idx) = split_at {
            self.ensure_snapshot(target_image.clone());
            // Pass A
            self.begin_pass(builder, framebuffer.clone(), Some(clear_color));
            self.set_viewport(builder);
            self.draw_items(builder, &self.core.paint_items[..idx]);
            self.end_pass(builder);
            // Snapshot copy. vulkano's auto command buffer inserts the
            // layout transitions for us (ColorAttachmentOptimal →
            // TransferSrcOptimal on `target_image`, then back before
            // Pass B begins).
            let snapshot = self.snapshot.as_ref().expect("snapshot ensured");
            builder
                .copy_image(CopyImageInfo::images(target_image, snapshot.image.clone()))
                .expect("aetna-vulkano: copy target → snapshot");
            // Pass B
            self.begin_pass(builder, framebuffer, None);
            self.set_viewport(builder);
            // Skip the BackdropSnapshot marker itself — it's a boundary
            // only, not a draw.
            self.draw_items(builder, &self.core.paint_items[idx + 1..]);
            self.end_pass(builder);
        } else {
            self.begin_pass(builder, framebuffer, Some(clear_color));
            self.set_viewport(builder);
            self.draw_items(builder, &self.core.paint_items);
            self.end_pass(builder);
        }
    }

    /// Begin a render pass. `clear_color = Some(_)` uses the Clear
    /// render pass with that color; `None` uses the Load render pass
    /// to preserve previous contents. Both passes are render-pass
    /// compatible with the framebuffer (same attachment format), so
    /// the host's framebuffer (built against `render_pass()`) works
    /// for either by overriding `render_pass` in `RenderPassBeginInfo`.
    fn begin_pass(
        &self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        framebuffer: Arc<Framebuffer>,
        clear_color: Option<[f32; 4]>,
    ) {
        let (render_pass, clear_values) = match clear_color {
            Some(c) => (self.render_pass.clone(), vec![Some(c.into())]),
            // Load render pass declares `load_op: Load` for its sole
            // attachment, so the matching `clear_values` slot must be
            // `None`.
            None => (self.load_render_pass.clone(), vec![None]),
        };
        builder
            .begin_render_pass(
                RenderPassBeginInfo {
                    render_pass,
                    clear_values,
                    ..RenderPassBeginInfo::framebuffer(framebuffer)
                },
                SubpassBeginInfo {
                    contents: SubpassContents::Inline,
                    ..Default::default()
                },
            )
            .expect("aetna-vulkano: begin_render_pass");
    }

    fn end_pass(&self, builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        builder
            .end_render_pass(SubpassEndInfo::default())
            .expect("aetna-vulkano: end_render_pass");
    }

    fn set_viewport(&self, builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>) {
        let (px_w, px_h) = self.core.viewport_px;
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
    }

    /// Walk a slice of `PaintItem`s and record per-run draw commands.
    /// `BackdropSnapshot` is a marker that `render()` splits on; if it
    /// appears here it's silently skipped (no-op).
    fn draw_items(
        &self,
        builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
        items: &[PaintItem],
    ) {
        let (px_w, px_h) = self.core.viewport_px;
        let full = PhysicalScissor {
            x: 0,
            y: 0,
            w: px_w,
            h: px_h,
        };

        for item in items {
            match *item {
                PaintItem::QuadRun(idx) => {
                    let run = &self.core.runs[idx];
                    set_scissor(builder, run.scissor, full);
                    let pipeline = self
                        .pipelines
                        .get(&run.handle)
                        .expect("run handle has no pipeline (bug in prepare)");
                    let is_backdrop_shader = matches!(
                        run.handle,
                        ShaderHandle::Custom(name) if self.backdrop_shaders.contains(name)
                    );
                    builder
                        .bind_pipeline_graphics(pipeline.clone())
                        .expect("bind_pipeline_graphics");
                    // Backdrop pipelines expect set 0 = FrameUniforms
                    // and set 1 = (snapshot view + sampler). All
                    // backdrop shaders share a structurally-identical
                    // set 1 layout so one descriptor set serves them
                    // all. If `render()` wasn't used (no snapshot was
                    // built), the bind is skipped — the pipeline will
                    // sample undefined memory, which is a no-op visual
                    // bug rather than a validation error since every
                    // backdrop shader still has the binding declared.
                    let sets: Vec<DescriptorSetWithOffsets> =
                        if is_backdrop_shader && let Some(bg) = &self.backdrop_descriptor_set {
                            vec![self.frame_descriptor_set.clone().into(), bg.clone().into()]
                        } else {
                            vec![self.frame_descriptor_set.clone().into()]
                        };
                    builder
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            pipeline.layout().clone(),
                            0,
                            sets,
                        )
                        .expect("bind_descriptor_sets");
                    builder
                        .bind_vertex_buffers(0, (self.quad_vbo.clone(), self.instance_buf.clone()))
                        .expect("bind_vertex_buffers");
                    // SAFETY: the pipeline expects 4 vertices (the unit
                    // quad strip) and `run.count` instances starting at
                    // `run.first`; the instance buffer was sized to fit
                    // every packed instance in `prepare()`.
                    unsafe {
                        builder.draw(4, run.count, 0, run.first).expect("draw");
                    }
                }
                PaintItem::BackdropSnapshot => {
                    // Marker only — `render()` splits the slice on
                    // these and never includes one in a draw range.
                    // If we're here via `draw()`, the host opted out
                    // of the multi-pass entry point; the boundary is
                    // a no-op and any backdrop draws after it sample
                    // undefined memory.
                }
                PaintItem::Text(idx) => {
                    let run = self.text_paint.run(idx);
                    set_scissor(builder, run.scissor, full);
                    let text_pipeline = self.text_paint.pipeline();
                    builder
                        .bind_pipeline_graphics(text_pipeline.clone())
                        .expect("bind_pipeline_graphics text");
                    // set 0 = FrameUniforms (shared with rect pipelines);
                    // set 1 = the per-page atlas image + sampler. The
                    // descriptor sets must be passed as a tuple (not a
                    // single set) so vulkano binds both at once.
                    builder
                        .bind_descriptor_sets(
                            PipelineBindPoint::Graphics,
                            text_pipeline.layout().clone(),
                            0,
                            (
                                self.frame_descriptor_set.clone(),
                                self.text_paint.page_descriptor(run.page).clone(),
                            ),
                        )
                        .expect("bind_descriptor_sets text");
                    builder
                        .bind_vertex_buffers(
                            0,
                            (
                                self.quad_vbo.clone(),
                                self.text_paint.instance_buf().clone(),
                            ),
                        )
                        .expect("bind_vertex_buffers text");
                    unsafe {
                        builder.draw(4, run.count, 0, run.first).expect("draw text");
                    }
                }
            }
        }
    }

    /// (Re)allocate the snapshot image to match `target_image`'s
    /// extent + format. Idempotent when the size matches; rebuilds the
    /// `backdrop_descriptor_set` whenever the snapshot is recreated.
    fn ensure_snapshot(&mut self, target_image: Arc<Image>) {
        let want = target_image.extent();
        if let Some(s) = &self.snapshot
            && s.extent == want
        {
            return;
        }
        let image = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: target_image.format(),
                extent: want,
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: backdrop snapshot image");
        let view =
            ImageView::new_default(image.clone()).expect("aetna-vulkano: backdrop snapshot view");
        let layout = self
            .backdrop_set_layout
            .clone()
            .expect("ensure_snapshot called but no backdrop shader registered");
        let set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            layout,
            [
                WriteDescriptorSet::image_view(0, view),
                WriteDescriptorSet::sampler(1, self.backdrop_sampler.clone()),
            ],
            [],
        )
        .expect("aetna-vulkano: backdrop descriptor set");
        self.snapshot = Some(SnapshotImage {
            image,
            extent: want,
        });
        self.backdrop_descriptor_set = Some(set);
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
