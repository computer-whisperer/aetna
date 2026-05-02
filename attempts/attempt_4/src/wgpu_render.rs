//! Wgpu renderer — the production paint path.
//!
//! v0.1 scope: paints `stock::rounded_rect` quads + `stock::text_sdf`
//! glyph runs. Focus rings and shadow are reserved (focus_ring lights up
//! easily via the same instance-buffer pattern; shadow needs SDF
//! widening — both follow). Custom-shader registration also lands later.
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
//! // per frame:
//! ui.prepare(&device, &queue, &mut tree, viewport);
//! ui.draw(&mut pass);
//! ```
//!
//! `prepare` is split from `draw` so all `queue.write_buffer` calls and
//! glyphon atlas updates happen before the render pass begins, matching
//! wgpu's expected order.
//!
//! # Text rendering
//!
//! `stock::text_sdf` is implemented with [glyphon](https://github.com/grovesNL/glyphon),
//! which wraps cosmic-text for shaping/layout and rasterizes glyphs into
//! a wgpu texture atlas. We rebuild glyphon `Buffer`s per frame from
//! [`DrawOp::GlyphRun`]s — fine for v0.1 (no caching), and matches our
//! current "tree is rebuilt each frame" loop.
//!
//! Paint order: rounded_rect quads first, then text on top.

use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};
use glyphon::cosmic_text::Align;
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphColor, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
};
use wgpu::util::DeviceExt;

use crate::draw_ops;
use crate::ir::{DrawOp, TextAnchor};
use crate::layout;
use crate::shader::{ShaderHandle, StockShader, UniformValue};
use crate::tree::{Color, El, FontWeight, Rect};

/// Per-frame globals bound at @group(0).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct FrameUniforms {
    viewport: [f32; 2],
    _pad: [f32; 2],
}

/// One instance of `stock::rounded_rect`. Layout matches the wgsl
/// `InstanceInput` struct in `shaders/rounded_rect.wgsl`.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
struct QuadInstance {
    rect: [f32; 4],
    fill: [f32; 4],
    stroke: [f32; 4],
    /// x = stroke_width, y = radius, z = shadow, w = _pad
    params: [f32; 4],
}

const ROUNDED_RECT_WGSL: &str = include_str!("../shaders/rounded_rect.wgsl");

/// Initial size for the dynamic instance buffer (grows as needed).
const INITIAL_INSTANCE_CAPACITY: usize = 256;

/// Renderer state owned by the host. One instance per surface/format.
pub struct UiRenderer {
    // Quad pipeline (stock::rounded_rect).
    quad_pipeline: wgpu::RenderPipeline,
    quad_bind_group: wgpu::BindGroup,
    frame_buf: wgpu::Buffer,
    quad_vbo: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    quad_count: u32,
    quad_scratch: Vec<QuadInstance>,

    // Text pipeline (stock::text_sdf, via glyphon).
    font_system: FontSystem,
    swash_cache: SwashCache,
    glyph_atlas: TextAtlas,
    glyph_viewport: Viewport,
    text_renderer: TextRenderer,
    text_buffers: Vec<Buffer>,
    text_metas: Vec<TextMeta>,
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
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, target_format: wgpu::TextureFormat) -> Self {
        // ---- Quad pipeline ----
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
            contents: bytemuck::cast_slice::<f32, u8>(&[
                0.0, 0.0,
                1.0, 0.0,
                0.0, 1.0,
                1.0, 1.0,
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("attempt_4::instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<QuadInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stock::rounded_rect"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(ROUNDED_RECT_WGSL)),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("attempt_4::quad_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let quad_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("attempt_4::quad_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[
                    // vertex buffer 0: corner uv (4 verts of the unit quad)
                    wgpu::VertexBufferLayout {
                        array_stride: (2 * std::mem::size_of::<f32>()) as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                        }],
                    },
                    // vertex buffer 1: per-instance QuadInstance
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
        });

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
        let mut glyph_atlas = TextAtlas::new(device, queue, &glyph_cache, target_format);
        let text_renderer = TextRenderer::new(
            &mut glyph_atlas,
            device,
            wgpu::MultisampleState::default(),
            None,
        );

        Self {
            quad_pipeline,
            quad_bind_group,
            frame_buf,
            quad_vbo,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            quad_count: 0,
            quad_scratch: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),

            font_system,
            swash_cache,
            glyph_atlas,
            glyph_viewport,
            text_renderer,
            text_buffers: Vec::new(),
            text_metas: Vec::new(),
        }
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
        layout::layout(root, viewport);
        let ops = draw_ops::draw_ops(root);

        // ---- Quads ----
        self.quad_scratch.clear();
        for op in &ops {
            if let Some(inst) = quad_instance(op) {
                self.quad_scratch.push(inst);
            }
        }
        self.quad_count = self.quad_scratch.len() as u32;

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

        // ---- Text ----
        //
        // Canonical HiDPI: rasterize glyphs at physical DPI. Pre-multiply
        // every text quantity (size, line height, position, bounds) by
        // scale_factor and set TextArea.scale = 1.0. Glyphon then sees
        // physical-coord positions and rasterizes at physical glyph size,
        // producing crisp text. The glyphon Viewport is the physical
        // framebuffer resolution.
        self.text_buffers.clear();
        self.text_metas.clear();
        for op in &ops {
            if let DrawOp::GlyphRun {
                rect, color, text, size, weight, mono, anchor, ..
            } = op {
                build_text_buffer(
                    &mut self.font_system,
                    *rect,
                    text,
                    *size,
                    *weight,
                    *mono,
                    *anchor,
                    *color,
                    scale_factor,
                    &mut self.text_buffers,
                    &mut self.text_metas,
                );
            }
        }

        self.glyph_viewport.update(
            queue,
            Resolution {
                width: (viewport.w * scale_factor) as u32,
                height: (viewport.h * scale_factor) as u32,
            },
        );

        // Split borrow so glyphon::prepare can take &mut on font_system,
        // glyph_atlas, etc. simultaneously with &TextArea borrowed from
        // self.text_buffers.
        let Self {
            text_buffers,
            text_metas,
            text_renderer,
            glyph_atlas,
            glyph_viewport,
            font_system,
            swash_cache,
            ..
        } = self;
        let text_areas: Vec<TextArea> = text_buffers
            .iter()
            .zip(text_metas.iter())
            .map(|(buffer, meta)| TextArea {
                buffer,
                left: meta.left,
                top: meta.top,
                // Positions/sizes are pre-multiplied to physical pixels
                // already; tell glyphon not to scale further.
                scale: 1.0,
                bounds: meta.bounds,
                default_color: meta.color,
                custom_glyphs: &[],
            })
            .collect();
        text_renderer
            .prepare(
                device,
                queue,
                font_system,
                glyph_atlas,
                glyph_viewport,
                text_areas,
                swash_cache,
            )
            .expect("glyphon prepare");
    }

    /// Record draws into the host-managed render pass. Call after
    /// [`Self::prepare`]. Paint order: rounded_rect quads, then text.
    pub fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>) {
        if self.quad_count > 0 {
            pass.set_pipeline(&self.quad_pipeline);
            pass.set_bind_group(0, &self.quad_bind_group, &[]);
            pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
            pass.set_vertex_buffer(1, self.instance_buf.slice(..));
            pass.draw(0..4, 0..self.quad_count);
        }
        if !self.text_buffers.is_empty() {
            self.text_renderer
                .render(&self.glyph_atlas, &self.glyph_viewport, pass)
                .expect("glyphon render");
        }
    }
}

