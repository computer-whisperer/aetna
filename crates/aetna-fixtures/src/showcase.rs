//! Showcase — one app exercising every Aetna primitive.
//!
//! A single `App` impl with a sidebar nav and a content panel. Each
//! sidebar entry switches the active section; per-section state
//! persists across switches (selection, scroll offset, search). The
//! shape is a real multi-region app — the same shape host applications
//! end up with — and exercises:
//!
//! - **Buttons + click routing** in both the sidebar and the Counter
//!   section. The Counter buttons also carry `.tooltip(text)` so the
//!   library-driven tooltip layer is visible on hover.
//! - **Scroll viewport** with persistent offset in the List section.
//! - **Animated props** (`scale` / `translate` / `opacity` / `fill`) in
//!   the Palette section, including spring overshoot on selection.
//! - **Hotkey routing** scoped to the Picker section: `App::hotkeys`
//!   returns chords only when that section is active, so `j`/`k` don't
//!   leak into other panels.
//! - **Static composition** (cards, badges, danger zone) in the
//!   Settings section.
//!
//! The five legacy per-feature bins (`counter`, `scroll_list`,
//! `animated_palette`, `hotkey_picker`, `settings`) are kept as
//! minimal-fixture proof points alongside this. Showcase is the
//! integration view; they're the unit views.

use aetna_core::prelude::*;

pub const LIQUID_GLASS_WGSL: &str = include_str!("../shaders/liquid_glass.wgsl");

/// Which section the user is currently looking at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Section {
    #[default]
    Counter,
    List,
    Palette,
    Picker,
    Settings,
    Forms,
    Split,
    Glass,
    Toasts,
    Images,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Section::Counter => "Counter",
            Section::List => "List",
            Section::Palette => "Palette",
            Section::Picker => "Picker",
            Section::Settings => "Settings",
            Section::Forms => "Forms",
            Section::Split => "Split",
            Section::Glass => "Glass",
            Section::Toasts => "Toasts",
            Section::Images => "Images",
        }
    }

    fn nav_key(self) -> &'static str {
        match self {
            Section::Counter => "nav-counter",
            Section::List => "nav-list",
            Section::Palette => "nav-palette",
            Section::Picker => "nav-picker",
            Section::Settings => "nav-settings",
            Section::Forms => "nav-forms",
            Section::Split => "nav-split",
            Section::Glass => "nav-glass",
            Section::Toasts => "nav-toasts",
            Section::Images => "nav-images",
        }
    }

    const ALL: [Section; 10] = [
        Section::Counter,
        Section::List,
        Section::Palette,
        Section::Picker,
        Section::Settings,
        Section::Forms,
        Section::Split,
        Section::Glass,
        Section::Toasts,
        Section::Images,
    ];
}

#[derive(Default)]
struct CounterState {
    value: i32,
}

#[derive(Default)]
struct ListState {
    selected: Option<usize>,
}

#[derive(Default)]
struct PaletteState {
    selected: Option<usize>,
}

#[derive(Default)]
struct PickerState {
    selected: usize,
    opened: Option<usize>,
    search: String,
    search_active: bool,
}

struct FormsState {
    /// Push notifications toggle (checkbox).
    push_notifications: bool,
    /// Email-digest toggle (checkbox).
    email_digest: bool,
    /// Weekly-summary toggle (checkbox).
    weekly_summary: bool,
    /// Currently picked theme (radio_group). Stored as the string token
    /// emitted by the radio key (`system` / `light` / `dark`).
    theme: String,
    /// Auto-lock after idle (switch).
    auto_lock: bool,
    /// Share anonymous usage stats (switch).
    share_usage: bool,
}

impl Default for FormsState {
    fn default() -> Self {
        // Defaults that show all four widgets in non-degenerate states
        // when the section is rendered headlessly: at least one
        // checkbox on, at least one off, a non-default radio choice,
        // both switches in non-default positions.
        Self {
            push_notifications: true,
            email_digest: true,
            weekly_summary: false,
            theme: "light".into(),
            auto_lock: true,
            share_usage: false,
        }
    }
}

struct SplitState {
    /// Current sidebar width in logical pixels.
    sidebar_w: f32,
    /// Drag-anchor state owned by the app, fed back into
    /// `resize_handle::apply_event_fixed` on every routed event.
    sidebar_drag: ResizeDrag,
}

impl Default for SplitState {
    fn default() -> Self {
        Self {
            sidebar_w: tokens::SIDEBAR_WIDTH,
            sidebar_drag: ResizeDrag::default(),
        }
    }
}

#[derive(Default)]
struct ToastsState {
    /// Pending toasts the runtime should drain at the start of the
    /// next frame. The view's buttons push to this vec; the runtime
    /// auto-stamps each entry with an id + expiry and synthesizes the
    /// floating layer.
    pending: Vec<ToastSpec>,
    /// Click counter — used so demo toasts have unique-looking
    /// messages instead of all reading "Saved".
    fires: u32,
}

#[derive(Default)]
struct GlassState {
    /// Index into `GLASS_PRESETS` — cycles on the "Next preset"
    /// button so the same fixture exercises a few corners of the
    /// shader's parameter space.
    preset: usize,
    /// Index into `DRIFT_OFFSETS` — cycles on the "Drift" button.
    /// Drives a `.translate(...).animate(SPRING_BOUNCY)` on the
    /// glass card so it slides horizontally across the colored
    /// stripes. The interesting bit is that the snapshot is read
    /// fresh every frame, so as the glass animates between target
    /// positions the backdrop it samples actually changes mid-flight
    /// — animation and backdrop sampling cooperating live.
    drift: usize,
}

/// The showcase app. State for every section lives here so switching
/// sections is non-destructive.
#[derive(Default)]
pub struct Showcase {
    section: Section,
    counter: CounterState,
    list: ListState,
    palette: PaletteState,
    picker: PickerState,
    forms: FormsState,
    split: SplitState,
    glass: GlassState,
    toasts: ToastsState,
}

