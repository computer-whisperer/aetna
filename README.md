# Aetna

A thin UI library that inserts into an existing Vulkan or wgpu renderer rather than owning the device, queue, or swapchain. The name echoes the API it sits on — Vulkan is named for Vulcan, the Roman smith-god, and Mt. Aetna is the volcano where his forge stood. The library doesn't replace the host's renderer; it shares its pass.

Aetna is shaped around how **an LLM** authors UI, not how a human web developer does. The thesis: when the author is a model, the load-bearing constraints flip — vocabulary parity with the training distribution matters more than configurability, the *minimum* output should be the *correct* output, and the visual ceiling is set by what shaders the model can write, not by what the framework's CSS-shaped surface exposes.

Two manifesto documents stand at the repo root — read these before reviewing. They are deliberately independent:

- **`SHADER_VISION.md`** — the *rendering* layer. Why we paint UI through wgpu pipelines, why CSS-shaped concerns (gradients, shadows, frosted glass) become shader concerns here, why the library inserts into the host's existing render pass rather than owning the device/queue/swapchain. Argues that LLMs writing shaders is the ceiling-raiser.
- **`LIBRARY_VISION.md`** — the *application* layer. The shape: a declarative scene library that projects time-varying state into a tree, with **two escape hatches** (custom shader, custom layout) and **zero state model**. The library is a thin helper over wgpu/vulkano; host-painted regions (3D viewports, video panes, custom canvases) fall out of the library/host split rather than needing a designed primitive. Sets out what the library owns (layout, paint, hit-test, visual lifecycle, scroll/clip, animation, modal stacks, rich text) vs. doesn't (state model, persistence, network, theme runtime, window management).

`V5.md` documents the v5.0 slice — the cargo workspace split + the side-map refactor that landed under the `crates/` tree. `attempts/attempt_1..4` remain in the repo as the lineage that settled the load-bearing premises before this code existed.

## Where we are at

v5.0 is in. Aetna lives under `crates/`:

