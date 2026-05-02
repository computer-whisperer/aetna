# Current Assessment After v2

This document captures the current project judgment after the second iteration of `ui_lib_demo`.

## High-level take

The v2 iteration made the **architecture more convincing**, but did not yet make the demo visually competitive with `whisper-git`.

That is expected. The current code is still an MVP of the **agentic UI substrate**, not a mature design system or renderer. The important result is that the system is starting to expose its own failures through artifacts: inspect trees, lint reports, and responsive tapes.

The core question is not whether `out/git_dashboard.svg` is prettier than the current `whisper-git` UI. It is not. The core question is whether this architecture can create a tighter LLM polish loop than egui or ad-hoc custom layout. Current judgment: **probably yes, if the inspection/artifact layer remains central and the component/design kit becomes much richer.**

## What is working

### 1. Artifact-driven feedback is already useful

The new responsive tape immediately exposed a real layout failure: the fixed sidebar and fixed inspector consume too much width at 600px and collapse the main content. This is not a theoretical benefit. It is exactly the sort of failure that LLMs often miss from a single screenshot.

This validates the direction of generating review artifacts beyond one screenshot:

- semantic tree,
- token lint,
- responsive tape,
- motion contact sheet,
- eventually overflow reports, hit-test heatmaps, focus-order traces, and interaction storyboards.

### 2. Sizing intents are better than raw desired sizes

Replacing `desired_w` / `desired_h` with:

```rust
enum Size {
    Fixed(f32),
    Fill(f32),
    Hug,
}
```

is a meaningful improvement. It lets the author express layout intent rather than only pixel dimensions.

This is still not enough for real UI, but it is the correct direction. The next layer should add min/max constraints, overflow behavior, responsive breakpoints, and richer measurement.

### 3. Source mapping is becoming real

The `src_here!("Component")` macro is crude, but it means many example-created nodes now point back to the call site rather than only the component definition. This supports the desired loop:

> visual issue → node ID → inspect tree → source location → targeted edit.

This needs to become less verbose and more automatic, but the mechanism is proving useful.

### 4. Typed message actions feel like the right event direction

`Node<Msg>` / `El<Msg>` and `.on_action(Msg::...)` are only metadata right now, but they confirm that an Elm/Iced-like architecture is a good fit:

```rust
fn view(state: &State) -> El<Msg>
fn update(state: &mut State, msg: Msg)
```

This should remain the preferred event model over closure-heavy callback APIs.

### 5. Token linting is promising

The lint artifact currently reports duplicate IDs, raw colors, raw numbers, and token usage. It is primitive but points toward an important agentic feedback mechanism:

> "This screen used raw colors / magic numbers / duplicate IDs; fix those before polishing further."

This is a capability egui and most retained UI libraries do not naturally provide.

## What is not working yet

### 1. Visual quality is still low

The current `git_dashboard.svg` is visibly worse than the existing `whisper-git` UI. It has overflows and a basic "rectangles with text" feel.

This is not surprising. `whisper-git` already has a richer product-specific visual language: graph rows, branches, pills, shadows, context menus, toasts, staging UI, diff UI, etc. The MVP only has a tiny generic component set.

The correct interpretation is:

- **Bad sign:** if the architecture cannot explain and localize the failures.
- **Acceptable sign:** if the architecture makes those failures visible and gives a path to fix them.

v2 is closer to the second case.

### 2. Overflow behavior is missing

Observed visual issues mostly come from missing overflow semantics:

- text does not truncate or clip,
- rows do not ellipsize,
- panes can collapse to unusable width,
- SVG still renders text outside its intended visual bounds,
- no artifact reports overflow nodes.

Needed next primitive:

```rust
enum Overflow {
    Visible,
    Clip,
    Ellipsis,
    Fade,
    Scroll,
}
```

Likely defaults:

- text rows: `Ellipsis`,
- cards/panels: `Clip`,
- lists: `Scroll`,
- debug artifact: show overflow bounds in red.