impl Showcase {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with a specific starting section. Used by headless
    /// render bins to pin the showcase on one section without needing
    /// to drive the navigation through events.
    pub fn with_section(section: Section) -> Self {
        Self {
            section,
            ..Default::default()
        }
    }
}

impl App for Showcase {
    fn build(&self) -> El {
        // Root is an overlay stack so the runtime can append
        // tooltip / toast layers as siblings of the main view
        // without those layers competing for row-axis space —
        // same convention any app uses for popovers and modals.
        overlays(row([sidebar(self.section), content(self)]), [])
    }

    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        match self.section {
            Section::Picker => picker_hotkeys(&self.picker),
            _ => Vec::new(),
        }
    }

    fn on_event(&mut self, event: UiEvent) {
        // Sidebar navigation: any click on a `nav-*` key switches sections.
        if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate)
            && let Some(k) = event.route()
            && let Some(target) = nav_section(k)
        {
            self.section = target;
            return;
        }

        match self.section {
            Section::Counter => counter_on_event(&mut self.counter, event),
            Section::List => list_on_event(&mut self.list, event),
            Section::Palette => palette_on_event(&mut self.palette, event),
            Section::Picker => picker_on_event(&mut self.picker, event),
            Section::Settings => {} // static fixture, no events
            Section::Forms => forms_on_event(&mut self.forms, event),
            Section::Split => split_on_event(&mut self.split, event),
            Section::Glass => glass_on_event(&mut self.glass, event),
            Section::Toasts => toasts_on_event(&mut self.toasts, event),
            Section::Images => {} // static fixture, no events
        }
    }

    fn drain_toasts(&mut self) -> Vec<ToastSpec> {
        std::mem::take(&mut self.toasts.pending)
    }

    fn shaders(&self) -> Vec<AppShader> {
        // The Glass section needs the liquid_glass custom shader. The
        // host harness registers it once at startup; the WGSL ships
        // bundled in the fixture crate alongside the shared App.
        vec![AppShader {
            name: "liquid_glass",
            wgsl: LIQUID_GLASS_WGSL,
            samples_backdrop: true,
        }]
    }
}

fn nav_section(key: &str) -> Option<Section> {
    Section::ALL.into_iter().find(|s| s.nav_key() == key)
}

// ---- Shell: sidebar + content ----

fn sidebar(active: Section) -> El {
    let mut entries: Vec<El> = vec![
        text("Aetna").bold().font_size(18.0),
        text("showcase").muted().small(),
    ];
    for s in Section::ALL {
        let mut b = button(s.label()).key(s.nav_key());
        b = if s == active { b.primary() } else { b.ghost() };
        entries.push(b);
    }
    column(entries)
        .gap(tokens::SPACE_SM)
        .padding(tokens::SPACE_LG)
        .width(Size::Fixed(180.0))
        .height(Size::Fill(1.0))
        .fill(tokens::BG_CARD)
        .stroke(tokens::BORDER)
}

fn content(app: &Showcase) -> El {
    // The Glass section needs to paint over a colorful backdrop, so
    // it manages its own padding/sizing via a `stack(...)` — wrapping
    // it in the standard padded column would inset the backdrop and
    // make the glass effect harder to see. Every other section uses
    // the standard padded layout.
    let body = match app.section {
        Section::Counter => counter_view(&app.counter),
        Section::List => list_view(&app.list),
        Section::Palette => palette_view(&app.palette),
        Section::Picker => picker_view(&app.picker),
        Section::Settings => settings_view(),
        Section::Forms => forms_view(&app.forms),
        Section::Split => split_view(&app.split),
        Section::Glass => {
            return glass_view(&app.glass)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0));
        }
        Section::Toasts => toasts_view(&app.toasts),
        Section::Images => images_view(),
    };
    column([body])
        .padding(tokens::SPACE_XL)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}

// ---- Counter section ----

fn counter_view(state: &CounterState) -> El {
    column([
        h1(format!("{}", state.value)),
        // Hover any of the three for ~500ms to see the runtime-driven
        // tooltip layer appear. The `.tooltip(text)` modifier is the
        // entire app-side surface; the rest is library-owned.
        row([
            button("−")
                .key("counter-dec")
                .secondary()
                .tooltip("Decrement"),
            button("Reset")
                .key("counter-reset")
                .ghost()
                .tooltip("Set count to 0"),
            button("+")
                .key("counter-inc")
                .primary()
                .tooltip("Increment"),
        ])
        .gap(tokens::SPACE_MD),
        text(if state.value == 0 {
            "Click + or −, or hover for a tooltip.".to_string()
        } else {
            format!("You have clicked +/− a net {} times.", state.value)
        })
        .center_text()
        .muted(),
    ])
    .gap(tokens::SPACE_LG)
    .align(Align::Center)
    // Claim the full content area and center the small demo
    // vertically, so the counter sits in the middle of the panel
    // instead of pinned to the top.
    .height(Size::Fill(1.0))
    .justify(Justify::Center)
}

fn counter_on_event(state: &mut CounterState, e: UiEvent) {
    match (e.kind, e.route()) {
        (UiEventKind::Click | UiEventKind::Activate, Some("counter-inc")) => state.value += 1,
        (UiEventKind::Click | UiEventKind::Activate, Some("counter-dec")) => state.value -= 1,
        (UiEventKind::Click | UiEventKind::Activate, Some("counter-reset")) => state.value = 0,
        _ => {}
    }
}

// ---- List section ----

