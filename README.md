# Aetna

A thin UI rendering library that can insert into an existing Vulkan or wgpu renderer rather than owning the device, queue, or swapchain. The core/backends don't replace the host's renderer; they share its pass. For simple desktop apps, the workspace also ships an optional winit + wgpu host crate that packages the common window/surface loop. The name echoes the API it sits on — Vulkan is named for Vulcan, the Roman smith-god, and Mt. Aetna is the volcano where his forge stood.

Aetna is shaped around how **an LLM** authors UI, not how a human web developer does. The thesis: when the author is a model, the load-bearing constraints flip — vocabulary parity with the training distribution matters more than configurability, the *minimum* output should be the *correct* output, and the visual ceiling is set by what shaders the model can write, not by what the framework's CSS-shaped surface exposes.

Two manifesto documents stand at the repo root — read these before reviewing. They are deliberately independent:

- **`SHADER_VISION.md`** — the *rendering* layer. Why we paint UI through wgpu pipelines, why CSS-shaped concerns (gradients, shadows, frosted glass) become shader concerns here, why the library inserts into the host's existing render pass rather than owning the device/queue/swapchain. Argues that LLMs writing shaders is the ceiling-raiser.
- **`LIBRARY_VISION.md`** — the *application* layer. The shape: a declarative scene library that projects time-varying state into a tree, with **two escape hatches** (custom shader, custom layout) and **zero state model**. The library is a thin helper over wgpu/vulkano; host-painted regions (3D viewports, video panes, custom canvases) fall out of the library/host split rather than needing a designed primitive. Sets out what the library owns (layout, paint, hit-test, visual lifecycle, scroll/clip, animation, modal stacks, rich text) vs. doesn't (state model, persistence, network, theme runtime, window management).

`V5.md` documents the v5.0 slice — the cargo workspace split + the side-map refactor that landed under the `crates/` tree. `attempts/attempt_1..4` remain in the repo as the lineage that settled the load-bearing premises before this code existed.

## Where we are at

v0.9 + v5.4 are in. Aetna lives under `crates/`, with runnable cross-crate examples in the workspace `examples/` package:

| Crate | Role |
|---|---|
| `aetna-core` | Backend-agnostic core. Tree (`El`), layout, draw-op IR, stock shaders + custom-shader binding, animation primitives, hit-test, focus, hotkeys, lint + bundle artifacts. Plus the v5.4 cross-backend paint primitives (`paint::QuadInstance` + paint-stream batching) and `runtime::RunnerCore` (the interaction half every backend `Runner` composes). No backend deps. |
| `aetna-wgpu` | wgpu pipelines + per-page atlas textures + `Runner` shell. Wraps a shared `RunnerCore` from `aetna-core` for interaction state, paint-stream scratch, and the `pointer_*`/`key_down`/`set_hotkeys` surface; only GPU resources and the wgpu-flavoured `prepare()` GPU upload + `draw()` are backend-specific. |
| `aetna-fixtures` | Backend-neutral showcase apps and render fixtures (`Showcase`, icon gallery, text-quality matrix, liquid-glass lab). No windowing or GPU setup; examples, web, and backend parity crates import the same fixtures for parity. |
| `aetna-winit-wgpu` | Optional batteries-included native desktop host for simple winit + wgpu apps. Owns window/surface setup, MSAA target management, input mapping, IME forwarding, redraw-on-animation, plus opt-in host cadence / `before_build` hooks for live external state. Custom hosts can bypass it and call `aetna-wgpu::Runner` directly. |
| `aetna-examples` | Workspace examples package (`examples/`). User-facing interactive examples that intentionally pull multiple crates: `aetna-core` + `aetna-winit-wgpu`, plus `aetna-fixtures` or native helpers where needed. |
| `aetna-web` | wasm browser entry point. `crate-type = ["cdylib", "rlib"]`; re-exports `aetna_fixtures::Showcase` and ships a `#[wasm_bindgen(start)] start_web()` that opens a wgpu surface against an `<canvas id="aetna_canvas">` and drives the same backend-neutral App impl that native demos use. Same shape as `whisper-agent-webui` at `../../whisper-agent`. |
| `aetna-vulkano` | Vulkan backend, peer to `aetna-wgpu`. WGSL → SPIR-V via `naga`; `Runner` mirrors `aetna_wgpu::Runner`'s public surface with `Arc<Device>`/`Queue`/`Format` constructor args. v5.3 lands the rect + text + custom-shader paths native-only; v5.4 step 2 reroutes the interaction half + paint-stream loop through the shared `RunnerCore` so behaviour can no longer drift between backends. |
| `aetna-vulkano-demo` | winit + vulkano harness sibling of the wgpu demo path. v5.3 ships `bin/counter` (the v5.0 boundary A/B fixture) and `bin/custom` (the gradient WGSL fixture); v5.4 adds `bin/showcase`, which drives the same `aetna-fixtures::Showcase` app through `aetna-vulkano`. |

