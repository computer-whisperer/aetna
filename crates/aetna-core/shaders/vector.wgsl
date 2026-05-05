// stock::vector — tessellated SVG/vector geometry with analytic edge AA.
//
// CPU-side SVG assets are normalised into Aetna vector IR and tessellated
// into triangles. Each fill is accompanied by a thin band along the
// boundary whose outer verts carry a unit normal in `aa`; the vertex
// shader extrudes those verts by one physical pixel using
// `frame.scale_factor` so the AA fringe stays one screen pixel wide
// regardless of icon render size, and the fragment fades coverage from
// the fill edge to zero across that pixel.

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    scale_factor: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;

struct VertexInput {
    @location(0) pos_px: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) data: vec4<f32>,
    @location(4) aa: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local: vec2<f32>,
    @location(2) data: vec4<f32>,
    @location(3) coverage: f32,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    // 1 physical pixel expressed in logical units.
    let px_in_logical = 1.0 / max(frame.scale_factor, 0.001);
    let pos_extruded = in.pos_px + in.aa * px_in_logical;
    let clip = vec4<f32>(
        pos_extruded.x / frame.viewport.x * 2.0 - 1.0,
        1.0 - pos_extruded.y / frame.viewport.y * 2.0,
        0.0,
        1.0,
    );

    // Verts on the fill body have aa == 0 → coverage 1; fringe outer
    // verts have aa != 0 → coverage 0. Linear interp gives the 1-px
    // alpha ramp.
    let coverage = select(0.0, 1.0, all(in.aa == vec2<f32>(0.0, 0.0)));

    var out: VertexOutput;
    out.clip_pos = clip;
    out.color = in.color;
    out.local = in.local;
    out.data = in.data;
    out.coverage = coverage;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = in.color.a * in.coverage;
    return vec4<f32>(in.color.rgb * alpha, alpha);
}