fn list_view(state: &ListState) -> El {
    let rows: Vec<El> = (0..30)
        .map(|i| {
            let key = format!("list-row-{i}");
            let mut r = row([
                badge(format!("#{i}")).info(),
                text(format!("Item {i}")).bold(),
                spacer(),
                text(if Some(i) == state.selected {
                    "selected"
                } else {
                    ""
                })
                .muted(),
            ])
            .gap(tokens::SPACE_SM)
            .align(Align::Center)
            .height(Size::Fixed(44.0))
            .padding(Sides::xy(tokens::SPACE_MD, tokens::SPACE_SM))
            .key(key)
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_SM);
            if Some(i) == state.selected {
                r = r.fill(tokens::BG_CARD);
            }
            r
        })
        .collect();

    // Fill-height column so the scroll's `Fill(1.0)` resolves against
    // the leftover space (viewport − header − caption − gaps), not the
    // intrinsic sum of all rows. Without this, column defaults to Hug
    // → scroll falls back to intrinsic → ~1700px overflow.
    column([
        h2("Scrollable list"),
        text("Wheel inside the panel. Click a row to select.").muted(),
        scroll(rows)
            .key("list-items")
            .height(Size::Fill(1.0))
            .padding(tokens::SPACE_SM),
    ])
    .gap(tokens::SPACE_LG)
    .height(Size::Fill(1.0))
}

fn list_on_event(state: &mut ListState, e: UiEvent) {
    if let (UiEventKind::Click | UiEventKind::Activate, Some(k)) = (e.kind, e.route())
        && let Some(rest) = k.strip_prefix("list-row-")
        && let Ok(i) = rest.parse::<usize>()
    {
        state.selected = Some(i);
    }
}

// ---- Palette section ----

#[derive(Clone, Copy)]
struct Swatch {
    name: &'static str,
    fill: Color,
}

const SWATCHES: &[Swatch] = &[
    Swatch {
        name: "warm",
        fill: Color::rgb(255, 138, 76),
    },
    Swatch {
        name: "cool",
        fill: Color::rgb(76, 158, 255),
    },
    Swatch {
        name: "lime",
        fill: Color::rgb(140, 220, 110),
    },
];

fn palette_view(state: &PaletteState) -> El {
    let swatches: Vec<El> = SWATCHES
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_selected = Some(i) == state.selected;
            let fill = if is_selected {
                s.fill.mix(Color::rgb(255, 255, 255), 0.35)
            } else {
                s.fill
            };
            let scale = if is_selected { 1.15 } else { 1.0 };
            let lift = if is_selected { -8.0 } else { 0.0 };
            card(
                s.name,
                [text(if is_selected { "picked" } else { "tap" })
                    .center_text()
                    .muted()],
            )
            .key(format!("palette-swatch-{i}"))
            .fill(fill)
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(120.0))
            .scale(scale)
            .translate(0.0, lift)
            .animate(Timing::SPRING_BOUNCY)
        })
        .collect();

    let status = if let Some(i) = state.selected {
        format!("{} is picked.", SWATCHES[i].name)
    } else {
        "tap a card to pick.".to_string()
    };

    column([
        h2("Animated palette"),
        text("Cards spring up on tap; status fades on change.").muted(),
        row(swatches).gap(tokens::SPACE_MD),
        text(status)
            .key("palette-status")
            .center_text()
            .opacity(1.0)
            .animate(Timing::SPRING_GENTLE),
    ])
    .gap(tokens::SPACE_LG)
    .align(Align::Center)
    .height(Size::Fill(1.0))
    .justify(Justify::Center)
}

fn palette_on_event(state: &mut PaletteState, e: UiEvent) {
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate)
        && let Some(k) = e.route()
        && let Some(rest) = k.strip_prefix("palette-swatch-")
        && let Ok(i) = rest.parse::<usize>()
    {
        state.selected = if Some(i) == state.selected {
            None
        } else {
            Some(i)
        };
    }
}

// ---- Picker section ----

const PICKER_ITEMS: &[&str] = &[
    "build the renderer",
    "wire focus traversal",
    "ship scroll primitive",
    "design hotkey system",
    "polish artifact bundle",
    "write the next manifesto",
    "rest, eat, sleep, repeat",
];

