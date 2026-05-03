// stock::rounded_rect — the workhorse surface shader.
//
// One pipeline. Per-instance uniforms drive fill, stroke, radius. Shadow
// uniform is reserved (v0.1 ignores it; v0.2 will widen the quad and
// blur outside the SDF).
//
// Coordinate system: vertex positions arrive as a unit quad with uv 0..1.
// Per-instance `rect` (xy, wh) places it in pixel space; the vertex
// shader maps to clip space via the frame's `viewport` (width, height).
// The fragment shader computes the rounded-box SDF in centered pixel
// coordinates and antialiases at the boundary using fwidth.

struct FrameUniforms {
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect:        vec4<f32>,  // xy = top-left px, zw = size px
    @location(2) fill:        vec4<f32>,  // rgba 0..1
    @location(3) stroke:      vec4<f32>,  // rgba 0..1
    @location(4) params:      vec4<f32>,  // x=stroke_width, y=radius, z=shadow, w=_pad
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      local_px: vec2<f32>,  // signed, centered on rect
    @location(1)      half_size: vec2<f32>,
    @location(2)      fill: vec4<f32>,
    @location(3)      stroke: vec4<f32>,
    @location(4)      params: vec4<f32>,
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

    let half_size = inst.rect.zw * 0.5;
    let local_px = (in.corner_uv - vec2<f32>(0.5, 0.5)) * inst.rect.zw;

    var out: VertexOutput;
    out.clip_pos = clip;
    out.local_px = local_px;
    out.half_size = half_size;
    out.fill = inst.fill;
    out.stroke = inst.stroke;
    out.params = inst.params;
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
    let radius = min(in.params.y, min(in.half_size.x, in.half_size.y));

    let d = sdf_rounded_box(in.local_px, in.half_size, radius);

    // Anti-aliasing width derived from screen-space derivatives.
    let aa = max(fwidth(d), 0.5);

    // Inside coverage.
    let inside = 1.0 - smoothstep(-aa, 0.0, d);

    var color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    if (in.fill.a > 0.0) {
        color = vec4<f32>(in.fill.rgb, in.fill.a * inside);
    }

    // Stroke: a band of width stroke_width centered on the boundary.
    if (stroke_width > 0.0 && in.stroke.a > 0.0) {
        let stroke_d = abs(d) - stroke_width * 0.5;
        let stroke_alpha = (1.0 - smoothstep(-aa, aa, stroke_d)) * in.stroke.a;
        // stroke paints over fill near the boundary.
        color = vec4<f32>(
            mix(color.rgb, in.stroke.rgb, stroke_alpha),
            max(color.a, stroke_alpha),
        );
    }

    return color;
}
