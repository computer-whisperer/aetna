//! Showcase — one app exercising every Aetna primitive.
//!
//! A single `App` impl with a sidebar nav and a content panel. Each
//! sidebar entry switches the active section; per-section state
//! persists across switches (selection, scroll offset, search). The
//! shape is a real multi-region app — the same shape host applications
//! end up with — and exercises:
//!
//! - **Buttons + click routing** in both the sidebar and the Counter
//!   section.
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

use aetna_core::*;

/// Which section the user is currently looking at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Section {
    #[default]
    Counter,
    List,
    Palette,
    Picker,
    Settings,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Section::Counter => "Counter",
            Section::List => "List",
            Section::Palette => "Palette",
            Section::Picker => "Picker",
            Section::Settings => "Settings",
        }
    }

    fn nav_key(self) -> &'static str {
        match self {
            Section::Counter => "nav-counter",
            Section::List => "nav-list",
            Section::Palette => "nav-palette",
            Section::Picker => "nav-picker",
            Section::Settings => "nav-settings",
        }
    }

    const ALL: [Section; 5] = [
        Section::Counter,
        Section::List,
        Section::Palette,
        Section::Picker,
        Section::Settings,
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

/// The showcase app. State for every section lives here so switching
/// sections is non-destructive.
#[derive(Default)]
pub struct Showcase {
    section: Section,
    counter: CounterState,
    list: ListState,
    palette: PaletteState,
    picker: PickerState,
}

impl Showcase {
    pub fn new() -> Self {
        Self::default()
    }
}

impl App for Showcase {
    fn build(&self) -> El {
        row([sidebar(self.section), content(self)])
            .gap(0.0)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
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
            && let Some(k) = event.key.as_deref()
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
        }
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
    let body = match app.section {
        Section::Counter => counter_view(&app.counter),
        Section::List => list_view(&app.list),
        Section::Palette => palette_view(&app.palette),
        Section::Picker => picker_view(&app.picker),
        Section::Settings => settings_view(),
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
        row([
            button("−").key("counter-dec").secondary(),
            button("Reset").key("counter-reset").ghost(),
            button("+").key("counter-inc").primary(),
        ])
        .gap(tokens::SPACE_MD),
        text(if state.value == 0 {
            "Click + or − to change the count.".to_string()
        } else {
            format!("You have clicked +/− a net {} times.", state.value)
        })
        .center_text()
        .muted(),
    ])
    .gap(tokens::SPACE_LG)
    .align(Align::Center)
}

fn counter_on_event(state: &mut CounterState, e: UiEvent) {
    match (e.kind, e.key.as_deref()) {
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

    column([
        h2("Scrollable list"),
        text("Wheel inside the panel. Click a row to select.").muted(),
        scroll(rows)
            .key("list-items")
            .height(Size::Fill(1.0))
            .padding(tokens::SPACE_SM),
    ])
    .gap(tokens::SPACE_LG)
}

fn list_on_event(state: &mut ListState, e: UiEvent) {
    if let (UiEventKind::Click | UiEventKind::Activate, Some(k)) = (e.kind, e.key.as_deref())
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
}

fn palette_on_event(state: &mut PaletteState, e: UiEvent) {
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate)
        && let Some(k) = e.key.as_deref()
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
    .gap(tokens::SPACE_SM);

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

    column([
        h2("Hotkey picker"),
        text("Keyboard-only navigation — chords scope to this section.").muted(),
        header,
        scroll(rows).key("picker-items").height(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_LG)
}

fn picker_hotkeys(state: &PickerState) -> Vec<(KeyChord, String)> {
    let mut out = vec![
        (KeyChord::vim('j'), "picker-move-down".into()),
        (KeyChord::vim('k'), "picker-move-up".into()),
        (KeyChord::vim('g'), "picker-go-top".into()),
        (KeyChord::ctrl('l'), "picker-clear-search".into()),
        (KeyChord::named(UiKey::Enter), "picker-open".into()),
        (
            KeyChord {
                key: UiKey::Character("/".into()),
                modifiers: KeyModifiers::default(),
            },
            "picker-toggle-search".into(),
        ),
        (
            KeyChord {
                key: UiKey::Character("G".into()),
                modifiers: KeyModifiers {
                    shift: true,
                    ..Default::default()
                },
            },
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
    match (e.kind, e.key.as_deref()) {
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
                row([text("Email"), spacer(), text("user@example.com").muted()]),
                row([
                    text("Two-factor authentication"),
                    spacer(),
                    badge("Enabled").success(),
                ]),
                row([
                    text("Recovery codes"),
                    spacer(),
                    button("Generate").secondary().key("settings-generate"),
                ]),
            ],
        ),
        card(
            "Appearance",
            [
                row([
                    text("Theme"),
                    spacer(),
                    button("Dark").secondary().key("settings-theme"),
                ]),
                row([text("Compact mode"), spacer(), badge("Off").muted()]),
                row([text("Font size"), spacer(), text("14")]),
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
                .align(Align::Start)
                .width(Size::Hug),
                spacer(),
                button("Delete").destructive().key("settings-delete"),
            ])],
        ),
        row([
            spacer(),
            button("Cancel").ghost().key("settings-cancel"),
            button("Save").primary().key("settings-save"),
        ]),
    ])
    .gap(tokens::SPACE_LG)
}
