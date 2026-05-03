//! Atlas-backed text rendering for `stock::text`.
//!
//! Mirrors `aetna_wgpu::text` — owns the core-side
//! [`aetna_core::text_atlas::GlyphAtlas`] (cosmic-text shaping + swash
//! rasterization) and per-page Vulkan images that mirror the CPU pages.
//! Per-glyph quads are emitted by `record()`; dirty atlas regions get
//! copy_buffer_to_image'd to the GPU mirror in `flush()`.
//!
//! Atlas dirty uploads run inside a one-shot command buffer that is
//! submitted + waited on inside `flush()`. That's a strict serialisation
//! point — fine for v5.3 because text shaping isn't on the hot path
//! (the atlas only churns when the glyph set changes); v5.4 can revisit
//! batching uploads into the host's main draw command buffer if perf
//! data ever shows it matters.

use std::ops::Range;
use std::sync::Arc;

use aetna_core::ir::TextAnchor;
use aetna_core::shader::stock_wgsl;
use aetna_core::text_atlas::{AtlasPage, AtlasRect, GlyphAtlas};
use aetna_core::tree::{Color, FontWeight, Rect, TextWrap};
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
        Image, ImageAspects, ImageCreateInfo, ImageSubresourceLayers, ImageType, ImageUsage,
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
        layout::PipelineDescriptorSetLayoutCreateInfo,
    },
    render_pass::Subpass,
    shader::{ShaderModule, ShaderModuleCreateInfo},
    sync::{self, GpuFuture},
};

use aetna_core::paint::{PhysicalScissor, rgba_f32};
use aetna_core::runtime::TextRecorder;

use crate::naga_compile::wgsl_to_spirv;

const INITIAL_INSTANCE_CAPACITY: u64 = 256;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct GlyphInstance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
}

#[derive(Clone, Copy)]
pub(crate) struct TextRun {
    pub page: u32,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

struct PageGpu {
    image: Arc<Image>,
    descriptor_set: Arc<DescriptorSet>,
}

pub(crate) struct TextPaint {
    pub atlas: GlyphAtlas,
    pages: Vec<PageGpu>,
    instances: Vec<GlyphInstance>,
    runs: Vec<TextRun>,

    pipeline: Arc<GraphicsPipeline>,
    sampler: Arc<Sampler>,
    instance_buf: Subbuffer<[GlyphInstance]>,
    instance_capacity: u64,

