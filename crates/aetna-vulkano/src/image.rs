//! GPU raster-image rendering for the Vulkano backend.
//!
//! Mirrors `aetna-wgpu/src/image.rs`: one pipeline (stock::image)
//! plus a per-image GPU texture cache keyed on
//! [`aetna_core::image::Image::content_hash`]. Two equal `Image`s
//! share a slot; cache entries unreferenced for one frame are dropped
//! at flush so transient images don't pin GPU memory.
//!
//! Per-frame lifecycle:
//! 1. `frame_begin()` clears the per-frame instance + run buffers.
//! 2. `record(...)` is called once per `DrawOp::Image`. The first
//!    call for a content hash uploads the texture (synchronous fence
//!    wait — same shape `IconPaint` uses for its MSDF atlas pages);
//!    subsequent calls reuse the cached descriptor set.
//! 3. `flush()` writes the instance buffer and drops cache entries
//!    that weren't touched this frame.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use aetna_core::image::Image as RasterImage;
use aetna_core::paint::{PhysicalScissor, rgba_f32};
use aetna_core::shader::stock_wgsl;
use aetna_core::tree::{Color, Rect};
use bytemuck::{Pod, Zeroable};
use smallvec::smallvec;
use vulkano::{
    buffer::{Buffer, BufferCreateInfo, BufferUsage, Subbuffer},
    command_buffer::{
        AutoCommandBufferBuilder, BufferImageCopy, CommandBufferUsage, CopyBufferToImageInfo,
        allocator::StandardCommandBufferAllocator,
    },
    descriptor_set::{
        DescriptorSet, WriteDescriptorSet, allocator::StandardDescriptorSetAllocator,
    },
    device::{Device, Queue},
    format::Format,
    image::{
        Image as VkImage, ImageAspects, ImageCreateInfo, ImageSubresourceLayers, ImageType,
        ImageUsage,
        sampler::{Filter, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode},
        view::ImageView,
    },
    memory::allocator::{AllocationCreateInfo, MemoryTypeFilter, StandardMemoryAllocator},
    pipeline::{
        DynamicState, GraphicsPipeline, Pipeline, PipelineShaderStageCreateInfo,
        graphics::{
            GraphicsPipelineCreateInfo,
            color_blend::{
                AttachmentBlend, BlendFactor, BlendOp, ColorBlendAttachmentState, ColorBlendState,
            },
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
    sync::{self, GpuFuture},
};

use crate::naga_compile::wgsl_to_spirv;
use crate::pipeline::build_shared_pipeline_layout;

const INITIAL_INSTANCE_CAPACITY: u64 = 32;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct ImageInstance {
    rect: [f32; 4],
    tint: [f32; 4],
    params: [f32; 4],
    uv: [f32; 4],
}

pub(crate) struct ImageRun {
    pub texture_idx: usize,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

struct CachedTexture {
    descriptor_set: Arc<DescriptorSet>,
    last_used_frame: u64,
}

pub(crate) struct ImagePaint {
    instances: Vec<ImageInstance>,
    instance_buf: Subbuffer<[ImageInstance]>,
    instance_capacity: u64,
    runs: Vec<ImageRun>,

    pipeline: Arc<GraphicsPipeline>,
    sampler: Arc<Sampler>,

    cache: HashMap<u64, CachedTexture>,
    /// Per-frame: index → content hash. Kept in record order so
    /// `ImageRun::texture_idx` can name a stable slot for the dispatch
    /// loop. Rebuilt on `frame_begin`.
    bind_group_lookup: Vec<u64>,
    frame_counter: u64,

    memory_alloc: Arc<StandardMemoryAllocator>,
    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    queue: Arc<Queue>,
}

impl ImagePaint {
    pub(crate) fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        memory_alloc: Arc<StandardMemoryAllocator>,
        descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
        cmd_alloc: Arc<StandardCommandBufferAllocator>,
        subpass: Subpass,
    ) -> Self {
        let pipeline = build_image_pipeline(device.clone(), subpass);
        let sampler = Sampler::new(
            device,
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Linear,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: image sampler");
        let instance_buf = create_image_instance_buffer(&memory_alloc, INITIAL_INSTANCE_CAPACITY);

        Self {
            instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY as usize),
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            runs: Vec::new(),
            pipeline,
            sampler,
            cache: HashMap::new(),
            bind_group_lookup: Vec::new(),
            frame_counter: 0,
            memory_alloc,
            descriptor_alloc,
            cmd_alloc,
            queue,
        }
    }

    pub(crate) fn frame_begin(&mut self) {
        self.instances.clear();
        self.runs.clear();
        self.bind_group_lookup.clear();
        self.frame_counter = self.frame_counter.wrapping_add(1);
    }

    pub(crate) fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        image: &RasterImage,
        tint: Option<Color>,
        radius: f32,
    ) -> Range<usize> {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            let start = self.runs.len();
            return start..start;
        }
        let start = self.runs.len();
        let texture_idx = self.ensure_texture(image);
        let tint_rgba = tint.map(rgba_f32).unwrap_or([1.0, 1.0, 1.0, 1.0]);
        let instance = ImageInstance {
            rect: [rect.x, rect.y, rect.w, rect.h],
            tint: tint_rgba,
            params: [radius.max(0.0), 0.0, 0.0, 0.0],
            uv: [0.0, 0.0, 1.0, 1.0],
        };
        let first = self.instances.len() as u32;
        self.instances.push(instance);
        self.runs.push(ImageRun {
            texture_idx,
            scissor,
            first,
            count: 1,
        });
        start..self.runs.len()
    }

    /// Look up or upload a texture for `image`. Returns an index into
    /// the per-frame `bind_group_lookup` table.
    fn ensure_texture(&mut self, image: &RasterImage) -> usize {
        let hash = image.content_hash();
        if !self.cache.contains_key(&hash) {
            let cached = self.upload_image(image);
            self.cache.insert(hash, cached);
        }
        let entry = self.cache.get_mut(&hash).expect("just inserted");
        entry.last_used_frame = self.frame_counter;
        if let Some(idx) = self.bind_group_lookup.iter().position(|&h| h == hash) {
            idx
        } else {
            self.bind_group_lookup.push(hash);
            self.bind_group_lookup.len() - 1
        }
    }

    fn upload_image(&self, image: &RasterImage) -> CachedTexture {
        let (w, h) = (image.width(), image.height());
        let gpu_image = VkImage::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                // Same convention as the wgpu side: sRGB-encoded user
                // art, sampler decodes to linear at sample time.
                format: Format::R8G8B8A8_SRGB,
                extent: [w, h, 1],
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: image gpu texture");

        let staging = Buffer::from_iter(
            self.memory_alloc.clone(),
            BufferCreateInfo {
                usage: BufferUsage::TRANSFER_SRC,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_HOST
                    | MemoryTypeFilter::HOST_SEQUENTIAL_WRITE,
                ..Default::default()
            },
            image.pixels().iter().copied(),
        )
        .expect("aetna-vulkano: image staging buf");

        let mut builder = AutoCommandBufferBuilder::primary(
            self.cmd_alloc.clone(),
            self.queue.queue_family_index(),
            CommandBufferUsage::OneTimeSubmit,
        )
        .expect("aetna-vulkano: image upload cmd builder");

        let copy_info = CopyBufferToImageInfo {
            regions: smallvec![BufferImageCopy {
                buffer_offset: 0,
                buffer_row_length: 0,
                buffer_image_height: 0,
                image_subresource: ImageSubresourceLayers {
                    aspects: ImageAspects::COLOR,
                    mip_level: 0,
                    array_layers: 0..1,
                },
                image_offset: [0, 0, 0],
                image_extent: [w, h, 1],
                ..Default::default()
            }],
            ..CopyBufferToImageInfo::buffer_image(staging, gpu_image.clone())
        };
        builder
            .copy_buffer_to_image(copy_info)
            .expect("aetna-vulkano: image copy_buffer_to_image");
        let cb = builder
            .build()
            .expect("aetna-vulkano: image upload cmd build");
        sync::now(self.queue.device().clone())
            .then_execute(self.queue.clone(), cb)
            .expect("aetna-vulkano: image upload then_execute")
            .then_signal_fence_and_flush()
            .expect("aetna-vulkano: image upload flush")
            .wait(None)
            .expect("aetna-vulkano: image upload fence wait");

        let view = ImageView::new_default(gpu_image).expect("aetna-vulkano: image view");
        let descriptor_set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            self.pipeline.layout().set_layouts()[1].clone(),
            [
                WriteDescriptorSet::image_view(0, view),
                WriteDescriptorSet::sampler(1, self.sampler.clone()),
            ],
            [],
        )
        .expect("aetna-vulkano: image descriptor set");

        CachedTexture {
            descriptor_set,
            last_used_frame: 0,
        }
    }

    pub(crate) fn flush(&mut self) {
        // GC cache entries not used this frame.
        let frame = self.frame_counter;
        self.cache.retain(|_, v| v.last_used_frame == frame);

        // Resize + write instance buffer.
        if (self.instances.len() as u64) > self.instance_capacity {
            let new_cap = (self.instances.len() as u64).next_power_of_two();
            self.instance_buf = create_image_instance_buffer(&self.memory_alloc, new_cap);
            self.instance_capacity = new_cap;
        }
        if !self.instances.is_empty() {
            let mut write = self
                .instance_buf
                .write()
                .expect("aetna-vulkano: image instance buf write");
            write[..self.instances.len()].copy_from_slice(&self.instances);
        }
    }

    pub(crate) fn run(&self, index: usize) -> &ImageRun {
        &self.runs[index]
    }

    pub(crate) fn pipeline(&self) -> &Arc<GraphicsPipeline> {
        &self.pipeline
    }

    pub(crate) fn instance_buf(&self) -> &Subbuffer<[ImageInstance]> {
        &self.instance_buf
    }

    pub(crate) fn descriptor_for_run(&self, run: &ImageRun) -> &Arc<DescriptorSet> {
        let hash = self.bind_group_lookup[run.texture_idx];
        &self
            .cache
            .get(&hash)
            .expect("cache entry alive for the frame")
            .descriptor_set
    }
}

