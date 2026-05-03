//! Per-instance vertex packing, paint-stream batching, and scissor math.
//!
//! [`QuadInstance`] is the GPU layout shared by `stock::rounded_rect`,
//! `stock::focus_ring`, and any custom shader registered through
//! [`crate::Runner::register_shader`]. [`InstanceRun`] groups consecutive
//! instances with the same pipeline + scissor so the draw loop can issue
//! one `pass.draw(...)` call per run while still preserving paint order.

use bytemuck::{Pod, Zeroable};

use aetna_core::shader::{ShaderHandle, StockShader, UniformBlock, UniformValue};
use aetna_core::tree::{Color, Rect};

/// One instance of a rect-shaped shader. Layout is shared between
/// `stock::rounded_rect` and any custom shader registered via
/// [`crate::Runner::register_shader`]. The fragment shader interprets
/// the three vec4 slots however it wants; the vertex shader needs `rect`
/// to place the unit quad in pixel space.
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

/// A contiguous run of instances drawn with the same pipeline. Built in
/// tree order so a custom shader sandwiched between two stock surfaces
/// is drawn at the right z-position.
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

/// Sequencing entry for the recorded paint stream — either a quad-run
/// (indexed into [`crate::Runner::runs`]) or a text layer (indexed into
/// the [`crate::text::TextLayer`] vector).
#[derive(Clone, Copy)]
pub(crate) enum PaintItem {
    QuadRun(usize),
    Text(usize),
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

pub(crate) fn set_scissor(
    pass: &mut wgpu::RenderPass<'_>,
    scissor: Option<PhysicalScissor>,
    full: PhysicalScissor,
) {
    let s = scissor.unwrap_or(full);
    pass.set_scissor_rect(s.x, s.y, s.w, s.h);
}

/// Pack a quad's uniforms into the shared `QuadInstance` layout. Stock
/// `rounded_rect` reads its named uniforms; everything else reads the
/// generic `vec_a`/`vec_b`/`vec_c` slots.
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

pub(crate) fn rgba_f32(c: Color) -> [f32; 4] {
    // Tokens are authored in sRGB display space; the surface is an
    // *Srgb format so alpha blending happens in linear space (correct
    // for color blending, slightly fattens light-on-dark text — see
    // the font notes in the runner module docs).
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
    use aetna_core::shader::UniformBlock;
    use aetna_core::tokens;

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
