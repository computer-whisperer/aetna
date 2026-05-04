# Aetna Polish Calibration

This is the pre-port design-system slice. The goal is not to make Aetna
look like one specific web library. The goal is to calibrate Aetna's
defaults against UI patterns the industry has already validated, then
keep the authoring surface small enough that an agent can reliably reach
for those defaults.

## Premise

Aetna already has the application skeleton: state projects to an `El`
tree, interaction state lives in `UiState`, stock widgets are pure
compositions, and custom shader/layout escape hatches exist. The weak
spot is not mechanics. It is encoded taste.

Today too many polished screens reduce to:

- a dark background,
- rounded rectangles,
- a border,
- text,
- hover/focus animation.

That is a good substrate, but not yet a default-polish stance. Modern UI
systems also encode surface hierarchy, density, typography rhythm, icon
usage, selected/disabled/error states, overflow behavior, menu/list
composition, and restrained depth.

## Calibration Target

Use shadcn/ui as the primary calibration language because it maps well
to the model's training distribution: Tailwind-like token names, small
composable widgets, clear variant vocabulary, and high-quality defaults.
The point is not pixel-perfect imitation as a product goal. Pixel
matching is an instrument for tuning.

Canonical reference shapes:

1. Settings form
2. Command palette
3. Data table
4. Sidebar app shell
5. Dropdown/context menu
6. Dialog/form validation
7. Dashboard cards
8. Dense list/detail pane

For each shape, build an Aetna fixture using only stock widgets and
tokens. If a fixture needs many one-off colors, hand-tuned spacers, or
custom shaders, the library is missing a default.

## Shader-Themed By Default

Aetna's main advantage over CSS-shaped UI libraries is that the theme
does not have to stop at color substitution. A theme should be able to
control the shader program and the uniform recipe for stock surfaces.

That means a theme can change:

- colour roles,
- border and focus treatment,
- shadow/elevation math,
- bevels, highlights, inset edges,
- noise/grain/subtle texture,
- glass/refraction behavior,
- motion response curves,
- even the entire stock surface shader.

The intended shape is: authors still write familiar code like
`card(...).primary()` or `button("Save").secondary()`, but the active
theme decides how the surface role is rendered. A "quiet shadcn-like"
theme, a "native macOS" theme, a "Windows XP" theme, and an in-house app
theme should be aftermarket shader packages, not forks of every widget.

This reframes theme resolution:

```text
El surface role + token-backed colors + semantic state
    -> Theme resolves shader handle + uniforms
    -> backend draws using the selected surface program
```

The stock shader is the baseline, not the ceiling. Aetna should make it
easy for downstream apps to replace that baseline globally while keeping
the same author vocabulary and widget kit.

## Non-Goals

- Do not port CSS. Aetna's renderer is shader/native; CSS details are
  evidence, not architecture.
- Do not add a large theme runtime that must be threaded through every
  builder.
- Do not make every widget configurable before the defaults are good.
- Do not chase a single visual trend. Calibrate hierarchy, rhythm, and
  state treatment first.

## Method

### 1. Reference Extraction

For each canonical shape, record what matters:

- component dimensions: button/input/menu row/table row heights,
- spacing rhythm: inner padding, section gaps, dense row gaps,
- surface roles: app, panel, raised, sunken, popover, selected,
- border strength and alpha,
- shadow softness and elevation levels,
- typography roles: title, section heading, body, muted, caption, mono,
- state treatments: hover, pressed, selected, disabled, invalid, loading,
- icon placement and sizing,
- overflow/truncation policy.

Store observations as rules, not screenshots. Example:

```text
Menu rows are dense, usually 28-32 px tall, with left icon, label, and
right shortcut. Hover uses a subtle filled row, not a loud border.
```

### 2. Aetna Fixture

`cargo run -p aetna-core --example polish_calibration` renders a
representative screen into `crates/aetna-core/out/polish_calibration.*`.
This is the first tuning bench. It intentionally combines:

- app shell,
- sidebar nav,
- toolbar buttons,
- KPI cards,
- table/list rows,
- command/menu panel,
- form controls,
- selected/error/disabled-looking states,
- empty/help text,
- token-heavy styling.

The fixture is not the desired final design. It is the surface where
token, shader, and widget-default changes become visible.

### 2b. Reference Harness

`references/shadcn-calibration/` is a separate web fixture used only to
produce reference screenshots. It should stay isolated from the Rust
workspace. The harness uses Vite + React + Tailwind with shadcn-style
copied components, because that is the real shadcn consumption model:
components are source in the app, not an opaque runtime package.

The reference harness exists to answer calibration questions:

- What does a shadcn-like settings/data/table/menu shell look like at
  the same viewport as Aetna's fixture?
- Are Aetna's default row heights, radii, borders, text hierarchy, and
  selected states in the same basin?
- Which global token/shader changes move Aetna closer without local
  fixture hacks?

### 3. Tune Global Defaults First

When the fixture looks off, fix in this order:

1. theme-to-shader resolution,
2. tokens,
3. surface/elevation shader,
4. style profile behavior,
5. stock widget defaults,
6. new kit primitive,
7. local fixture workaround.

Local workarounds are failures unless they identify a real missing
primitive.

### 4. Compare By Contact Sheet

Do not ask "is this good?" in isolation. Render contact sheets:

- reference vs Aetna,
- baseline vs token change,
- dark vs light,
- accent variants,
- hover/focus/selected/disabled states.

Pairwise comparison is more reliable than absolute aesthetic judgment.

### 5. Encode Checks

Polish should become inspectable. Add lints/artifacts for:

