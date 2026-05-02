# Next Steps

This file distills the current direction after Mavis's initial sketch and the independent Opus review.

## Highest-confidence direction

Build an **LLM-native UI grammar and inspection loop**, not just a renderer.

The central product should be:

- retained semantic nodes,
- source-mapped visual artifacts,
- typed design tokens,
- high-level pretty components,
- semantic motion presets,
- review/lint/test artifacts designed for LLM feedback loops.

## Immediate implementation priorities

### 1. Replace width/height hints with sizing intents

Current MVP uses `desired_w` / `desired_h`, which encourages magic numbers.

Introduce something like:

```rust
enum Size {
    Fixed(f32),
    Fill(f32),
    Hug,
}

struct Constraints {
    width: Size,
    height: Size,
    min_w: Option<f32>,
    max_w: Option<f32>,
    min_h: Option<f32>,
    max_h: Option<f32>,
}
```

Goal: express cards, rows, sidebars, and panels as intent (`Fill`, `Hug`, `Fixed`) rather than pixels.

### 2. Add a source-map/call-site macro

Current component constructors record the component definition location, not the user's call site.

Add a small macro layer only for call-site capture, not a full custom DSL:

```rust
card!(theme, "Commit Graph", [ ... ])
button!(theme, "Commit").primary().on_press(Msg::Commit)
```

The macro should inject:

- caller file,
- caller line,
- optional stable call-site key,
- maybe parent-derived node path.

### 3. Pick an event model

Use an Elm/Iced-like model:

```rust
fn view(state: &State) -> El<Msg>
fn update(state: &mut State, msg: Msg)
```

First target: make `Button` emit a message.

Avoid closure-heavy callback APIs as the primary model.

### 4. Add token/magic-number linting

Generate a report per screen:

- colors used,
- spacing tokens used,
- radii used,
- raw/magic numbers used,
- nodes missing stable keys,
- duplicate IDs.

This becomes a direct LLM feedback artifact.

### 5. Add multi-width artifact generation

For every example/fixture, generate side-by-side outputs at widths like:

- 600,
- 900,
- 1200,
- 1600.

This directly attacks LLM weakness on responsive layout.

### 6. Build one hard component end-to-end

Candidate: `CommandPalette`.

It exercises:

- overlay,
- focus,
- keyboard navigation,
- filtering,
- virtualized list,
- selection state,
- modal/popover motion,
- escape/cancel behavior,
- screenshot and interaction artifacts.

## Validation benchmark

Do not judge only by whether the first screenshot looks good.

Compare this toolkit to egui on:

1. one-shot compile success,
2. one-shot visual quality,
3. number of magic numbers,
4. ease of iterative polish,
5. whether feedback edits touch the right source node,
6. whether unrelated regions regress.

The main expected win is iterative polish convergence, not initial prototyping.

## Principle to protect

Do not let this become renderer-first.

The renderer matters, especially for Christian's native/Vulkan preferences, but the distinct value is:

> inspectable, reviewable, lintable, source-mapped UI authoring for LLM agents.
