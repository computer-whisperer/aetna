//! GPU vector-icon rendering for the Vulkano backend.
//!
//! This mirrors `aetna-wgpu::icon`: built-in SVG icons are parsed into
//! Aetna's shared vector IR, tessellated into [`VectorMeshVertex`]
//! triangles, then uploaded into a backend-owned vertex buffer.

use std::ops::Range;
use std::sync::Arc;

use aetna_core::icons::icon_vector_asset;
use aetna_core::paint::{IconRun, PhysicalScissor};
use aetna_core::shader::stock_wgsl;
use aetna_core::tree::{Color, IconName, Rect};
use aetna_core::vector::{
    IconMaterial, VectorMeshOptions, VectorMeshVertex, append_vector_asset_mesh,
};
use vulkano::{
    buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer},
    device::Device,
    format::Format,
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{
        DynamicState, GraphicsPipeline, PipelineShaderStageCreateInfo,
        graphics::{
            GraphicsPipelineCreateInfo,
            color_blend::{AttachmentBlend, ColorBlendAttachmentState, ColorBlendState},
            input_assembly::{InputAssemblyState, PrimitiveTopology},
            multisample::MultisampleState,
            rasterization::RasterizationState,
            subpass::PipelineSubpassType,
            vertex_input::{
                VertexInputAttributeDescription, VertexInputBindingDescription, VertexInputRate,
                VertexInputState,
            },
            viewport::ViewportState,
        },
    },
    render_pass::Subpass,
    shader::{ShaderModule, ShaderModuleCreateInfo},
};

use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::build_shared_pipeline_layout;

const INITIAL_VERTEX_CAPACITY: u64 = 1024;

pub(crate) struct IconPaint {
    vertices: Vec<VectorMeshVertex>,
    runs: Vec<IconRun>,

    flat_pipeline: Arc<GraphicsPipeline>,
    relief_pipeline: Arc<GraphicsPipeline>,
    glass_pipeline: Arc<GraphicsPipeline>,

    vertex_buf: Subbuffer<[VectorMeshVertex]>,
    vertex_capacity: u64,
    memory_alloc: Arc<StandardMemoryAllocator>,

    material: IconMaterial,
}

impl IconPaint {
    pub(crate) fn new(
        device: Arc<Device>,
        memory_alloc: Arc<StandardMemoryAllocator>,
        subpass: Subpass,
    ) -> Self {
        let flat_pipeline = build_vector_pipeline(
            device.clone(),
            subpass.clone(),
            "stock::vector",
            stock_wgsl::VECTOR,
        );
        let relief_pipeline = build_vector_pipeline(
            device.clone(),
            subpass.clone(),
            "stock::vector_relief",
            stock_wgsl::VECTOR_RELIEF,
        );
        let glass_pipeline = build_vector_pipeline(
            device,
            subpass,
            "stock::vector_glass",
            stock_wgsl::VECTOR_GLASS,
        );
        let vertex_buf = create_vector_vertex_buffer(&memory_alloc, INITIAL_VERTEX_CAPACITY);

        Self {
            vertices: Vec::with_capacity(INITIAL_VERTEX_CAPACITY as usize),
            runs: Vec::new(),
            flat_pipeline,
            relief_pipeline,
            glass_pipeline,
            vertex_buf,
            vertex_capacity: INITIAL_VERTEX_CAPACITY,
            memory_alloc,
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
        if mesh_run.count == 0 {
            let start = self.runs.len();
            return start..start;
        }

        let start = self.runs.len();
        self.runs.push(IconRun {
            scissor,
            first,
            count: mesh_run.count,
            material: self.material,
        });
        start..self.runs.len()
    }

    pub(crate) fn flush(&mut self) {
        if (self.vertices.len() as u64) > self.vertex_capacity {
            let new_cap = (self.vertices.len() as u64).next_power_of_two();
            self.vertex_buf = create_vector_vertex_buffer(&self.memory_alloc, new_cap);
            self.vertex_capacity = new_cap;
        }
        if !self.vertices.is_empty() {
            let mut write = self
                .vertex_buf
                .write()
                .expect("aetna-vulkano: icon vertex buffer write");
            write[..self.vertices.len()].copy_from_slice(&self.vertices);
        }
    }

    pub(crate) fn run(&self, index: usize) -> IconRun {
        self.runs[index]
    }

    pub(crate) fn pipeline(&self, material: IconMaterial) -> &Arc<GraphicsPipeline> {
        match material {
            IconMaterial::Flat => &self.flat_pipeline,
            IconMaterial::Relief => &self.relief_pipeline,
            IconMaterial::Glass => &self.glass_pipeline,
        }
    }

    pub(crate) fn vertex_buf(&self) -> &Subbuffer<[VectorMeshVertex]> {
        &self.vertex_buf
    }
}

fn create_vector_vertex_buffer(
    allocator: &Arc<StandardMemoryAllocator>,
    capacity: u64,
) -> Subbuffer<[VectorMeshVertex]> {
    Buffer::new_slice::<VectorMeshVertex>(
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
    .expect("aetna-vulkano: icon vertex buffer alloc")
}

fn vector_vertex_input_state() -> VertexInputState {
    let bind_vertex = VertexInputBindingDescription {
        stride: std::mem::size_of::<VectorMeshVertex>() as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let attr = |offset: u32, format: Format| VertexInputAttributeDescription {
        binding: 0,
        offset,
        format,
        ..Default::default()
    };

    VertexInputState::new()
        .binding(0, bind_vertex)
        // location 0 — logical-pixel position
        .attribute(0, attr(0, Format::R32G32_SFLOAT))
        // location 1 — local SVG/viewBox coordinate
        .attribute(1, attr(8, Format::R32G32_SFLOAT))
        // location 2 — linear RGBA
        .attribute(2, attr(16, Format::R32G32B32A32_SFLOAT))
        // location 3 — material metadata
        .attribute(3, attr(32, Format::R32G32B32A32_SFLOAT))
        // location 4 — analytic-AA fringe normal (logical px; (0,0) interior)
        .attribute(4, attr(48, Format::R32G32_SFLOAT))
}

fn build_vector_pipeline(
    device: Arc<Device>,
    subpass: Subpass,
    name: &str,
    wgsl: &str,
) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv(name, wgsl)
        .unwrap_or_else(|e| panic!("aetna-vulkano: icon WGSL compile for `{name}`: {e}"));
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .unwrap_or_else(|e| panic!("aetna-vulkano: icon ShaderModule::new for `{name}`: {e}"))
    };
    let vs = module
        .entry_point("vs_main")
        .unwrap_or_else(|| panic!("{name}: missing vs_main"));
    let fs = module
        .entry_point("fs_main")
        .unwrap_or_else(|| panic!("{name}: missing fs_main"));

    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = build_shared_pipeline_layout(device.clone(), &stages);

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vector_vertex_input_state()),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            multisample_state: Some(MultisampleState::default()),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState {
                    blend: Some(AttachmentBlend::alpha()),
                    ..Default::default()
                },
            )),
            dynamic_state: [DynamicState::Viewport, DynamicState::Scissor]
                .into_iter()
                .collect(),
            subpass: Some(PipelineSubpassType::BeginRenderPass(subpass)),
            ..GraphicsPipelineCreateInfo::layout(layout)
        },
    )
    .unwrap_or_else(|e| panic!("aetna-vulkano: icon GraphicsPipeline::new for `{name}`: {e:?}"))
}
