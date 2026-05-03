# Aetna — Shader Vision

> *Aetna's manifesto for the rendering layer. The premises were settled during the attempt_4 milestone (`attempts/attempt_4/`); references to attempt_4 and the v0.X roadmap below describe the historical slice progression that walked the vision into working code. The vision itself carries forward to Aetna proper.*

This document captures Aetna's design intent: a wgpu-first UI library where shader-based rendering is first-class. attempt_3 settled the grammar layer (layout, tokens, style profiles, source-mapped lint, bundle pipeline). attempt_4 kept that grammar largely intact and **rebuilt the rendering layer** so that what looks like "css concerns" in other libraries — gradients, shadows, frosted glass, hover transitions, custom shapes — are shader concerns here.

attempt_3 validated the grammar with a cold-LLM-session test (a fresh sub-agent produced a polished login screen one-shot, lint-clean, with no prior context). What attempt_3 didn't test, and can't test from its `RenderCmd::Rect { fill, stroke, radius, shadow }` IR, is the visual ceiling. That ceiling is what attempt_4 raises.

## Four load-bearing premises

1. **wgpu/vulkano is the only production backend.** SVG and tree-dump artifacts are agent-loop feedback; production paint is GPU. The IR can be gpu-shaped — pipelines, bind groups, push constants — instead of abstractly portable.

2. **LLMs can write good shader code.** Most CSS, and most "graphics layer" UI library design, exists because the average human web dev can't write a fragment shader. LLMs can. The library can therefore **delegate visual concerns to shaders** — both shipped stock shaders and user-authored ones.

3. **Insert into an existing render pass.** UI is one part of a host's larger frame — a 3D viewport, a game scene, a viz tool. The library does not own the device, queue, swapchain, surface, or the render pass. It records draw commands into the host's command encoder.

4. **Floor and ceiling.** A pretty dialog should be one-line trivial. **Liquid glass** should be possible — not as a built-in library mode, but as something a user crate can build inside this library. The library is the substrate that makes that possible.

## Backend matrix

| Backend | v0.1 | First-class target |
|---|---|---|
| wgpu native | yes (primary) | yes |
| wgpu wasm | not yet | yes — design for this from day 1 |
| vulkano native | not yet | yes — design for this from day 1 |

We commit to the day-1 architectural constraint that **shader source must cross-target wgpu (wgsl-or-spirv) and vulkano (spirv) from a single source**, even though wgpu native is the only backend wired in v0.1. That constraint shapes the shader-language choice and the host-integration surface: no native-only deps, no wgpu-internals leaking into the public API.

## IR shape

```rust
enum DrawOp {
    Quad {
        rect: Rect,
        scissor: Option<Rect>,
        shader: ShaderHandle,
        uniforms: UniformBlock,
    },
    GlyphRun {
        rect: Rect,
        scissor: Option<Rect>,
        shader: ShaderHandle,    // typically stock::text_sdf
        uniforms: UniformBlock,  // color, atlas binding, etc.
        glyphs: Arc<[GlyphInstance]>,
    },
    BackdropSnapshot {
        // Mid-frame copy of the current target into a sampled texture,
        // for shaders downstream that bind it as `backdrop`.
    },
}
```

There is no `fill: Color` field. Color is a uniform on `stock::rounded_rect`. There is no `radius`, no `shadow`, no `stroke`. All of those are uniforms passed to a stock shader. Tokens (`tokens::PRIMARY`, `tokens::RADIUS_LG`, …) carry their uniform-binding metadata — the user-facing API still says `.primary()`; the uniform plumbing is invisible.

`UniformBlock` is a typed struct keyed by name, packed to a `Vec<u8>` at record time using a registered uniform layout descriptor per shader.

## Stock shaders — v1 inventory

Five shipped stock shaders, named, versioned, source-readable:

