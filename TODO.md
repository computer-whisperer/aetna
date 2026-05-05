# Aetna — open work

Tracked items, ordered by current priority. Architectural framing lives in
`docs/LIBRARY_VISION.md`, `docs/SHADER_VISION.md`, and
`docs/POLISH_CALIBRATION.md`; this file is just the punch list.

## Floating-layer architecture

Three categories, three treatments. The architectural commitment from
`crates/aetna-core/src/widgets/popover.rs` — *no portal hoist; floating
layers live where they paint at the root* — is load-bearing. Everything
below leans into that rule rather than working around it.

1. **Modals** — app-owned, root-stacked. Already works.
2. **Popovers / dropdowns / context menus** — app-owned, root-stacked.
   A dropdown opened from inside a modal is a *second* overlay layer
   appended to the root stack; the "from inside" relationship is an
   app-state fact (modal open AND dropdown open), not a tree fact.
   Click-outside semantics already nest correctly: each scrim emits
   `{key}:dismiss` for that key only, topmost scrim eats the click.
3. **Tooltips** — library-owned, runtime-appended. Hover state lives in
   `UiState`; the runtime synthesizes tooltip layers from a `.tooltip()`
   modifier on the trigger and appends them to the root tree after
   `build()` returns. Same pattern as focus rings (library writes from
   envelope state); the user's `El` tree is never mutated, so this is
   an extension, not a portal.

Runtime ordering: `[user main + user overlays..., library tooltips...]`.

## Slices

### Slice 1 — controlled `select` helpers

- [x] **`select::apply_event(&mut value, &mut open, &event, key, parse)`
      and `select::classify_event(event, key) -> Option<SelectAction>`.**
      Absorb the toggle / dismiss / option-pick dispatch so apps stop
      hand-parsing `{key}:option:{value}` and `{key}:dismiss` suffixes.
      Trigger / popover-layer split stays (root mount per the
      architectural rule). Rewrite the volume profile picker in the
      same slice so the helper signature is exercised by a real call
      site (~75 lines → ~15).

### Slice 2 — keyboard reach into popovers

- [x] **Focus stack on `UiState`.** Push current focus when an overlay
      opens, pop on close. Single `request_focus_key` slot isn't enough
      for nested cases (modal → dropdown): closing the inner layer must
      return focus to the trigger inside the modal, not to the trigger
      that opened the modal.
- [x] **Arrow-nav inside `menu_item` lists.** Up / Down / Home / End
      navigate siblings inside a `popover_panel`; handled by the
      runtime against any `arrow_nav_siblings` parent. Tab traversal is
      unchanged.
- [x] **Auto-focus on popover open + Escape returns focus.** Built on
      the focus stack — Escape goes to the app, the app dismisses the
      layer, and the disappearing `popover_layer` triggers the restore.

### Slice 3 — slider keyboard

- [x] **`slider::apply_event(&mut value, &event, step, page_step)`** and
      `slider::classify_event` returning a `SliderAction`. ArrowUp /
      ArrowRight + ArrowDown / ArrowLeft step by `step`; PageUp / PageDown
      by `page_step`; Home / End jump to 0 / 1. Wired into `aetna-volume`
      so the volume sliders take focused-keyboard input alongside the
      existing pointer scrub.

### Slice 4 — tooltips

- [ ] **`.tooltip(text)` modifier on `El`.** Library-side runtime
      synthesizes the tooltip layer from hover envelope state, anchored
      to the trigger's rect, and appends it to the root tree after
      `build()` returns. No author-side overlay composition. Slice
      delivers the runtime synthesis, hover-delay timing, and the
      `popover_panel`-styled visual. Volume doesn't need this; a real
      desktop shell will.

### Slice 5 — list-row primitive

- [ ] **`list_row` with leading slot / title+subtitle slot / trailing
      slots, density-token driven.** Add a calibration fixture in
      `aetna-fixtures` exercising ellipsis with realistic long names.
      The pavucontrol-style row is a missing reference shape from
      `docs/POLISH_CALIBRATION.md`.

### Slice 6 — async-into-redraw

- [ ] **Documented host-agnostic story for backend threads waking the
      UI loop** (e.g. winit `EventLoopProxy` exposed through a
      `HostConfig::with_external_wakeup` hook). Use `aetna-volume`'s
      PipeWire meters as the worked example. Today the volume app
      polls at 33 ms via `HostConfig::with_redraw_interval`.

### Slice 7 — recipes + helpers

- [x] **Document the optimistic-override pattern** in `widget_kit.md`
      §6.1, with the volume app's `percent_for` as the worked example.
      Apps that need to reflect external snapshots while keeping user
      input feel responsive can copy the shape directly.
- [x] **`overlays(main, [Option<El>, …])` helper** in
      `widgets/overlay.rs`. Filters `None`s; tidies the root-level
      layer composition pattern. Volume uses it for the profile menu.

## Pre-release housekeeping

- [x] Crate-level rustdoc skim. `cargo doc -p aetna-{core,wgpu,vulkano,winit-wgpu}`
      now produces zero warnings; the only remaining mentions of
      private crates in docs are the contextual `aetna-web` and
      `aetna-volume` references in `widget_kit.md`, which are prose
      pointers rather than intra-doc links.
- [ ] Once the recipes above exist, mirror them in `examples/` so packaged
      users discover them via cargo.

## Deferred

Out of scope for the current cycle; flagged so they don't get rediscovered:

- Tab / segmented-control widget. The volume port styles buttons; works
  fine for now.
- Slider tick marks (e.g. nominal-100% mark). Audio-app-specific.
- Variable-height list virtualization, drag-resizable splits. Not surfaced
  by any port yet.