    memory_alloc: Arc<StandardMemoryAllocator>,
    descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
    cmd_alloc: Arc<StandardCommandBufferAllocator>,
    queue: Arc<Queue>,
}

impl TextPaint {
    pub(crate) fn new(
        device: Arc<Device>,
        queue: Arc<Queue>,
        memory_alloc: Arc<StandardMemoryAllocator>,
        descriptor_alloc: Arc<StandardDescriptorSetAllocator>,
        cmd_alloc: Arc<StandardCommandBufferAllocator>,
        subpass: Subpass,
    ) -> Self {
        let pipeline = build_text_pipeline(device.clone(), subpass);

        let sampler = Sampler::new(
            device,
            SamplerCreateInfo {
                mag_filter: Filter::Linear,
                min_filter: Filter::Linear,
                mipmap_mode: SamplerMipmapMode::Nearest,
                address_mode: [SamplerAddressMode::ClampToEdge; 3],
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: text sampler");

        let instance_buf = create_glyph_instance_buffer(&memory_alloc, INITIAL_INSTANCE_CAPACITY);

        Self {
            atlas: GlyphAtlas::new(),
            pages: Vec::new(),
            instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY as usize),
            runs: Vec::new(),
            pipeline,
            sampler,
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            memory_alloc,
            descriptor_alloc,
            cmd_alloc,
            queue,
        }
    }

    pub(crate) fn frame_begin(&mut self) {
        self.instances.clear();
        self.runs.clear();
    }

    /// Shape `text` and append per-glyph instances. Logic is identical
    /// to `aetna_wgpu::text::TextPaint::record_inner` — only the stored
    /// type of `instances` and `runs` differs.
    #[allow(clippy::too_many_arguments)]
    fn record_inner(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        color: Color,
        text: &str,
        size: f32,
        weight: FontWeight,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize> {
        let physical_size = size * scale_factor;
        let avail = match (wrap, anchor) {
            (TextWrap::Wrap, _) => Some(rect.w * scale_factor),
            (TextWrap::NoWrap, TextAnchor::Start) => None,
            (TextWrap::NoWrap, TextAnchor::Middle | TextAnchor::End) => Some(rect.w * scale_factor),
        };
        let shaped =
            self.atlas
                .shape_and_rasterize(text, physical_size, weight, wrap, anchor, avail);

        let runs_start = self.runs.len();
        if shaped.glyphs.is_empty() {
            return runs_start..runs_start;
        }

        let logical_line_height = shaped.layout.line_height / scale_factor;
        let v_offset = match wrap {
            TextWrap::NoWrap => ((rect.h - logical_line_height).max(0.0)) * 0.5,
            TextWrap::Wrap => 0.0,
        };
        let origin_x = rect.x;
        let origin_y = rect.y + v_offset;
        let color_linear = rgba_f32(color);

        let mut current_page: Option<u32> = None;
        let mut run_first = self.instances.len() as u32;

        for glyph in &shaped.glyphs {
            let Some(slot) = self.atlas.slot(glyph.key) else {
                continue;
            };
            if slot.rect.w == 0 || slot.rect.h == 0 {
                continue;
            }
            let page = slot.page;
            let bx = origin_x + (glyph.x + slot.offset.0 as f32) / scale_factor;
            let by = origin_y + (glyph.y - slot.offset.1 as f32) / scale_factor;
            let bw = slot.rect.w as f32 / scale_factor;
            let bh = slot.rect.h as f32 / scale_factor;

            let atlas_page = self
                .atlas
                .page(page)
                .expect("shaped glyph references missing atlas page");
            let page_w = atlas_page.width as f32;
            let page_h = atlas_page.height as f32;
            let uv = [
                slot.rect.x as f32 / page_w,
                slot.rect.y as f32 / page_h,
                slot.rect.w as f32 / page_w,
                slot.rect.h as f32 / page_h,
            ];

            if current_page != Some(page) {
                if let Some(p) = current_page {
                    let count = self.instances.len() as u32 - run_first;
                    if count > 0 {
                        self.runs.push(TextRun {
                            page: p,
                            scissor,
                            first: run_first,
                            count,
                        });
                    }
                    run_first = self.instances.len() as u32;
                }
                current_page = Some(page);
            }

            self.instances.push(GlyphInstance {
                rect: [bx, by, bw, bh],
                uv,
                color: color_linear,
            });
        }
        if let Some(p) = current_page {
            let count = self.instances.len() as u32 - run_first;
            if count > 0 {
                self.runs.push(TextRun {
                    page: p,
                    scissor,
                    first: run_first,
                    count,
                });
            }
        }

        runs_start..self.runs.len()
    }

    /// Sync atlas dirty regions to GPU images and upload glyph instance
    /// data. Call once per frame after all `record` calls, before
    /// `Runner::draw` records the host's command buffer.
    ///
    /// The dirty-region uploads run inside a one-shot command buffer
    /// that is submitted and *waited on* here. v5.4 can batch these
    /// into the host's main command buffer if perf demands it.
    pub(crate) fn flush(&mut self) {
        let dirty = self.atlas.take_dirty();

        // Allocate page images for any new atlas pages.
        while self.pages.len() < self.atlas.pages().len() {
            let i = self.pages.len();
            let page = &self.atlas.pages()[i];
            let new_page = self.create_page(page.width, page.height);
            self.pages.push(new_page);
        }

        // Upload dirty regions via a one-shot command buffer.
        if !dirty.is_empty() {
            let mut builder = AutoCommandBufferBuilder::primary(
                self.cmd_alloc.clone(),
                self.queue.queue_family_index(),
                CommandBufferUsage::OneTimeSubmit,
            )
            .expect("aetna-vulkano: text upload cmd builder");

            for (page_idx, rect) in &dirty {
                if rect.w == 0 || rect.h == 0 {
                    continue;
                }
                let page = &self.atlas.pages()[*page_idx];
                let bytes = pack_rect_bytes(page, *rect);
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
                    bytes,
                )
                .expect("aetna-vulkano: text staging buffer");

                let copy_info = CopyBufferToImageInfo {
                    regions: smallvec![BufferImageCopy {
                        buffer_offset: 0,
                        // bytes_per_row defaults to tightly-packed when
                        // buffer_image_height/buffer_row_length are 0.
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: ImageSubresourceLayers {
                            aspects: ImageAspects::COLOR,
                            mip_level: 0,
                            array_layers: 0..1,
                        },
                        image_offset: [rect.x, rect.y, 0],
                        image_extent: [rect.w, rect.h, 1],
                        ..Default::default()
                    }],
                    ..CopyBufferToImageInfo::buffer_image(
                        staging,
                        self.pages[*page_idx].image.clone(),
                    )
                };
                builder
                    .copy_buffer_to_image(copy_info)
                    .expect("aetna-vulkano: copy_buffer_to_image");
            }

            let cb = builder
                .build()
                .expect("aetna-vulkano: text upload cmd build");
            let future = sync::now(self.queue.device().clone())
                .then_execute(self.queue.clone(), cb)
                .expect("aetna-vulkano: text upload then_execute")
                .then_signal_fence_and_flush()
                .expect("aetna-vulkano: text upload flush");
            future
                .wait(None)
                .expect("aetna-vulkano: text upload fence wait");
        }

        // Resize + write the glyph instance buffer.
        if (self.instances.len() as u64) > self.instance_capacity {
            let new_cap = (self.instances.len() as u64).next_power_of_two();
            self.instance_buf = create_glyph_instance_buffer(&self.memory_alloc, new_cap);
            self.instance_capacity = new_cap;
        }
        if !self.instances.is_empty() {
            let mut write = self
                .instance_buf
                .write()
                .expect("aetna-vulkano: text instance buf write");
            write[..self.instances.len()].copy_from_slice(&self.instances);
        }
    }

    fn create_page(&self, width: u32, height: u32) -> PageGpu {
        let image = Image::new(
            self.memory_alloc.clone(),
            ImageCreateInfo {
                image_type: ImageType::Dim2d,
                format: Format::R8_UNORM,
                extent: [width, height, 1],
                usage: ImageUsage::TRANSFER_DST | ImageUsage::SAMPLED,
                ..Default::default()
            },
            AllocationCreateInfo {
                memory_type_filter: MemoryTypeFilter::PREFER_DEVICE,
                ..Default::default()
            },
        )
        .expect("aetna-vulkano: text atlas page image");

        let view = ImageView::new_default(image.clone()).expect("aetna-vulkano: text page view");
        let descriptor_set = DescriptorSet::new(
            self.descriptor_alloc.clone(),
            self.pipeline.layout().set_layouts()[1].clone(),
            [
                WriteDescriptorSet::image_view(0, view),
                WriteDescriptorSet::sampler(1, self.sampler.clone()),
            ],
            [],
        )
        .expect("aetna-vulkano: text page descriptor set");

        PageGpu {
            image,
            descriptor_set,
        }
    }

    pub(crate) fn pipeline(&self) -> &Arc<GraphicsPipeline> {
        &self.pipeline
    }
    pub(crate) fn instance_buf(&self) -> &Subbuffer<[GlyphInstance]> {
        &self.instance_buf
    }
    pub(crate) fn run(&self, index: usize) -> TextRun {
        self.runs[index]
    }
    pub(crate) fn page_descriptor(&self, page: u32) -> &Arc<DescriptorSet> {
        &self.pages[page as usize].descriptor_set
    }
}

impl TextRecorder for TextPaint {
    fn record(
        &mut self,
        rect: Rect,
        scissor: Option<PhysicalScissor>,
        color: Color,
        text: &str,
        size: f32,
        weight: FontWeight,
        wrap: TextWrap,
        anchor: TextAnchor,
        scale_factor: f32,
    ) -> Range<usize> {
        self.record_inner(
            rect,
            scissor,
            color,
            text,
            size,
            weight,
            wrap,
            anchor,
            scale_factor,
        )
    }
}

/// Slice the `rect`-bounded subregion out of `page.pixels` (row-major
/// u8 alpha bitmap) into a tightly-packed Vec for staging-buffer
/// upload — same shape `aetna_wgpu::text::upload_page_region` uses
/// when calling `queue.write_texture`.
fn pack_rect_bytes(page: &AtlasPage, rect: AtlasRect) -> Vec<u8> {
    let mut bytes = Vec::with_capacity((rect.w * rect.h) as usize);
    for row in 0..rect.h {
        let y = rect.y + row;
        let start = (y * page.width + rect.x) as usize;
        let end = start + rect.w as usize;
        bytes.extend_from_slice(&page.pixels[start..end]);
    }
    bytes
}

fn create_glyph_instance_buffer(
    allocator: &Arc<StandardMemoryAllocator>,
    capacity: u64,
) -> Subbuffer<[GlyphInstance]> {
    Buffer::new_slice::<GlyphInstance>(
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
    .expect("aetna-vulkano: glyph instance buffer alloc")
}

fn build_text_pipeline(device: Arc<Device>, subpass: Subpass) -> Arc<GraphicsPipeline> {
    let words = wgsl_to_spirv("stock::text", stock_wgsl::TEXT)
        .unwrap_or_else(|e| panic!("aetna-vulkano: text WGSL compile: {e}"));
    let module = unsafe {
        ShaderModule::new(device.clone(), ShaderModuleCreateInfo::new(&words))
            .expect("aetna-vulkano: text ShaderModule::new")
    };
    let vs = module
        .entry_point("vs_main")
        .expect("text.wgsl: missing vs_main");
    let fs = module
        .entry_point("fs_main")
        .expect("text.wgsl: missing fs_main");

    let stages = [
        PipelineShaderStageCreateInfo::new(vs),
        PipelineShaderStageCreateInfo::new(fs),
    ];
    let layout = vulkano::pipeline::PipelineLayout::new(
        device.clone(),
        PipelineDescriptorSetLayoutCreateInfo::from_stages(&stages)
            .into_pipeline_layout_create_info(device.clone())
            .expect("aetna-vulkano: text pipeline layout from stages"),
    )
    .expect("aetna-vulkano: text pipeline layout new");

    let bind_vertex = VertexInputBindingDescription {
        stride: (2 * std::mem::size_of::<f32>()) as u32,
        input_rate: VertexInputRate::Vertex,
        ..Default::default()
    };
    let bind_instance = VertexInputBindingDescription {
        stride: std::mem::size_of::<GlyphInstance>() as u32,
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
        // location 0 — corner_uv
        .attribute(0, attr(0, 0, Format::R32G32_SFLOAT))
        // location 1 — rect
        .attribute(1, attr(1, 0, Format::R32G32B32A32_SFLOAT))
        // location 2 — uv
        .attribute(2, attr(1, 16, Format::R32G32B32A32_SFLOAT))
        // location 3 — color
        .attribute(3, attr(1, 32, Format::R32G32B32A32_SFLOAT));

    // Premultiplied-alpha blend: matches aetna_wgpu::text byte for byte.
    // The fragment shader outputs `vec4(color.rgb * a, a)`.
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
    .expect("aetna-vulkano: text GraphicsPipeline::new")
}
