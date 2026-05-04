//! Aetna — wgpu backend.
//!
//! v5.0 scope: paints `stock::rounded_rect` quads + `stock::text_sdf`
//! glyph runs (focus indicators ride on each focusable node's own quad
//! via `focus_color`/`focus_width` uniforms — no separate ring pipeline),
//! plus user-registered custom shaders that share the rounded_rect
//! vertex layout. This is the
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
//! glyph atlas page uploads happen before the render pass begins,
//! matching wgpu's expected order.
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

use std::collections::{HashMap, HashSet};
// `web_time::Instant` is API-identical to `std::time::Instant` on
// native and uses `performance.now()` on wasm32 — std's `Instant::now()`
// panics in the browser because there is no monotonic clock there.
use web_time::Instant;

use wgpu::util::DeviceExt;

use aetna_core::event::{KeyChord, KeyModifiers, PointerButton, UiEvent, UiKey};
use aetna_core::paint::{PhysicalScissor, QuadInstance};
use aetna_core::runtime::RunnerCore;
use aetna_core::shader::{ShaderHandle, StockShader, stock_wgsl};
use aetna_core::state::{AnimationMode, UiState};
use aetna_core::tree::{El, Rect};

pub use aetna_core::paint::PaintItem;
pub use aetna_core::runtime::{PrepareResult, PrepareTimings};

use crate::instance::set_scissor;
use crate::pipeline::{FrameUniforms, build_quad_pipeline};
use crate::text::TextPaint;

/// Initial size for the dynamic instance buffer (grows as needed).
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// Wgpu runtime owned by the host. One instance per surface/format.
///
/// All backend-agnostic state — interaction state, paint-stream scratch,
/// per-stage layout/animation hooks — lives in `core: RunnerCore` and
/// is shared with the vulkano backend (v5.4 step 2). The fields below
/// are wgpu-specific resources only.
pub struct Runner {
    target_format: wgpu::TextureFormat,

    // Shared resources.
    pipeline_layout: wgpu::PipelineLayout,
    /// Pipeline layout for `samples_backdrop` custom shaders — adds
    /// `@group(1)` for the snapshot texture + sampler.
    backdrop_pipeline_layout: wgpu::PipelineLayout,
    quad_bind_group: wgpu::BindGroup,
    backdrop_bind_layout: wgpu::BindGroupLayout,
    backdrop_sampler: wgpu::Sampler,
    frame_buf: wgpu::Buffer,
    quad_vbo: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,

    // One pipeline per registered shader (stock + custom).
    pipelines: HashMap<ShaderHandle, wgpu::RenderPipeline>,
    // Custom shader names registered with `samples_backdrop=true`. The
    // paint scheduler queries this to insert pass boundaries before the
    // first backdrop-sampling draw.
    backdrop_shaders: HashSet<&'static str>,

    // stock::text resources — atlas, page textures, glyph instances.
    text_paint: TextPaint,

    /// Lazily-allocated snapshot of the color target, sized to match
    /// the current target on each `render()`. Backdrop-sampling
    /// shaders read this via `@group(1)` after Pass A.
    snapshot: Option<SnapshotTexture>,
    /// Bind group binding the snapshot view + sampler. Rebuilt each
    /// time the snapshot texture is reallocated.
    backdrop_bind_group: Option<wgpu::BindGroup>,

    /// Wall-clock origin for the `time` field in `FrameUniforms`.
    /// `prepare()` writes `(now - start_time).as_secs_f32()`.
    start_time: Instant,

    // Backend-agnostic state shared with aetna-vulkano: interaction
    // state, paint-stream scratch (quad_scratch / runs / paint_items),
    // viewport_px, last_tree, the 13 input plumbing methods.
    core: RunnerCore,
}

struct SnapshotTexture {
    texture: wgpu::Texture,
    extent: (u32, u32),
}

