# LLM-Native UI Library Rationale

This project is a small MVP sketch for a UI toolkit designed around a new assumption:

> SOTA LLM coding agents will author, revise, and polish a large fraction of application UI code.

The goal is not merely to build a custom renderer. The more interesting target is a **pretty, inspectable, LLM-friendly UI grammar** with a performant native renderer underneath.

## Origin of the idea

Christian's current UI baseline is mostly `egui`, with a few motivating reference projects:

- `whisper-agent`: egui+wasm web UI and egui desktop client.
- `polychora`: custom Vulkan/game renderer with egui used for menus and dev overlays.
- `whisper-git`: a Git client written by LLMs with Christian's oversight, using a custom Vulkan renderer and a custom retained widget layer.

The observed pattern is that `egui` is excellent for rapid prototyping and internal tools, but becomes harder to polish through LLM-driven iteration. `whisper-git` demonstrates that a custom renderer can get surprisingly close to egui-level polish, but also exposes the risk of ad-hoc layout, render architecture, and repeated one-off styling decisions.

## Core hypothesis

Immediate-mode UI was designed around fast human iteration. React/HTML works unusually well for LLMs because the training distribution contains enormous amounts of component-tree UI code, Tailwind-style utility tokens, design system conventions, and examples of modern app polish.

A good LLM-native UI library should therefore borrow the *shape* of the web/react ecosystem without borrowing the web stack:

- semantic component tree
- familiar layout primitives
- design tokens
- component variants
- pretty defaults
- animation/motion presets
- screenshot and interaction artifacts
- source-mapped visual nodes
- inspectable retained UI tree

The target is not `React but Rust`, and not `egui but retained`. The target is closer to:

> shadcn/ui-style component grammar + SwiftUI/Flutter-ish retained declarative structure + game-engine/native rendering discipline + first-class LLM inspection artifacts.

## Why egui is not ideal for this use case

`egui` is very good at local, procedural UI construction. It is less good as a substrate for agentic polish because:

1. **Layout is temporal and procedural.**
   The placement of a widget often depends on the history of previous UI calls mutating a cursor/spacing state.

2. **The semantic UI tree is ephemeral.**
   LLMs benefit from a persistent inspectable tree of nodes, roles, styles, and computed rectangles.

3. **Polish becomes non-local.**
   Spacing, density, hover states, focus behavior, and typography often end up encoded as scattered constants.

4. **Visual feedback is hard to localize.**
   If a screenshot shows a bad element, an agent needs a stable node identity and source mapping to edit the right component.

5. **Dynamic feel is difficult to reason about.**
   Screenshots do not show hover feel, modal timing, scroll behavior, drag affordances, or transition quality.

Immediate mode is still useful for debug panels, experiments, and overlays. The claim is narrower: polished LLM-authored applications want a more structured substrate.

## What React/HTML gets right for LLMs

The web stack is aesthetically and technically unpleasant in many ways, but its UI authoring shape is extremely LLM-friendly:

- a nested semantic tree
- a massive vocabulary of common components (`Card`, `Badge`, `Dialog`, `Tabs`, `CommandPalette`, etc.)
- compact style tokens and utility classes
- polished default components from mature libraries
- common animation presets and interaction conventions
- endless examples in the training distribution

This project attempts to steal those strengths while avoiding the HTML/JS/React runtime stack.

## Design principles

### 1. Pretty component grammar first, renderer second

The library should let an LLM one-shot something that looks like a mature app by composing high-level components:

- `Card`
- `Button`
- `Badge`
- `Sidebar`
- `Toolbar`
- `VirtualList`
- `Modal`
- `Toast`
- eventually `CommandPalette`, `DataTable`, `CodeBlock`, `DiffView`, `ChatBubble`, `InspectorPanel`, etc.

The renderer matters, but the primary product is the component/design grammar.

### 2. Retained semantic tree

Every UI element should have:

- stable node ID
- semantic role
- computed rectangle
- source map / component label
- style references
- children

This enables inspection, visual/source mapping, automated review, and targeted edits.

### 3. Explicit layout primitives

Prefer declarative layout structures over cursor mutation:

- `Column`
- `Row`
- `Split`
- `Grid`
- `Overlay`
- `Scroll`
- `VirtualList`
- `Dock`
- `Canvas` escape hatch