fn picker_view(state: &PickerState) -> El {
    let header = row([
        badge(if state.search_active {
            "/ active"
        } else {
            "/ search"
        })
        .info(),
        text(if state.search.is_empty() {
            "(press / to type, Ctrl+L to clear)".to_string()
        } else {
            format!("\"{}\"", state.search)
        })
        .muted(),
        spacer(),
        text(format!("{}/{}", state.selected + 1, PICKER_ITEMS.len())).muted(),
    ])
    .gap(tokens::SPACE_SM)
    .align(Align::Center);

    let rows: Vec<El> = PICKER_ITEMS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let mut r = row([
                badge(format!("{}", i + 1)).muted(),
                text(*label),
                spacer(),
                text(if Some(i) == state.opened {
                    "opened"
                } else {
                    ""
                })
                .muted(),
            ])
            .gap(tokens::SPACE_SM)
            .align(Align::Center)
            .padding(Sides::xy(tokens::SPACE_MD, tokens::SPACE_SM))
            .height(Size::Fixed(40.0))
            .key(format!("picker-row-{i}"))
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_SM);
            if i == state.selected {
                r = r.fill(tokens::BG_CARD);
            }
            r
        })
        .collect();

    // Fill-height column for the same reason as `list_view`: the inner
    // scroll's `Fill(1.0)` only resolves correctly when the parent
    // column has bounded height.
    column([
        h2("Hotkey picker"),
        text("Keyboard-only navigation — chords scope to this section.").muted(),
        header,
        scroll(rows).key("picker-items").height(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_LG)
    .height(Size::Fill(1.0))
}

fn picker_hotkeys(state: &PickerState) -> Vec<(KeyChord, String)> {
    let mut out = vec![
        (KeyChord::vim('j'), "picker-move-down".into()),
        (KeyChord::vim('k'), "picker-move-up".into()),
        (KeyChord::vim('g'), "picker-go-top".into()),
        (KeyChord::ctrl('l'), "picker-clear-search".into()),
        (KeyChord::named(UiKey::Enter), "picker-open".into()),
        (
            KeyChord::named(UiKey::Character("/".into())),
            "picker-toggle-search".into(),
        ),
        (
            KeyChord::named(UiKey::Character("G".into())).with_modifiers(KeyModifiers {
                shift: true,
                ..Default::default()
            }),
            "picker-go-bottom".into(),
        ),
    ];
    if state.search_active {
        for c in b'a'..=b'z' {
            out.push((
                KeyChord::vim(c as char),
                format!("picker-search-{}", c as char),
            ));
        }
    }
    out
}

fn picker_on_event(state: &mut PickerState, e: UiEvent) {
    match (e.kind, e.route()) {
        (UiEventKind::Hotkey, Some("picker-move-down"))
            if state.selected + 1 < PICKER_ITEMS.len() =>
        {
            state.selected += 1;
        }
        (UiEventKind::Hotkey, Some("picker-move-up")) => {
            state.selected = state.selected.saturating_sub(1);
        }
        (UiEventKind::Hotkey, Some("picker-go-top")) => state.selected = 0,
        (UiEventKind::Hotkey, Some("picker-go-bottom")) => state.selected = PICKER_ITEMS.len() - 1,
        (UiEventKind::Hotkey, Some("picker-open")) => state.opened = Some(state.selected),
        (UiEventKind::Hotkey, Some("picker-toggle-search")) => {
            state.search_active = !state.search_active
        }
        (UiEventKind::Hotkey, Some("picker-clear-search")) => {
            state.search.clear();
            state.search_active = false;
        }
        (UiEventKind::Hotkey, Some(name)) if name.starts_with("picker-search-") => {
            if let Some(c) = name
                .strip_prefix("picker-search-")
                .and_then(|s| s.chars().next())
            {
                state.search.push(c);
            }
        }
        (UiEventKind::Click | UiEventKind::Activate, Some(k)) => {
            if let Some(rest) = k.strip_prefix("picker-row-")
                && let Ok(i) = rest.parse::<usize>()
            {
                state.selected = i;
                state.opened = Some(i);
            }
        }
        _ => {}
    }
}

// ---- Settings section (static) ----

fn settings_view() -> El {
    column([
        h1("Settings"),
        card(
            "Account",
            [
                row([text("Email"), text("user@example.com").muted()])
                    .align(Align::Center)
                    .justify(Justify::SpaceBetween),
                row([
                    text("Two-factor authentication"),
                    badge("Enabled").success(),
                ])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
                row([
                    text("Recovery codes"),
                    button("Generate").secondary().key("settings-generate"),
                ])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
            ],
        ),
        card(
            "Appearance",
            [
                row([
                    text("Theme"),
                    button("Dark").secondary().key("settings-theme"),
                ])
                .align(Align::Center)
                .justify(Justify::SpaceBetween),
                row([text("Compact mode"), badge("Off").muted()])
                    .align(Align::Center)
                    .justify(Justify::SpaceBetween),
                row([text("Font size"), text("14")])
                    .align(Align::Center)
                    .justify(Justify::SpaceBetween),
            ],
        ),
        card(
            "Danger zone",
            [row([
                column([
                    text("Delete account").bold(),
                    text("Permanently remove your account and all data.")
                        .muted()
                        .small(),
                ])
                .gap(tokens::SPACE_XS)
                .align(Align::Start),
                button("Delete").destructive().key("settings-delete"),
            ])
            .align(Align::Center)
            .justify(Justify::SpaceBetween)],
        ),
        row([
            button("Cancel").ghost().key("settings-cancel"),
            button("Save").primary().key("settings-save"),
        ])
        .gap(tokens::SPACE_SM)
        .justify(Justify::End),
    ])
    .gap(tokens::SPACE_LG)
}

// ---- Forms section ----

const FORMS_THEME_OPTIONS: &[(&str, &str)] = &[
    ("system", "Match system"),
    ("light", "Light"),
    ("dark", "Dark"),
];

/// Profile-completion percentage derived from the form state. Drives
/// the read-only `progress` bar at the top of the section so the bar's
/// motion is visibly tied to user input on the controls below.
fn forms_completion(state: &FormsState) -> f32 {
    // Six fields, each on/off (or in radio's case "default vs not").
    // Counting "not the default" as a step toward completion gives the
    // progress bar movement on every interaction without any field
    // dominating.
    let mut steps = 0u32;
    if state.push_notifications {
        steps += 1;
    }
    if state.email_digest {
        steps += 1;
    }
    if state.weekly_summary {
        steps += 1;
    }
    if state.theme != "system" {
        steps += 1;
    }
    if state.auto_lock {
        steps += 1;
    }
    if state.share_usage {
        steps += 1;
    }
    steps as f32 / 6.0
}

fn forms_view(state: &FormsState) -> El {
    let completion = forms_completion(state);
    // Color the progress fill based on how full it is — primary by
    // default, success once everything is on. Apps that want a
    // different palette can pass any token (DESTRUCTIVE near full,
    // WARNING mid-range, etc.).
    let progress_color = if completion >= 0.999 {
        tokens::SUCCESS
    } else {
        tokens::PRIMARY
    };

    let header = column([
        h1("Form preferences"),
        row([
            text("Profile completeness").muted().label(),
            spacer(),
            text(format!("{}%", (completion * 100.0).round() as u32)).muted(),
        ])
        .align(Align::Center),
        progress(completion, progress_color),
    ])
    .gap(tokens::SPACE_XS);

    let notifications = card(
        "Notifications",
        [
            checkbox_row(
                "forms-push-notifications",
                state.push_notifications,
                "Push notifications",
                "Send a system notification on new activity.",
            ),
            checkbox_row(
                "forms-email-digest",
                state.email_digest,
                "Email digest",
                "A bundled daily summary, instead of per-event email.",
            ),
            checkbox_row(
                "forms-weekly-summary",
                state.weekly_summary,
                "Weekly summary",
                "Sunday-evening recap of the week's activity.",
            ),
        ],
    );

    let theme = card(
        "Theme",
        [radio_group(
            "forms-theme",
            &state.theme,
            FORMS_THEME_OPTIONS.iter().copied(),
        )],
    );

    let privacy = card(
        "Privacy",
        [
            switch_row(
                "forms-auto-lock",
                state.auto_lock,
                "Auto-lock after 5 minutes",
                "Require the password again when idle.",
            ),
            switch_row(
                "forms-share-usage",
                state.share_usage,
                "Share anonymous usage statistics",
                "Help us understand how Aetna is used in the wild.",
            ),
        ],
    );

    // The cards stack taller than the showcase viewport (~640 px),
    // so route the body through a scroll viewport like the List
    // section does. Header stays outside the scroll so the progress
    // bar remains visible while the cards scroll. Fill-height column
    // for the same reason as `list_view` — the inner scroll's
    // `Fill(1.0)` only resolves correctly when the parent column has
    // bounded height.
    column([
        header,
        scroll([notifications, theme, privacy])
            .key("forms-scroll")
            .height(Size::Fill(1.0))
            .padding(Sides::xy(0.0, tokens::SPACE_SM))
            .gap(tokens::SPACE_LG),
    ])
    .gap(tokens::SPACE_LG)
    .height(Size::Fill(1.0))
}

/// Helper: a row pairing a [`checkbox`] with a label + helper-text
/// description. The whole row layout comes from `row` + spacer; the
/// widget itself is a single line.
fn checkbox_row(key: &str, value: bool, label: &str, description: &str) -> El {
    row([
        checkbox(value).key(key.to_string()),
        column([text(label).label(), text(description).muted().small()])
            .gap(tokens::SPACE_XS)
            .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_MD)
    .align(Align::Center)
}

/// Helper: the same shape as [`checkbox_row`] but for a `switch`
/// trailing the description (the right-aligned position is the shadcn
/// convention for boolean toggles inside a settings list).
fn switch_row(key: &str, value: bool, label: &str, description: &str) -> El {
    row([
        column([text(label).label(), text(description).muted().small()])
            .gap(tokens::SPACE_XS)
            .width(Size::Fill(1.0)),
        switch(value).key(key.to_string()),
    ])
    .gap(tokens::SPACE_MD)
    .align(Align::Center)
}

fn forms_on_event(state: &mut FormsState, e: UiEvent) {
    // Radio group folds first; its routed-key shape (`forms-theme:radio:*`)
    // wouldn't match the bool checkboxes/switches anyway, but folding it
    // up front keeps the bool dispatch flat.
    if radio::apply_event(&mut state.theme, &e, "forms-theme", |s| Some(s.to_string())) {
        return;
    }
    // Checkboxes and switches share `apply_event(&mut bool, ...)`, so
    // a single dispatch table covers both.
    let _ = checkbox::apply_event(
        &mut state.push_notifications,
        &e,
        "forms-push-notifications",
    ) || checkbox::apply_event(&mut state.email_digest, &e, "forms-email-digest")
        || checkbox::apply_event(&mut state.weekly_summary, &e, "forms-weekly-summary")
        || switch::apply_event(&mut state.auto_lock, &e, "forms-auto-lock")
        || switch::apply_event(&mut state.share_usage, &e, "forms-share-usage");
}

// ---- Split section ----
//
// Resizable sidebar — the dominant use of `resize_handle`. The app
// owns `sidebar_w` (in logical pixels) and the drag-anchor state;
// `resize_handle::apply_event_fixed` folds PointerDown / Drag /
// PointerUp / Arrow keys back into the value, clamped to the
// `SIDEBAR_WIDTH_MIN..=SIDEBAR_WIDTH_MAX` range from the tokens.

const SPLIT_HANDLE_KEY: &str = "split-resize";

fn split_view(state: &SplitState) -> El {
    let sidebar = column([
        text("Files").bold(),
        text("README.md").muted(),
        text("Cargo.toml").muted(),
        text("src/").muted(),
        text("examples/").muted(),
        text("tests/").muted(),
    ])
    .gap(tokens::SPACE_SM)
    .padding(tokens::SPACE_MD)
    .width(Size::Fixed(state.sidebar_w))
    .height(Size::Fill(1.0))
    .fill(tokens::BG_CARD)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_SM);

    let content = column([
        text("README.md").heading(),
        text(format!(
            "Drag the divider on the left to resize the sidebar. \
             Width clamps between {min}px and {max}px. The handle is \
             focusable — Tab to it, then use ←/→ to nudge by {step}px \
             or PageUp/PageDown for {page}px steps.",
            min = tokens::SIDEBAR_WIDTH_MIN as i32,
            max = tokens::SIDEBAR_WIDTH_MAX as i32,
            step = resize_handle::KEYBOARD_STEP_PX as i32,
            page = resize_handle::KEYBOARD_PAGE_STEP_PX as i32,
        ))
        .muted()
        .wrap_text(),
        row([
            text("Sidebar width:").muted(),
            text(format!("{:.0} px", state.sidebar_w)).bold(),
        ])
        .gap(tokens::SPACE_SM),
    ])
    .gap(tokens::SPACE_MD)
    .padding(tokens::SPACE_MD)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0));

    column([
        h2("Resizable sidebar"),
        text("Drag the divider, or focus it and use Arrow keys.").muted(),
        row([
            sidebar,
            resize_handle(Axis::Row).key(SPLIT_HANDLE_KEY),
            content,
        ])
        .height(Size::Fill(1.0))
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM),
    ])
    .gap(tokens::SPACE_LG)
    .height(Size::Fill(1.0))
}

