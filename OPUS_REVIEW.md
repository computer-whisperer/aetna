# Opus Review: LLM-Native UI Substrate

This document preserves the independent Claude Opus review requested by Christian on 2026-05-02. The review focused on `/home/christian/mavis_data/ui_lib_demo` as an early sketch of an LLM-native Rust/native UI toolkit.

## TL;DR

The core thesis is correct: for LLM-authored UI, the bottleneck is less the renderer and more the **grammar plus feedback loop**. The MVP gets three important things right:

1. retained semantic tree with stable IDs,
2. typed token vocabulary,
3. headless/inspectable artifacts.

The largest gaps are:

1. layout expressiveness,
2. state/event semantics,
3. robust ID/source-map handling.

The inspection/artifact strategy is the most novel contribution and should remain the center of the project.

## What the design gets right

### Stable source-mapped node IDs

Stable IDs plus SVG `data-node` attributes are the highest-value feature. They enable:

- screenshot element → node ID,
- node ID → source/component,
- source/component → targeted edit.

This is the feedback loop egui does not naturally provide because widgets are ephemeral between frames.

### Typed tokens reduce taste entropy

Typed tokens and variants such as:

- `theme.colors.surface_raised`,
- `theme.space.md`,
- `ButtonVariant::Primary`,
- `BadgeVariant::Success`,
- `MotionPreset::ModalEnter`,

help LLMs choose from a finite vocabulary instead of inventing arbitrary colors, spacing, and curves.

### Motion contact sheets are a real idea

Static screenshots make it easy for LLMs to produce visually plausible but dead-feeling UI. Contact sheets showing timed animation states give agents a readable artifact for dynamic feel.

### Render-command IR is the right factoring

`Node → layout → RenderCommand → backend` keeps SVG/headless output, future GPU output, hit-testing, and inspection connected to the same intermediate representation.

### MVP scope is appropriate

The MVP correctly punts text shaping, input, accessibility, and production rendering. The first hypothesis is whether the API/artifact shape is promising.

## Main critiques

### 1. Layout model is too thin

The current `desired_w` / `desired_h` hints push authors back toward magic pixel constants. That undermines the goal of tokenized, intent-level UI authoring.

Recommended replacement:

```rust
enum Sizing {
    Fixed(f32),
    Fill(f32),
    Hug,
    Min(Box<Sizing>, f32),
    Max(Box<Sizing>, f32),
}
```

Needed layout concepts:

- `Fixed(px)`,
- `Fill(weight)`,
- `Hug` / intrinsic size,
- min/max constraints,
- cross-axis alignment,
- asymmetric padding,
- max width / typographic measure,
- better split/dock semantics.

A 3-pane shell should express `[fixed_left, fill_main, fixed_right]` without custom hardcoding.

### 2. State and events are load-bearing

The MVP currently has no event model. This must be chosen before adding many stateful components.

Recommended direction:

- Elm/Iced-style messages,
- pure-ish `view(state) -> El<Msg>`,
- centralized `update(msg, &mut state)`,
- avoid closure-heavy callback APIs,
- avoid hidden state-hook systems like SwiftUI/Compose `@State` / `remember` as the primary model.

Example desired shape:

```rust
button("Commit")
    .primary()
    .on_press(AppMsg::Commit)
```

### 3. String IDs are fragile

Hand-written string IDs create silent collision risks and boilerplate. They also do not currently capture the user's call site correctly: `SourceMap::new("Card", file!(), line!())` inside the component function points to the component definition, not the call site.

Recommended fixes:

- derive IDs from parent path + component + sibling index,
- allow explicit `.key(...)` for stable list identity,
- add a thin macro only for call-site capture:

```rust
card!(theme, "Commit Graph", [ ... ])
```

The macro should inject caller `file!()` / `line!()` and maybe a stable call-site key.

### 4. Current component kit only proves easy rectangle components

The MVP has simple components:

- `Card`,
- `Button`,
- `Badge`,
- `Sidebar`,
- `Toolbar`,
- `Modal`,
- `Toast`,
- `VirtualList`.

The API will be tested by harder components:

- `TextField` / `TextArea`,
- `Combobox` / `Autocomplete`,
- `Tabs`,
- `DataTable`,
- `Tree`,
- `CommandPalette`,
- `Tooltip` / `Popover`,
- `ContextMenu`,
- `DiffView` / `CodeBlock`,
- draggable `Splitter`.

A `CommandPalette` is a good next end-to-end component because it exercises state, focus, filtering, virtualized results, overlay positioning, keyboard navigation, and motion.

### 5. Motion presets should attach to components and interaction states

The current motion preset/contact-sheet idea is good but not wired into component behavior.

Desired motion vocabulary:

- `Hover::Lift`,
- `Press::Sink`,
- `Focus::Ring`,
- `Enter::FadeUp`,
- `Exit::FadeDown`,
- `Reorder::Swap`,
- `Loading::Shimmer`.

Motion should be semantic and interaction-driven, not raw `t_ms` in application code.

### 6. SVG is useful but should not become the only artifact

Recommended future backends/artifacts:

- render-command JSON,
- PNG/raster output, maybe via `tiny-skia`,
- eventually a real GPU backend,
- ensure artifacts match production rendering as closely as possible.

### 7. Responsive/theme variance should be automatic

Every fixture should eventually render:

