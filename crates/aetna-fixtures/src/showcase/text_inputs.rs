//! Text & value inputs — slider, text_input, text_area, select,
//! numeric_input, input_otp, command palette.
//!
//! Every value-capture widget side-by-side. The page also contributes
//! two floating layers via `region_layer` and `command_layer`: the
//! select-dropdown menu and the command palette overlay.

use aetna_core::prelude::*;

use super::{Section, Showcase};

const REGION_OPTIONS: &[(&str, &str)] = &[
    ("us-east", "US East (Virginia)"),
    ("us-west", "US West (Oregon)"),
    ("eu-central", "EU Central (Frankfurt)"),
    ("ap-tokyo", "AP Tokyo"),
    ("ap-sydney", "AP Sydney"),
];

const COMMAND_ENTRIES: &[(&str, &str, IconName)] = &[
    ("cmd:new-file", "New file", IconName::Plus),
    ("cmd:open-file", "Open file", IconName::FileText),
    ("cmd:settings", "Settings", IconName::Settings),
    ("cmd:dashboard", "Dashboard", IconName::LayoutDashboard),
    ("cmd:search", "Search everywhere", IconName::Search),
];

pub struct State {
    pub volume: f32,
    pub display_name: String,
    pub email: String,
    pub bio: String,
    pub selection: Selection,
    pub region: String,
    pub region_open: bool,
    pub quantity: String,
    pub otp_code: String,
    pub command_open: bool,
    pub last_command: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            volume: 0.6,
            display_name: "Christian".into(),
            email: "user@example.com".into(),
            bio: "Building Aetna — a renderer-agnostic UI kit for Rust apps and AI agents.".into(),
            selection: Selection::default(),
            region: "us-east".into(),
            region_open: false,
            quantity: "12".into(),
            otp_code: "248".into(),
            command_open: false,
            last_command: None,
        }
    }
}

pub fn view(state: &State) -> El {
    let volume_card = titled_card(
        "Slider",
        [
            row([
                text("Output volume").label(),
                spacer(),
                text(format!("{}%", (state.volume * 100.0).round() as i32)).muted(),
            ])
            .align(Align::Center),
            slider(state.volume, tokens::PRIMARY).key("ti-volume"),
            text("Drag the thumb, or focus and use ←/→ · PageUp/Down · Home/End.")
                .small()
                .muted(),
        ],
    );

    let single_line = titled_card(
        "Single-line",
        [
            input_row(
                "Display name",
                text_input(&state.display_name, &state.selection, "ti-display-name"),
            ),
            input_row(
                "Email",
                text_input(&state.email, &state.selection, "ti-email"),
            ),
        ],
    );

    let multi_line = titled_card(
        "Multi-line",
        [text_area(&state.bio, &state.selection, "ti-bio").height(Size::Fixed(96.0))],
    );

    let region_card = titled_card(
        "Select dropdown",
        [input_row(
            "Region",
            select_trigger("ti-region", region_label(&state.region)),
        )],
    );

    let quantity_card = titled_card(
        "Numeric input",
        [
            input_row(
                "Items",
                numeric_input(
                    &state.quantity,
                    &state.selection,
                    "ti-quantity",
                    quantity_opts(),
                ),
            ),
            text("Click `−` / `+` to step; clamps to 0..=99. Type to edit directly.")
                .small()
                .muted(),
        ],
    );

    let otp_card = titled_card(
        "Verification code",
        [
            input_row("Code", input_otp(&state.otp_code, "ti-otp", 6)),
            paragraph(
                "Six-digit code; the next-to-fill cell shows the active border. \
                 Backspace pops the last entry.",
            )
            .small()
            .muted(),
        ],
    );

    let command_card = titled_card(
        "Command palette",
        [
            input_row(
                "Trigger",
                row([
                    button("Open command palette")
                        .secondary()
                        .key("ti-command-trigger"),
                    spacer(),
                    text(match &state.last_command {
                        Some(c) => format!("last: {c}"),
                        None => "none yet".into(),
                    })
                    .small()
                    .muted(),
                ])
                .align(Align::Center),
            ),
            paragraph(
                "`command_*` widgets compose a fuzzy palette anatomy — \
                 group / item / icon / shortcut. The trigger button \
                 mounts the palette into a floating layer.",
            )
            .small()
            .muted(),
        ],
    );

    column([
        h1("Text & value"),
        scroll([
            volume_card,
            single_line,
            multi_line,
            region_card,
            quantity_card,
            otp_card,
            command_card,
        ])
        .key("ti-scroll")
        .height(Size::Fill(1.0))
        .gap(tokens::SPACE_4)
        .padding(Sides::xy(0.0, tokens::SPACE_2)),
    ])
    .gap(tokens::SPACE_4)
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    // Slider
    if matches!(
        e.kind,
        UiEventKind::PointerDown | UiEventKind::Drag | UiEventKind::Click
    ) && e.route() == Some("ti-volume")
        && let (Some(rect), Some(x)) = (e.target_rect(), e.pointer_x())
    {
        state.volume = slider::normalized_from_event(rect, x);
        return;
    }
    if slider::apply_event(&mut state.volume, &e, "ti-volume", 0.05, 0.25) {
        return;
    }
    // Select
    if select::apply_event(
        &mut state.region,
        &mut state.region_open,
        &e,
        "ti-region",
        Some,
    ) {
        return;
    }
    // Numeric
    if numeric_input::apply_event(
        &mut state.quantity,
        &mut state.selection,
        "ti-quantity",
        &quantity_opts(),
        &e,
    ) {
        return;
    }
    // OTP
    if input_otp::apply_event(&mut state.otp_code, "ti-otp", 6, &e) {
        return;
    }
    // Command palette open/close + pick
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        match e.route() {
            Some("ti-command-trigger") => {
                state.command_open = !state.command_open;
                return;
            }
            Some("ti-command:dismiss") => {
                state.command_open = false;
                return;
            }
            Some(k) if k.starts_with("cmd:") => {
                state.last_command = Some(k.to_string());
                state.command_open = false;
                return;
            }
            _ => {}
        }
    }
    // Text inputs
    match e.target_key() {
        Some("ti-display-name") => {
            text_input::apply_event(
                &mut state.display_name,
                &mut state.selection,
                "ti-display-name",
                &e,
            );
        }
        Some("ti-email") => {
            text_input::apply_event(&mut state.email, &mut state.selection, "ti-email", &e);
        }
        Some("ti-bio") => {
            text_area::apply_event(&mut state.bio, &mut state.selection, "ti-bio", &e);
        }
        _ => {}
    }
}

