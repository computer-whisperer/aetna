# attempt_4 — Library Vision

`SHADER_VISION.md` covers the *rendering* layer: why we paint UI through wgpu pipelines, why LLMs can author shaders, why insert-into-pass is the right host integration. This document covers the *library* layer: what kind of UI library this is, what it owns, and what it doesn't.

attempt_4's grammar substrate already validated — a fresh sub-agent built a polished login screen one-shot from the attempt_3 DSL. The visual ceiling is rising via stock + custom shaders. What's missing is the shape of the application layer — the part that turns "renders a static fixture" into "is the right substrate for a real native app."

## The thesis

> A **declarative scene library that projects time-varying state into a tree**, with three escape hatches (custom shader, custom layout, embedded viewport), rich domain primitives, and **zero state model**.

Not SwiftUI. Not Iced. Not egui. The shape is:

- **State lives in the host.** The library is a pure projection from state to scene. No reactivity, no signals, no effects.
- **Time-varying source is generic.** WebSocket events, filesystem watcher, worker thread, GPU readback, animation timer — all the same shape from the library's point of view.
- **Build closure is the central abstraction.** `fn build(&self) -> El`, called when the host requests a redraw. That's it.
- **Library handles the visual lifecycle.** Hover, press, focus, animations, scroll, modal stacks — none of these require the author to thread state through their tree. The library tracks them and applies visual deltas after the build pass.

## Why this shape

The reference applications (whisper-git, whisper-agent, whisper-tensor, volumetric) share five shapes:

1. **Time-varying input source → state → tree.** Filesystem watchers, websocket streams, worker threads, GPU readbacks. None of these are "form submit."
2. **Multi-pane drag-resizable layouts.** Sidebar + main + modals + overlays.
3. **Domain-specific visualizations** that no widget set ships — commit graphs with bezier merges, nested tensor supergraphs, fused tool-call cards, volume-rendered viewports.
4. **Keyboard-first interaction.** `j/k`, `Ctrl+F`, hotkeys-everywhere, focus traversal.
5. **Rich text composition.** Markdown, code blocks, inline diff highlighting, embedded inline elements.

What no existing library does well *together*: shader-level visual customization, structural layout customization, and host-painted regions inside the same render pass. Existing libraries either handle the "form + table" case (egui, iced) or are full game engines (Bevy UI). The middle ground — "polished native app with custom domain visualizations" — is where each project we surveyed re-invented its own renderer.

## The three escape hatches

LLMs can write GPU code. The library's contract with its authors is: when stock isn't enough, **drop down**. Three separate dropdown points exist:

| Hatch | Purpose | Status |
|---|---|---|
| **Custom shader** | Visual ceiling — gradients, frosted glass, noise, shaders that go beyond `rounded_rect`'s uniforms. | Implemented in v0.1; `gradient.wgsl` proof of concept. |
| **Custom layout** | Structural ceiling — force-directed graphs, commit-graph lanes, timelines, treemaps, anything `column`/`row` can't express. Author registers a `LayoutFn(children, constraints) -> rects`. | Not yet built. |
| **Embedded viewport** | Host-painted region inside our render pass. The host gets a sub-rect + access to the encoder, paints whatever it wants (volume rendering, video, custom canvas), and the library renders chrome around it. | Substrate exists (insert-into-pass); no DSL surface yet. |

These three hatches close out the ceiling. Together they say: anything an existing GPU UI library can render can be expressed inside this one too, without forking.

## What the library owns vs. doesn't

**Owns** (the library is responsible for these — host should not need to invent any of them):

- Layout. `Hug` / `Fill` / `Fixed`, `column`/`row`/`stack`, `gap`/`padding`/`align`/`justify`.
- Paint. Stock + custom shaders, glyphon-backed text, hi-DPI, sRGB.
- Hit-testing. Given a point, tell us which interactive node was hit.
- Event routing. Pointer + keyboard events flow through hit-test → focus tree → handlers.
- Visual lifecycle. Hover, press, focus, disabled, loading — applied automatically based on internal trackers.
- Scroll + clip. Virtualized lists, panning regions, scissor management.
- Animation. Spring/tween primitives with a tick source.
- Modal/overlay stacks. Z-layered popovers, tooltips, dialogs that don't pollute the layout tree.
- Rich text composition. Markdown-style runs, inline syntax highlighting, embedded elements.
- Keyboard-first event model. Hotkey routing, focus graph, vim-style navigation primitives.

