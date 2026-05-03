//! Render pipeline construction for the shared rect-shaped layout.
//!
//! Stock surfaces (`rounded_rect`, `focus_ring`) and any user-registered
//! custom shader all use the same vertex layout — a unit-quad strip plus
//! the [`crate::instance::QuadInstance`] attributes. That means one
//! pipeline-builder function covers the whole catalog; the only thing
//! that varies is the WGSL source and a label.

use std::borrow::Cow;

use bytemuck::{Pod, Zeroable};

use crate::instance::QuadInstance;

/// Per-frame globals bound at @group(0).
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct FrameUniforms {
    pub viewport: [f32; 2],
    pub _pad: [f32; 2],
}

/// Per-instance vertex attributes — must match the shared
/// `InstanceInput` struct in `shaders/rounded_rect.wgsl` and any
/// registered custom shader.
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    1 => Float32x4,  // rect (xy=topleft px, zw=size px)
    2 => Float32x4,  // vec_a (stock::rounded_rect: fill)
    3 => Float32x4,  // vec_b (stock::rounded_rect: stroke)
    4 => Float32x4,  // vec_c (stock::rounded_rect: stroke_width, radius, shadow, _)
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