fn split_on_event(state: &mut SplitState, event: UiEvent) {
    resize_handle::apply_event_fixed(
        &mut state.sidebar_w,
        &mut state.sidebar_drag,
        &event,
        SPLIT_HANDLE_KEY,
        Axis::Row,
        tokens::SIDEBAR_WIDTH_MIN,
        tokens::SIDEBAR_WIDTH_MAX,
    );
}

// ---- Glass section ----

/// One configuration of the `liquid_glass.wgsl` parameter space — the
/// "Next preset" button cycles through these so a single fixture
/// covers a meaningful slice of the shader without needing live
/// sliders.
#[derive(Clone, Copy)]
struct GlassPreset {
    label: &'static str,
    blurb: &'static str,
    blur_px: f32,
    refraction: f32,
    specular: f32,
    tint: Color,
}

const GLASS_PRESETS: &[GlassPreset] = &[
    GlassPreset {
        label: "Soft",
        blurb: "Gentle blur, faint warm tint, soft bevel.",
        blur_px: 4.0,
        refraction: 0.45,
        specular: 0.8,
        tint: Color {
            r: 240,
            g: 240,
            b: 250,
            a: 110,
            token: None,
        },
    },
    GlassPreset {
        label: "Heavy",
        blurb: "Wide blur, stronger refraction at the rim.",
        blur_px: 10.0,
        refraction: 0.85,
        specular: 1.1,
        tint: Color {
            r: 230,
            g: 235,
            b: 250,
            a: 140,
            token: None,
        },
    },
    GlassPreset {
        label: "Cool",
        blurb: "Cool blue tint, crisp specular bevel.",
        blur_px: 6.0,
        refraction: 0.55,
        specular: 1.4,
        tint: Color {
            r: 180,
            g: 215,
            b: 255,
            a: 170,
            token: None,
        },
    },
    GlassPreset {
        label: "Crisp",
        blurb: "Minimal blur, pure refraction lensing.",
        blur_px: 1.5,
        refraction: 0.95,
        specular: 1.6,
        tint: Color {
            r: 250,
            g: 250,
            b: 255,
            a: 60,
            token: None,
        },
    },
];

