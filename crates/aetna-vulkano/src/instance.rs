//! Per-instance vertex packing, paint-stream batching, and scissor math.
//!
//! Direct port of `aetna_wgpu::instance` — the `QuadInstance` ABI is the
//! cross-backend contract every rect-shaped pipeline reads, so it stays
//! byte-for-byte identical between the two backends. The only divergence
//! is `set_scissor`, which records into a vulkano command-buffer builder
//! instead of a `wgpu::RenderPass`.
//!
//! v5.4 may decide to lift this into `aetna-core`. v5.3 duplicates so the
//! `aetna-core` "additive only" constraint stays clean.

use bytemuck::{Pod, Zeroable};

use aetna_core::shader::{ShaderHandle, StockShader, UniformBlock, UniformValue};
use aetna_core::tree::{Color, Rect};
use smallvec::smallvec;
use vulkano::command_buffer::{AutoCommandBufferBuilder, PrimaryAutoCommandBuffer};
use vulkano::pipeline::graphics::viewport::Scissor;

// `BufferContents` is blanket-implemented for any `bytemuck::AnyBitPattern + Send + Sync`,
// which `Pod + Zeroable` already gives us — no extra derive needed here.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub(crate) struct QuadInstance {
    /// xy = top-left px, zw = size px.
    pub rect: [f32; 4],
    /// `vec_a` slot — for stock::rounded_rect, this is `fill`.
    pub slot_a: [f32; 4],
    /// `vec_b` slot — for stock::rounded_rect, this is `stroke`.
    pub slot_b: [f32; 4],
    /// `vec_c` slot — for stock::rounded_rect, this is
    /// `(stroke_width, radius, shadow, _)`.
    pub slot_c: [f32; 4],
}

#[derive(Clone, Copy)]
pub(crate) struct InstanceRun {
    pub handle: ShaderHandle,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PhysicalScissor {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Copy)]
pub(crate) enum PaintItem {
    QuadRun(usize),
    // Step 6 will add a `Text(usize)` arm.
}

pub(crate) fn close_run(
    runs: &mut Vec<InstanceRun>,
    paint_items: &mut Vec<PaintItem>,
    run_key: Option<(ShaderHandle, Option<PhysicalScissor>)>,
    first: u32,
    end: u32,
) {
    if let Some((handle, scissor)) = run_key {
        let count = end - first;
        if count > 0 {
            let index = runs.len();
            runs.push(InstanceRun {
                handle,
                scissor,
                first,
                count,
            });
            paint_items.push(PaintItem::QuadRun(index));
        }
    }
}

pub(crate) fn physical_scissor(
    scissor: Option<Rect>,
    scale: f32,
    viewport_px: (u32, u32),
) -> Option<PhysicalScissor> {
    let r = scissor?;
    let x1 = (r.x * scale).floor().clamp(0.0, viewport_px.0 as f32) as u32;
    let y1 = (r.y * scale).floor().clamp(0.0, viewport_px.1 as f32) as u32;
    let x2 = (r.right() * scale).ceil().clamp(0.0, viewport_px.0 as f32) as u32;
    let y2 = (r.bottom() * scale).ceil().clamp(0.0, viewport_px.1 as f32) as u32;
    Some(PhysicalScissor {
        x: x1,
        y: y1,
        w: x2.saturating_sub(x1),
        h: y2.saturating_sub(y1),
    })
}

/// Apply `scissor` (or `full` when `None`) to the given primary
/// command-buffer builder via vulkano's dynamic scissor state. The
/// pipeline must declare `Scissor` in its `dynamic_state`.
pub(crate) fn set_scissor(
    builder: &mut AutoCommandBufferBuilder<PrimaryAutoCommandBuffer>,
    scissor: Option<PhysicalScissor>,
    full: PhysicalScissor,
) {
    let s = scissor.unwrap_or(full);
    builder
        .set_scissor(
            0,
            smallvec![Scissor {
                offset: [s.x, s.y],
                extent: [s.w.max(1), s.h.max(1)],
            }],
        )
        .expect("set_scissor");
}

pub(crate) fn pack_instance(
    rect: Rect,
    shader: ShaderHandle,
    uniforms: &UniformBlock,
) -> QuadInstance {
    let rect_arr = [rect.x, rect.y, rect.w, rect.h];

    match shader {
        ShaderHandle::Stock(StockShader::RoundedRect) => QuadInstance {
            rect: rect_arr,
            slot_a: uniforms
                .get("fill")
                .and_then(as_color)
                .map(rgba_f32)
                .unwrap_or([0.0; 4]),
            slot_b: uniforms
                .get("stroke")
                .and_then(as_color)
                .map(rgba_f32)
                .unwrap_or([0.0; 4]),
            slot_c: [
                uniforms.get("stroke_width").and_then(as_f32).unwrap_or(0.0),
                uniforms.get("radius").and_then(as_f32).unwrap_or(0.0),
                uniforms.get("shadow").and_then(as_f32).unwrap_or(0.0),
                0.0,
            ],
        },
        ShaderHandle::Stock(StockShader::FocusRing) => QuadInstance {
            rect: rect_arr,
            slot_a: [0.0; 4],
            slot_b: uniforms
                .get("color")
                .and_then(as_color)
                .map(rgba_f32)
                .unwrap_or([0.0; 4]),
            slot_c: [
                uniforms.get("width").and_then(as_f32).unwrap_or(0.0),
                uniforms.get("radius").and_then(as_f32).unwrap_or(0.0),
                0.0,
                0.0,
            ],
        },
        _ => QuadInstance {
            rect: rect_arr,
            slot_a: uniforms.get("vec_a").map(value_to_vec4).unwrap_or([0.0; 4]),
            slot_b: uniforms.get("vec_b").map(value_to_vec4).unwrap_or([0.0; 4]),
            slot_c: uniforms.get("vec_c").map(value_to_vec4).unwrap_or([0.0; 4]),
        },
    }
}

fn as_color(v: &UniformValue) -> Option<Color> {
    match v {
        UniformValue::Color(c) => Some(*c),
        _ => None,
    }
}
fn as_f32(v: &UniformValue) -> Option<f32> {
    match v {
        UniformValue::F32(f) => Some(*f),
        _ => None,
    }
}

fn value_to_vec4(v: &UniformValue) -> [f32; 4] {
    match v {
        UniformValue::Color(c) => rgba_f32(*c),
        UniformValue::Vec4(a) => *a,
        UniformValue::Vec2([x, y]) => [*x, *y, 0.0, 0.0],
        UniformValue::F32(f) => [*f, 0.0, 0.0, 0.0],
        UniformValue::Bool(b) => [if *b { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
    }
}

pub(crate) fn rgba_f32(c: Color) -> [f32; 4] {
    [
        srgb_to_linear(c.r as f32 / 255.0),
        srgb_to_linear(c.g as f32 / 255.0),
        srgb_to_linear(c.b as f32 / 255.0),
        c.a as f32 / 255.0,
    ]
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
