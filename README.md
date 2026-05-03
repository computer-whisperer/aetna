# new_ui_library

An exploratory project building a UI library shaped around how **an LLM** authors UI, not how a human web developer does. The thesis: when the author is a model, the load-bearing constraints flip — vocabulary parity with the training distribution matters more than configurability, the *minimum* output should be the *correct* output, and the visual ceiling is set by what shaders the model can write, not by what the framework's CSS-shaped surface exposes.

The repo is structured as a sequence of attempts. Each rebuilds the prior one once a load-bearing premise had been settled.

| Attempt | What it settled | Status |
|---|---|---|
| `attempts/attempt_1` | Initial investigation. Tried "egui + better artifacts." Surfaced that the gap isn't iteration — it's first-shot polish. | archive |
| `attempts/attempt_2` | "Polished defaults, no required ceremony." Settled the grammar shape (`column`/`row`/`card`/`button`) and `#[track_caller]` source mapping. See `PREMISE.md` inside. | archive |
| `attempts/attempt_3` | Settled the agent loop: every fixture compiles to a bundle of artifacts (SVG, tree dump, IR, lint) in one call. Validated by a cold sub-agent producing a polished login screen one-shot. | archive |
| `attempts/attempt_4` | **Current.** Rebuilds the rendering layer. wgpu-first, GPU pipelines, stock shaders + custom-shader escape hatch. The grammar carries over from attempt_3 largely intact. | active |

Two manifesto documents stand under attempt_4 — read these before reviewing. They are deliberately independent:

- **`attempts/attempt_4/SHADER_VISION.md`** — the *rendering* layer. Why we paint UI through wgpu pipelines, why CSS-shaped concerns (gradients, shadows, frosted glass) become shader concerns here, why the library inserts into the host's existing render pass rather than owning the device/queue/swapchain. Argues that LLMs writing shaders is the ceiling-raiser.
- **`attempts/attempt_4/LIBRARY_VISION.md`** — the *application* layer. The shape: a declarative scene library that projects time-varying state into a tree, with **three escape hatches** (custom shader, custom layout, embedded viewport) and **zero state model**. Sets out what the library owns (layout, paint, hit-test, visual lifecycle, scroll/clip, animation, modal stacks, rich text) vs. doesn't (state model, persistence, network, theme runtime, window management).

## Where we are at

attempt_4 has reached **v0.2**. Concrete state:

| Capability | Status | Proof point |
|---|---|---|
| Grammar | carried from attempt_3 | `column`/`row`/`card`/`button`/`badge`/`text`/`spacer`, intrinsic + `Fill`/`Hug`/`Fixed` sizing, `pub const` tokens |
| Wgpu rendering | working | `attempts/attempt_4_demo --bin settings` and `out/settings.wgpu.png` |
| Stock shaders | `rounded_rect` + `text_sdf` | `focus_ring` / `solid_quad` / `divider_line` deferred to v0.7+ |
| Custom-shader escape hatch | working | `out/custom_shader.wgpu.png` — gradient buttons rendered by user-authored `shaders/gradient.wgsl` |
| App trait + hit-test + automatic hover/press | working | `attempt_4_demo --bin counter` (interactive); `out/counter.wgpu.png` (headless render with simulated hover) |
| HiDPI text | bundled Roboto, physical-pixel rasterization | SVG fallback (`out/settings.svg`) aligned with wgpu output |
| Bundle pipeline | `tree.txt` + `draw_ops.txt` + `shader_manifest.txt` + `lint.txt` + `.svg` + `.png` per fixture | `out/*` (PNGs gitignored, regenerated from SVG by `tools/svg_to_png.sh`) |

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

No JSX, no signals, no `useState`, no retained-mode component identity. Hover and press visuals are applied automatically by the library — the author never writes `.hovered()` or `.pressed()`. `key` is the hit-test target *and* the event-routing identifier — same string, no separate `.on_click(...)` registration that can drift.

## Roadmap (from `LIBRARY_VISION.md`)

