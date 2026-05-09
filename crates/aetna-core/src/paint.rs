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
use crate::vector::IconMaterial;

/// One instance of a rect-shaped shader. Layout is shared between
/// `stock::rounded_rect` and any custom shader registered via the host's
/// `register_shader`. The fragment shader interprets the slots however
/// it wants; the vertex shader uses `rect` to place the unit quad in
/// pixel space.
///
/// `inner_rect` is the original layout rect — equal to `rect` when
/// `paint_overflow` is zero, smaller (set inside `rect`) when the
/// element has opted into painting outside its bounds. SDF shaders
/// anchor their geometry to `inner_rect` so the rounded outline stays
/// where layout placed it; the overflow band is where focus rings,
/// drop shadows, and other halos render.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct QuadInstance {
    /// Painted rect — xy = top-left px, zw = size px. Equal to
    /// `inner_rect` when no `paint_overflow`. Vertex shader reads at
    /// `@location(1)`.
    pub rect: [f32; 4],
    /// `vec_a` slot — for stock::rounded_rect, this is `fill`. Vertex
    /// shader reads at `@location(2)`.
    pub slot_a: [f32; 4],
    /// `vec_b` slot — for stock::rounded_rect, this is `stroke`.
    /// Vertex shader reads at `@location(3)`.
    pub slot_b: [f32; 4],
    /// `vec_c` slot — for stock::rounded_rect, this is
    /// `(stroke_width, radius, shadow, focus_width)`. Vertex shader
    /// reads at `@location(4)`.
    pub slot_c: [f32; 4],
    /// Layout rect (xy = top-left px, zw = size px). SDF shaders use
    /// this so the rect outline stays anchored to layout bounds even
    /// when `rect` has been outset for `paint_overflow`. Vertex shader
    /// reads at `@location(5)` — declared *after* the legacy slots so
    /// custom shaders that only consume locations 1..=4 keep working
    /// unchanged.
    pub inner_rect: [f32; 4],
    /// `vec_d` slot — for stock::rounded_rect, this is the ring
    /// color (rgba) with eased alpha already multiplied in. Zero when
    /// the node isn't focused or isn't focusable. Vertex shader reads
    /// at `@location(6)`.
    pub slot_d: [f32; 4],
}

/// One line-segment primitive in a vector icon. The instance renders a
/// single antialiased stroke into `rect`; higher-level icon paths are
/// flattened into runs of these records by the backend recorder.
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable, Debug)]
pub struct IconInstance {
    /// Painted bounds for the segment, outset for stroke width and AA.
    /// Vertex shader reads at `@location(1)`.
    pub rect: [f32; 4],
    /// Segment endpoints in logical px: `(x0, y0, x1, y1)`.
    /// Fragment shader reads at `@location(2)`.
    pub line: [f32; 4],
    /// Linear rgba color. Fragment shader reads at `@location(3)`.
    pub color: [f32; 4],
    /// `(stroke_width, reserved, reserved, reserved)`.
    /// Fragment shader reads at `@location(4)`.
    pub params: [f32; 4],
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

/// Which icon-draw path a backend uses for this run.
///
/// `Tess` runs index into the backend's tessellated vector mesh
/// (vertex range, expanded triangles). `Msdf` runs index into the
/// backend's per-instance MSDF buffer (one entry = one icon quad) and
/// must bind the atlas page identified by `IconRun::page`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconRunKind {
    Tess,
    Msdf,
}