### 3. Responsive behavior is missing

The responsive tape exposes failure, but the layout system cannot yet express the fix. A desktop 3-pane Git layout should not simply squeeze at 600px; it should reflow or collapse.

Needed concepts:

- breakpoints,
- conditional visibility,
- collapse-to-icons,
- drawer/sheet behavior,
- vertical reflow for narrow widths,
- min/max width constraints,
- layout variants per width class.

Possible future shape:

```rust
inspector.visible(Breakpoint::Desktop)
sidebar.collapse_to_icons(Breakpoint::Tablet)
shell.variant(LayoutVariant::DesktopThreePane).at(Breakpoint::Desktop)
shell.variant(LayoutVariant::Stacked).below(Breakpoint::Tablet)
```

### 4. IDs and keys are still not good enough

Current IDs are often path-derived, such as:

```text
app.1.row.1.column.0.card.2.list.3.row
```

This is inspectable but not pleasant or stable under insertion/reordering.

The `.key(...)` calls exist but are not yet used to derive semantic list IDs.

Target direction:

```text
commit_list.row[a18f2c3]
staging.subject_badge
toolbar.commit_button
```

Stable semantic IDs are critical for visual diffs and targeted LLM edits.

### 5. Component API is still too mechanical

Current authoring style is too verbose:

```rust
card(theme, src_here!("Card"), "Commit Graph", vec![ ... ])
```

The target should be closer to a fluent builder with named slots:

```rust
Card::new("Commit Graph")
    .subtitle("Virtualized rows; selection is a component default.")
    .body(...)
    .height(Size::Fill(1.0))
```

or:

```rust
card("Commit Graph")
    .subtitle(...)
    .body(...)
```

The `theme` and source-map plumbing should largely disappear from the user-facing API.

### 6. No hard component has been proven yet

The MVP components are mostly rectangles with text. The real test is a component that exercises state, focus, keyboard interaction, overlay positioning, filtering, and motion.

Best next candidate: **CommandPalette**.

It would test:

- overlay,
- keyboard navigation,
- focus,
- filtering,
- virtualized list,
- selected item state,
- enter/exit motion,
- escape behavior,
- empty states,
- artifacts beyond screenshot.

## Updated confidence

Current subjective scores:

- Visual quality: **3/10**
- Architecture clarity: **6.5/10**
- Agent inspectability: **7/10**
- Real app readiness: **2/10**

Expected near-term scores after overflow, responsive primitives, semantic IDs, and API cleanup:

- Visual quality: **5/10**
- Architecture clarity: **7.5/10**
- Agent inspectability: **8/10**
- Real app readiness: **4/10**

The likely validation point is rebuilding one real `whisper-git` pane. If the branch sidebar, staging well, or commit list becomes easier to improve in this toolkit than in the current custom code, the project becomes much more credible.

## Current bet

This architecture can plausibly outperform egui for **iterative LLM polish**, not because the current MVP is prettier, but because it supports a better feedback loop:

1. generate UI,
2. render deterministic artifacts,
3. identify visual/layout/token/focus/overflow problems,
4. map those problems to semantic nodes and source locations,
5. apply targeted edits,
6. verify with artifacts and diffs.

That loop is the product. The renderer and widgets are necessary, but the moat is the **inspectable, reviewable, lintable, source-mapped UI grammar**.

## Next iteration priorities

1. Add overflow/clipping/ellipsis semantics and an overflow report.
2. Add responsive primitives or at least breakpoint-driven visibility/collapse.
3. Improve semantic ID/key generation.
4. Clean up the builder API so `theme` and `src_here!` are not everywhere.
5. Build a non-trivial `CommandPalette` component end-to-end.

Do not optimize the renderer yet. The design substrate still needs to prove that it can make LLM-authored UI easier to polish than egui or ad-hoc custom layout.
