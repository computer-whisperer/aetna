//! GPU vector-icon rendering.
//!
//! Built-in icons are parsed from SVG through `usvg` into Aetna's
//! backend-agnostic vector IR, then tessellated with lyon into triangle
//! vertices for this backend. This is the canonical geometry path;
//! shader experimentation can layer on top of these vector vertices.

use std::borrow::Cow;
use std::ops::Range;

use aetna_core::icons::icon_vector_asset;
use aetna_core::paint::{IconRun, PhysicalScissor, rgba_f32};
use aetna_core::shader::stock_wgsl;
use aetna_core::tree::{Color, IconName, Rect};
use aetna_core::vector::{
    VectorAsset, VectorColor, VectorFillRule, VectorLineCap, VectorLineJoin, VectorPath,
    VectorSegment,
};

use bytemuck::{Pod, Zeroable};
use lyon_tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{
    FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions, StrokeTessellator,
    StrokeVertex,
};

const INITIAL_VERTEX_CAPACITY: usize = 1024;

const VERTEX_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
    0 => Float32x2, // position in logical px
    1 => Float32x4, // linear rgba
];

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct VectorVertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
}

pub(crate) struct IconPaint {
    vertices: Vec<VectorVertex>,
    vertex_buf: wgpu::Buffer,
    vertex_capacity: usize,
    runs: Vec<IconRun>,
    pipeline: wgpu::RenderPipeline,
}

impl IconPaint {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        frame_bind_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aetna_wgpu::icon::vertex_buf"),
            size: (INITIAL_VERTEX_CAPACITY * std::mem::size_of::<VectorVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aetna_wgpu::icon::pipeline_layout"),
            bind_group_layouts: &[frame_bind_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stock::vector"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(stock_wgsl::VECTOR)),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stock::vector"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<VectorVertex>() as u64,
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
        });

        Self {
            vertices: Vec::with_capacity(INITIAL_VERTEX_CAPACITY),
            vertex_buf,
            vertex_capacity: INITIAL_VERTEX_CAPACITY,
            runs: Vec::new(),
            pipeline,
        }
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
        tessellate_asset(asset, rect, color, stroke_width, &mut self.vertices);
        let count = self.vertices.len() as u32 - first;
        if count == 0 {
            let start = self.runs.len();
            return start..start;
        }

        let start = self.runs.len();
        self.runs.push(IconRun {
            scissor,
            first,
            count,
        });
        start..self.runs.len()
    }

    pub(crate) fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.vertices.len() > self.vertex_capacity {
            let new_cap = self.vertices.len().next_power_of_two();
            self.vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aetna_wgpu::icon::vertex_buf (resized)"),
                size: (new_cap * std::mem::size_of::<VectorVertex>()) as u64,
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

    pub(crate) fn pipeline(&self) -> &wgpu::RenderPipeline {
        &self.pipeline
    }

    pub(crate) fn vertex_buf(&self) -> &wgpu::Buffer {
        &self.vertex_buf
    }
}