The architectural decision v5.0 settled: `El` is the author's description of the scene; everything the library writes during a frame — computed rects, hover/press/focus state, envelope amounts, scroll offsets, animation tracker entries — lives in `UiState` side maps keyed by `El::computed_id`. The build closure produces a fresh `El` carrying zero library state; the runtime layer holds the state across rebuilds.

| Capability | Status | Proof point |
|---|---|---|
| Grammar | carried from attempt_3 | `column`/`row`/`card`/`button`/`badge`/`text`/`spacer`, intrinsic + `Fill`/`Hug`/`Fixed` sizing, `pub const` tokens |
| Wgpu rendering | working | `cargo run -p aetna-examples --bin settings`; `cargo run -p aetna-wgpu --example render_png` writes `crates/aetna-wgpu/out/settings.wgpu.png` |
| Stock shaders | `rounded_rect` + `text_sdf` + `focus_ring` | `solid_quad` / `divider_line` / `shadow` deferred to v0.7+ |
| Custom-shader escape hatch | working | `crates/aetna-wgpu/out/custom_shader.wgpu.png` — gradient buttons rendered by user-authored `shaders/gradient.wgsl` |
| Custom-layout escape hatch (v0.5) | working | `El::layout(f)` accepts a `LayoutFn(LayoutCtx) -> Vec<Rect>` that replaces the column/row/overlay distribution for a node's children. The library still recurses, still drives hit-test/focus/animation/scroll off the produced rects. `cargo run -p aetna-core --example circular_layout` → `crates/aetna-core/out/circular_layout.svg`; `cargo run -p aetna-examples --bin circular_layout` (interactive compass rose, click-routed through LayoutFn-produced rects) |
| Virtualized list (v0.5) | working — fixed row height | `virtual_list(count, row_height, build_row)` realizes only the rows whose rect intersects the viewport. Wheel events route via the existing scroll machinery; computed_ids derive from row keys so hover/press/focus state survives scrolling. `cargo run -p aetna-core --example virtual_list` (10k rows, ~9 realized in tree dump); `cargo run -p aetna-examples --bin virtual_list` (100k rows, interactive scroll + click). Variable-height rows deferred to a later slice. |
| App trait + hit-test + automatic hover/press | working | `cargo run -p aetna-examples --bin counter` (interactive); `crates/aetna-wgpu/out/counter.wgpu.png` |
| HiDPI text + shaped core layout + paragraph wrapping + text alignment | bundled Roboto, `cosmic-text` core layout + swash rasterization, core-owned glyph atlas | SVG fallback (`crates/aetna-core/out/settings.svg`) aligned with wgpu output |
| Clip + modal/overlay (v0.3) | working | `cargo run -p aetna-core --example modal` → `crates/aetna-core/out/modal.svg` |
| Scroll viewport (v0.3) | working | `cargo run -p aetna-core --example scroll_list` → `crates/aetna-core/out/scroll_list.svg`; `cargo run -p aetna-examples --bin scroll_list` (interactive, wheel) |
| Host-painted regions | working | reserve a keyed node in the tree, call `Runner::rect_of_key("viewport")` after `prepare()`, and let the host renderer paint into that rect |
| Focus traversal + keyboard routing (v0.4) | working | Tab / Shift+Tab / Enter / Space / Escape in any interactive demo |
| Hotkey system (v0.4) | working | `cargo run -p aetna-examples --bin hotkey_picker` — `j`/`k` movement, Ctrl+L, `/`, etc., zero per-key matching in the app |
| Animation primitives (v0.4) | spring + tween + per-(node, prop) tracker; library-owned hover / press / focus envelopes auto-ease on every keyed interactive node; author-facing `.animate(timing)` + `.opacity` / `.translate` / `.scale` for app-driven prop interpolation; `prepare()` returns `needs_redraw` so frames tick only while motion is in flight | `cargo run -p aetna-examples --bin animated_palette` — selection scales, fades, slides; counter & hotkey_picker get hover/press easing for free |
| Rich text (v0.6.1) | attributed runs, per-glyph color / weight / italic / strikethrough, hard breaks, paragraph alignment shared between SVG fallback and GPU paths | `cargo run -p aetna-core --example inline_runs` → `crates/aetna-core/out/inline_runs.svg` |
| Backdrop sampling (v0.7) | multi-pass render API + snapshot copy + `@group(1)` backdrop sampler made available to custom shaders; `liquid_glass.wgsl` is the architectural acceptance test | `cargo run -p aetna-wgpu --example render_liquid_glass`; runs identically through wgpu native, vulkano native, and WebGPU |
| Widget kit (v0.7.5) + input plumbing (v0.7.6) | symmetry invariant — stock widgets compose only public surface (`widget_state::<T>`, `capture_keys`, `paint_overflow`, `set_modifiers`, `LayoutCtx::rect_of_key`, etc.). `crates/aetna-core/src/widget_kit.md` is the author contract; every stock widget under `crates/aetna-core/src/widgets/` is a pure composition. v0.7.6 lands `PointerDown`, `SecondaryClick`, drag tracking, character / IME `TextInput` events, focused-node key capture, and `metrics::hit_text` as kit-public primitives | `widgets/button.rs` is the smallest dogfood example; `widget_kit.md` lists every kit surface |
| `text_input` / `text_area` (v0.8.1–v0.8.4) | single + multi-line editing, `TextSelection { anchor, head }`, drag-to-select, shift-arrow extension, Ctrl+A/C/X/V via app-owned clipboard (`text_input::clipboard_request` detects keystrokes; app dispatches against `arboard` natively or web Clipboard API), preferred-column up/down motion, line-wise Home/End. Both widgets share `(value, TextSelection)` shape and the same `apply_event` helper. Built using only the public widget kit. | `cargo run -p aetna-examples --bin text_input`; `cargo run -p aetna-examples --bin text_area` |
| Anchored popovers (v0.9) | two-pass layout positioning a popover relative to a trigger key (current-frame rect, no staleness); viewport-edge auto-flip; click-outside / Escape dismiss. `dropdown` and `context_menu` are compositions of `popover` + `popover_panel` + `menu_item` — no extra runtime wiring. New kit primitive: `LayoutCtx::rect_of_key` (any custom layout can position relative to keyed elements outside its own subtree). | `cargo run -p aetna-examples --bin popover` — top dropdown, bottom dropdown (auto-flip-up), context menu, non-scrim tooltip |
| Bundle pipeline | `tree.txt` + `draw_ops.txt` + `shader_manifest.txt` + `lint.txt` + `.svg` + `.png` per fixture | `crates/aetna-{core,wgpu}/out/*` (gitignored under `crates/*/out/`; regenerate by re-running the example, then `tools/svg_to_png.sh` for PNGs) |

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