**Doesn't own** (host's responsibility — the library does not invent these):

- State model. Plain `&mut self`, channels, signals, ECS, redux — host's pick.
- Persistence. Saving / loading state, undo/redo. The library doesn't keep durable state.
- Network. `feed`/`chat_log` consume an iterator/channel; the host fills it from whatever source.
- Theme runtime. v0.1 ships `const` tokens. Runtime themes can come later if needed.
- Window management. Single window via winit. Multi-window/menubar/tray are host concerns.
- Application lifecycle. Main loop, signal handling, graceful shutdown — host owns these.

This split keeps the library small enough that an LLM can hold the whole API in context, while leaving every "what state model do you use" debate where it belongs.

## Authorship — what the LLM sees

The author writes:

```rust
struct Counter { value: i32 }

impl App for Counter {
    fn build(&self) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("-").key("dec"),
                button("Reset").key("reset"),
                button("+").key("inc"),
            ]),
        ])
    }

    fn on_event(&mut self, event: UiEvent) {
        match (event.kind, event.key.as_deref()) {
            (UiEventKind::Click, Some("inc"))   => self.value += 1,
            (UiEventKind::Click, Some("dec"))   => self.value -= 1,
            (UiEventKind::Click, Some("reset")) => self.value = 0,
            _ => {}
        }
    }
}
```

That's it. No JSX, no signals, no `useState`, no `setState`. No `Component` wrapper, no `props`/`state` distinction, no lifecycle methods. The build closure is pure. The event handler is plain `&mut self`.

Hover, press, and focus visuals are applied automatically by the library — the author never sets `state = Hover` themselves. Click hit-testing is automatic — the author tags interactive nodes with `.key("...")` and matches on those keys.

For an LLM, the surface area to learn is:

- The DSL (`column`, `row`, `card`, `button`, etc. — same as v0.1).
- One trait with two methods.
- One enum with a few variants.

Compared to the surface area of React/Iced/SwiftUI, this is a rounding error.

## Roadmap

| Slice | Scope | Premise it proves |
|---|---|---|
| **v0.1** | Layout, stock surface, glyphon text, custom shader. | Rendering substrate works; LLMs can write shaders. |
| **v0.2** | Hit-testing, click events, automatic hover/press, App trait, state-driven rebuild. | Real interactive apps possible; build-from-state shape works. |
| **v0.3** | Scroll/clip, embedded viewport (third escape hatch), modal/overlay primitive. | Multi-pane apps with host-painted regions possible. |
| **v0.4** | Animation primitives, focus traversal, keyboard event routing, hotkey system. | Polished interaction; vim-style apps possible. |
| **v0.5** | Custom layout (second escape hatch), virtualized lists, `feed`/`chat_log` primitives. | Domain visualizations possible; large streams render efficiently. |
| **v0.6** | Rich text composition (markdown runs, inline highlighting, embedded elements). | Whisper-agent-grade chat, whisper-git-grade diff viewer possible. |
| **v0.7+** | Stock shader: shadow, focus_ring, divider_line. Backdrop sampling. wgpu-wasm + vulkano backends. Liquid glass. | Visual ceiling reaches the SHADER_VISION premise. |

These are slices, not deadlines. Each slice's MVP is one fixture that exercises every primitive in the slice end-to-end.

## What this is not

- **Not reactive.** When state changes, the host calls `request_redraw()`. The library doesn't observe state. This is a deliberate, hard line. Pushing reactivity into the library means picking a state model, which violates the "host owns state" rule.
- **Not retained.** No widget instances persist across rebuilds. The build closure produces a fresh tree. Identity for state-restoration purposes (preserved scroll position, in-flight animation) lives in the library's internal trackers keyed by `El.key`, not in the tree itself.
- **Not a game engine.** No ECS, no scene graph beyond the tree, no physics, no spatial audio. We'll borrow from game-engine UI work where it overlaps (gpu pipelines, custom shaders) but the library is bounded by "application UI."
- **Not an agent framework.** The bundle artifacts (`tree.txt`, `draw_ops.txt`, `shader_manifest.txt`, `lint.txt`) are designed for an LLM author's edit-render-inspect loop, but the library doesn't dictate how that loop works. It just emits the artifacts.

The discipline through every future slice: **does this primitive let an LLM author a polished native app a step closer than the previous slice?** If yes, ship it. If it expands the API surface without expanding what's expressible, push back.
