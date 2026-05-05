# Aetna — open work

Tracked items, ordered by current priority. Architectural framing lives in
`docs/LIBRARY_VISION.md`, `docs/SHADER_VISION.md`, and
`docs/POLISH_CALIBRATION.md`; this file is just the punch list.

## From the first validation port

The first real port (`aetna-volume`, a PipeWire control panel) surfaced these
gaps. Items are loosely ranked by friction observed in the port.

- [ ] **Controlled `select` widget mirroring `text_input`'s shape.** App owns
      `(value, open)`; one builder returns one `El`;
      `select::apply_event(&mut value, &mut open, &event, &options)` folds
      clicks / dismiss / option-pick. Internally anchor the popover at the
      trigger's keyed rect — no app-side root hoist, no manual
      `parse_*_event` for routed key suffixes. Today the volume profile
      picker spends ~75 lines on glue that should be ~10.

- [ ] **`UiState::request_focus_key(&str)` + arrow-nav inside `menu_item`
      lists.** Auto-focus the panel on popover open; Up/Down/Home/End
      navigate sibling menu items; Escape returns focus to the trigger.
      Couples naturally with the controlled `select` above; doing them as
      one slice is cheaper than three.

- [ ] **`slider::apply_event(&mut value, &event, step)` for keyboard.** Up
      / Down adjust by `step`; PageUp / PageDown by a coarse step. Pure
      controlled-state addition; matches the kit invariant.

- [ ] **List-row primitive + dense list/table calibration fixture.**
      `list_row` with leading slot / title+subtitle slot / trailing slots,
      density-token driven. Add a fixture in `aetna-fixtures` exercising
      ellipsis with realistic long names. The pavucontrol-style row is a
      missing reference shape from `docs/POLISH_CALIBRATION.md`.

- [ ] **Documented async-into-redraw recipe.** Host-agnostic story for
      backend threads to wake the UI loop (e.g. winit `EventLoopProxy`
      exposed through a `HostConfig::with_external_wakeup` hook). Use
      `aetna-volume`'s PipeWire meters as the worked example. Today the
      volume app polls at 33 ms via `HostConfig::with_redraw_interval`.

- [ ] **Document the optimistic-override pattern** (HashMap of overrides
      reconciled against snapshot equality on the next frame) in the
      widget kit / under `examples/`. Pattern repeated three times in
      `aetna-volume`; not a library change, but worth a recipe so the
      next port doesn't reinvent it.

## Pre-release housekeeping

- [ ] Crate-level rustdoc skim for accidental references to private /
      fixture crates.
- [ ] Once the recipes above exist, mirror them in `examples/` so packaged
      users discover them via cargo.

## Deferred

Out of scope for the current cycle; flagged so they don't get rediscovered:

- Tab / segmented-control widget. The volume port styles buttons; works
  fine for now.
- Slider tick marks (e.g. nominal-100% mark). Audio-app-specific.
- Variable-height list virtualization, drag-resizable splits. Not surfaced
  by any port yet.
