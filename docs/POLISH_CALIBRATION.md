# Aetna Polish Calibration

This is the maintainer-facing plan for improving Aetna's default visual
quality. The goal is not to copy one web library. The goal is to encode enough
spacing, hierarchy, typography, state, and material taste that an LLM author can
reach polished native UI using stock widgets and themes.

## Premise

Aetna now has the hard mechanical pieces: app state projects into an `El` tree,
interaction state lives in `UiState`, widgets are composable, text and icons
render, themes can route surface roles to shaders, and backend runners can
paint real frames.

The next risk is encoded taste. If every serious screen requires raw colors,
one-off spacers, custom shaders, or hand-tuned fills, then the public API is
technically capable but not author-friendly.

## Calibration Goals

The default system should provide:

- clear surface hierarchy across app, panel, raised, sunken, popover,
  selected, current, and danger roles,
- restrained but visible elevation and borders,
- consistent typography roles and line-height policy,
- predictable row heights for menus, tables, lists, and forms,
- standard icon placement and sizing,
- reliable hover, pressed, focused, disabled, selected, invalid, and loading
  treatments,
- text overflow behavior that does not surprise authors,
- theme overrides that change material behavior without rewriting widgets,
- inspectable artifacts and lints for visual mistakes.

## Current Foundation

Several important pieces are already in place:

- `Theme` can route implicit surfaces and per-role surfaces through custom
  shaders.
- `SurfaceRole` is represented on `El` and appears in artifacts.
- semantic modifiers such as `.selected()`, `.disabled()`, `.loading()`,
  `.invalid()`, and `.current()` exist.
- text roles and overflow controls exist, including ellipsis and max-line
  handling.
- stock icon helpers and icon-bearing buttons exist.
- backend runners can receive themes through the shared core path.

That means the next polish work should tune global defaults first. Local
fixture tweaks should be treated as evidence of a missing default or missing
primitive.

## Calibration Targets

Use familiar, well-validated UI shapes as references:

1. Settings form
2. Command palette
3. Data table
4. Sidebar app shell
5. Dropdown/context menu
6. Dialog with validation
7. Dashboard cards
8. Dense list/detail pane

For each shape, build an Aetna fixture using stock widgets, roles, text styles,
icons, and tokens. A fixture that needs many raw colors or local spacing hacks
is a failing test for the design system.

Shadcn-style defaults remain a useful reference because they map well to LLM
training data and have compact composable vocabulary. The calibration objective
is the basin of quality: density, rhythm, contrast, state treatment, and
component proportions. Pixel-perfect copying is only a temporary measuring
tool.

## Method

### 1. Extract Rules

For each reference shape, record reusable observations:

- component heights,
- inner padding and section gaps,
- radius and border strength,
- typography roles,
- shadow/elevation treatment,
- icon size and slot placement,
- state treatments,
- overflow and truncation policy,
- shortcut and secondary-text alignment.

Store these as rules, not screenshots. Example:

```text
Menu rows are dense, usually 28-32 px tall, with left icon, label, and
right shortcut. Hover uses a subtle filled row, not a loud border.
```

### 2. Maintain Aetna Fixtures

The main calibration fixture should combine:

- app shell,
- sidebar navigation,
- toolbar buttons,
- KPI cards,
- table/list rows,
- command or menu panel,
- form controls,
- selected/error/disabled/loading states,
- empty/help text,
- token-heavy styling.

The fixture is not the final product design. It is the bench where token,
shader, widget, and lint changes become visible.

### 3. Compare By Contact Sheet

Use contact sheets instead of isolated judgment:

- reference vs Aetna,
- baseline vs token/theme change,
- dark vs light,
- accent variants,
- hover/focus/selected/disabled states.

Pairwise comparison is more reliable than asking whether a single screenshot
"looks good."

The shadcn reference harness lives in `references/shadcn-calibration/`:

```bash
cd references/shadcn-calibration
npm run capture
cd ../..
cargo run -p aetna-tools --bin make_calibration_sheet
```

`npm run capture` starts Vite on a free local port and captures Chromium
screenshots through Playwright. It pins the default reference scale to:

- viewport `1180x780` CSS px,
- `deviceScaleFactor = 1`,
- Chromium forced device scale factor `1`,
- browser zoom `1`,
- root font size `16px` (`SHADCN_REFERENCE_UI_SCALE=1`).

This keeps the web stack comparable to Aetna's logical layout scale. Vary
`SHADCN_REFERENCE_UI_SCALE` when testing app-level UI scale; avoid changing
browser zoom or desktop scale for normal polish calibration. The capture writes
`out/*.json` metadata next to each screenshot so scale drift is visible.

`make_calibration_sheet` writes the normal Aetna-only sheet and, when shadcn
captures are present, `reference_calibration_sheet.png` with shadcn references
paired against Aetna counterparts.

The shadcn reference app marks major surfaces with
`data-calibration-boundary`; the capture script fails if visible descendants
overflow those marked boxes. Reference screenshots are inputs to calibration,
so they should be held to the same mechanical standards as Aetna fixtures.

### 4. Tune In Order

When a fixture looks off, fix in this order:

1. theme-to-shader resolution,
2. role/elevation material defaults,
3. tokens,
4. style profile behavior,
5. stock widget defaults,
6. new kit primitive,
7. local fixture workaround.

Local fixture workarounds are acceptable only when they expose a concrete API
gap.

### 5. Encode Checks

Polish should be inspectable. Add or maintain lints/artifacts for:

- raw colors and raw spacing,
- contrast issues,
- text overflow and missing ellipsis,
- inconsistent radius, spacing, or font scale,
- interactive nodes below minimum target size,
- focusable nodes with weak focus visibility,
- selected/disabled/error states missing visual distinction,
- elevation tokens that do not affect output.

## Near-Term Work

The next cleanup/polish milestones should be:

1. Refresh the calibration fixture around the current widget kit and theme
   roles.
2. Restore or update the reference screenshot harness so comparisons are easy
   to regenerate.
3. Tune stock role uniforms for border strength, shadow, inset highlight, and
   selected/current/danger surfaces.
4. Add a light theme and at least one accent variant to prove the role system
   is not dark-theme-specific.
5. Tighten menu, table, list, and icon-button helpers where fixtures still need
   repeated local composition.
6. Expand lints for raw visual constants, weak focus, overflow, and target
   sizing.

## Gate Before Serious App Ports

Before using Aetna for the whisper-git port or an initial serious release,
Aetna should satisfy:

- calibration fixtures render without avoidable lint findings,
- stock defaults carry most visual quality,
- selected, disabled, current, invalid, and loading states need no ad hoc
  fills,
- table/list/menu rows have icon and shortcut conventions,
- elevation is visible but restrained,
- the same fixture can render at least dark and light themes without code
  changes,
- examples that teach public APIs live where packaged users can find them.

The app port should test generalization, not discover the basic design system.