/// Floating layer for the region select menu when its dropdown is open.
pub fn region_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::TextInputs && app.text_inputs.region_open)
        .then(|| select_menu("ti-region", REGION_OPTIONS.iter().copied()))
}

/// Floating layer for the command palette when open. Each entry is a
/// `command_item` keyed with its action token; clicks fold through
/// `on_event` and close the palette.
pub fn command_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::TextInputs && app.text_inputs.command_open).then(|| {
        let items = COMMAND_ENTRIES
            .iter()
            .map(|(key, label, icon_name)| {
                command_item([
                    command_icon(*icon_name),
                    command_label(*label),
                    spacer(),
                    command_shortcut("⌘K"),
                ])
                .key(*key)
            })
            .collect::<Vec<_>>();
        dropdown("ti-command", "ti-command-trigger", [command_group(items)])
    })
}

fn region_label(value: &str) -> &'static str {
    REGION_OPTIONS
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, l)| *l)
        .unwrap_or("Pick a region")
}

fn quantity_opts() -> NumericInputOpts<'static> {
    NumericInputOpts::default()
        .min(0.0)
        .max(99.0)
        .step(1.0)
        .placeholder("0")
}

fn input_row(label: &str, control: El) -> El {
    row([
        text(label).width(Size::Fixed(110.0)).muted(),
        control.width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn select_pick_closes_dropdown_and_sets_value() {
        let mut s = State {
            region_open: true,
            ..Default::default()
        };
        on_event(&mut s, click("ti-region"));
        assert!(!s.region_open, "trigger click toggles open flag");
        on_event(&mut s, click("ti-region"));
        assert!(s.region_open);
        on_event(&mut s, click("ti-region:option:eu-central"));
        assert_eq!(s.region, "eu-central");
        assert!(!s.region_open);
    }

    #[test]
    fn command_palette_open_close_pick_round_trip() {
        let mut s = State::default();
        assert!(!s.command_open);
        on_event(&mut s, click("ti-command-trigger"));
        assert!(s.command_open);
        on_event(&mut s, click("cmd:new-file"));
        assert!(!s.command_open);
        assert_eq!(s.last_command.as_deref(), Some("cmd:new-file"));
    }
}
