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
Inter as the default UI font, shadcn-shaped palette tokens and stock
zinc/neutral plus Radix slate+blue palettes, theme-driven component
size/density metrics, card/form/text rhythm polish, table/sidebar/menu/
command/dropdown/dialog/sheet/widget-kit expansion, lint coverage for
icon/text row spacing and alignment, calibration/reference harnesses,
and shadcn-aligned interaction-feedback polish including non-hovering
tab-list gaps. New work lands as numbered slices below.

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

### Slice 1 — `scroll` stick-to-bottom

Chat-log / activity-feed scenes want the "hug the tail while at the
bottom, release when the user scrolls up, re-pin if they return"
behavior egui shipped as
`egui::ScrollArea::stick_to_bottom(true)`. The whisper-agent
aetna port currently has no way to implement this client-side:

- `App::drain_scroll_requests` can emit
  `ScrollRequest::EnsureVisible { container_key, y, h }`, but that
  would force-scroll even when the user has deliberately scrolled
  up.
- The app can't read scroll offset / viewport / content size from
  `App::build` or `App::before_build` (`BuildCx` only carries the
  theme; `UiState::scroll_offset` lives behind the host-private
  `&mut UiState` the runtime owns). So an app-side
  "auto-pin-when-at-bottom" decision has nothing to branch on.

Two shapes worth weighing:

- [ ] **Library-managed pin via builder.** `scroll(children).pin_end()`
      (or `.auto_follow_tail()`) on the `Kind::Scroll` `El`. The
      runtime tracks one per-scroll bool — set when a layout pass
      finds the previous offset within `epsilon` of max-offset, cleared
      by any user-initiated wheel / drag / keyboard scroll that moves
      the offset off the tail. When the bit is set and a subsequent
      layout pass discovers content has grown, the resolver clamps the
      offset back to the new max. Matches the egui shape; single
      builder, zero app bookkeeping. Recommended.
- [ ] **App-readable scroll state.** A new `App::before_build_with(&mut
      self, &UiState)` (or `&UiState` on `BuildCx`) so the app can
      read `scroll_offset(key)` and decide for itself whether to push
      an `EnsureVisible`. More general — other app patterns (jump-to-
      latest button visibility, unread-divider placement) could lean
      on the same read — but more surface to design, and apps still
      have to reimplement the "was-at-tail" debounce against
      programmatic offset jumps.

Edge cases the implementer should consider:

- Pin state should survive viewport resizes (e.g. sidebar drag)
  that change the max offset without user input.
- Programmatic `set_scroll_offset` / `EnsureVisible` requests that
  land at the tail should set the bit (so a "jump to latest"
  button activates pinning).
- A scroll container mounted with `pin_end()` should start pinned
  to the tail on first layout, not at offset 0.
- Cooperation with `EnsureVisible` to a non-tail anchor: presumably
  the explicit request wins and clears the pin until the user
  returns to the tail.

The whisper-agent aetna port needs this for the chat log; the same
shape would close the related Markdown-viewer "streaming text
sometimes scrolls flat to the top of the body" symptom the port
also observes.

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
- **Lint suppressions.** Apps can post-filter the report by
  `FindingKind` / source, but there is no deliberate in-tree
  suppression API. Add familiar `.allow_lint(...)` semantics before
  polishing lints get stricter: suppress by `FindingKind`, require a
  reason string, keep suppressed findings auditable, and avoid
  suppressing `DuplicateId` until attribution is stronger.
- **Themed editable-text geometry.** Plain text nodes now carry
  `FontFamily` through theme application, layout, SVG, and backend
  shaping. `text_input` / `text_area` editing helpers still construct
  `TextGeometry` before runtime theme application, so caret and
  selection math use the default family unless those widgets grow an
  explicit family parameter or a theme-aware editing context.