fn create_image_instance_buffer(
    allocator: &Arc<StandardMemoryAllocator>,
    capacity: u64,
) -> Subbuffer<[ImageInstance]> {
    Buffer::new_slice::<ImageInstance>(
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
    .expect("aetna-vulkano: image instance buffer alloc")
}

fn build_image_pipeline(device: Arc<Device>, subpass: Subpass) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv("stock::image", stock_wgsl::IMAGE)
        .expect("aetna-vulkano: image WGSL compile");
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .expect("aetna-vulkano: image ShaderModule::new")
    };
    let vs = module
        .entry_point("vs_main")
        .expect("image.wgsl: missing vs_main");
    let fs = module
        .entry_point("fs_main")
        .expect("image.wgsl: missing fs_main");
    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = build_shared_pipeline_layout(device.clone(), &stages);

    let bind_vertex = VertexInputBindingDescription {
        stride: (2 * std::mem::size_of::<f32>()) as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let bind_instance = VertexInputBindingDescription {
        stride: std::mem::size_of::<ImageInstance>() as u32,
        input_rate: VertexInputRate::Instance { divisor: 1 },
        ..Default::default()
    };
    let attr = |binding: u32, offset: u32, format: Format| VertexInputAttributeDescription {
        binding,
        offset,
        format,
        ..Default::default()
    };
    let vertex_input_state = VertexInputState::new()
        .binding(0, bind_vertex)
        .binding(1, bind_instance)
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT))
        // location 1: rect      @ offset 0  (4*f32 = 16)
        // location 2: tint      @ offset 16 (4*f32 = 16)
        // location 3: params    @ offset 32 (4*f32 = 16)
        // location 4: uv subrect@ offset 48 (4*f32 = 16)
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT))
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT))
        .attribute(3, attr(1, 32, Format::R32G32B32A32_SFLOAT))
        .attribute(4, attr(1, 48, Format::R32G32B32A32_SFLOAT));

    // Premultiplied output (matches the wgpu side and stock::text_msdf).
    let premultiplied = AttachmentBlend {
        src_color_blend_factor: BlendFactor::One,
        dst_color_blend_factor: BlendFactor::OneMinusSrcAlpha,
        color_blend_op: BlendOp::Add,
        src_alpha_blend_factor: BlendFactor::One,
        dst_alpha_blend_factor: BlendFactor::OneMinusSrcAlpha,
        alpha_blend_op: BlendOp::Add,
    };

    GraphicsPipeline::new(
        device,
        None,
        GraphicsPipelineCreateInfo {
            stages: stages.into_iter().collect(),
            vertex_input_state: Some(vertex_input_state),
            input_assembly_state: Some(InputAssemblyState {
                topology: PrimitiveTopology::TriangleStrip,
                ..Default::default()
            }),
            viewport_state: Some(ViewportState::default()),
            rasterization_state: Some(RasterizationState::default()),
            multisample_state: Some(MultisampleState::default()),
            color_blend_state: Some(ColorBlendState::with_attachment_states(
                subpass.num_color_attachments(),
                ColorBlendAttachmentState {
                    blend: Some(premultiplied),
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
    .expect("aetna-vulkano: image GraphicsPipeline::new")
}
