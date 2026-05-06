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

- [x] **`.tooltip(text)` modifier on `El`** in `tree/mod.rs`,
      runtime synthesis in `crates/aetna-core/src/tooltip.rs`. The
      runtime watches hover state, waits 500ms, then appends a
      tooltip layer (anchored to the trigger by `computed_id` via
      `LayoutCtx::rect_of_id`, a new lookup) to the El root. Pointer
      leave or primary press dismisses; the dismissed flag clears on
      hover-target change. Documented in `widget_kit.md` §6.2 and
      demonstrated in `examples/src/bin/tooltip.rs`.
      Out of scope (deferred): fade-in animation, focus-driven
      tooltips for keyboard-only users, multi-line wrapping at a
      max-width.

### Slice 6 — async-into-redraw

- [x] **Document the meter-class vs event-class trade-off** in
      `crates/aetna-winit-wgpu/README.md`. Apps with high-frequency
      live data (peak meters, throughput graphs) keep
      `HostConfig::with_redraw_interval`; apps with sparse events
      (registry events, downloads, file watchers) drop down to
      `EventLoopProxy::send_event` against `aetna-wgpu::Runner`
      directly. Folding the push hook into `HostConfig` is deferred
      until a non-meter use case inside the workspace pressure-tests
      the shape — the original framing (PipeWire meters as the
      worked example) had it backwards: meters are the case where
      fixed cadence is right, not the case that needs push-wake.

### Slice 7 — recipes + helpers

- [x] **Document the optimistic-override pattern** in `widget_kit.md`
      §6.1, with the volume app's `percent_for` as the worked example.
      Apps that need to reflect external snapshots while keeping user
      input feel responsive can copy the shape directly.
- [x] **`overlays(main, [Option<El>, …])` helper** in
      `widgets/overlay.rs`. Filters `None`s; tidies the root-level
      layer composition pattern. Volume uses it for the profile menu.

### Slice 8 — tabs / segmented control

- [x] **`tabs_list(key, &current, options)` + `tab_trigger` + the
      `tabs::classify_event` / `tabs::apply_event` pair** in
      `widgets/tabs.rs`. Mirrors shadcn / Radix Tabs (`<TabsList>` +
      `<TabsTrigger value=...>`) and the WAI-ARIA tablist pattern, so
      LLM authors hit familiar terrain. Routed key convention
      `{key}:tab:{value}` parallels `select`'s `{key}:option:{value}`.
      Demonstrated end-to-end in `examples/src/bin/tabs.rs` and
      mentioned in `widget_kit.md` §6. No `tab_panel` wrapper —
      Rust's `match` on the controlled value is more honest than a
      hidden-when-not-active sibling, and shadcn's `<TabsContent>`
      adds no visual beyond a plain block.

### Slice 9 — form primitives + read-only data display

- [x] **`switch`, `checkbox`, `radio_group` / `radio_item`,
      `progress`** in `widgets/{switch,checkbox,radio,progress}.rs`.
      Switch and checkbox are controlled bools sharing the same
      `apply_event(&mut bool, &event, key)` shape. Radio_group is a
      vertical column-of-radios paralleling tabs_list with the routed
      key convention `{key}:radio:{value}`. Progress is non-interactive
      — track + fill — and takes a caller-chosen fill color so apps
      can swap to `tokens::DESTRUCTIVE` near full. Switch / checkbox /
      radio / tab_trigger ship with animated state changes (thumb
      slide via animatable `translate`; check + dot via opacity +
      scale; tab fill / text-color cross-fade), since polish on those
      transitions is what makes a toggle widget feel native rather
      than a hard cut. All four widgets are mentioned in
      `widget_kit.md` §6 and exercised together in the showcase
      `Forms` section.

## Pre-release housekeeping

- [x] Crate-level rustdoc skim. `cargo doc -p aetna-{core,wgpu,vulkano,winit-wgpu}`
      now produces zero warnings; the only remaining mentions of
      private crates in docs are the contextual `aetna-web` and
      `aetna-volume` references in `widget_kit.md`, which are prose
      pointers rather than intra-doc links.
- [x] Mirror the new recipes in `examples/`. The popover example
      already exercises arrow-nav + focus-restore (and now uses
      `overlays`); a new `slider_keyboard` example demonstrates
      `slider::apply_event` end-to-end. The optimistic-override
      pattern remains pointed at `aetna-volume` from `widget_kit.md`
      §6.1 — it doesn't fit a small example without a fake backend.

## Deferred

Out of scope for the current cycle; flagged so they don't get rediscovered:

- Slider tick marks (e.g. nominal-100% mark). Audio-app-specific.
- Variable-height list virtualization, drag-resizable splits. Not surfaced
  by any port yet.
- Roving-tabindex arrow-key nav inside `tabs_list` (Left/Right cycling
  the active tab as in WAI-ARIA's full tablist pattern). The runtime's
  `arrow_nav_siblings` only wires Up/Down/Home/End today; teaching it
  about a horizontal axis would let `tabs_list` opt in. For now, Tab +
  Enter activate each trigger one-by-one, which matches the simpler
  shadcn default.
- **Themed shadow color.** `stock::rounded_rect` hardcodes a 0.30-alpha
  black drop shadow; a `tokens::SHADOW_RGBA` (or a per-role color in
  `theme.rs`) would let dark themes opt into a denser shadow without
  every widget restating the color. Wait for a theme that actually
  needs it.
- **Multi-layer / inset shadows.** Tailwind's `shadow-2xl`-style stacked
  drops and `inner shadow` (sunken role) both want a second SDF pass;
  the params slot only carries one blur today. Hold for an explicit
  ask — most surface roles already look right with the single layer.
- **Shader-override `paint_overflow` for shadow.** Custom shaders that
  pack their own shadow value into a different uniform name don't get
  auto-expansion in `draw_ops`; they must set `paint_overflow`
  manually. Consider a per-shader "shadow extent" metadata if a custom
  shader in the workspace ever ships shadows.
