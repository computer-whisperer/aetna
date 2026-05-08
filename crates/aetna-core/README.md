# aetna-core

![Aetna showcase — Settings section, headless wgpu render](https://raw.githubusercontent.com/computer-whisperer/aetna/main/assets/showcase_settings.png)

Backend-agnostic UI primitives for Aetna apps.

Aetna is shaped around how an LLM authors UI: vocabulary parity with the
training distribution matters more than configurability, and the *minimum*
output should be the *correct* output. The catalog below — `card`, `sidebar`,
`tabs_list`, `dialog`, `toolbar`, etc. — mirrors the shadcn / WAI-ARIA
shapes models already know. Reach for those before composing primitives.

Start here for application code:

```rust
use aetna_core::prelude::*;

struct Counter {
    value: i32,
}

impl App for Counter {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("-").key("dec"),
                button("+").key("inc").primary(),
            ])
            .gap(tokens::SPACE_2),
        ])
        .gap(tokens::SPACE_3)
        .padding(tokens::SPACE_4)
    }

    fn on_event(&mut self, event: UiEvent) {
        if event.is_click_or_activate("inc") {
            self.value += 1;
        } else if event.is_click_or_activate("dec") {
            self.value -= 1;
        }
    }
}
```

`key` is the hit-test target *and* the event-routing identifier — same
string, no separate `.on_click(...)` registration. Hover, press, and focus
visuals are applied automatically; the author never tags a node "this one is
hovered."

Use `aetna-winit-wgpu` to open a native desktop window. Use `aetna-wgpu`
directly only when writing a custom host or embedding Aetna in an existing
render loop. If the UI mirrors external state, refresh it in
`App::before_build` — hosts call that hook immediately before each `build`.

## Reach for these first

When scaffolding a UI, prefer the named affordance over the underlying
primitives. The list is short:

| Intent | Idiomatic call | Avoid |
|---|---|---|
| Grouped content (settings card, panel of fields, any "boxed" surface) | `card([card_header([card_title("Title")]), card_content([...])])` or `titled_card("Title", [...])` | `column([...]).fill(CARD).stroke(BORDER).radius(...)` or `column(...).surface_role(SurfaceRole::Panel)` (Panel only sets stroke + shadow — not fill) |
| Sidebar / nav rail | `sidebar([sidebar_header(...), sidebar_group([...])])` plus `sidebar_menu_button_with_icon(...)` for items | custom nav rows, or `column(...).surface_role(SurfaceRole::Panel)` for the wrapper |
| Toolbar / page header row | `toolbar([toolbar_title("Documents"), spacer(), toolbar_group([...])])` | ad hoc action rows with inconsistent vertical alignment |
| Tabs / segmented control | `tabs_list(key, &current, options)` + `tab_trigger` + `tabs::apply_event` | manual `row([button, button]).fill(MUTED)` segment, or hand-rolled selected-tab state |
| Dialog | `dialog(key, [dialog_header([...]), body, dialog_footer([...])])` | a custom centered overlay card |
| Edge sheet | `sheet(key, SheetSide::Right, [sheet_header([...]), body])` | a modal manually pinned to the viewport edge |
| Dropdown / context menu | `dropdown_menu(key, trigger, [dropdown_menu_label(...), dropdown_menu_item_with_shortcut(...)])` | a popover full of hand-rolled rows |
| Standard tooltip | `.tooltip("...")` on any element | a manually-positioned popover |
| Callout / validation summary | `alert([alert_title("Heads up"), alert_description("Details")]).warning()` | a manually styled card with status-colored text |
| Status indicator (Online, Pending, Failed) | `badge("Online").success()` (also `.warning()` / `.destructive()` / `.info()` / `.muted()`) | `text("● Online").text_color(SUCCESS)` |
| User identity chip | `avatar_fallback("Alicia Koch")` or `avatar_image(img)` | a bare image/text node with custom circle styling |
| Loading placeholder | `skeleton().width(Size::Fixed(220.0))` or `skeleton_circle(32.0)` | hard-coded muted rectangles |
| Section divider | `separator()` / `vertical_separator()` | hand-rolled 1px boxes |
| Command/menu row | `command_row("git-branch", "New branch", "Ctrl+B")` or `command_item([...])` | repeating icon-slot/label/shortcut rows by hand |
| Collapsible section | `accordion_item("settings", "security", "Security", open, [...])` + `accordion::apply_event(...)` | a button plus hand-managed chevron row |
| Breadcrumb path | `breadcrumb_list([breadcrumb_link("Projects"), breadcrumb_separator(), breadcrumb_page("Aetna")])` | a raw slash-delimited text string |
| Pagination | `pagination_content([pagination_previous(), pagination_link("1", true), pagination_next()])` | unaligned text buttons with custom square sizing |
| Section heading / page title | `.heading()` / `h2(...)` (or `.title()` / `h3(...)`) | `.font_size(16.0).font_weight(Bold).text_color(...)` |
| Field label | `.label()` | `.font_weight(Semibold).text_color(...)` |
| Helper / hint text | `.caption()` or `.muted()` | `.font_size(12.0).text_color(tokens::MUTED_FOREGROUND)` |
| Inline code / mono | `.code()` or `mono(...)` | `.font_family("monospace")` (no such API) |
| Selected row in a collection | `.selected()` chainable | `surface_role(SurfaceRole::Selected)` (works, but `.selected()` reads better and sets content color) |
| Current nav / page item | `.current()` chainable | `surface_role(SurfaceRole::Current)` |
| Resizable divider between two panes | `resize_handle(Axis::Row).key(...)` + `resize_handle::apply_event_fixed(...)` | `divider()` (which is non-interactive) plus drag plumbing |
| Indent inside a list (e.g. tree depth) | `.padding(Sides { left: indent, ..Sides::zero() })` | `row([spacer().width(Fixed(indent)), ...])` |
| Toggle (preferences) | `switch(self.value).key(k)` + `switch::apply_event(...)` | a button with two text labels |
| Labelled control row (settings, prefs) | `field_row("Label", control)` | hand-rolled `row([text("Label").label(), spacer(), control])` repeated everywhere |
| Raster image (logo, screenshot, thumbnail) | `image(Image::from_rgba8(...)).image_fit(ImageFit::Contain)` | reaching for a custom shader |
| Throwaway notification | accumulate `ToastSpec::success("Saved")` and return them from `App::drain_toasts` | spinning a manual modal with a timer |

Smells that mean an affordance is being missed: `column(...).surface_role(SurfaceRole::Panel)` (use `card()` or `sidebar()` — Panel decorates, it doesn't fill); a `row([title, spacer(), action]).fill(MUTED).stroke(BORDER)` *header bar* sitting above a body inside a `card` — that's a hand-rolled `card_header`, lift the row into `card_header([...]).fill(MUTED)` (or split each "header bar over body" block into its own `card([card_header(...), card_content(...)])` and stack them in a column); a `column([row, body]).fill(CARD).stroke(BORDER)` reinventing the card silhouette (call `card([...])`); `.gap(0.0)` (already the default — delete it); `.font_size(...).font_weight(...).text_color(...)` on the same node (use a role modifier); wrapping a single child in `row([single])` to apply `.padding(...)` (every `El` has `.padding()` directly); an explicit `.fill(tokens::BACKGROUND)` on the root (the host already paints it); and `IconName::AlertCircle` as a placeholder when the project has its own SVG (use `SvgIcon::parse_current_color(include_str!("..."))` and pass it to `icon(...)`).

## Common app shells

When the catalog widget doesn't fit your data shape exactly, *wrap* it
rather than replace it. `card([...])` and `sidebar([...])` are
column-flavored containers that bundle the canonical fill + stroke +
radius + shadow + role recipe — you can put any composition inside.

A **two-pane workbench** (sidebar + main):

```ignore
row([
    sidebar([
        sidebar_header([h3("Repository")]),
        sidebar_group([
            sidebar_group_label("Branches"),
            sidebar_menu([/* sidebar_menu_button_with_icon(...) per branch */]),
        ]),
    ]),
    column([
        toolbar([toolbar_title("main"), spacer(), button("Push").primary().key("push")]),
        card([card_content([/* your view */])]).height(Size::Fill(1.0)),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0)),
])
```

A **three-column workbench** (sidebar + center + inspector). Use `card()`
for the inspector pane the same way — it gives you the same recipe the
sidebar uses, just at `Size::Fixed(WIDTH)` instead of `SIDEBAR_WIDTH`.
Reach into `card_header` for selected-item identity (title, metadata,
copy / open actions) and `card_content` for the scrollable body. The
slots pick up density-aware default padding from the metrics pass
(shadcn's anatomy at Compact / Comfortable / Spacious — see
[`metrics::card_header_metrics`](crate::metrics::card_header_metrics)),
so naive use produces the right visual without an explicit
`.padding(...)`. Override only when the design intentionally deviates:
pass `.padding(0.0)` on `card_content` when its only child is a
`scroll(...)` that should reach the card edges, or pass `Sides { ... }`
when you want a fixed recipe that won't adapt across densities.

```ignore
row([
    sidebar([/* nav */]),
    column([/* center pane */]).width(Size::Fill(1.0)),
    card([
        card_header([
            row([h3(item.title.clone()), spacer(), button("Copy ID").ghost().key("copy")])
                .align(Align::Center)
                .gap(tokens::SPACE_2),
            text(item.subtitle.clone()).muted().caption(),
        ]),
        card_content([scroll([/* sub-cards, fields */])])
            .padding(0.0)
            .height(Size::Fill(1.0)),
    ])
    .width(Size::Fixed(320.0))
    .height(Size::Fill(1.0)),
])
```

A **tabbed page**:

```ignore
column([
    tabs_list("view", &self.tab, [("working", "Working"), ("history", "History")]),
    match self.tab.as_str() {
        "history" => history_view(self),
        _ => working_view(self),
    },
])
```

If `sidebar_menu_button_with_icon` doesn't fit your row anatomy (count
badges, nested sub-groups, custom leading icons), keep the outer
`sidebar([...])` for the panel surface and compose the rows freely
inside. Same for `card_content` — anything column-shaped goes there.

## Surface roles, briefly

`SurfaceRole` is a *decoration* layer, not a fill recipe. `Panel` and
`Raised` set stroke and shadow only — they assume you (or the widget
wrapping you) supplied a fill. `Sunken` / `Selected` / `Current` /
`Input` / `Danger` *do* default a fill from the palette. Per-variant
contracts live on the `SurfaceRole` enum's rustdoc; reach for the
`.selected()` / `.current()` chainables for state, and for `card()` /
`sidebar()` / `dialog()` / `popover()` for surface containers.

## API layers

- `prelude` — the app and widget author surface; what an LLM should
  usually import.
- `widgets` — controlled widget builders and their `apply_event` /
  `apply_input` helpers (e.g. `text_input::apply_event`,
  `slider::normalized_from_event`).
- `bundle` — headless artifacts (`tree.txt` / `draw_ops.txt` / `lint.txt`
  / `.svg`) for tests and design review. The lint pass catches raw
  colors, text overflow, alignment misses, missing surface fills, and
  duplicate ids.
- `ir`, `paint`, `runtime`, text atlas, vector mesh, and MSDF modules
  are advanced backend / diagnostic surfaces. Public because sibling
  backend crates use them; ordinary app code should not start there.

The crate ships runnable examples under `examples/`: `settings`,
`scroll_list`, `virtual_list`, `inline_runs`, `modal`, `custom_shader`,
`circular_layout`, plus the `dashboard_01_calibration` reference fixture
that mirrors the shadcn dashboard-01 demo through stock widgets.
