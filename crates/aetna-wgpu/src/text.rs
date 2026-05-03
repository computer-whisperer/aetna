//! Atlas-backed text rendering for `stock::text`.
//!
//! Owns an [`aetna_core::text_atlas::GlyphAtlas`] (cosmic-text shaping +
//! swash rasterization, both backend-agnostic) and mirrors its CPU pages
//! to one R8Unorm wgpu texture per page. Recording a text run shapes the
//! string, ensures all glyphs are present in the atlas, and emits one
//! per-glyph quad instance that samples the matching page.
//!
//! Page-split runs: a single logical text op may use glyphs from more
//! than one atlas page (rare in practice — happens once a page fills
//! up). Each page-segment becomes its own [`TextRun`] so the draw loop
//! can rebind the page texture between segments.

use std::borrow::Cow;

use aetna_core::ir::TextAnchor;
use aetna_core::shader::stock_wgsl;
use aetna_core::text_atlas::{AtlasPage, AtlasRect, GlyphAtlas};
use aetna_core::tree::{Color, FontWeight, Rect, TextWrap};

use bytemuck::{Pod, Zeroable};

use aetna_core::paint::{PhysicalScissor, rgba_f32};

const INITIAL_INSTANCE_CAPACITY: usize = 256;

const INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect  (xy = top-left logical px, zw = size logical px)
    2 => Float32x4,  // uv    (xy = uv 0..1, zw = uv size 0..1)
    3 => Float32x4,  // color (linear rgba 0..1)
];

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

struct PageTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

pub(crate) struct TextPaint {
    pub atlas: GlyphAtlas,
    pages: Vec<PageTexture>,
    instances: Vec<GlyphInstance>,
    instance_buf: wgpu::Buffer,
    instance_capacity: usize,
    runs: Vec<TextRun>,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    page_bind_layout: wgpu::BindGroupLayout,
}

impl TextPaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        frame_bind_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let page_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aetna_wgpu::text::page_bind_layout"),
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

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aetna_wgpu::text::pipeline_layout"),
            bind_group_layouts: &[frame_bind_layout, &page_bind_layout],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stock::text"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::TEXT)),
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("aetna_wgpu::text::pipeline"),
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
                        array_stride: std::mem::size_of::<GlyphInstance>() as u64,
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
                    blend: Some(wgpu::BlendState {
                        // Premultiplied-alpha output from the fragment
                        // shader; standard alpha blending compositions it.
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("aetna_wgpu::text::sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::text::instance_buf"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<GlyphInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            atlas: GlyphAtlas::new(),
            pages: Vec::new(),
            instances: Vec::with_capacity(INITIAL_INSTANCE_CAPACITY),
            instance_buf,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            runs: Vec::new(),
            pipeline,
            sampler,
            page_bind_layout,
        }
    }

    pub(crate) fn frame_begin(&mut self) {
        self.instances.clear();
        self.runs.clear();
    }

    /// Shape `text` and append per-glyph instances for it. Returns the
    /// half-open range of `run_index` values created (one per atlas page
    /// the run touched, or zero if no glyphs were emitted).
    ///
    /// `scale_factor` is the HiDPI multiplier — atlas glyphs are
    /// rasterized at `size * scale_factor` (physical pixels) and the
    /// resulting bitmap dimensions are divided by the same factor when
    /// placing the screen quad. This produces 1:1 atlas-pixel to
    /// physical-pixel mapping at the host's render target resolution,
    /// matching what glyphon did in v5.0.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record(
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
    ) -> std::ops::Range<usize> {
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

        // Layout came back in physical px; convert to logical for screen
        // quad placement.
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
            // Convert physical-px glyph metrics back to logical-px screen
            // quads. The atlas bitmap is at native render-target
            // resolution; mapping logical px to NDC via the (logical)
            // viewport in the vertex shader puts each atlas pixel onto
            // exactly one physical pixel.
            let bx = origin_x + (glyph.x + slot.offset.0 as f32) / scale_factor;
            let by = origin_y + (glyph.y - slot.offset.1 as f32) / scale_factor;
            let bw = slot.rect.w as f32 / scale_factor;
            let bh = slot.rect.h as f32 / scale_factor;

            // UV in 0..1 across the page texture. Source the page
            // dimensions from the atlas — the wgpu texture mirror
            // doesn't exist yet until `flush()` runs.
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

    /// Sync atlas pages to GPU textures and upload instance data. Call
    /// once per frame, after all `record(...)` calls and before `draw`.
    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        // Drain dirty rects first, then allocate any missing page
        // textures, then upload the dirty rects (covers both new and
        // existing pages with one code path).
        let dirty = self.atlas.take_dirty();
        while self.pages.len() < self.atlas.pages().len() {
            let i = self.pages.len();
            let page = &self.atlas.pages()[i];
            self.pages
                .push(self.create_page_texture(device, page.width, page.height));
        }
        for (page_idx, rect) in dirty {
            let page = &self.atlas.pages()[page_idx];
            upload_page_region(queue, &self.pages[page_idx].texture, page, rect);
        }

        // Upload instances (resize buffer first if needed).
        if self.instances.len() > self.instance_capacity {
            let new_cap = self.instances.len().next_power_of_two();
            self.instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aetna_wgpu::text::instance_buf (resized)"),
                size: (new_cap * std::mem::size_of::<GlyphInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_cap;
        }
        if !self.instances.is_empty() {
            queue.write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(&self.instances));
        }
    }

    fn create_page_texture(&self, device: &wgpu::Device, width: u32, height: u32) -> PageTexture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("aetna_wgpu::text::atlas_page"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aetna_wgpu::text::atlas_page_bg"),
            layout: &self.page_bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        PageTexture {
            texture,
            bind_group,
        }
    }

    pub(crate) fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }
    pub(crate) fn instance_buf(&self) -> &wgpu::Buffer {
        &self.instance_buf
    }
    pub(crate) fn run(&self, index: usize) -> TextRun {
        self.runs[index]
    }
    pub(crate) fn page_bind_group(&self, page: u32) -> &wgpu::BindGroup {
        &self.pages[page as usize].bind_group
    }
}

fn upload_page_region(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    page: &AtlasPage,
    rect: AtlasRect,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    // Slice out just the dirty rect's rows from the page's row-major
    // pixel buffer so we don't pay to re-upload the whole page.
    let mut bytes = Vec::with_capacity((rect.w * rect.h) as usize);
    for row in 0..rect.h {
        let y = rect.y + row;
        let start = (y * page.width + rect.x) as usize;
        let end = start + rect.w as usize;
        bytes.extend_from_slice(&page.pixels[start..end]);
    }
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: rect.x,
                y: rect.y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        &bytes,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(rect.w),
            rows_per_image: Some(rect.h),
        },
        wgpu::Extent3d {
            width: rect.w,
            height: rect.h,
            depth_or_array_layers: 1,
        },
    );
}