/// Vivid wallpaper that sits behind the glass card. Four tall stripes
/// in saturated primaries — chosen so the blur kernel pulls visibly
/// distinct colors from neighbouring stripes near the glass rim,
/// proving the snapshot is being read locally rather than re-emitted
/// as a uniform tint.
fn glass_backdrop() -> El {
    fn stripe(c: Color) -> El {
        // `flex: 1` on the main axis (width) — height comes from the
        // row's `align(Stretch)`, the same way CSS items stretch on
        // the cross axis under `align-items: stretch`.
        column(Vec::<El>::new()).fill(c).width(Size::Fill(1.0))
    }
    row([
        stripe(Color::rgb(220, 60, 60)),
        stripe(Color::rgb(60, 200, 100)),
        stripe(Color::rgb(70, 110, 220)),
        stripe(Color::rgb(240, 200, 60)),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

/// Horizontal offsets the "Drift" button cycles through. Index 0 is
/// the resting position (centered); subsequent stops shift the glass
/// onto neighbouring stripes. The 420-wide card and 720-wide content
/// area leave ±150 of safe range from center; ±120 keeps a visible
/// margin so the rim is never flush with the panel edge.
const DRIFT_OFFSETS: &[f32] = &[0.0, -120.0, 120.0];

fn glass_card(preset: &GlassPreset, drift_x: f32) -> El {
    // Custom-shaded container. The shader binding maps preset values
    // into the generic vec_a/vec_b/vec_c slots that
    // `liquid_glass.wgsl` reads. Inner text uses
    // `text_color(TEXT_ON_SOLID_DARK)` rather than the default
    // foreground/muted tokens because the latter assume a stable
    // background; over a refractive glass surface they wash out.
    //
    // `.translate(drift_x, 0).animate(SPRING_BOUNCY)` lets the card
    // physically slide between drift stops with a satisfying spring
    // overshoot. The library tracks the per-(node, prop) target and
    // interpolates each frame, so the glass visibly accelerates,
    // overshoots, settles — all while sampling whatever stripes
    // happen to be under it that frame.
    column([
        text("Liquid glass")
            .bold()
            .font_size(22.0)
            .text_color(tokens::TEXT_ON_SOLID_DARK),
        text(preset.blurb).text_color(tokens::TEXT_ON_SOLID_DARK),
        spacer(),
        row([
            text(format!("preset: {}", preset.label))
                .bold()
                .text_color(tokens::TEXT_ON_SOLID_DARK),
            spacer(),
            button("Next preset").key("glass-next").secondary(),
            button("Drift →").key("glass-drift").primary(),
        ])
        .gap(tokens::SPACE_SM),
    ])
    .gap(tokens::SPACE_SM)
    .padding(tokens::SPACE_LG)
    .shader(
        ShaderBinding::custom("liquid_glass")
            .color("vec_a", preset.tint)
            .vec4(
                "vec_b",
                [preset.blur_px, preset.refraction, preset.specular, 0.0],
            )
            .vec4("vec_c", [28.0, 0.0, 0.0, 0.0]),
    )
    .width(Size::Fixed(420.0))
    .height(Size::Fixed(220.0))
    .translate(drift_x, 0.0)
    .animate(Timing::SPRING_BOUNCY)
}

fn glass_view(state: &GlassState) -> El {
    let preset = &GLASS_PRESETS[state.preset % GLASS_PRESETS.len()];
    let drift_x = DRIFT_OFFSETS[state.drift % DRIFT_OFFSETS.len()];
    stack([glass_backdrop(), glass_card(preset, drift_x)])
        .align(Align::Center)
        .justify(Justify::Center)
}

fn glass_on_event(state: &mut GlassState, e: UiEvent) {
    if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    match e.route() {
        Some("glass-next") => state.preset = (state.preset + 1) % GLASS_PRESETS.len(),
        Some("glass-drift") => state.drift = (state.drift + 1) % DRIFT_OFFSETS.len(),
        _ => {}
    }
}

// ---- Toasts section ----
//
// Demonstrates the runtime-managed toast stack. Each button click
// pushes a `ToastSpec` of the matching level onto `state.pending`;
// `App::drain_toasts` hands those over to the runtime, which stamps
// each with an id + TTL and synthesizes the `toast_stack` floating
// layer over the entire viewport. Click the X on any card to dismiss.

fn toasts_view(state: &ToastsState) -> El {
    column([
        h2("Toasts"),
        paragraph(
            "Each button queues a toast onto state.pending; the runtime \
             drains them via App::drain_toasts at the start of the next \
             frame, stacks the cards at the bottom-right, and dismisses \
             them on click or auto-expiry (4s default).",
        )
        .muted(),
        row([
            button("Success").key("toast-success").primary(),
            button("Warning").key("toast-warning"),
            button("Error").key("toast-error").destructive(),
            button("Info").key("toast-info").ghost(),
        ])
        .gap(tokens::SPACE_SM),
        text(format!(
            "fired {} toast{} this session",
            state.fires,
            if state.fires == 1 { "" } else { "s" }
        ))
        .small()
        .muted(),
    ])
    .gap(tokens::SPACE_LG)
    .padding(tokens::SPACE_XL)
    .align(Align::Start)
}

fn toasts_on_event(state: &mut ToastsState, e: UiEvent) {
    if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    let spec = match e.route() {
        Some("toast-success") => ToastSpec::success("Settings saved"),
        Some("toast-warning") => ToastSpec::warning("Battery low — connect charger"),
        Some("toast-error") => ToastSpec::error("Failed to reach update server"),
        Some("toast-info") => ToastSpec::info("New version available"),
        _ => return,
    };
    state.pending.push(spec);
    state.fires += 1;
}

// ---- Images section ----
//
// Apps construct `Image`s once (typically via `LazyLock` over a
// decoded byte slice; here we generate test patterns in code so the
// fixture is self-contained — no PNG dep). Equal pixel buffers share
// a backend texture-cache slot, so the four `image(SOLID.clone())`
// calls in the avatar row map to one GPU upload.

use std::sync::LazyLock;

static GRID_RG: LazyLock<Image> = LazyLock::new(|| make_gradient(64, 64, [255, 64, 64], [64, 96, 255]));
static GRID_GB: LazyLock<Image> = LazyLock::new(|| make_gradient(64, 64, [64, 200, 100], [40, 40, 60]));
static GRID_CHECKER: LazyLock<Image> = LazyLock::new(|| make_checker(64, 64, 8));
static GRID_RING: LazyLock<Image> = LazyLock::new(|| make_ring(64, 64));
static AVATAR_SOLID: LazyLock<Image> =
    LazyLock::new(|| Image::from_rgba8(32, 32, vec![255; 32 * 32 * 4]));

fn make_gradient(w: u32, h: u32, top_left: [u8; 3], bottom_right: [u8; 3]) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h - 2) as f32;
            let r = (top_left[0] as f32 * (1.0 - t) + bottom_right[0] as f32 * t) as u8;
            let g = (top_left[1] as f32 * (1.0 - t) + bottom_right[1] as f32 * t) as u8;
            let b = (top_left[2] as f32 * (1.0 - t) + bottom_right[2] as f32 * t) as u8;
            let i = ((y * w + x) * 4) as usize;
            pixels[i] = r;
            pixels[i + 1] = g;
            pixels[i + 2] = b;
            pixels[i + 3] = 255;
        }
    }
    Image::from_rgba8(w, h, pixels)
}