v0.1–v0.9 and the v5.0–v5.4 substrate work are summarized under [Shipped](#shipped) at the bottom of this README. The live roadmap is the work that turns Aetna from "Showcase exercises every primitive" into "you could port a real reference application onto this." It is organized around one invariant: **stock widgets get no APIs that user widgets don't.**

The v0.7.5 widget kit, the v0.8.x text-input track, and the v0.9 popover track all shipped without breaking that invariant — every stock widget composes only public surface, the library has zero behaviour dispatch on the decorative `Kind` variants, and `RunnerCore` is sealed from widget code. The contract is documented in `crates/aetna-core/src/widget_kit.md`. The next test is whether it survives a real app port.

| Slice | Scope | Status |
|---|---|---|
| **v0.9.x** | **Popover follow-ups.** Auto-focus on open (a `UiState::request_focus_key` shape that the popover panel can claim when it mounts), keyboard arrow-nav inside `menu_item` lists, hover-driven tooltip open. Each is documented in `widget_kit.md` as deferred from v0.9. Likely either rolled into v0.10 as port-revealed gaps, or shipped pre-port if the port immediately needs them. | queued |
| **v0.10** | **Validation port.** Take the smallest viable whisper-git slice — sidebar + commit list, read-only, no diff viewer, no remotes — and port it onto Aetna in a sibling crate. The point is not to ship a finished port; it is to let the gaps surface from a real app rather than guessing them. Whatever shows up determines v0.11+. | next |

## Repository tour

```
SHADER_VISION.md                 rendering-layer manifesto
LIBRARY_VISION.md                application-layer manifesto
V5.md                            v5.0 plan (crate split, side-map refactor)
V5_3.md                          v5.3 plan (vulkano backend; naga WGSL→SPIR-V)

crates/
  aetna-core/                    backend-agnostic core
    src/
      lib.rs                       prelude
      tree/                        El, Kind, Rect, Color (the scene description)
      layout.rs                    column/row/stack/scroll/overlay distribution; LayoutFn / VirtualItems

      ir.rs                        DrawOp::{Quad, GlyphRun, BackdropSnapshot}
      draw_ops.rs                  El + UiState → DrawOp[]; envelope-driven state visuals
      paint.rs                     v5.4 cross-backend paint ABI: QuadInstance, paint-stream batching, scissor
      shader.rs                    ShaderHandle, UniformBlock, ShaderBinding

      state.rs                     UiState — side maps, trackers, hotkeys, animations, widget_state::<T>
      runtime.rs                   v5.4 RunnerCore (shared interaction state + paint-stream loop) + TextRecorder
      event.rs                     App trait, UiEvent, UiEventKind, UiTarget, UiKey, KeyChord
      hit_test.rs                  pointer hit-test + scroll-target routing
      focus.rs                     linear focus traversal
      anim/                        AnimValue, Animation, SpringConfig, TweenConfig, per-node tick

      style.rs                     StyleProfile dispatch
      tokens.rs                    const tokens (colors, spacing, radii, font sizes)

      widgets/                     stock vocabulary — pure compositions of the public widget-kit surface
        button.rs                    button("Save").primary() etc.
        card.rs                      card("Title", [body])
        badge.rs                     badge("12")
        text.rs                      h1/h2/h3/paragraph/mono/text
        overlay.rs                   overlay/scrim/modal/modal_panel

      text/                        text shaping + atlas infrastructure
        atlas.rs                     unified RGBA glyph atlas (color emoji + outline glyphs)
        metrics.rs                   measure_text / wrap_lines / line_height / TextLayout

      bundle/                      artifact pipeline (the agent loop's feedback channel)
        artifact.rs                  bundle orchestration; render_bundle entry
        inspect.rs                   tree dump
        lint.rs                      provenance-tracked findings
        manifest.rs                  shader manifest + draw-op text
        svg.rs                       approximate SVG fallback

    shaders/
      rounded_rect.wgsl              the load-bearing stock shader
      gradient.wgsl                  custom-shader fixture
      liquid_glass.wgsl              v0.7 backdrop-sampling acceptance test
    examples/                      headless artifact fixtures
    out/                           rendered artifacts per example

  aetna-wgpu/                    wgpu backend (Runner shell + pipelines + atlas mirror)
  aetna-vulkano/                 vulkano backend (Runner shell + pipelines + naga compile)
  aetna-fixtures/                backend-neutral Showcase + render fixtures
  aetna-winit-wgpu/              optional native winit + wgpu app host
  aetna-vulkano-demo/            vulkano demo harness + backend parity bins
  aetna-web/                     wasm browser entry point — cdylib re-exporting Showcase
  aetna-fonts/                   bundled Roboto + emoji (split out in v0.7)
examples/                        interactive cross-crate examples (`aetna-examples`)
tools/                           Rust diagnostics (`aetna-tools`) plus helper scripts
attempts/
  attempt_1..4/                  archive — read each directory's top-level docs for lineage
```

## Try it locally

```bash
cargo run -p aetna-examples --bin showcase            # interactive wgpu host — shared Showcase fixture
cargo run -p aetna-examples --bin counter             # interactive — v0.2
cargo run -p aetna-examples --bin scroll_list         # interactive — v0.3 wheel
cargo run -p aetna-examples --bin hotkey_picker       # interactive — v0.4 keyboard
cargo run -p aetna-examples --bin animated_palette    # interactive — v0.4 .animate()
cargo run -p aetna-core --example scroll_list     # headless → crates/aetna-core/out/scroll_list.svg
cargo run -p aetna-core --example circular_layout # v0.5 — headless → crates/aetna-core/out/circular_layout.svg
cargo run -p aetna-examples --bin circular_layout     # v0.5 — interactive compass rose, custom LayoutFn
cargo run -p aetna-core --example virtual_list    # v0.5 — headless → crates/aetna-core/out/virtual_list.svg (10k rows; tree dump shows only the realized window)
cargo run -p aetna-examples --bin virtual_list        # v0.5 — interactive 100k-row list, wheel scroll + click
cargo run -p aetna-core --example inline_runs     # v0.6.1 — headless → crates/aetna-core/out/inline_runs.svg (attributed runs)
cargo run -p aetna-wgpu --example render_liquid_glass # v0.7 — backdrop-sampling acceptance test
cargo run -p aetna-examples --bin text_input          # v0.8.1–v0.8.3 — single-line input, selection, clipboard
cargo run -p aetna-examples --bin text_area           # v0.8.4 — multi-line input, wrapping caret, line-wise motion
cargo run -p aetna-examples --bin popover             # v0.9 — anchored popovers, dropdown, context menu, tooltip
cargo run -p aetna-wgpu --example render_counter      # headless wgpu PNG snapshot
cargo run -p aetna-vulkano-demo --bin counter     # v5.3 — same Counter, native vulkano (A/B vs wgpu demo counter)
cargo run -p aetna-vulkano-demo --bin custom      # v5.3 — gradient.wgsl through Runner::register_shader
cargo run -p aetna-vulkano-demo --bin showcase    # v5.4 — aetna-fixtures::Showcase through native vulkano
cargo test --workspace --lib                      # 200+ unit tests across aetna-core + demo/backend crates
```

For the browser:

```bash
tools/build_web.sh --serve                        # wasm-pack build + python static server
# open http://127.0.0.1:8080/assets/index.html
```

Same `Showcase` `App` impl from `aetna-fixtures` runs through `aetna-winit-wgpu` in `aetna-examples` (`cargo run -p aetna-examples --bin showcase`), through `aetna-vulkano-demo::run` on vulkano, and through the wasm-bindgen + canvas-bound winit event loop in `aetna-web::start_web` in the browser.

## Reviewing this

Aetna's rendering thesis is well-defended (liquid glass running on three backends; the v5.4 `RunnerCore` extraction means behavior literally cannot drift between backends). The v0.7.5–v0.9 widget-kit and text/popover slices closed two of the questions this section used to ask:

- **The symmetry invariant survived text input.** Every stock widget under `crates/aetna-core/src/widgets/` (`button`, `badge`, `card`, `text`, `overlay`, `popover`, `text_input`, `text_area`) composes only public surface — no `pub(crate)` reach-through, no `#[doc(hidden)]` items, no library-side `match` on the decorative `Kind` variants. `RunnerCore` is sealed from widget code. The `text_input` and `text_area` widgets keep their per-node edit state in `UiState::widget_state::<T>`, the same hook user widgets get. The contract is documented in `crates/aetna-core/src/widget_kit.md`.
- **The popover positioning model is genuinely two-pass.** `LayoutCtx::rect_of_key` reads the *current-frame* rect (not the previous frame's), so a popover anchored to a trigger that was just laid out sees the up-to-date position. `anchor_rect`'s viewport-edge auto-flip and secondary-axis clamping have unit-test coverage for both-sides-overflow, exact-edge, and missing-key cases. `dropdown` and `context_menu` are pure compositions of `popover` + `popover_panel` + `menu_item` — no extra runtime wiring.

What remains untested is the *application* thesis — that this shape is the right substrate for a polished native app, not just a Showcase. v0.10 (the validation port) is what tests it.

The highest-value places to push now:

1. **What primitive will v0.10's port reveal as missing?** Best current guesses: drag-resizable splits, variable-height virtualization, a documented async-channel-into-redraw recipe, an `App`-side data-loaded-redraw helper, focus-on-mount (the v0.9.x `request_focus_key` deferral). Likely something we haven't named.

2. **Is `Kind` still slimmable further?** The decorative variants (`Button`, `Card`, `Badge`, `Heading`, `Modal`, `Scrim`) are inspector-only — the library has zero behavioural dispatch on them — so collapsing them into `Custom("button")` etc. would be a no-op functionally and would shrink the public surface. The case for keeping them is that named tags read better in tree dumps than freeform strings. Worth doing, or churn?

3. **Does the library/host split still hold under v0.7's backdrop sampling?** The host needs to declare `COPY_SRC` / `TRANSFER_SRC` usage on its color target. Is that a clean enough integration cost, or is it too much knowledge leaking through the seam?

4. **Anything missing you would expect a UI library to claim?** What's a real, polished native application that this design *can't* express, even after v0.9? If you can name one, that's the most valuable signal.

This is a young project. Concrete pushback — including "the symmetry invariant will fail at X, here's why" — is more valuable than incremental polish.

## Shipped

The slices below have all landed. The capability table at the top of this README documents the resulting user-visible contract; the artifacts are in each crate's `out/`.

| Slice | Scope |
|---|---|
| v0.1 | Layout, stock surface, glyphon text, custom shader. |
| v0.2 | Hit-testing, click events, automatic hover/press, App trait, state-driven rebuild. |
| v0.3 | Scroll/clip, modal/overlay primitive. |
| v0.4 | Animation primitives, focus traversal, keyboard event routing, hotkey system. |
| v0.5 | Custom layout (second escape hatch) + virtualized lists. |
| v0.6.1 | Rich-text composition (attributed runs, per-glyph color/weight/italic, hard breaks). v0.6.2/v0.6.3 (semantic highlighting, inline embeds) folded into v0.10's port-driven priorities. |
| v0.7 | Backdrop sampling — multi-pass + snapshot + `@group(1)` on wgpu native, vulkano, and WebGPU. `liquid_glass.wgsl` as the architectural acceptance test from `SHADER_VISION.md`. |
| v0.7.5 | Widget kit. `widget_state::<T>` typed bucket on `UiState`; documented author contract at `crates/aetna-core/src/widget_kit.md`. Stock `button`/`card`/`badge`/`text` rewritten as pure compositions of public surface — symmetry invariant established. |
| v0.7.6 | Input plumbing. `PointerDown`, `SecondaryClick`, drag tracking, `KeyModifiers` mask on every event, focused-node key capture (`.capture_keys()`) ahead of hotkey routing, character / IME `TextInput` events, `metrics::hit_text` exposing cosmic-text's `Buffer::hit`. Each piece a kit-public primitive. |
| v0.8.1 | `text_input` — single-line. `(value, caret)` lives in app state; widget composes `Kind::Custom("text_input")` + `.focusable()` + `.capture_keys()` + `.paint_overflow()` over `text` segments + a caret bar. `apply_event(value, caret, &UiEvent)` folds events back into app state. `El::axis()` promoted from `pub(crate)` to `pub`. |
| v0.8.2 | Selection. `TextSelection { anchor, head }` replaces the bare caret index. Drag-select, Shift+arrow extension, Ctrl+A, replace-on-type / replace-on-backspace. Helpers: `selected_text`, `replace_selection`, `select_all`. New token: `SELECTION_BG`. New flag: `El::alpha_follows_focused_ancestor()` so the caret fades on the standard focus envelope. |
| v0.8.3 | Clipboard. `text_input::clipboard_request(&UiEvent) -> Option<ClipboardKind>` detects Ctrl/Cmd+C/X/V; the app dispatches against whatever clipboard backend it owns (`arboard` natively, the web Clipboard API on wasm). Library stays platform-agnostic. |
| v0.8.4 | `text_area` — multi-line. Same `(value, TextSelection)` shape as `text_input`; widget composes `wrap_text` + per-line selection bands + 2D-translated caret. `metrics::caret_xy` and `metrics::selection_rects` land as kit-public helpers. Up/Down preserve visual column; Enter inserts `"\n"`. Clipboard is shared with `text_input`. (IME deferred — Latin-1 first.) |
| v0.9 | Anchored popovers + `dropdown` / `context_menu`. New kit primitive: `LayoutCtx::rect_of_key(&str) -> Option<Rect>` — current-frame rect lookup, used by the popover-anchors-trigger pattern but available to any custom layout. `anchor_rect()` flips on viewport-edge overflow and clamps the secondary axis; `popover(key, anchor, panel)` ships the overlay + dismiss scrim + anchored panel layer. Apps own open/closed state and compose at the root; no portal hoist. |
| v5.0 | Crate split into `aetna-{core,wgpu,demo}`; `El` side-map refactor (build closure produces zero library state; `UiState` carries per-frame bookkeeping). |
| v5.1 | Text decoupled from glyphon (cosmic-text + swash + own atlas). |
| v5.2 | wasm target via `aetna-web`; consolidated Showcase runs in the browser. |
| v5.3 | Vulkano backend; naga WGSL→SPIR-V. |
| v5.4 | Vulkano parity with wgpu: cross-backend `paint::QuadInstance` ABI, shared `runtime::RunnerCore`. |
