// stock::surface — app-owned-texture compositing.
//
// One vertex stage; three fragment entry points, one per SurfaceAlpha
// mode. The backend builds three pipelines that share everything except
// the fragment entry point and the blend state — no per-instance switch.
//
// Per-instance data is just `rect.xy/zw` (logical pixels). Sampling
// uses `corner_uv` directly so the texture covers the rect 1:1; format-
// dependent decode (sRGB → linear) is handled by the texture view.

struct FrameUniforms {
    viewport: vec2<f32>,
    time: f32,
    scale_factor: f32,
};

@group(0) @binding(0) var<uniform> frame: FrameUniforms;
@group(1) @binding(0) var surf_tex: texture_2d<f32>;
@group(1) @binding(1) var surf_smp: sampler;

struct VertexInput {
    @location(0) corner_uv: vec2<f32>,
};

struct InstanceInput {
    @location(1) rect: vec4<f32>,  // xy = top-left logical px, zw = size logical px
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0)      uv:        vec2<f32>,
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
    out.uv       = in.corner_uv;
    return out;
}

// Premultiplied input, premultiplied output. Pairs with
// (One, OneMinusSrcAlpha) blend.
@fragment
fn fs_premul(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(surf_tex, surf_smp, in.uv);
}

// Straight (unpremultiplied) input — premultiply in the shader before
// blending. Pairs with (One, OneMinusSrcAlpha) blend.
@fragment
fn fs_straight(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(surf_tex, surf_smp, in.uv);
    return vec4<f32>(s.rgb * s.a, s.a);
}

// Opaque — alpha channel of the texture is ignored; output replaces
// the destination. Pairs with (One, Zero) blend or no blend at all.
@fragment
fn fs_opaque(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = textureSample(surf_tex, surf_smp, in.uv);
    return vec4<f32>(s.rgb, 1.0);
}