impl Runner {
    /// Create a runner for the given target color format. The host
    /// passes its swapchain/render-target format here so pipelines and
    /// the glyph atlas are built compatible.
    pub fn new(
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        // ---- Shared resources ----
        let frame_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::frame_uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let frame_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aetna_wgpu::frame_bind_layout"),
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
            label: Some("aetna_wgpu::frame_bind_group"),
            layout: &frame_bind_layout,
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
            bind_group_layouts: &[&frame_bind_layout],
            push_constant_ranges: &[],
        });

        // ---- Backdrop sampling resources ----
        //
        // Custom shaders that opt into backdrop sampling (registered
        // via `register_shader_with(..samples_backdrop=true)`) get a
        // pipeline layout with `@group(1)` for the snapshot texture
        // and sampler. The bind group is rebuilt whenever the
        // snapshot is (re)allocated.
        let backdrop_bind_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("aetna_wgpu::backdrop_bind_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let backdrop_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("aetna_wgpu::backdrop_pipeline_layout"),
                bind_group_layouts: &[&frame_bind_layout, &backdrop_bind_layout],
                push_constant_ranges: &[],
            });
        let backdrop_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aetna_wgpu::backdrop_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
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

        // Text pipeline + atlas (replaces glyphon).
        let text_paint = TextPaint::new(device, target_format, &frame_bind_layout);

        let mut core = RunnerCore::new();
        core.quad_scratch = Vec::with_capacity(INITIAL_INSTANCE_CAPACITY);

        Self {
            target_format,
            pipeline_layout,
            backdrop_pipeline_layout,
            quad_bind_group,
            backdrop_bind_layout,
            backdrop_sampler,
            frame_buf,
            quad_vbo,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            pipelines,
            backdrop_shaders: HashSet::new(),
            text_paint,
            snapshot: None,
            backdrop_bind_group: None,
            start_time: Instant::now(),
            core,
        }
    }

    /// Tell the runner the swapchain texture size in physical pixels.
    /// Call this once after `surface.configure(...)` and again on every
    /// `WindowEvent::Resized`. The runner uses this as the canonical
    /// `viewport_px` for scissor math; without it, the value is derived
    /// from `viewport.w * scale_factor`, which can drift by one pixel
    /// when `scale_factor` is fractional and trip wgpu's
    /// `set_scissor_rect` validation.
    pub fn set_surface_size(&mut self, width: u32, height: u32) {
        self.core.set_surface_size(width, height);
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
        self.register_shader_with(device, name, wgsl, false);
    }

    /// Register a custom shader, with an opt-in flag for backdrop
    /// sampling. When `samples_backdrop` is true, the renderer schedules
    /// the shader's draws into Pass B (after a snapshot of Pass A's
    /// rendered content) and binds the snapshot texture as
    /// `@group(2) binding=0` (`backdrop_tex`) plus a sampler at
    /// `binding=1` (`backdrop_smp`). See `SHADER_VISION.md`
    /// §"Backdrop sampling architecture".
    ///
    /// v0.7 caps backdrop depth at 1: glass-on-glass shows the same
    /// underlying content, not a second snapshot of the first glass
    /// composited.
    pub fn register_shader_with(
        &mut self,
        device: &wgpu::Device,
        name: &'static str,
        wgsl: &str,
        samples_backdrop: bool,
    ) {
        let label = format!("custom::{name}");
        let layout = if samples_backdrop {
            &self.backdrop_pipeline_layout
        } else {
            &self.pipeline_layout
        };
        let pipeline = build_quad_pipeline(device, layout, self.target_format, &label, wgsl);
        self.pipelines.insert(ShaderHandle::Custom(name), pipeline);
        if samples_backdrop {
            self.backdrop_shaders.insert(name);
        } else {
            self.backdrop_shaders.remove(name);
        }
    }

    /// Borrow the internal [`UiState`] — primarily for headless fixtures
    /// that want to look up a node's rect after `prepare` (e.g., to
    /// simulate a pointer at a specific button's center).
    pub fn ui_state(&self) -> &UiState {
        self.core.ui_state()
    }

    /// One-line diagnostic snapshot of interactive state — passes through
    /// to [`UiState::debug_summary`]. Intended for per-frame logging
    /// (e.g., `console.log` from the wasm host while debugging hover /
    /// animation glitches).
    pub fn debug_summary(&self) -> String {
        self.core.debug_summary()
    }

    /// Return the most recently laid-out rectangle for a keyed node.
    ///
    /// Call after [`Self::prepare`]. This is the host-composition hook:
    /// reserve a keyed Aetna element in the UI tree, ask for its rect
    /// here, then record host-owned rendering into that region using the
    /// same encoder / render flow that surrounds Aetna's pass.
    pub fn rect_of_key(&self, key: &str) -> Option<Rect> {
        self.core.rect_of_key(key)
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
        let mut timings = PrepareTimings::default();

        // Layout + state apply + animation tick + draw_ops resolution.
        // Writes timings.layout + timings.draw_ops.
        let (ops, needs_redraw) =
            self.core
                .prepare_layout(root, viewport, scale_factor, &mut timings);

        // Paint stream: pack quads, record text, preserve z-order. The
        // closure is the wgpu-specific "is this shader registered?"
        // query (different pipeline types per backend prevent moving the
        // check itself into core).
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

        // GPU upload — wgpu-specific. Resize the instance buffer if
        // needed, then write quad_scratch + frame uniforms + flush text
        // atlas dirty regions.
        let t_paint_end = Instant::now();
        if self.core.quad_scratch.len() > self.instance_capacity {
            let new_cap = self.core.quad_scratch.len().next_power_of_two();
            self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aetna_wgpu::instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<QuadInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }
        if !self.core.quad_scratch.is_empty() {
            queue.write_buffer(
                &self.instance_buf,
                0,
                bytemuck::cast_slice(&self.core.quad_scratch),
            );
        }
        self.text_paint.flush(device, queue);
        let time = (Instant::now() - self.start_time).as_secs_f32();
        let frame = FrameUniforms {
            viewport: [viewport.w, viewport.h],
            time,
            _pad: 0.0,
        };
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&frame));
        timings.gpu_upload = Instant::now() - t_paint_end;

        // Snapshot the laid-out tree for next-frame hit-testing.
        self.core.snapshot(root, &mut timings);

        PrepareResult {
            needs_redraw,
            timings,
        }
    }

    // ---- v0.2 input plumbing ----
    //
    // The host (winit-side) calls these from its event loop.
    // Coordinates are **logical pixels** — divide winit's physical
    // PhysicalPosition by the window scale factor before handing them in.

    /// Update pointer position and recompute the hovered key.
    /// Returns the new hovered key, if any (host can use it for cursor
    /// styling or to decide whether to call `request_redraw`).
    /// Pointer moved to `(x, y)` (logical px). Returns a `Drag` event
    /// when the primary button is held; the host should dispatch it
    /// via `App::on_event`. The hovered node is updated on
    /// `ui_state().hovered` regardless.
    pub fn pointer_moved(&mut self, x: f32, y: f32) -> Option<UiEvent> {
        self.core.pointer_moved(x, y)
    }

    /// Pointer left the window — clear hover/press.
    pub fn pointer_left(&mut self) {
        self.core.pointer_left();
    }

    /// Mouse button down at `(x, y)` (logical px) for the given
    /// `button`. For `Primary`, records the pressed key for press-
    /// visual feedback and updates focus; for `Secondary` / `Middle`,
    /// records on a side channel. The actual click event fires on the
    /// matching `pointer_up`.
    pub fn pointer_down(&mut self, x: f32, y: f32, button: PointerButton) {
        self.core.pointer_down(x, y, button);
    }

    /// Mouse button up at `(x, y)` for the given `button`. Returns
    /// the events the host should dispatch in order: for `Primary`,
    /// always a `PointerUp` (when there was a corresponding down)
    /// followed by an optional `Click` (when the up landed on the
    /// down's node). For `Secondary` / `Middle`, an optional
    /// `SecondaryClick` / `MiddleClick` on the same-node match.
    pub fn pointer_up(&mut self, x: f32, y: f32, button: PointerButton) -> Vec<UiEvent> {
        self.core.pointer_up(x, y, button)
    }

    pub fn key_down(
        &mut self,
        key: UiKey,
        modifiers: KeyModifiers,
        repeat: bool,
    ) -> Option<UiEvent> {
        self.core.key_down(key, modifiers, repeat)
    }

    /// Forward an OS-composed text-input string (winit's keyboard event
    /// `.text` field, or an `Ime::Commit`) to the focused element as a
    /// `TextInput` event.
    pub fn text_input(&mut self, text: String) -> Option<UiEvent> {
        self.core.text_input(text)
    }

    /// Replace the hotkey registry. Call once per frame, after `app.build()`,
    /// passing `app.hotkeys()` so chords stay in sync with state.
    pub fn set_hotkeys(&mut self, hotkeys: Vec<(KeyChord, String)>) {
        self.core.set_hotkeys(hotkeys);
    }

    /// Switch animation pacing. Default is [`AnimationMode::Live`].
    /// Headless render binaries should call this with
    /// [`AnimationMode::Settled`] so a single-frame snapshot reflects
    /// the post-animation visual without depending on integrator timing.
    pub fn set_animation_mode(&mut self, mode: AnimationMode) {
        self.core.set_animation_mode(mode);
    }

    /// Apply a wheel delta in **logical** pixels at `(x, y)`. Routes to
    /// the deepest scrollable container under the cursor in the last
    /// laid-out tree. Returns `true` if the event landed on a scrollable
    /// (host should `request_redraw` so the next frame applies the new
    /// offset).
    pub fn pointer_wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        self.core.pointer_wheel(x, y, dy)
    }

    /// Record draws into the host-managed render pass. Call after
    /// [`Self::prepare`]. Paint order follows the draw-op stream.
    ///
    /// **No backdrop sampling.** This entry point cannot honor pass
    /// boundaries (the host owns the pass lifetime), so any
    /// `BackdropSnapshot` items in the paint stream are no-ops and any
    /// shader bound with `samples_backdrop=true` reads an undefined
    /// backdrop binding. Use [`Self::render`] for backdrop-aware
    /// rendering.
    pub fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        self.draw_items(pass, &self.core.paint_items);
    }

    /// Record draws into a host-supplied encoder, owning pass
    /// lifetimes ourselves so backdrop-sampling shaders can sample a
    /// snapshot of Pass A's content.
    ///
    /// The host hands us:
    /// - the encoder (we record into it),
    /// - the color target's `wgpu::Texture` (used as `copy_src` when
    ///   we snapshot it; must include `COPY_SRC` in its usage flags),
    /// - the corresponding `wgpu::TextureView` (we attach it to every
    ///   render pass we begin), and
    /// - the `LoadOp` to use on the *first* pass — `Clear(color)` to
    ///   clear behind us, `Load` to composite onto whatever was
    ///   already in the target.
    ///
    /// Multi-pass schedule when the paint stream contains a
    /// `BackdropSnapshot`:
    ///
    /// 1. Pass A — every paint item before the snapshot, with the
    ///    caller-supplied `LoadOp`.
    /// 2. `copy_texture_to_texture` — target → snapshot.
    /// 3. Pass B — paint items from the snapshot onward, with
    ///    `LoadOp::Load` so Pass A's pixels remain underneath.
    ///
    /// Without a snapshot, this collapses to a single pass and is
    /// equivalent to [`Self::draw`] called inside a host-managed
    /// pass with the same `LoadOp`.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target_tex: &wgpu::Texture,
        target_view: &wgpu::TextureView,
        load_op: wgpu::LoadOp<wgpu::Color>,
    ) {
        // Locate the (at most one) snapshot boundary.
        let split_at = self
            .core
            .paint_items
            .iter()
            .position(|p| matches!(p, PaintItem::BackdropSnapshot));

        if let Some(idx) = split_at {
            self.ensure_snapshot(device, target_tex);
            // Pass A
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("aetna_wgpu::pass_a"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: load_op,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                self.draw_items(&mut pass, &self.core.paint_items[..idx]);
            }
            // Snapshot copy. Target must support COPY_SRC; snapshot
            // texture (created in `ensure_snapshot`) supports COPY_DST
            // + TEXTURE_BINDING.
            let snapshot = self.snapshot.as_ref().expect("snapshot ensured");
            encoder.copy_texture_to_texture(
                wgpu::ImageCopyTexture {
                    texture: target_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::ImageCopyTexture {
                    texture: &snapshot.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: snapshot.extent.0,
                    height: snapshot.extent.1,
                    depth_or_array_layers: 1,
                },
            );
            // Pass B
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("aetna_wgpu::pass_b"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                // Skip the snapshot item itself; it's a marker, not a draw.
                self.draw_items(&mut pass, &self.core.paint_items[idx + 1..]);
            }
        } else {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("aetna_wgpu::pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: load_op,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.draw_items(&mut pass, &self.core.paint_items);
        }
    }

    /// (Re)allocate the snapshot texture to match `target_tex`'s
    /// extent + format. Idempotent when the size matches; rebuilds the
    /// `backdrop_bind_group` whenever the snapshot is recreated.
    fn ensure_snapshot(&mut self, device: &wgpu::Device, target_tex: &wgpu::Texture) {
        let extent = target_tex.size();
        let want = (extent.width, extent.height);
        if let Some(s) = &self.snapshot
            && s.extent == want
        {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aetna_wgpu::backdrop_snapshot"),
            size: wgpu::Extent3d {
                width: want.0,
                height: want.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.target_format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aetna_wgpu::backdrop_bind_group"),
            layout: &self.backdrop_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.backdrop_sampler),
                },
            ],
        });
        self.snapshot = Some(SnapshotTexture {
            texture,
            extent: want,
        });
        self.backdrop_bind_group = Some(bind_group);
    }

    /// Walk a slice of `PaintItem`s into the given pass. Helper shared
    /// by [`Self::draw`] and [`Self::render`]. `BackdropSnapshot`
    /// items are no-ops here; `render()` handles them by splitting
    /// the slice before passing to this helper.
    fn draw_items<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        items: &'pass [PaintItem],
    ) {
        let full = PhysicalScissor {
            x: 0,
            y: 0,
            w: self.core.viewport_px.0,
            h: self.core.viewport_px.1,
        };
        for item in items {
            match *item {
                PaintItem::QuadRun(index) => {
                    let run = &self.core.runs[index];
                    set_scissor(pass, run.scissor, full);
                    pass.set_bind_group(0, &self.quad_bind_group, &[]);
                    let is_backdrop_shader = matches!(
                        run.handle,
                        ShaderHandle::Custom(name) if self.backdrop_shaders.contains(name)
                    );
                    if is_backdrop_shader && let Some(bg) = &self.backdrop_bind_group {
                        pass.set_bind_group(1, bg, &[]);
                    }
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
                    let run = self.text_paint.run(index);
                    set_scissor(pass, run.scissor, full);
                    pass.set_pipeline(self.text_paint.pipeline());
                    pass.set_bind_group(0, &self.quad_bind_group, &[]);
                    pass.set_bind_group(1, self.text_paint.page_bind_group(run.page), &[]);
                    pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
                    pass.set_vertex_buffer(1, self.text_paint.instance_buf().slice(..));
                    pass.draw(0..4, run.first..run.first + run.count);
                }
                PaintItem::BackdropSnapshot => {
                    // Marker only — `render()` splits the slice on
                    // these and never includes one in a draw range.
                }
            }
        }
    }
}
