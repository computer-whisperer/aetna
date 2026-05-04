//! GPU vector-icon rendering.
//!
//! Icons are flattened by `aetna-core` into 24x24 line strokes. This
//! module scales those strokes into the laid-out icon rect and renders
//! each segment with a small SDF shader. Keeping the icon vocabulary as
//! stroke instances gives future theme shaders a concrete GPU hook for
//! non-flat treatments without changing widget APIs.

use std::borrow::Cow;
use std::ops::Range;

use aetna_core::icons::icon_strokes;
use aetna_core::paint::{IconInstance, IconRun, PhysicalScissor, rgba_f32};
use aetna_core::shader::stock_wgsl;
use aetna_core::tree::{Color, IconName, Rect};

const INITIAL_INSTANCE_CAPACITY: usize = 256;

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x4, // rect
    2 => Float32x4, // line
    3 => Float32x4, // color
    4 => Float32x4, // params
];

pub(crate) struct IconPaint {
    instances: Vec<IconInstance>,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    runs: Vec<IconRun>,
    pipeline: wgpu::RenderPipeline,
}

impl IconPaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        frame_bind_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::icon::instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<IconInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aetna_wgpu::icon::pipeline_layout"),
            bind_group_layouts: &[frame_bind_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stock::icon_line"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::ICON_LINE)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stock::icon_line"),
            layout: Some(&pipeline_layout),
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
                        array_stride: std::mem::size_of::<IconInstance>() as u64,
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
            instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            runs: Vec::new(),
            pipeline,
        }
    }

    pub(crate) fn frame_begin(&mut self) {
        self.instances.clear();
        self.runs.clear();
    }

    pub(crate) fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        name: IconName,
        color: Color,
        stroke_width: f32,
    ) -> Range<usize> {
        let strokes = icon_strokes(name);
        if strokes.is_empty() || rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }

        let first = self.instances.len() as u32;
        let sx = rect.w / 24.0;
        let sy = rect.h / 24.0;
        let stroke_px = (stroke_width * (sx + sy) * 0.5).max(0.75);
        let color = rgba_f32(color);
        let outset = stroke_px * 0.5 + 2.0;

        for stroke in strokes {
            let x0 = rect.x + stroke.from[0] * sx;
            let y0 = rect.y + stroke.from[1] * sy;
            let x1 = rect.x + stroke.to[0] * sx;
            let y1 = rect.y + stroke.to[1] * sy;
            let min_x = x0.min(x1) - outset;
            let min_y = y0.min(y1) - outset;
            let max_x = x0.max(x1) + outset;
            let max_y = y0.max(y1) + outset;
            self.instances.push(IconInstance {
                rect: [min_x, min_y, max_x - min_x, max_y - min_y],
                line: [x0, y0, x1, y1],
                color,
                params: [stroke_px, 0.0, 0.0, 0.0],
            });
        }

        let count = self.instances.len() as u32 - first;
        let start = self.runs.len();
        self.runs.push(IconRun {
            scissor,
            first,
            count,
        });
        start..self.runs.len()
    }

    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.instances.len() > self.instance_capacity {
            let new_cap = self.instances.len().next_power_of_two();
            self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aetna_wgpu::icon::instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<IconInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }
        if !self.instances.is_empty() {
            queue.write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(&self.instances));
        }
    }

    pub(crate) fn run(&self, index: usize) -> IconRun {
        self.runs[index]
    }

    pub(crate) fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }

    pub(crate) fn instance_buf(&self) -> &wgpu::Buffer {
        &self.instance_buf
    }
}