- `stock::solid_quad` — flat colored rectangle. Fallback / debug / opaque divider.
- `stock::rounded_rect` — fill + stroke + radius + shadow + optional linear gradient. Handles ~80% of UI surfaces.
- `stock::text_sdf` — SDF glyph rendering, backed by glyphon for shaping/atlas in v1.
- `stock::focus_ring` — animated focus indicator.
- `stock::divider_line` — antialiased 1px line.

**Discipline: uniform proliferation, not shader proliferation.** A new card-surface variant is a different uniform combination on `rounded_rect`, not a new shader. We add a sixth stock shader only when something demonstrably can't be expressed as uniforms on the existing five.

## Shader language — open decision

The constraint: one shader source must produce both wgsl-or-spirv (for wgpu) and spirv (for vulkano).

**Option A: slang.** NVIDIA/Khronos shader language with native cross-compilation to spirv, wgsl, hlsl, metal. Modern features (interfaces, generics) suit shader composition. Adds a `slangc` build dependency. LLM training data on slang is thinner than on wgsl/glsl/hlsl.

**Option B: wgsl + naga.** Author in wgsl; `naga` (already in the wgpu stack) translates wgsl → spirv for vulkano. No extra build dependency. LLMs have seen far more wgsl in training. Caps expressiveness at wgsl's feature set; no shader-level composition primitives, only uniform-based variation.

**Lean: B (wgsl+naga) for v0.1, with slang as a v2 escalation trigger.**