| Slice | Scope | Status |
|---|---|---|
| v0.1 | Layout, stock surface, glyphon text, custom shader. | done |
| v0.2 | Hit-testing, click events, automatic hover/press, App trait, state-driven rebuild. | done |
| v0.3 | Scroll/clip, embedded viewport (third escape hatch), modal/overlay primitive. | next |
| v0.4 | Animation primitives, focus traversal, keyboard event routing, hotkey system. | |
| v0.5 | Custom layout (second escape hatch), virtualized lists, `feed`/`chat_log` primitives. | |
| v0.6 | Rich text composition (markdown runs, inline highlighting, embedded elements). | |
| v0.7+ | Stock shader: shadow, focus_ring, divider_line. Backdrop sampling. wgpu-wasm + vulkano backends. Liquid glass as the architectural acceptance test. | |

## Repository tour

```
attempts/
  attempt_1..3/                 archive — read each directory's top-level docs for lineage
  attempt_4/                    current
    SHADER_VISION.md              rendering-layer manifesto
    LIBRARY_VISION.md             application-layer manifesto
    src/
      tree.rs                     El, Kind, Sides, Size, Axis, Align, Justify, InteractionState
      tokens.rs                   const tokens (colors, spacing, radii, font sizes)
      style.rs                    StyleProfile, .primary()/.secondary()/.ghost()/...
      layout.rs                   column/row/stack pass, Fill/Hug/Fixed sizing
      shader.rs                   ShaderHandle, UniformBlock, UniformValue, ShaderBinding
      ir.rs                       DrawOp::{Quad, GlyphRun, BackdropSnapshot}
      draw_ops.rs                 El tree → DrawOp[], applies state visual deltas
      event.rs                    App trait, UiEvent, UiState, hit_test (v0.2)
      wgpu_render.rs              UiRenderer: pipelines, glyphon text, pointer plumbing
      svg.rs                      approximate SVG fallback for the agent loop
      bundle.rs / lint.rs / inspect.rs / manifest.rs   artifact emission
      button.rs / badge.rs / card.rs / text.rs          component files
    shaders/
      rounded_rect.wgsl           the load-bearing stock shader
      gradient.wgsl               custom-shader-escape-hatch fixture
    fonts/                        bundled Roboto (regular/medium/bold)
    out/                          rendered artifacts per fixture
  attempt_4_demo/               standalone winit + wgpu harness
    src/lib.rs                    run<A: App>(title, viewport, app)
    src/bin/
      settings.rs                   static settings screen (windowed)
      counter.rs                    interactive counter — v0.2 proof point
      render_settings.rs            \
      render_counter.rs              | headless artifact generators
      render_custom.rs              /
      render_png.rs                /
tools/                          agent-loop scripts (rendering helpers, etc.)
```

Try it locally:

```bash
cargo run -p attempt_4_demo --bin counter         # interactive
cargo run -p attempt_4_demo --bin render_counter  # headless → out/counter.wgpu.png
cargo test -p attempt_4                           # 7 unit tests + 1 doctest
```

## Reviewing this

If you've been pointed here for a fresh review, the highest-value places to push:

1. **Is `LIBRARY_VISION.md`'s thesis sound?** Specifically: *"a declarative scene that projects time-varying state into a tree, with three escape hatches and zero state model."* Does this shape hold up across the reference applications named in the doc (whisper-git, whisper-agent, whisper-tensor, volumetric), or does it implicitly assume server-streamed UIs?

2. **Is the v0.2 API surface load-bearing?** `App` (two methods) + `UiEvent` + `.key("...")` is the entire interactive contract today. Is anything missing that v0.3+ will paint into a corner — e.g., an event identity model that `key`-as-`String` won't extend cleanly to keyboard/focus/drag?

3. **Are the three escape hatches the right three?** Custom shader (done), custom layout (queued for v0.5), embedded viewport (queued for v0.3). Is there a fourth that this design can't grow into? Is one of the three not actually load-bearing?

4. **Does the library/host split hold?** The library doesn't own device, queue, swapchain, event loop, state model, or persistence. Is that line drawn in the right place — small enough that an LLM holds the whole API in context, large enough to free authors from boilerplate?

5. **How should v0.3 actually slice?** The roadmap currently lumps scroll/clip + embedded viewport + modal/overlay into one slice. They are arguably three independent substrate changes. We've been considering splitting them into v0.3a/b/c, each with its own fixture — is that the right call, or does landing them together force shared design decisions worth not separating?

6. **Anything missing you would expect a UI library to claim?** What's a real, polished native application that this design *can't* express, even after v0.7+? If you can name one, that's the most valuable signal.

This is a young project. Concrete pushback — including "the thesis is wrong, here's why" — is more valuable than incremental polish.