/// A contiguous run of backend-owned icon draws sharing a scissor.
///
/// For `Tess` runs, `first..first+count` is a vertex range in the
/// backend's vector-mesh buffer and `material` selects the fragment
/// shader (flat / relief / glass). For `Msdf` runs, `first..first+count`
/// is an instance range in the backend's MSDF instance buffer; `page`
/// names the atlas page to bind. `material` is always `Flat` for MSDF
/// runs — non-flat materials need the per-fragment local view-box
/// coordinate that the tessellated path provides, so they stay on the
/// `Tess` route.
#[derive(Clone, Copy)]
pub struct IconRun {
    pub kind: IconRunKind,
    pub scissor: Option<PhysicalScissor>,
    pub first: u32,
    pub count: u32,
    pub page: u32,
    pub material: IconMaterial,
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

/// Sequencing entry for the recorded paint stream.
///
/// - `QuadRun(idx)` — a contiguous instance run (indexed into `runs`).
/// - `IconRun(idx)` — a vector icon run (backend-owned storage,
///   indexed by the wgpu icon painter; other backends may keep using
///   text fallback and never emit this item).
/// - `Text(idx)` — a glyph layer (indexed into the backend's
///   `TextLayer` vector).
/// - `BackdropSnapshot` — a pass boundary. The backend ends the
///   current render pass, copies the current target into its managed
///   snapshot texture, and begins a new pass with `LoadOp::Load` so
///   subsequent quads can sample the snapshot via the `backdrop` bind
///   group. At most one of these is emitted per frame, inserted by
///   [`crate::runtime::RunnerCore::prepare_paint`] immediately before
///   the first quad bound to a `samples_backdrop` shader.
#[derive(Clone, Copy)]
pub enum PaintItem {
    QuadRun(usize),
    IconRun(usize),
    Text(usize),
    /// One raster image draw. Indexes into the backend's
    /// `ImagePaint`-equivalent storage. Produced by
    /// [`crate::runtime::TextRecorder::record_image`] from a
    /// [`crate::ir::DrawOp::Image`].
    Image(usize),
    /// One app-owned-texture composite. Indexes into the backend's
    /// `SurfacePaint`-equivalent storage. Produced by the backend's
    /// surface recorder from a [`crate::ir::DrawOp::AppTexture`].
    AppTexture(usize),
    BackdropSnapshot,
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
/// generic `vec_a`/`vec_b`/`vec_c`/`vec_d` slots. `inner_rect` falls
/// back to `rect` when the uniform isn't supplied — i.e. when the node
/// has no `paint_overflow`.
pub fn pack_instance(rect: Rect, shader: ShaderHandle, uniforms: &UniformBlock) -> QuadInstance {
    let rect_arr = [rect.x, rect.y, rect.w, rect.h];
    let inner_rect = uniforms
        .get("inner_rect")
        .map(value_to_vec4)
        .unwrap_or(rect_arr);

    match shader {
        ShaderHandle::Stock(StockShader::RoundedRect) => QuadInstance {
            rect: rect_arr,
            inner_rect,
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
                uniforms.get("focus_width").and_then(as_f32).unwrap_or(0.0),
            ],
            slot_d: uniforms
                .get("focus_color")
                .and_then(as_color)
                .map(rgba_f32)
                .unwrap_or([0.0; 4]),
        },
        _ => QuadInstance {
            rect: rect_arr,
            inner_rect,
            slot_a: uniforms.get("vec_a").map(value_to_vec4).unwrap_or([0.0; 4]),
            slot_b: uniforms.get("vec_b").map(value_to_vec4).unwrap_or([0.0; 4]),
            slot_c: uniforms.get("vec_c").map(value_to_vec4).unwrap_or([0.0; 4]),
            slot_d: uniforms.get("vec_d").map(value_to_vec4).unwrap_or([0.0; 4]),
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
    fn focus_uniforms_pack_into_rounded_rect_slots() {
        // Focus ring rides on the node's own RoundedRect quad: focus_color
        // packs into slot_d (rgba) and focus_width into slot_c.w (the
        // params slot's previously-padding lane).
        let mut uniforms = UniformBlock::new();
        uniforms.insert("fill", UniformValue::Color(Color::rgba(40, 40, 40, 255)));
        uniforms.insert("radius", UniformValue::F32(8.0));
        uniforms.insert("focus_color", UniformValue::Color(tokens::RING));
        uniforms.insert("focus_width", UniformValue::F32(tokens::RING_WIDTH));

        let inst = pack_instance(
            Rect::new(1.0, 2.0, 30.0, 40.0),
            ShaderHandle::Stock(StockShader::RoundedRect),
            &uniforms,
        );

        assert_eq!(inst.rect, [1.0, 2.0, 30.0, 40.0]);
        assert_eq!(
            inst.inner_rect, inst.rect,
            "no inner_rect uniform → fall back to painted rect"
        );
        assert_eq!(inst.slot_c[1], 8.0, "radius in slot_c.y");
        assert_eq!(
            inst.slot_c[3],
            tokens::RING_WIDTH,
            "focus_width in slot_c.w"
        );
        assert!(inst.slot_d[3] > 0.0, "focus_color alpha should be visible");
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