/// Per-instance vertex attributes — must match
/// `shaders/rounded_rect.wgsl`'s `InstanceInput`.
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect
    2 => Float32x4,  // fill
    3 => Float32x4,  // stroke
    4 => Float32x4,  // params (stroke_width, radius, shadow, _pad)
];

/// Resolve a single [`DrawOp::Quad`] into a `QuadInstance`. Returns
/// `None` for ops the v0.1 renderer doesn't yet handle (text, custom
/// shaders, focus rings). Those are emitted in subsequent slices.
fn quad_instance(op: &DrawOp) -> Option<QuadInstance> {
    let DrawOp::Quad { rect, shader, uniforms, .. } = op else { return None };

    if !matches!(shader, ShaderHandle::Stock(StockShader::RoundedRect)) {
        return None;
    }

    let fill = uniforms.get("fill").and_then(as_color).map(rgba_f32).unwrap_or([0.0; 4]);
    let stroke = uniforms.get("stroke").and_then(as_color).map(rgba_f32).unwrap_or([0.0; 4]);
    let stroke_width = uniforms.get("stroke_width").and_then(as_f32).unwrap_or(0.0);
    let radius = uniforms.get("radius").and_then(as_f32).unwrap_or(0.0);
    let shadow = uniforms.get("shadow").and_then(as_f32).unwrap_or(0.0);

    Some(QuadInstance {
        rect: [rect.x, rect.y, rect.w, rect.h],
        fill,
        stroke,
        params: [stroke_width, radius, shadow, 0.0],
    })
}

#[allow(clippy::too_many_arguments)]
fn build_text_buffer(
    font_system: &mut FontSystem,
    rect: Rect,
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    anchor: TextAnchor,
    color: Color,
    scale: f32,
    out_buffers: &mut Vec<Buffer>,
    out_metas: &mut Vec<TextMeta>,
) {
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
    let buffer_width = match anchor {
        TextAnchor::Start => None,
        TextAnchor::Middle | TextAnchor::End => Some(rect.w * scale),
    };
    buffer.set_size(
        font_system,
        buffer_width,
        Some((rect.h * scale).max(physical_line_height)),
    );

    // Use bundled Roboto for sans-serif so typography is consistent
    // regardless of what fonts the host has installed. fontdb resolves
    // Name("Roboto") to whichever weight matches the request.
    let family = if mono { Family::Monospace } else { Family::Name("Roboto") };
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

    // Vertically center one line of text inside the rect (in logical),
    // then scale to physical.
    let top_logical = rect.y + ((rect.h - size * 1.4) * 0.5).max(0.0);
    let top = top_logical * scale;
    let left = rect.x * scale;

    // v0.1: don't tightly clip text to its rect bounds — the layout's
    // intrinsic-width estimator is approximate and can be a few pixels
    // narrower than cosmic-text's actual run width. Real overflow shows
    // up in the lint pass and as visible overlap, not silent glyph
    // chopping.
    out_buffers.push(buffer);
    out_metas.push(TextMeta {
        left,
        top,
        color: glyphon_color(color),
        bounds: TextBounds {
            left: (rect.x * scale).floor() as i32 - 2,
            top: (rect.y * scale).floor() as i32 - 2,
            right: i32::MAX / 2,
            bottom: i32::MAX / 2,
        },
    });
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
    match v { UniformValue::Color(c) => Some(*c), _ => None }
}
fn as_f32(v: &UniformValue) -> Option<f32> {
    match v { UniformValue::F32(f) => Some(*f), _ => None }
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
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}
