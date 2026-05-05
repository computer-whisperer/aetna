//! GPU vector-icon rendering.
//!
//! Built-in icons are parsed from SVG through `usvg` into Aetna's
//! backend-agnostic vector IR, tessellated into Aetna's shared vector
//! mesh, then uploaded by this backend.

use std::borrow::Cow;
use std::ops::Range;

use aetna_core::icons::icon_vector_asset;
use aetna_core::paint::{IconRun, PhysicalScissor};
use aetna_core::shader::stock_wgsl;
use aetna_core::tree::{Color, IconName, Rect};
use aetna_core::vector::{
    IconMaterial, VectorMeshOptions, VectorMeshVertex, append_vector_asset_mesh,
};

const INITIAL_VERTEX_CAPACITY: usize = 1024;

const VERTEX_ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
    0 => Float32x2, // position in logical px
    1 => Float32x2, // local SVG/viewBox coordinate
    2 => Float32x4, // linear rgba
    3 => Float32x4, // vector metadata
    4 => Float32x2, // aa (analytic-AA fringe normal in logical px; (0,0) for solid verts)
];

pub(crate) struct IconPaint {
    vertices: Vec<VectorMeshVertex>,
    vertex_buf: wgpu::Buffer,
    vertex_capacity: usize,
    runs: Vec<IconRun>,
    flat_pipeline: wgpu::RenderPipeline,
    relief_pipeline: wgpu::RenderPipeline,
    glass_pipeline: wgpu::RenderPipeline,
    material: IconMaterial,
}

impl IconPaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        frame_bind_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::icon::vertex_buf"),
            size: (INITIAL_VERTEX_CAPACITY * std::mem::size_of::<VectorMeshVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aetna_wgpu::icon::pipeline_layout"),
            bind_group_layouts: &[frame_bind_layout],
            push_constant_ranges: &[],
        });
        let flat_pipeline = build_vector_pipeline(
            device,
            &pipeline_layout,
            target_format,
            "stock::vector",
            stock_wgsl::VECTOR,
        );
        let relief_pipeline = build_vector_pipeline(
            device,
            &pipeline_layout,
            target_format,
            "stock::vector_relief",
            stock_wgsl::VECTOR_RELIEF,
        );
        let glass_pipeline = build_vector_pipeline(
            device,
            &pipeline_layout,
            target_format,
            "stock::vector_glass",
            stock_wgsl::VECTOR_GLASS,
        );

        Self {
            vertices: Vec::with_capacity(INITIAL_VERTEX_CAPACITY),
            vertex_buf,
            vertex_capacity: INITIAL_VERTEX_CAPACITY,
            runs: Vec::new(),
            flat_pipeline,
            relief_pipeline,
            glass_pipeline,
            material: IconMaterial::Flat,
        }
    }

    pub(crate) fn set_material(&mut self, material: IconMaterial) {
        self.material = material;
    }

    pub(crate) fn material(&self) -> IconMaterial {
        self.material
    }

    pub(crate) fn frame_begin(&mut self) {
        self.vertices.clear();
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
        let asset = icon_vector_asset(name);
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }

        let first = self.vertices.len() as u32;
        let mesh_run = append_vector_asset_mesh(
            asset,
            VectorMeshOptions::icon(rect, color, stroke_width),
            &mut self.vertices,
        );
        let count = mesh_run.count;
        if count == 0 {
            let start = self.runs.len();
            return start..start;
        }

        let start = self.runs.len();
        self.runs.push(IconRun {
            scissor,
            first,
            count,
            material: self.material,
        });
        start..self.runs.len()
    }

    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.vertices.len() > self.vertex_capacity {
            let new_cap = self.vertices.len().next_power_of_two();
            self.vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aetna_wgpu::icon::vertex_buf (resized)"),
                size: (new_cap * std::mem::size_of::<VectorMeshVertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.vertex_capacity = new_cap;
        }
        if !self.vertices.is_empty() {
            queue.write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&self.vertices));
        }
    }

    pub(crate) fn run(&self, index: usize) -> IconRun {
        self.runs[index]
    }

    pub(crate) fn pipeline(&self, material: IconMaterial) -> &wgpu::RenderPipeline {
        match material {
            IconMaterial::Flat => &self.flat_pipeline,
            IconMaterial::Relief => &self.relief_pipeline,
            IconMaterial::Glass => &self.glass_pipeline,
        }
    }

    pub(crate) fn vertex_buf(&self) -> &wgpu::Buffer {
        &self.vertex_buf
    }
}

fn build_vector_pipeline(
    device: &wgpu::Device,
    pipeline_layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    label: &'static str,
    wgsl: &'static str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<VectorMeshVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VERTEX_ATTRS,
            }],
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
            topology: wgpu::PrimitiveTopology::TriangleList,
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