Reasons:
- LLM authoring is the load-bearing use case (premise 2). LLMs write more idiomatic wgsl/glsl/hlsl than slang today.
- naga is already on the dependency tree. No extra binary on user machines.
- For v1's stock-shader inventory, uniform-based variation is sufficient — we don't need slang's interface system yet.
- The escalation trigger is concrete: when a user crate hits a shader-composition wall (e.g., wants to inherit `stock::rounded_rect`'s SDF math and override only the fragment color), we revisit slang.

This is the **only** part of attempt_4 we should be willing to revisit cheaply. Final commitment happens when the first stock shader is written.

## Backdrop sampling architecture

For frosted glass, liquid glass, parallax, distortion — anything sampling already-rendered content — a fragment shader needs the backdrop bound as a sampled texture. This is committed day 1 as a multi-pass architecture:

1. **Pass A — opaque.** Every node whose shader does not bind the backdrop.
2. **Snapshot.** Copy/blit the current target into a sampled texture.
3. **Pass B — backdrop-sampling.** Nodes whose shader binds `backdrop`. Sample, distort, refract, recompose.
4. **Pass C — foreground.** Anything painted on top of glass-class surfaces (e.g., text inside a glass card).

A node opts into backdrop sampling via `ShaderHandle` metadata (the registered shader declares `samples_backdrop: bool`). The renderer schedules it into Pass B and binds the snapshot texture in its uniform block.

Multiple backdrop layers (glass on glass) become A → B1 → B2 → C. v0.1 caps depth at 1 (one snapshot, one B pass). Increase when needed.

## Host integration surface

```rust
let mut renderer = UiRenderer::new(&device, queue, target_format, &Config::default())?;
renderer.register_shader(
    "hexagon",
    include_str!("../shaders/hexagon.wgsl"),
    HexagonUniforms::layout(),
)?;

// per frame, inside the host's encoder:
renderer.layout(&mut root, viewport);
renderer.draw(&mut encoder, &mut render_pass, &root)?;
```

**Library owns:** pipeline cache, shader cache, glyph atlas, per-frame uniform/vertex/index buffers, layout cache, intermediate snapshot textures.

**Library does not own:** device, queue, swapchain, surface, the render pass, color/depth attachments, present timing, input event delivery.

A separate `attempt_4_demo` crate provides a winit-based standalone harness so authors can run UI fixtures without a host application. The core crate has zero winit dependency.

## Bundle pipeline updates

attempt_3's bundle (SVG, tree dump, render commands, lint) extends:

- `draw_ops.txt` — gpu-shaped IR, replaces `commands.txt`.
- `shader_manifest.txt` — every shader used by this tree, with source path, declared uniforms, and resolved uniform values per draw.
- `shader_compile.txt` — per-shader compile diagnostics from naga (and later slangc), so the agent loop sees compile errors as readable text alongside the layout/lint feedback.
- SVG remains, but explicitly approximate. Stock shaders have hand-tuned SVG approximations; custom shaders render as labeled placeholder rects with metadata in a `<title>` element.

Agent loop shape is unchanged: edit Rust → `tools/render` → look at PNG + lint + shader manifest + compile diagnostics. New: when an LLM authors a custom shader, the manifest tells them whether it compiled and how its uniforms were resolved.

## Liquid glass as forcing function

Liquid glass is **not** a v1 component. It is the architectural acceptance test: the substrate must make a liquid glass widget expressible by a user crate, with these capabilities present:

- Backdrop sampling (Pass B).
- Bind a texture as a uniform (displacement / normal map).
- Time uniform for animation.
- The user authors `liquid_glass.wgsl` (or `.slang`).
- Their `pub fn liquid_glass_card(...)` registers the shader and returns an `El` with `shader: ShaderHandle::custom("liquid_glass", uniforms)`.

We validate this once the v0.1 substrate works — that's the milestone for "the architecture is real." If we can't build liquid glass on top of attempt_4 in a single user-crate file with a single `.wgsl` shader, the architecture is wrong.

## What carries forward from attempt_3

Largely intact:
- Tree shape (`El`, `Kind`, `Sides`, `Size`, `Axis`, `Align`, `Justify`).
- Layout pass (`column`, `row`, `stack`, `Fill`/`Hug`/`Fixed` sizing, intrinsic sizing).
- Token vocabulary (`tokens::PRIMARY`, `tokens::SPACE_MD`, etc.) — though token *types* extend to carry uniform-binding metadata.
- Style-profile dispatch (`StyleProfile::Solid/Tinted/Surface/TextOnly`, `.primary()`/`.muted()`/etc.).
- `#[track_caller]` source mapping.
- Lint pass (with provenance).
- Bundle pipeline shape (artifacts emitted in one call).

Replaced:
- IR (`RenderCmd::Rect/Text` → `DrawOp::Quad/GlyphRun/BackdropSnapshot`).
- Renderer (SVG-as-primary → wgpu-as-primary; SVG demoted to approximate fixture).
- Style → render bridge (visual properties on `El` → uniform values bound to a shader handle).
- State styling (in-renderer color deltas → shader-side or uniform-side).

Carry-forward bug fixes (caught by attempt_3's cold-session test):
- `Justify::Center` / `Justify::End` — math was wrong when no `Fill` children present.
- `text(...).align()` silently does nothing — either remove or wire to text-anchor uniform.

## Initial scope — v0.1

Concretely, the first working pass:

1. Carry forward attempt_3's grammar layer with the bug fixes above.
2. Replace `RenderCmd` with `DrawOp`.
3. Implement `UiRenderer` for wgpu native, with one stock shader: `stock::rounded_rect.wgsl`.
4. Wire glyphon for text via `stock::text_sdf`.
5. Reproduce attempt_3's `settings` example. Visual parity (or better) with attempt_3 is the v0.1 milestone.
6. Demo harness: `attempt_4_demo` crate with winit window.
7. Bundle pipeline emits `draw_ops.txt` and `shader_manifest.txt`. SVG approximation kept.

Deferred until v0.1 lands:
- Backdrop sampling (Pass B) wiring — architecture committed, implementation later.
- Vulkano backend.
- wgpu-wasm host integration.
- `solid_quad`, `focus_ring`, `divider_line` stock shaders (rounded_rect can fake them).
- Custom-shader registration API for user crates.

## Validation question

Can a fresh LLM session, given attempt_4, author a custom shader-rendered component (e.g., a circular progress widget, or a procedural hex-tile button) on the first try, with the bundle pipeline as feedback? That is the analogue of attempt_3's first-shot polish bar, scaled to the new ceiling.

Until that succeeds, attempt_4 hasn't earned its premise.