fn make_checker(w: u32, h: u32, cell: u32) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let on = ((x / cell) + (y / cell)).is_multiple_of(2);
            let v = if on { 240 } else { 32 };
            let i = ((y * w + x) * 4) as usize;
            pixels[i] = v;
            pixels[i + 1] = v;
            pixels[i + 2] = v;
            pixels[i + 3] = 255;
        }
    }
    Image::from_rgba8(w, h, pixels)
}

fn make_ring(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    let r_outer = w.min(h) as f32 * 0.45;
    let r_inner = r_outer - 6.0;
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let on = d <= r_outer && d >= r_inner;
            let i = ((y * w + x) * 4) as usize;
            if on {
                pixels[i] = 255;
                pixels[i + 1] = 255;
                pixels[i + 2] = 255;
                pixels[i + 3] = 255;
            } else {
                pixels[i + 3] = 0;
            }
        }
    }
    Image::from_rgba8(w, h, pixels)
}

fn images_view() -> El {
    column([
        h2("Images"),
        paragraph(
            "Apps construct `Image`s once and embed them via `image(...)`. \
             Identity is content-hashed, so equal pixel buffers share a \
             GPU texture; cloning the handle is a cheap Arc bump.",
        )
        .muted(),
        // 4-cell grid showing each of the test patterns at natural size.
        row([
            tile(&GRID_RG, "gradient"),
            tile(&GRID_GB, "moss"),
            tile(&GRID_CHECKER, "checker"),
            tile(&GRID_RING, "ring"),
        ])
        .gap(tokens::SPACE_MD),
        // Avatar row: four references to the same Image with different
        // tints — exercises the tint multiply + content-hash sharing.
        h3("Tints share one texture (content-hashed)"),
        row([
            avatar(Color::rgb(96, 165, 250)),
            avatar(Color::rgb(244, 114, 182)),
            avatar(Color::rgb(248, 113, 113)),
            avatar(Color::rgb(132, 204, 22)),
        ])
        .gap(tokens::SPACE_SM),
        // Fit modes side-by-side: same image, four projections into
        // identically-sized boxes so the differences are visible.
        h3("ImageFit modes"),
        row([
            fit_demo("Contain", ImageFit::Contain),
            fit_demo("Cover", ImageFit::Cover),
            fit_demo("Fill", ImageFit::Fill),
            fit_demo("None", ImageFit::None),
        ])
        .gap(tokens::SPACE_MD),
    ])
    .gap(tokens::SPACE_LG)
    .padding(tokens::SPACE_XL)
    .align(Align::Start)
}

