//! Paint-stream types and helpers shared by every backend.
//!
//! The `QuadInstance` ABI is the cross-backend contract: every
//! rect-shaped pipeline (stock or custom) reads the same 4 × `vec4<f32>`
//! layout, so the layout pass's logical-pixel rects compose with each
//! backend's GPU pipelines without per-backend tweaking. `aetna-wgpu`
//! and `aetna-vulkano` build different pipelines around it; the bytes
//! the vertex shader sees are identical.
//!
//! `PaintItem` + `InstanceRun` + [`close_run`] are the paint-stream
//! batching shape: walk the [`crate::DrawOp`] list, pack `Quad`s into
//! the instance buffer in groups of consecutive same-pipeline +
//! same-scissor runs, intersperse text layers in their original
//! z-order. Both backends consume this exactly the same way.
//!
//! The one paint concern this module *doesn't* own is `set_scissor` —
//! that one needs the backend-specific encoder type, so each backend
//! keeps a thin `set_scissor` of its own.

use bytemuck::{Pod, Zeroable};

use crate::shader::{ShaderHandle, StockShader, UniformBlock, UniformValue};
use crate::tree::{Color, Rect};

/// One instance of a rect-shaped shader. Layout is shared between
/// `stock::rounded_rect`, `stock::focus_ring`, and any custom shader
/// registered via the host's `register_shader`. The fragment shader
/// interprets the three vec4 slots however it wants; the vertex shader
/// needs `rect` to place the unit quad in pixel space.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct QuadInstance {
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

/// A contiguous run of instances drawn with the same pipeline + scissor.
/// Built in tree order so a custom shader sandwiched between two stock
/// surfaces is drawn at the right z-position.
#[derive(Clone, Copy)]
pub struct InstanceRun {
    pub handle: ShaderHandle,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
}

/// Scissor in **physical pixels** (host swapchain extent), already
/// clamped to the surface and snapped to integer pixel boundaries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysicalScissor {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Sequencing entry for the recorded paint stream — either a quad-run
/// (indexed into the runner's `runs` vector) or a text layer (indexed
/// into the runner's per-backend `TextLayer` vector).
#[derive(Clone, Copy)]
pub enum PaintItem {
    QuadRun(usize),
    Text(usize),
}

/// Close the current run and append it to `runs` + `paint_items`. No-op
/// when `run_key` is `None` or the run is empty.
pub fn close_run(
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

/// Convert a logical-pixel scissor to physical pixels, clamping to the
/// physical viewport. Returns `None` when the input is `None`.
pub fn physical_scissor(
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

/// Pack a quad's uniforms into the shared `QuadInstance` layout. Stock
/// `rounded_rect` reads its named uniforms; everything else reads the
/// generic `vec_a`/`vec_b`/`vec_c` slots.
pub fn pack_instance(rect: Rect, shader: ShaderHandle, uniforms: &UniformBlock) -> QuadInstance {
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

/// Coerce any `UniformValue` into the four floats of a vec4 slot.
/// Custom-shader authors typically pass `Color` (rgba) or `Vec4`
/// (arbitrary semantics); `F32` packs into `.x` so a single scalar like
/// `radius` doesn't need a Vec4 wrapper.
fn value_to_vec4(v: &UniformValue) -> [f32; 4] {
    match v {
        UniformValue::Color(c) => rgba_f32(*c),
        UniformValue::Vec4(a) => *a,
        UniformValue::Vec2([x, y]) => [*x, *y, 0.0, 0.0],
        UniformValue::F32(f) => [*f, 0.0, 0.0, 0.0],
        UniformValue::Bool(b) => [if *b { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
    }
}

/// Convert a token sRGB color to the four linear floats the shader
/// reads. Tokens are authored in sRGB display space; the surface is an
/// *Srgb format so alpha blending happens in linear space (correct
/// for color blending, slightly fattens light-on-dark text).
pub fn rgba_f32(c: Color) -> [f32; 4] {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::UniformBlock;
    use crate::tokens;

    #[test]
    fn focus_ring_uniforms_pack_into_rounded_rect_layout() {
        let mut uniforms = UniformBlock::new();
        uniforms.insert("color", UniformValue::Color(tokens::FOCUS_RING));
        uniforms.insert("width", UniformValue::F32(2.0));
        uniforms.insert("radius", UniformValue::F32(9.0));

        let inst = pack_instance(
            Rect::new(1.0, 2.0, 30.0, 40.0),
            ShaderHandle::Stock(StockShader::FocusRing),
            &uniforms,
        );

        assert_eq!(inst.rect, [1.0, 2.0, 30.0, 40.0]);
        assert_eq!(inst.slot_a, [0.0; 4]);
        assert!(inst.slot_b[3] > 0.0, "focus ring stroke should be visible");
        assert_eq!(inst.slot_c[0], 2.0);
        assert_eq!(inst.slot_c[1], 9.0);
    }

    #[test]
    fn physical_scissor_converts_logical_to_physical_pixels() {
        let scissor = physical_scissor(Some(Rect::new(10.2, 20.2, 30.2, 40.2)), 2.0, (200, 200))
            .expect("scissor");

        assert_eq!(
            scissor,
            PhysicalScissor {
                x: 20,
                y: 40,
                w: 61,
                h: 81
            }
        );
    }
}