- dark + light theme,
- multiple widths such as 600 / 900 / 1200 / 1600,
- possibly scale-factor variants.

This addresses a known LLM weakness: designs that look good at one size and break elsewhere.

## Builder vs macro vs markup recommendation

Preferred authoring surface:

- primary: plain Rust builders,
- secondary: thin macro only for call-site capture / ID/source-map ergonomics,
- avoid full custom DSL or markup unless it becomes the only surface.

Rationale:

- builders give type errors and method completion,
- full macro DSLs have poor errors and weak training priors,
- separate markup doubles the surface area and creates impedance mismatch,
- a tiny macro that preserves the builder model but captures source location is worthwhile.

Recommended ergonomic improvement:

- `IntoNode` / `IntoEl` polymorphism so `Node`, `Option<Node>`, `Vec<Node>`, and raw strings compose naturally.

## How much to imitate React/Tailwind/shadcn/Framer

Recommendation:

> Imitate the vocabulary aggressively. Reject the runtime semantics aggressively.

Steal:

- component names (`Card`, `Dialog`, `Tooltip`, `Tabs`, `CommandPalette`),
- variant names (`primary`, `secondary`, `ghost`, `destructive`),
- token names (`surface`, `muted`, `accent`),
- motion concepts (`enter`, `exit`, `hover`, `press`),
- shadcn's opinionatedness,
- Tailwind's finite scale vocabulary.

Reject:

- JSX,
- hooks,
- virtual DOM,
- `className: string`,
- CSS cascade,
- props spread,
- render props,
- free-form animation blobs.

Desired mental model:

> shadcn component vocabulary on an Iced-like runtime, with SwiftUI-ish layout and a custom inspection/artifact layer.

## Proposed benchmark

The benchmark should measure not just one-shot output, but closed-loop polish.

### Part A: one-shot fidelity

Give the same model the same product spec with egui and with this toolkit. Score:

- compile success,
- visual coherence,
- number of magic numbers,
- token usage to first compilable result.

### Part B: iterative polish

Start from generated code and apply three rounds of feedback:

- "sidebar too narrow",
- "badge wraps",
- "modal feels abrupt",
- etc.

Score:

- whether the right node/source was edited,
- whether unrelated visual regions regressed,
- rounds to quality threshold.

The toolkit should win primarily on Part B: faster and safer convergence.

Smallest meaningful version:

- 3 product specs,
- 5 LLMs,
- 2 toolkits,
- 3 polish rounds.

## API shape Opus would prefer

```rust
fn commit_pane(state: &State, theme: &Theme) -> El<Msg> {
    column()
        .gap(theme.space.md)
        .padding(theme.space.lg)
        .children([
            card("Commit Graph")
                .subtitle("Virtualized rows; selection is a component default.")
                .body(
                    virtual_list(state.commits.iter())
                        .row(|c| commit_row(c, c.id == state.selected))
                        .on_select(Msg::Select)
                        .height(Fill)
                )
                .height(Fill),
            card("Staging")
                .body(column().gap(theme.space.sm).children([
                    text(format!("{} modified · {} staged", state.modified, state.staged)).muted(),
                    badge("subject under 72 chars").info(),
                    button("Commit").primary().on_press(Msg::Commit),
                ]))
                .height(Hug),
        ])
}
```

Key preferences embedded in this sketch:

- no magic numbers,
- derived IDs,
- variants as methods (`.primary()`) rather than enum arguments,
- children via `IntoNode`,
- `view(state) -> El<Msg>` pure-ish architecture,
- named component slots (`title`, `subtitle`, `body`, `footer`).

## Dynamic-feel artifacts to add

Beyond screenshots and the existing motion contact sheet:

1. **Interaction storyboard**: default → hover → focus → press → disabled for each interactive component.
2. **State-machine trace**: contact sheet annotated with semantic events such as `OPEN`, `FOCUS_FIRST_INPUT`, `ESC_HANDLED`.
3. **Hit-test heatmap**: roles tinted and point-pick behavior visible.
4. **Layout tape under stretch**: same UI at 600/900/1200/1600 widths side-by-side.
5. **Token usage report**: colors/spacings/radii/magic numbers used by a screen.
6. **Diff-aware screenshots**: before/after/diff with changed regions outlined.
7. **Z-tape**: each visual layer rendered separately plus composited.
8. **Focus order trace**: numbered tab-order overlay.

## Ranked next steps

1. Replace `desired_w/h` with a real `Sizing` model (`Fixed`, `Fill`, `Hug`, min/max).
2. Add call-site-capturing macro for IDs and source maps.
3. Pick and implement an Iced/Elm-style `Msg` event model for `Button`.
4. Write the egui-vs-this-toolkit polish-loop benchmark before adding many components.
5. Add light theme and multi-width artifact generation.
6. Add token/magic-number linting.
7. Replace raw `Vec<Node>` children with `IntoNode` ergonomics.
8. Add named slots for compound components.
9. Rework generic/custom roles into a more consistent component/role system.
10. Build `CommandPalette` end-to-end before declaring the API shape stable.

## Verdict

This is worth pursuing, but only if the inspection/artifact loop remains the central product. Otherwise it risks becoming just another retained Rust UI library.

The renderer is table stakes. The moat is the **inspectable, reviewable, lintable, source-mapped grammar** for LLM authors.