The MVP implements only a few of these.

### 4. Typed design tokens

Avoid arbitrary colors, radii, spacings, and easing curves scattered through app code. Prefer:

- `theme.colors.surface_raised`
- `theme.space.md`
- `theme.radius.xl`
- `ButtonVariant::Primary`
- `BadgeVariant::Success`
- `MotionPreset::ModalEnter`

LLMs should choose from a curated taste vocabulary instead of inventing magic numbers.

### 5. Opinionated motion presets

Dynamic feel is hard for LLMs to tune from raw timing curves. The toolkit should provide semantic motion tokens:

- modal enter/exit
- popover open/close
- toast slide/fade
- list insertion/removal
- hover/press/focus transitions
- drag/reorder behavior

The MVP includes a modal contact-sheet generator to show the artifact style.

### 6. Headless visual artifacts are first-class

Agents need reproducible feedback loops. The library should make it easy to generate:

- static screenshots
- named UI fixtures
- UI tree dumps
- render command dumps
- node/source maps
- point-pick results
- animation contact sheets
- interaction trace replays
- visual diffs

The MVP currently outputs SVG, a semantic tree dump, and a motion contact sheet.

### 7. Escape hatches should be boxed and inspectable

Some UI surfaces need custom drawing: commit graphs, diff views, charts, game HUDs, 4D visualizers. The library should provide a `Canvas`/custom-paint escape hatch, but keep it source-mapped and isolated so the rest of the UI remains structured.

### 8. Performance should be native-minded

The intended implementation path is retained dirty subtrees, virtualized lists, cached text/layout, batched render commands, and native GPU backends. However, the MVP intentionally starts with API shape and artifacts rather than premature renderer optimization.

## What this MVP demonstrates

The current sketch implements:

- `Theme::dark_blue_gray()` token set.
- `Node` retained semantic tree.
- `Role`, `SourceMap`, `Layout`, and `Style` metadata.
- Basic layout pass for `Column`, `Row`, `Split`, and `VirtualList`.
- `RenderCommand` IR.
- SVG backend with `data-node` attributes.
- `inspect_tree()` for node/role/rect/source output.
- `motion_contact_sheet()` for semantic motion review.
- A Git-dashboard-like example composed from high-level components.

Run:

```bash
cd /home/christian/mavis_data/ui_lib_demo
RUSTUP_HOME=/home/christian/.rustup CARGO_HOME=/home/christian/.cargo cargo +stable run --example git_dashboard
```

Artifacts:

- `out/git_dashboard.svg`
- `out/git_dashboard.inspect.txt`
- `out/git_dashboard.motion.svg`

## What this MVP intentionally does not solve yet

- real text shaping
- input/event routing
- accessibility
- actual retained state update/diffing
- scroll physics
- hover/focus/pressed transitions
- clipping
- z-order beyond traversal order
- image/icon atlases
- GPU backend
- responsive measurement beyond simple hints
- source maps to actual call sites for every user-created node
- real visual regression tests

Those are future layers. The first question is whether the authoring and artifact shape feels right for LLM agents.

## Open questions

1. Should the authoring surface be pure Rust builders, a macro DSL, or a small declarative markup language?
2. How close should the component names and layout model stay to React/Tailwind conventions to exploit LLM priors?
3. Should style tokens be fully typed Rust values, compact class-like utilities, or both?
4. How should component state and app/domain state be separated?
5. What is the right event model: Elm-style messages, callbacks, commands, or hybrid?
6. Should the renderer backend start with Vulkan, wgpu, or a retained CPU/SVG/headless backend?
7. How much animation should be automatic vs explicitly requested?
8. Can interaction trace/contact-sheet artifacts make LLMs meaningfully better at dynamic feel?
9. How should a visual point-pick map from pixel → node → source → suggested edit?
10. What subset of components would make the system useful for `whisper-git`, `whisper-agent`, and `polychora` first?

## Current intuition

The highest-leverage next step is not building a full renderer. It is building a slightly richer component kit and a better inspection loop, then asking multiple SOTA LLMs to author and revise small UI surfaces with it. The benchmark should be:

> Given a one-paragraph product request, can an LLM produce a polished, coherent UI surface without hand-placing rectangles or inventing raw style constants?

If yes, this is worth turning into a serious project.
