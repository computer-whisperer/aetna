// stock::rounded_rect — the workhorse surface shader.
//
// One pipeline. Per-instance uniforms drive fill, stroke, radius. The
// quad is sized to `rect` (the painted rect, possibly outset for
// `paint_overflow`); SDF + outline use `inner_rect` so the rect stays
// anchored to the layout bounds even when the painted area extends
// outward. Focus ring renders in the band between inner_rect and rect
// when `focus_color.a * focus_alpha > 0`.
//
// Coordinate system: vertex positions arrive as a unit quad with uv 0..1.
// Per-instance `rect` (xy, wh) places it in pixel space; the vertex
// shader maps to clip space via the frame's `viewport` (width, height).
// The fragment shader computes the rounded-box SDF in centered pixel
// coordinates relative to `inner_rect` and antialiases at the boundary
// using the SDF gradient magnitude (length of dpdx/dpdy).

struct FrameUniforms {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:        vec4<f32>,  // painted rect: xy = top-left px, zw = size px
    @location(2) fill:        vec4<f32>,  // rgba 0..1
    @location(3) stroke:      vec4<f32>,  // rgba 0..1
    @location(4) params:      vec4<f32>,  // x=stroke_width, y=radius, z=shadow, w=focus_width
    @location(5) inner_rect:  vec4<f32>,  // layout rect (== rect when no paint_overflow)
    @location(6) focus_color: vec4<f32>,  // rgba 0..1, alpha already eased
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      pos_px: vec2<f32>,       // pixel-space position (top-left origin)
    @location(1)      inner_center: vec2<f32>,
    @location(2)      inner_half_size: vec2<f32>,
    @location(3)      fill: vec4<f32>,
    @location(4)      stroke: vec4<f32>,
    @location(5)      params: vec4<f32>,
    @location(6)      focus_color: vec4<f32>,
};

@vertex
fn vs_main(in: VertexInput, inst: InstanceInput) -> VertexOutput {
    let pos_px = in.corner_uv * inst.rect.zw + inst.rect.xy;
    let clip = vec4<f32>(
        pos_px.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - pos_px.y / frame.viewport.y * 2.0,
        0.0,
        1.0,
    );

    var out: VertexOutput;
    out.clip_pos = clip;
    out.pos_px = pos_px;
    out.inner_center = inst.inner_rect.xy + inst.inner_rect.zw * 0.5;
    out.inner_half_size = inst.inner_rect.zw * 0.5;
    out.fill = inst.fill;
    out.stroke = inst.stroke;
    out.params = inst.params;
    out.focus_color = inst.focus_color;
    return out;
}

// SDF for a centered rounded box. Returns signed distance: negative
// inside, zero on the boundary, positive outside.
fn sdf_rounded_box(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let stroke_width = in.params.x;
    let radius = min(in.params.y, min(in.inner_half_size.x, in.inner_half_size.y));
    let focus_width = in.params.w;

    // Pixel position relative to the inner (layout) rect's centre.
    let local_px = in.pos_px - in.inner_center;
    let d = sdf_rounded_box(local_px, in.inner_half_size, radius);

    // Anti-aliasing width derived from screen-space derivatives. Use the
    // L2 norm of the SDF gradient (not fwidth's L1) so the AA band is the
    // same width on diagonals as on axis-aligned edges — fwidth would be
    // sqrt(2)x larger at 45° and visibly fatten the rounded corners.
    let aa = max(length(vec2<f32>(dpdx(d), dpdy(d))), 0.5);

    // Inside coverage of the layout rect.
    let inside = 1.0 - smoothstep(-aa, 0.0, d);

    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (in.fill.a > 0.0) {
        color = vec4<f32>(in.fill.rgb, in.fill.a * inside);
    }

    // Stroke: a band of width stroke_width centered on the boundary of
    // the layout rect.
    if (stroke_width > 0.0 && in.stroke.a > 0.0) {
        let stroke_d = abs(d) - stroke_width * 0.5;
        let stroke_alpha = (1.0 - smoothstep(-aa, aa, stroke_d)) * in.stroke.a;
        color = vec4<f32>(
            mix(color.rgb, in.stroke.rgb, stroke_alpha),
            max(color.a, stroke_alpha),
        );
    }

    // Focus ring: a band of width `focus_width` centered just outside
    // the layout rect's boundary, so it lives in the paint_overflow
    // halo. Alpha is the eased per-frame focus envelope (already
    // baked into focus_color.a by draw_ops).
    if (focus_width > 0.0 && in.focus_color.a > 0.0) {
        let ring_center = focus_width * 0.5;
        let ring_d = abs(d - ring_center) - focus_width * 0.5;
        let ring_alpha = (1.0 - smoothstep(-aa, aa, ring_d)) * in.focus_color.a;
        color = vec4<f32>(
            mix(color.rgb, in.focus_color.rgb, ring_alpha),
            max(color.a, ring_alpha),
        );
    }

    return color;
}
