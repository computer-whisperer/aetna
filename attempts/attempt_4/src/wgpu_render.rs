//! Wgpu renderer — the production paint path.
//!
//! v0.1 scope: paints `stock::rounded_rect` quads only. Text
//! (`stock::text_sdf`) and focus rings (`stock::focus_ring`) land in the
//! next slice. Shadow is reserved as a uniform but not visually applied
//! yet (v0.2 will widen the instance quad and add a blur term outside
//! the SDF). Custom-shader registration also lands later.
//!
//! # Insert-into-pass integration
//!
//! The renderer does not own the device, queue, swapchain, or render
//! pass. The host creates all of those, configures the surface, begins
//! the encoder + pass, and calls [`UiRenderer::draw`] to record draws
//! into the pass. The host then ends the pass, submits, and presents.
//!
//! ```ignore
//! let mut ui = UiRenderer::new(&device, surface_format);
//! // per frame:
//! ui.prepare(&queue, &mut tree, viewport);
//! pass.set_*(...);
//! ui.draw(&mut pass);
//! ```
//!
//! `prepare` is split from `draw` so all `queue.write_buffer` calls
//! happen before the render pass begins, matching wgpu's expected order.

use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::draw_ops;
use crate::ir::DrawOp;
use crate::layout;
use crate::shader::{ShaderHandle, StockShader, UniformValue};
use crate::tree::{Color, El, Rect};

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
    quad_pipeline: wgpu::RenderPipeline,
    quad_bind_group: wgpu::BindGroup,
    frame_buf: wgpu::Buffer,
    quad_vbo: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    pending: PendingFrame,
}

#[derive(Default)]
struct PendingFrame {
    instances: Vec<QuadInstance>,
    /// Cached so `draw()` knows how many instances to issue without
    /// touching `pending.instances` again (it's already been uploaded).
    instance_count: u32,
}

impl UiRenderer {
    /// Create a renderer for the given target color format. The host
    /// passes its swapchain/render-target format here so the pipeline
    /// is built compatible.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
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

        Self {
            quad_pipeline,
            quad_bind_group,
            frame_buf,
            quad_vbo,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            pending: PendingFrame::default(),
        }
    }

    /// Lay out the tree, resolve to draw ops, and upload per-frame
    /// buffers. Must be called before [`Self::draw`] and outside of any
    /// render pass.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        root: &mut El,
        viewport: Rect,
    ) {
        layout::layout(root, viewport);
        let ops = draw_ops::draw_ops(root);

        self.pending.instances.clear();
        for op in &ops {
            if let Some(inst) = quad_instance(op) {
                self.pending.instances.push(inst);
            }
        }
        self.pending.instance_count = self.pending.instances.len() as u32;

        // Grow instance buffer if needed (next pow2).
        if self.pending.instances.len() > self.instance_capacity {
            let new_cap = self.pending.instances.len().next_power_of_two();
            self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("attempt_4::instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<QuadInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }

        if !self.pending.instances.is_empty() {
            queue.write_buffer(
                &self.instance_buf,
                0,
                bytemuck::cast_slice(&self.pending.instances),
            );
        }

        let frame = FrameUniforms {
            viewport: [viewport.w, viewport.h],
            _pad: [0.0, 0.0],
        };
        queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&frame));
    }

    /// Record draws into the host-managed render pass. Call after
    /// [`Self::prepare`].
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.pending.instance_count == 0 {
            return;
        }
        pass.set_pipeline(&self.quad_pipeline);
        pass.set_bind_group(0, &self.quad_bind_group, &[]);
        pass.set_vertex_buffer(0, self.quad_vbo.slice(..));
        pass.set_vertex_buffer(1, self.instance_buf.slice(..));
        pass.draw(0..4, 0..self.pending.instance_count);
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

    // v0.1: only rounded_rect lands. Other stock shaders + custom
    // shaders are deferred.
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

fn as_color(v: &UniformValue) -> Option<Color> {
    match v { UniformValue::Color(c) => Some(*c), _ => None }
}
fn as_f32(v: &UniformValue) -> Option<f32> {
    match v { UniformValue::F32(f) => Some(*f), _ => None }
}

fn rgba_f32(c: Color) -> [f32; 4] {
    // Tokens are authored in sRGB display space; the surface is typically
    // an *Srgb format which auto-converts linear→sRGB at write. So we
    // convert sRGB→linear here to keep the round-trip neutral and
    // produce display-correct colors. Alpha stays linear.
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
