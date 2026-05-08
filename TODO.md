# Aetna — open work

Tracked items, ordered by current priority. Architectural framing lives in
`docs/LIBRARY_VISION.md`, `docs/SHADER_VISION.md`, and
`docs/POLISH_CALIBRATION.md`; this file is just the punch list.

The 0.2.0 release closed every numbered slice (controlled `select`,
keyboard reach into popovers, slider keyboard, tooltips,
async-into-redraw documentation, optimistic-override + `overlays`
helpers, tabs / segmented control, form primitives + progress bar) and
the pre-release housekeeping that went with them.

Post-0.2.0 polish has landed without being tracked as numbered slices:
runtime-managed toasts (`App::drain_toasts` + the showcase `Toasts`
section), `image()` widget across wgpu + vulkano, scrollbar thumb with
hover-expand + click-to-page, `resize_handle` for movable dividers,
`field_row` + `slider::apply_input` form helpers, the global static-text
selection model (`.selectable()` + cross-leaf drag + double/triple-click
+ Linux primary-selection + integration with `text_input` / `text_area`),
caret blink, the pointer-cursor model + native/web cursor forwarding,
text decorations (`.underline()` / `.strikethrough()` / `.link(url)`),
and shadcn-aligned interaction-feedback polish. New work lands as
numbered slices below.

## Floating-layer architecture (ratified)

Three categories, three treatments — kept here as the load-bearing rule
new floating-layer work should still respect. The architectural
commitment from `crates/aetna-core/src/widgets/popover.rs` — *no portal
hoist; floating layers live where they paint at the root* — is the
invariant. Everything below leans into that rule rather than working
around it.

1. **Modals** — app-owned, root-stacked.
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

## Deferred

Out of scope; flagged so they don't get rediscovered:

- Slider tick marks (e.g. nominal-100% mark). Audio-app-specific.
- Variable-height list virtualization. Not surfaced by any port yet.
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
- **Lint suppressions.** Apps can scope lint findings with
  `app_path_marker` and post-filter reports, but there is no deliberate
  in-tree suppression API. Add familiar `.allow_lint(...)` semantics
  before polishing lints get stricter: suppress by `FindingKind`, require
  a reason string, keep suppressed findings auditable, and avoid
  suppressing `DuplicateId` until attribution is stronger.
- **Themed editable-text geometry.** Plain text nodes now carry
  `FontFamily` through theme application, layout, SVG, and backend
  shaping. `text_input` / `text_area` editing helpers still construct
  `TextGeometry` before runtime theme application, so caret and
  selection math use the default family unless those widgets grow an
  explicit family parameter or a theme-aware editing context.
