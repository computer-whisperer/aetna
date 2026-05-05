//! Render pipeline construction for the shared rect-shaped layout.
//!
//! Stock `rounded_rect` and any user-registered custom shader all use
//! the same vertex layout — a unit-quad strip plus the
//! [`aetna_core::paint::QuadInstance`] attributes. That means one
//! pipeline-builder function covers the whole catalog; the only thing
//! that varies is the WGSL source and a label. Focus indicators ride
//! on each focusable node's own quad via uniforms on `rounded_rect` —
//! no separate ring pipeline.

use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};

use aetna_core::paint::QuadInstance;

/// Per-frame globals bound at @group(0).
///
/// Layout matches the shared WGSL convention:
/// ```wgsl
/// struct FrameUniforms {
///     viewport:     vec2<f32>,  // logical px (width, height)
///     time:         f32,        // seconds since runner start
///     scale_factor: f32,        // physical px per logical px (1, 1.5, 2…)
/// };
/// ```
/// Custom shaders that previously declared `_pad: vec2<f32>` keep
/// working — the byte layout is unchanged; the trailing `_pad.y` slot
/// is now `scale_factor` and shaders can either ignore it or rename
/// the field to consume it.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct FrameUniforms {
    pub viewport: [f32; 2],
    pub time: f32,
    pub scale_factor: f32,
}

/// Per-instance vertex attributes — must match the shared
/// `InstanceInput` struct in `shaders/rounded_rect.wgsl` and any
/// registered custom shader. Order matches `aetna_core::paint::QuadInstance`
/// field order so byte offsets line up. The legacy locations 1..=4 are
/// preserved for backward compat; `inner_rect` and `vec_d` slot in at
/// the end so custom shaders that only declare 1..=4 keep working.
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect (xy=topleft px, zw=size px) — painted rect
    2 => Float32x4,  // vec_a (stock::rounded_rect: fill)
    3 => Float32x4,  // vec_b (stock::rounded_rect: stroke)
    4 => Float32x4,  // vec_c (stock::rounded_rect: stroke_width, radius, shadow, focus_width)
    5 => Float32x4,  // inner_rect (xy=topleft px, zw=size px) — layout rect (NEW)
    6 => Float32x4,  // vec_d (stock::rounded_rect: focus_color rgba, alpha eased) (NEW)
];

pub(crate) fn build_quad_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
    label: &str,
    wgsl: &str,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
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
    })
}