| Crate | Role |
|---|---|
| `aetna-core` | Backend-agnostic core. Tree (`El`), layout, draw-op IR, stock shaders + custom-shader binding, animation primitives, hit-test, focus, hotkeys, lint + bundle artifacts. Plus the v5.4 cross-backend paint primitives (`paint::QuadInstance` + paint-stream batching) and `runtime::RunnerCore` (the interaction half every backend `Runner` composes). No backend deps. |
| `aetna-wgpu` | wgpu pipelines + per-page atlas textures + `Runner` shell. Wraps a shared `RunnerCore` from `aetna-core` for interaction state, paint-stream scratch, and the `pointer_*`/`key_down`/`set_hotkeys` surface; only GPU resources and the wgpu-flavoured `prepare()` GPU upload + `draw()` are backend-specific. |
| `aetna-demo` | Winit harness + interactive bins + headless render fixtures (`render_counter`, `render_png`, `render_custom`). |
| `aetna-web` | wasm browser entry point. `crate-type = ["cdylib", "rlib"]`; re-exports `aetna_demo::Showcase` and ships a `#[wasm_bindgen(start)] start_web()` that opens a wgpu surface against an `<canvas id="aetna_canvas">` and drives the same App impl `aetna-demo` runs natively. Same shape as `whisper-agent-webui` at `../../whisper-agent`. |
| `aetna-vulkano` | Vulkan backend, peer to `aetna-wgpu`. WGSL → SPIR-V via `naga`; `Runner` mirrors `aetna_wgpu::Runner`'s public surface with `Arc<Device>`/`Queue`/`Format` constructor args. v5.3 lands the rect + text + custom-shader paths native-only; v5.4 step 2 reroutes the interaction half + paint-stream loop through the shared `RunnerCore` so behaviour can no longer drift between backends. |
| `aetna-vulkano-demo` | winit + vulkano harness sibling of `aetna-demo`. v5.3 ships `bin/counter` (the v5.0 boundary A/B fixture) and `bin/custom` (the gradient WGSL fixture); v5.4 adds `bin/showcase` (broader-coverage A/B against `aetna-demo`'s Showcase). |

The architectural decision v5.0 settled: `El` is the author's description of the scene; everything the library writes during a frame — computed rects, hover/press/focus state, envelope amounts, scroll offsets, animation tracker entries — lives in `UiState` side maps keyed by `El::computed_id`. The build closure produces a fresh `El` carrying zero library state; the runtime layer holds the state across rebuilds.

| Capability | Status | Proof point |
|---|---|---|
| Grammar | carried from attempt_3 | `column`/`row`/`card`/`button`/`badge`/`text`/`spacer`, intrinsic + `Fill`/`Hug`/`Fixed` sizing, `pub const` tokens |
| Wgpu rendering | working | `cargo run -p aetna-demo --bin settings` and `crates/aetna-demo/out/settings.wgpu.png` |
| Stock shaders | `rounded_rect` + `text_sdf` + `focus_ring` | `solid_quad` / `divider_line` / `shadow` deferred to v0.7+ |
| Custom-shader escape hatch | working | `crates/aetna-demo/out/custom_shader.wgpu.png` — gradient buttons rendered by user-authored `shaders/gradient.wgsl` |
| Custom-layout escape hatch (v0.5) | working | `El::layout(f)` accepts a `LayoutFn(LayoutCtx) -> Vec<Rect>` that replaces the column/row/overlay distribution for a node's children. The library still recurses, still drives hit-test/focus/animation/scroll off the produced rects. `cargo run -p aetna-core --example circular_layout` → `crates/aetna-core/out/circular_layout.svg`; `cargo run -p aetna-demo --bin circular_layout` (interactive compass rose, click-routed through LayoutFn-produced rects) |
| App trait + hit-test + automatic hover/press | working | `cargo run -p aetna-demo --bin counter` (interactive); `crates/aetna-demo/out/counter.wgpu.png` |
| HiDPI text + shaped core layout + paragraph wrapping + text alignment | bundled Roboto, `cosmic-text` core layout + swash rasterization, core-owned glyph atlas | SVG fallback (`crates/aetna-core/out/settings.svg`) aligned with wgpu output |
| Clip + modal/overlay (v0.3) | working | `cargo run -p aetna-core --example modal` → `crates/aetna-core/out/modal.svg` |
| Scroll viewport (v0.3) | working | `cargo run -p aetna-core --example scroll_list` → `crates/aetna-core/out/scroll_list.svg`; `cargo run -p aetna-demo --bin scroll_list` (interactive, wheel) |
| Host-painted regions | working | reserve a keyed node in the tree, call `Runner::rect_of_key("viewport")` after `prepare()`, and let the host renderer paint into that rect |
| Focus traversal + keyboard routing (v0.4) | working | Tab / Shift+Tab / Enter / Space / Escape in any interactive demo |
| Hotkey system (v0.4) | working | `cargo run -p aetna-demo --bin hotkey_picker` — `j`/`k` movement, Ctrl+L, `/`, etc., zero per-key matching in the app |
| Animation primitives (v0.4) | spring + tween + per-(node, prop) tracker; library-owned hover / press / focus envelopes auto-ease on every keyed interactive node; author-facing `.animate(timing)` + `.opacity` / `.translate` / `.scale` for app-driven prop interpolation; `prepare()` returns `needs_redraw` so frames tick only while motion is in flight | `cargo run -p aetna-demo --bin animated_palette` — selection scales, fades, slides; counter & hotkey_picker get hover/press easing for free |
| Bundle pipeline | `tree.txt` + `draw_ops.txt` + `shader_manifest.txt` + `lint.txt` + `.svg` + `.png` per fixture | `crates/aetna-{core,demo}/out/*` (gitignored under `crates/*/out/`; regenerate by re-running the example, then `tools/svg_to_png.sh` for PNGs) |

Author surface today — the entire interactive contract:

```rust
struct Counter { value: i32 }

impl App for Counter {
    fn build(&self) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("−").key("dec").secondary(),
                button("Reset").key("reset").ghost(),
                button("+").key("inc").primary(),
            ]).gap(tokens::SPACE_MD),
        ])
        .gap(tokens::SPACE_LG).padding(tokens::SPACE_XL).align(Align::Center)
    }

    fn on_event(&mut self, e: UiEvent) {
        match (e.kind, e.key.as_deref()) {
            (UiEventKind::Click, Some("inc"))   => self.value += 1,
            (UiEventKind::Click, Some("dec"))   => self.value -= 1,
            (UiEventKind::Click, Some("reset")) => self.value = 0,
            _ => {}
        }
    }
}
```

No JSX, no signals, no `useState`, no retained-mode component identity. Hover, press, and focus visuals are applied automatically by the library — the author never tags a node "this one is hovered." `key` is the hit-test target *and* the event-routing identifier — same string, no separate `.on_click(...)` registration that can drift.

## Roadmap

v0.x slices come from `LIBRARY_VISION.md`; the v5.x slices come from `V5.md` and structure how Aetna grows beyond the v0.4 baseline.

| Slice | Scope | Status |
|---|---|---|
| v0.1 | Layout, stock surface, glyphon text, custom shader. | done |
| v0.2 | Hit-testing, click events, automatic hover/press, App trait, state-driven rebuild. | done |
| v0.3 | Scroll/clip, modal/overlay primitive. | done |
| v0.4 | Animation primitives, focus traversal, keyboard event routing, hotkey system. | done |
| v5.0 | Crate split into `aetna-{core,wgpu,demo}`; module split inside core; `El` side-map refactor. | done |
| v5.1 | Decouple text from glyphon (cosmic-text + swash + own atlas). | done |
| v5.2 | wasm target. | done — the consolidated `Showcase` (counter / list / palette / picker / settings) runs in the browser via `aetna-web`'s wasm-pack bundle, with a per-frame timing breakdown logged to the JS console |
| v5.3 | Vulkano backend; naga WGSL→SPIRV. | done — counter App renders end-to-end through `aetna-vulkano` (rect + text + custom shaders); the v5.0 core/backend boundary holds across two GPU APIs. Native-only; one-fixture scope per `V5_3.md` |
| v5.4 | Vulkano parity with wgpu. | done — (1) Showcase (sidebar + Counter / List / Palette / Picker / Settings) renders end-to-end through vulkano via `aetna-vulkano-demo --bin showcase`; (2) the cross-backend paint primitives (`QuadInstance` ABI, paint-stream batching, `pack_instance`, `physical_scissor`) live in `aetna_core::paint`; (3) the interaction half + paint-stream loop live in `aetna_core::runtime::RunnerCore` — both backend `Runner`s now hold a `core: RunnerCore` and forward 13 byte-for-byte-identical interaction methods. Each Runner owns only its GPU resources + a thin `prepare()` GPU-upload sequence + `draw()`. A `Painter` trait was considered and declined (see `runtime.rs` module doc): the residual `prepare()` tails + `draw()` walks need backend-typed encoder handles that a trait can't hide without re-fragmenting through generics; the duplication worth abstracting is one layer up (winit + swapchain harness), not at the paint surface. |
| v0.5 | Custom layout (second escape hatch), virtualized lists, `feed`/`chat_log` primitives. | in progress — (1) custom-layout escape hatch landed: `El::layout(f)` takes a `LayoutFn(LayoutCtx) -> Vec<Rect>` that replaces the column/row distribution; hit-test, focus, animation, scroll all keep working off the rects the function produces. v0.5 scope-limit: custom-layout nodes must size with `Fixed`/`Fill` on both axes (`Hug` panics, deferred). Compass-rose fixture in `aetna-core/examples/circular_layout` + `aetna-demo --bin circular_layout`. (2) Virtualized lists + (3) `feed`/`chat_log` primitives queued. |
| v0.6 | Rich text composition (markdown runs, inline highlighting, embedded elements). | paragraph wrapping + text alignment landed (partial) |
| v0.7+ | Stock shader: shadow, focus_ring, divider_line. Backdrop sampling. Liquid glass as the architectural acceptance test. | `focus_ring` shared with `rounded_rect` pipeline |

## Repository tour

```
SHADER_VISION.md                 rendering-layer manifesto
LIBRARY_VISION.md                application-layer manifesto
V5.md                            v5.0 plan (crate split, side-map refactor)
V5_3.md                          v5.3 plan (vulkano backend; naga WGSL→SPIR-V)

crates/
  aetna-core/                    backend-agnostic core
    src/
      tree/                        El, builders, types, color
        mod.rs                       El struct + impls + column/row/scroll/stack/spacer/divider
        types.rs                     Rect, Sides, Size, Axis, Align, Justify, FontWeight, Kind, …
        color.rs                     Color + arithmetic
      tokens.rs                    const tokens (colors, spacing, radii, font sizes)
      style.rs                     StyleProfile, .primary()/.secondary()/.ghost()/...
      layout.rs                    column/row/stack/scroll pass; writes UiState side maps. v0.5: LayoutFn / LayoutCtx — custom-layout escape hatch
      shader.rs                    ShaderHandle, UniformBlock, UniformValue, ShaderBinding
      ir.rs                        DrawOp::{Quad, GlyphRun}
      draw_ops.rs                  El + UiState → DrawOp[]; envelope-driven state visuals
      anim/
        mod.rs                       AnimValue, Animation, SpringConfig, TweenConfig, Timing
        tick.rs                      per-node walker that retargets / steps / writes back
      event.rs                     App trait, UiEvent, UiEventKind, UiTarget, UiKey, KeyChord
      state.rs                     UiState — side maps, trackers, hotkeys, animation map
      hit_test.rs                  pointer hit-test + scroll-target routing
      focus.rs                     linear focus traversal
      overlay.rs                   modal / scrim / overlay primitives
      svg.rs                       approximate SVG fallback for the agent loop
      bundle.rs / lint.rs / inspect.rs / manifest.rs   artifact emission
      button.rs / badge.rs / card.rs / text.rs         component files
      paint.rs                     v5.4 — cross-backend paint ABI: QuadInstance, InstanceRun, PaintItem, physical_scissor, pack_instance
      runtime.rs                   v5.4 — RunnerCore (shared interaction state + paint-stream loop) + TextRecorder trait + PrepareResult/Timings
    shaders/
      rounded_rect.wgsl              the load-bearing stock shader
      gradient.wgsl                  custom-shader fixture
    fonts/                         bundled Roboto (regular/medium/bold)
    examples/                      headless artifact fixtures (settings, modal, scroll_list, custom_shader, circular_layout)
    out/                           rendered artifacts per example
  aetna-wgpu/                    wgpu backend
    src/
      lib.rs                       Runner shell — pipelines, text atlas, GPU upload sequence; wraps aetna_core::runtime::RunnerCore for everything backend-agnostic
      pipeline.rs                  shared quad pipeline factory
      instance.rs                  wgpu-shaped set_scissor (the only paint-side function that needs a wgpu::RenderPass)
      text.rs                      stock::text pipeline + page texture mirror of GlyphAtlas; impls TextRecorder for RunnerCore's paint loop
  aetna-demo/                    winit harness + bins
    src/
      lib.rs                       run<A: App>(title, viewport, app)
      bin/
        showcase.rs                  consolidated demo (counter / list / palette / picker / settings) — v5.2 wasm parity target
        counter.rs                   interactive counter — v0.2 proof point
        scroll_list.rs               interactive scroll list — v0.3 wheel
        hotkey_picker.rs             keyboard-only picker — v0.4 keyboard routing
        animated_palette.rs          selection picker with .animate() — v0.4 proof point
        settings.rs                  static settings screen (windowed)
        render_counter.rs          ┐
        render_png.rs              │ headless artifact generators
        render_custom.rs           ┘
    out/                           rendered PNGs
  aetna-web/                     v5.2 wasm slice — cdylib + rlib
    src/
      lib.rs                       re-exports aetna_demo::Showcase + #[wasm_bindgen(start)] start_web; per-frame timing breakdown
    assets/
      index.html                   browser harness: <canvas id="aetna_canvas"> + import init from /pkg/aetna_web.js
  aetna-vulkano/                 v5.3 Vulkan backend (native only)
    src/
      lib.rs                       module wiring + public re-exports
      runner.rs                    Runner shell — pipelines, render pass, frame uniforms, GPU upload sequence; wraps aetna_core::runtime::RunnerCore (v5.4)
      pipeline.rs                  rect-shaped pipeline factory; FrameUniforms layout
      instance.rs                  vulkano-shaped set_scissor (the only paint-side function that needs an AutoCommandBufferBuilder)
      text.rs                      TextPaint — atlas mirror to per-page R8 images, premul-alpha pipeline; impls TextRecorder for RunnerCore's paint loop
      naga_compile.rs              wgsl_to_spirv helper (pinned naga 23.1)
  aetna-vulkano-demo/            v5.3 winit + vulkano harness
    src/
      lib.rs                       run<A: App>(...) and run_with_init for custom-shader registration
      bin/
        counter.rs                   interactive counter — v0.2 proof point through vulkano
        custom.rs                    custom-shader fixture (gradient.wgsl) — register_shader contract
        hello.rs                     minimal clear-color smoke test (vulkano + winit bring-up)
        showcase.rs                  v5.4 — Showcase (sidebar nav + 5 sections) routed through vulkano
attempts/
  attempt_1..4/                  archive — read each directory's top-level docs for lineage
tools/                           agent-loop scripts (rendering helpers, etc.)
```

## Try it locally

```bash
cargo run -p aetna-demo --bin showcase            # interactive — v5.2 consolidated demo (the browser parity target)
cargo run -p aetna-demo --bin counter             # interactive — v0.2
cargo run -p aetna-demo --bin scroll_list         # interactive — v0.3 wheel
cargo run -p aetna-demo --bin hotkey_picker       # interactive — v0.4 keyboard
cargo run -p aetna-demo --bin animated_palette    # interactive — v0.4 .animate()
cargo run -p aetna-core --example scroll_list     # headless → crates/aetna-core/out/scroll_list.svg
cargo run -p aetna-core --example circular_layout # v0.5 — headless → crates/aetna-core/out/circular_layout.svg
cargo run -p aetna-demo --bin circular_layout     # v0.5 — interactive compass rose, custom LayoutFn
cargo run -p aetna-demo --bin render_counter      # headless wgpu PNG snapshot
cargo run -p aetna-vulkano-demo --bin counter     # v5.3 — same Counter, native vulkano (A/B vs aetna-demo's counter)
cargo run -p aetna-vulkano-demo --bin custom      # v5.3 — gradient.wgsl through Runner::register_shader
cargo run -p aetna-vulkano-demo --bin showcase    # v5.4 — same Showcase, native vulkano (A/B vs aetna-demo's showcase)
cargo test --workspace --lib                      # 60+ unit tests across aetna-core + aetna-{wgpu,vulkano}
```

For the browser:

```bash
tools/build_web.sh --serve                        # wasm-pack build + python static server
# open http://127.0.0.1:8080/assets/index.html
```

Same `Showcase` `App` impl runs through `aetna-demo::run` natively (`cargo run -p aetna-demo --bin showcase`) and through the wasm-bindgen + canvas-bound winit event loop in `aetna-web::start_web` in the browser.

## Reviewing this

If you've been pointed here for a fresh review, the highest-value places to push:

1. **Is `LIBRARY_VISION.md`'s thesis sound?** Specifically: *"a declarative scene that projects time-varying state into a tree, with two escape hatches and zero state model."* Does this shape hold up across the reference applications named in the doc (whisper-git, whisper-agent, whisper-tensor, volumetric), or does it implicitly assume server-streamed UIs?

2. **Is the v0.2 API surface load-bearing?** `App` (two methods) + `UiEvent` + `.key("...")` is the entire interactive contract today. Is anything missing that v0.3+ will paint into a corner — e.g., an event identity model that `key`-as-`String` won't extend cleanly to keyboard/focus/drag?

3. **Two hatches, not three.** A 2026-05 review of whisper-tensor / polychora / volumetric concluded that "embedded viewport" is not a designed primitive — it falls out of the library/host split (host owns the encoder; the library inserts into its render pass; the host can record any other draws into the same encoder). The remaining hatches are custom shader (done) and custom layout (queued for v0.5). Is there a *fourth* hatch that this design can't grow into, or is one of the remaining two actually redundant?

4. **Does the library/host split hold?** The library doesn't own device, queue, swapchain, event loop, state model, or persistence. Is that line drawn in the right place — small enough that an LLM holds the whole API in context, large enough to free authors from boilerplate? In particular: is the implicit "host paints whatever else it needs into its own encoder, alongside our pass" contract crisp enough, or does it need a small public accessor (e.g., `Runner::rect_of(key)`) to be usable in practice?

5. **Does the v5.0 side-map architecture earn its complexity?** `El` is now the build-closure-produced description; `UiState` carries the per-frame bookkeeping. The wins (a fresh `El` carries no library state; multiple readers read from one place; layout, draw-op, and animation passes don't fight over scratch fields on the tree) come at the cost of `String`-keyed `HashMap` lookups per node per frame. Is the tradeoff right for v5.0, or should id interning come sooner?

6. **Anything missing you would expect a UI library to claim?** What's a real, polished native application that this design *can't* express, even after v0.7+? If you can name one, that's the most valuable signal.

This is a young project. Concrete pushback — including "the thesis is wrong, here's why" — is more valuable than incremental polish.