fn tessellate_asset(
    asset: &VectorAsset,
    rect: Rect,
    current_color: Color,
    stroke_width: f32,
    out: &mut Vec<VectorVertex>,
) {
    let [vx, vy, vw, vh] = asset.view_box;
    let sx = rect.w / vw.max(1.0);
    let sy = rect.h / vh.max(1.0);
    let stroke_scale = (sx + sy) * 0.5;

    for vector_path in &asset.paths {
        let path = build_lyon_path(vector_path, rect, [vx, vy], [sx, sy]);
        if let Some(fill) = vector_path.fill {
            let color = resolve_color(fill.color, current_color, fill.opacity);
            let mut geometry: VertexBuffers<VectorVertex, u16> = VertexBuffers::new();
            let options = FillOptions::tolerance(0.05).with_fill_rule(match fill.rule {
                VectorFillRule::NonZero => lyon_tessellation::FillRule::NonZero,
                VectorFillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
            });
            let _ = FillTessellator::new().tessellate_path(
                &path,
                &options,
                &mut BuffersBuilder::new(&mut geometry, |v: FillVertex<'_>| {
                    let p = v.position();
                    VectorVertex {
                        pos: [p.x, p.y],
                        color,
                    }
                }),
            );
            append_indexed(&geometry, out);
        }

        if let Some(stroke) = vector_path.stroke {
            let color = resolve_color(stroke.color, current_color, stroke.opacity);
            let width = if matches!(stroke.color, VectorColor::CurrentColor) {
                stroke_width * stroke_scale
            } else {
                stroke.width * stroke_scale
            }
            .max(0.5);
            let mut geometry: VertexBuffers<VectorVertex, u16> = VertexBuffers::new();
            let options = StrokeOptions::tolerance(0.05)
                .with_line_width(width)
                .with_line_cap(match stroke.line_cap {
                    VectorLineCap::Butt => LineCap::Butt,
                    VectorLineCap::Round => LineCap::Round,
                    VectorLineCap::Square => LineCap::Square,
                })
                .with_line_join(match stroke.line_join {
                    VectorLineJoin::Miter => LineJoin::Miter,
                    VectorLineJoin::MiterClip => LineJoin::MiterClip,
                    VectorLineJoin::Round => LineJoin::Round,
                    VectorLineJoin::Bevel => LineJoin::Bevel,
                })
                .with_miter_limit(stroke.miter_limit.max(1.0));
            let _ = StrokeTessellator::new().tessellate_path(
                &path,
                &options,
                &mut BuffersBuilder::new(&mut geometry, |v: StrokeVertex<'_, '_>| {
                    let p = v.position();
                    VectorVertex {
                        pos: [p.x, p.y],
                        color,
                    }
                }),
            );
            append_indexed(&geometry, out);
        }
    }
}

fn build_lyon_path(
    path: &VectorPath,
    rect: Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
) -> LyonPath {
    let mut builder = LyonPath::builder();
    let mut open = false;
    for segment in &path.segments {
        match *segment {
            VectorSegment::MoveTo(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(map_point(rect, view_origin, scale, p));
                open = true;
            }
            VectorSegment::LineTo(p) => {
                builder.line_to(map_point(rect, view_origin, scale, p));
            }
            VectorSegment::QuadTo(c, p) => {
                builder.quadratic_bezier_to(
                    map_point(rect, view_origin, scale, c),
                    map_point(rect, view_origin, scale, p),
                );
            }
            VectorSegment::CubicTo(c0, c1, p) => {
                builder.cubic_bezier_to(
                    map_point(rect, view_origin, scale, c0),
                    map_point(rect, view_origin, scale, c1),
                    map_point(rect, view_origin, scale, p),
                );
            }
            VectorSegment::Close => {
                if open {
                    builder.close();
                    open = false;
                }
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

fn map_point(
    rect: Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
    p: [f32; 2],
) -> lyon_tessellation::math::Point {
    point(
        rect.x + (p[0] - view_origin[0]) * scale[0],
        rect.y + (p[1] - view_origin[1]) * scale[1],
    )
}

fn resolve_color(color: VectorColor, current_color: Color, opacity: f32) -> [f32; 4] {
    let mut rgba = match color {
        VectorColor::CurrentColor => rgba_f32(current_color),
        VectorColor::Solid(color) => rgba_f32(color),
    };
    rgba[3] *= opacity.clamp(0.0, 1.0);
    rgba
}

fn append_indexed(geometry: &VertexBuffers<VectorVertex, u16>, out: &mut Vec<VectorVertex>) {
    for index in &geometry.indices {
        if let Some(vertex) = geometry.vertices.get(*index as usize) {
            out.push(*vertex);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aetna_core::icons::all_icon_names;

    #[test]
    fn tessellates_every_builtin_icon() {
        for name in all_icon_names() {
            let mut vertices = Vec::new();
            tessellate_asset(
                icon_vector_asset(*name),
                Rect::new(0.0, 0.0, 16.0, 16.0),
                Color::rgb(15, 23, 42),
                2.0,
                &mut vertices,
            );
            assert!(
                !vertices.is_empty(),
                "{} produced no tessellated vertices",
                name.name()
            );
        }
    }
}