- raw colors and raw spacing,
- contrast issues,
- overflow and missing ellipsis,
- inconsistent radius/spacing/font scale,
- interactive nodes below minimum target size,
- focusable nodes with weak focus visibility,
- selected/disabled/error states missing visual distinction,
- shadow/elevation tokens that do not affect output.

## Expected Aetna Work

### v0.9.5a: Shader Theme Resolution

Initial slice landed:

- `Theme` can globally route implicit surfaces through a custom shader.
- `draw_ops_with_theme`, `render_bundle_themed`, and
  `render_bundle_with_theme` expose the themed path for fixtures.
- `RunnerCore`, `aetna-wgpu::Runner`, and `aetna-vulkano::Runner` expose
  `set_theme`, so the same mechanism is available in live GPU runners.
- The themed path preserves rounded-rect uniforms and adds compatible
  `vec_a`..`vec_d` slots for aftermarket surface shaders.

Remaining work: keep token constants in the author API, but allow
token-backed colors to resolve through a render-time `Theme`. Raw
`Color::rgba` remains raw.

This enables dark/light/accent/high-contrast themes without passing a
theme handle through every widget.

The theme must also be able to override stock shader bindings for
surface roles. Aetna should not bake "rounded dark card with border" as
the permanent visual identity of every app.

### v0.9.5b: Surface Roles + Real Elevation

Initial slice landed:

- `Surface::Panel`
- `Surface::Raised`
- `Surface::Sunken`
- `Surface::Popover`
- `Surface::Selected`
- `Surface::Current`
- `Surface::Danger`

Implemented as `SurfaceRole` on `El`:

- Stock widgets assign roles: cards are `Panel`, buttons/menu rows are
  `Raised`, inputs/text areas are `Input`, popovers/modals are
  `Popover`.
- Semantic modifiers assign roles: `.selected()` uses `Selected`,
  `.current()` uses `Current`, `.invalid()` uses `Danger`.
- Tree dumps show `surface_role=...`.
- Shader manifests include a `surface_role` uniform id for each surface
  draw.
- `Theme` supports per-role shader and uniform overrides via
  `with_role_shader` and `with_role_uniform`.

Remaining work: make elevation and role treatment visually richer in
the stock shader/theme defaults. The role plumbing is in place; the next
step is tuning role-specific uniforms for border strength, shadow,
highlight/inset treatment, and theme-specific material shaders.

### v0.9.5c: Semantic Visual States

Initial slice landed:

- `.selected()`
- `.disabled()`
- `.loading()`
- `.invalid()`
- `.current()`

These now exist as first-class `El` modifiers for the common visual
treatments. The calibration fixtures use them instead of hand-authored
fills/strokes/opacity for selected nav rows, selected table rows,
invalid inputs, disabled controls, and loading buttons.

Remaining work:

- decide whether these should also be represented as inspectable
  semantic flags, not only resolved visual properties,
- connect disabled/current/selected into hit-testing and accessibility
  semantics more explicitly,
- move selected/invalid/current treatment through `Theme` once surface
  roles land.

These should affect rendering and artifacts. A selected row should not
be a hand-written fill color in every app.

### v0.9.5d: Typography + Overflow

Initial slice landed:

- `TextOverflow` with `Clip` and `Ellipsis`,
- `.text_overflow(...)` and `.ellipsis()` on `El`,
- draw-op-time ellipsizing so SVG and GPU paths receive the same text,
- tree dumps that show `overflow=Ellipsis`,
- `FindingKind::TextOverflow` for horizontally overflowing nowrap text,
- calibration fixtures use `.ellipsis()` for sidebar labels, table cells,
  sales rows, and command rows.
- `TextRole` with `Body`, `Caption`, `Label`, `Title`, `Heading`,
  `Display`, and `Code`,
- role modifiers (`.caption()`, `.label()`, `.title()`, etc.) that apply
  default size/weight/color and show up in tree dumps as `text_role=...`,
- stock text constructors and label-bearing widgets set text roles.
- `.max_lines(n)` for bounded wrapped text, with draw-op-time clamping and
  final-line ellipsizing.

Remaining text policy:

- line-height tokens,
- richer text overflow reports for wrapped text and max-line clipping.

This is especially important for Git UIs, tables, sidebars, and command
palettes.

### v0.9.5e: Icons

Initial slice landed:

- `icon(name)`,
- `IconName` with familiar shadcn/lucide-like string names,
- first bundled vector vocabulary for app chrome, nav, command rows, and
  status/actions,
- `Icon` draw ops and `icon=...` tree dump output,
- SVG artifact rendering for vector icons,
- GPU fallback glyph rendering until a dedicated vector icon pipeline lands,
- calibration fixtures use icons for sidebar navigation and command rows.
- `icon_button(name)` for standard icon-only controls.
- `button_with_icon(name, label)` for label+icon actions with style
  variants propagating content color to the child icon/text.

Remaining work:

- dedicated GPU vector icon pipeline,
- icon slots in table/list/menu helper builders,
- larger icon vocabulary once the first app port reveals missing names.

Without icons, Aetna will keep producing text-only rectangles.

## Gate Before Whisper-Git

Before the validation port, Aetna should satisfy:

- the calibration fixture renders without lint findings,
- stock defaults carry most of the visual quality,
- the fixture uses no raw colors outside deliberate calibration probes,
- selected/disabled/invalid states are expressible without ad hoc fills,
- table/list/menu rows have an icon/shortcut story,
- shadows/elevation are visibly represented,
- the same fixture can render at least two themes without code changes.

Then the `whisper-git` port tests generalization.