fn tile(img: &LazyLock<Image>, label: &str) -> El {
    column([
        image((*img).clone())
            .width(Size::Fixed(96.0))
            .height(Size::Fixed(96.0))
            .image_fit(ImageFit::Contain)
            .radius(tokens::RADIUS_MD),
        text(label.to_string()).small().muted(),
    ])
    .gap(tokens::SPACE_XS)
    .align(Align::Center)
}

fn avatar(tint: Color) -> El {
    image(AVATAR_SOLID.clone())
        .width(Size::Fixed(48.0))
        .height(Size::Fixed(48.0))
        .image_fit(ImageFit::Fill)
        .image_tint(tint)
        .radius(24.0)
}

fn fit_demo(label: &str, fit: ImageFit) -> El {
    column([
        // 96x96 box; the gradient image is 64x64 so each fit produces
        // visibly different geometry.
        image(GRID_RG.clone())
            .width(Size::Fixed(96.0))
            .height(Size::Fixed(48.0))
            .image_fit(fit)
            .radius(tokens::RADIUS_SM)
            .stroke(tokens::BORDER),
        text(label.to_string()).small().muted(),
    ])
    .gap(tokens::SPACE_XS)
    .align(Align::Center)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn glass_next_cycles_through_presets() {
        let mut s = GlassState::default();
        assert_eq!(s.preset, 0);
        glass_on_event(&mut s, click("glass-next"));
        assert_eq!(s.preset, 1);
        // Cycle the full length and confirm we wrap back to 0.
        for _ in 0..GLASS_PRESETS.len() - 1 {
            glass_on_event(&mut s, click("glass-next"));
        }
        assert_eq!(s.preset, 0);
    }

    #[test]
    fn glass_drift_cycles_horizontal_offsets() {
        let mut s = GlassState::default();
        // Default rest position must be 0 — otherwise the glass card
        // is offset before the user's first click.
        assert_eq!(DRIFT_OFFSETS[s.drift], 0.0);
        glass_on_event(&mut s, click("glass-drift"));
        assert_ne!(DRIFT_OFFSETS[s.drift], 0.0);
        for _ in 0..DRIFT_OFFSETS.len() - 1 {
            glass_on_event(&mut s, click("glass-drift"));
        }
        // Wrapped back to rest.
        assert_eq!(DRIFT_OFFSETS[s.drift], 0.0);
    }

    #[test]
    fn drift_offsets_stay_inside_content_bounds() {
        // Glass card is 420 wide; showcase content area is ~720 wide
        // (900 viewport − 180 sidebar). Half the spare room is 150 —
        // any drift offset beyond that pushes the card past the
        // panel edge or into the sidebar.
        for &offset in DRIFT_OFFSETS {
            assert!(
                offset.abs() <= 150.0,
                "drift offset {offset} exceeds safe range"
            );
        }
    }

    #[test]
    fn forms_checkbox_toggles_via_apply_event() {
        let mut s = FormsState::default();
        let was = s.weekly_summary;
        forms_on_event(&mut s, click("forms-weekly-summary"));
        assert_eq!(s.weekly_summary, !was);
    }

    #[test]
    fn forms_switch_toggles_via_apply_event() {
        let mut s = FormsState::default();
        let was = s.auto_lock;
        forms_on_event(&mut s, click("forms-auto-lock"));
        assert_eq!(s.auto_lock, !was);
    }

    #[test]
    fn forms_radio_swaps_theme() {
        let mut s = FormsState::default();
        assert_eq!(s.theme, "light");
        forms_on_event(&mut s, click("forms-theme:radio:dark"));
        assert_eq!(s.theme, "dark");
        forms_on_event(&mut s, click("forms-theme:radio:system"));
        assert_eq!(s.theme, "system");
    }

    #[test]
    fn forms_completion_reaches_full_when_everything_is_on() {
        let mut s = FormsState {
            push_notifications: true,
            email_digest: true,
            weekly_summary: true,
            theme: "dark".into(),
            auto_lock: true,
            share_usage: true,
        };
        assert!((forms_completion(&s) - 1.0).abs() < 1e-3);
        // Switching theme back to the default reduces completion.
        s.theme = "system".into();
        assert!(forms_completion(&s) < 1.0);
    }

    #[test]
    fn forms_section_routes_unrelated_events_without_panic() {
        // Events that don't match any of the form keys must not cause
        // the dispatch chain to misroute (e.g. apply_event short-
        // circuiting onto the wrong field).
        let mut s = FormsState::default();
        let before = (
            s.push_notifications,
            s.email_digest,
            s.weekly_summary,
            s.theme.clone(),
            s.auto_lock,
            s.share_usage,
        );
        forms_on_event(&mut s, click("nav-forms"));
        forms_on_event(&mut s, click("save"));
        forms_on_event(&mut s, click("forms-theme")); // group key, not a value
        let after = (
            s.push_notifications,
            s.email_digest,
            s.weekly_summary,
            s.theme.clone(),
            s.auto_lock,
            s.share_usage,
        );
        assert_eq!(before, after, "unrelated events left state unchanged");
    }

    #[test]
    fn glass_section_advertises_liquid_glass_shader() {
        let app = Showcase::with_section(Section::Glass);
        let shaders = app.shaders();
        assert_eq!(shaders.len(), 1);
        assert_eq!(shaders[0].name, "liquid_glass");
        assert!(
            shaders[0].samples_backdrop,
            "liquid_glass must opt into backdrop sampling"
        );
        assert!(
            shaders[0].wgsl.contains("backdrop_tex"),
            "shipped wgsl must reference the backdrop binding"
        );
    }
}
