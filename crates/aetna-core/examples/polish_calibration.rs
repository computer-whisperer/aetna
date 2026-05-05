//! polish_calibration — design-system tuning fixture.
//!
//! This is not a product screen. It is the surface where Aetna's default
//! tokens, stock widgets, typography, menus, rows, and state treatments
//! are calibrated before the whisper-git validation port.
//!
//! Run:
//! `cargo run -p aetna-core --example polish_calibration`

use aetna_core::prelude::*;

fn main() -> std::io::Result<()> {
    let mut root = polish_calibration();
    let viewport = Rect::new(0.0, 0.0, 1180.0, 780.0);
    let bundle = render_bundle(&mut root, viewport, Some("crates/aetna-core/src"));

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "polish_calibration")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}

fn polish_calibration() -> El {
    row([sidebar(), main_panel()])
        .gap(0.0)
        .fill_size()
        .align(Align::Stretch)
        .fill(tokens::BG_APP)
}

fn sidebar() -> El {
    column([
        column([h2("Aetna"), text("calibration").caption()])
            .gap(tokens::SPACE_XS)
            .height(Size::Hug),
        nav_item("01", "Overview", true),
        nav_item("02", "Commands", false),
        nav_item("03", "Tables", false),
        nav_item("04", "Forms", false),
        spacer(),
        badge("dark theme").muted(),
    ])
    .gap(tokens::SPACE_SM)
    .padding(tokens::SPACE_LG)
    .width(Size::Fixed(220.0))
    .height(Size::Fill(1.0))
    .fill(tokens::BG_CARD)
    .stroke(tokens::BORDER)
}

fn nav_item(icon: &'static str, label: &'static str, selected: bool) -> El {
    let mut item = row([
        icon_cell(icon),
        text(label)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .key(format!("nav-{label}"))
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(38.0))
    .padding(Sides::xy(tokens::SPACE_SM, 0.0))
    .radius(tokens::RADIUS_MD)
    .focusable();

    if selected {
        item = item.current();
    }

    item
}

fn main_panel() -> El {
    column([
        toolbar(),
        row([
            kpi_card("Latency", "42 ms", "-18%", true),
            kpi_card("Runs", "1,284", "+12%", true),
            kpi_card("Errors", "7", "+2", false),
        ])
        .gap(tokens::SPACE_MD),
        row([table_card(), command_card()])
            .gap(tokens::SPACE_MD)
            .height(Size::Fill(1.0))
            .align(Align::Stretch),
    ])
    .padding(tokens::SPACE_XL)
    .gap(tokens::SPACE_LG)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn toolbar() -> El {
    row([
        column([
            h1("Polish calibration").display().font_size(24.0),
            text("A representative app surface for default tuning.").caption(),
        ])
        .gap(tokens::SPACE_XS)
        .height(Size::Hug),
        spacer(),
        button_with_icon("search", "Preview")
            .secondary()
            .key("preview"),
        button_with_icon("upload", "Publish")
            .primary()
            .key("publish"),
    ])
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(54.0))
}

fn kpi_card(label: &'static str, value: &'static str, delta: &'static str, positive: bool) -> El {
    let delta_badge = if positive {
        badge(delta).success()
    } else {
        badge(delta).destructive()
    };
    card(
        label,
        [
            row([h2(value), spacer(), delta_badge]).align(Align::Center),
            text(if positive {
                "Moving in the expected direction"
            } else {
                "Needs visual attention"
            })
            .caption(),
        ],
    )
    .width(Size::Fill(1.0))
}

fn table_card() -> El {
    card(
        "Reference rows",
        [
            row([
                text("Status").caption().width(Size::Fixed(86.0)),
                text("Surface").caption().width(Size::Fill(1.0)),
                text("Owner").caption().width(Size::Fixed(110.0)),
                text("State").caption().width(Size::Fixed(86.0)),
            ])
            .height(Size::Fixed(28.0))
            .padding(Sides::xy(tokens::SPACE_SM, 0.0)),
            divider(),
            data_row("OK", "Settings card", "core", "selected", true, "success"),
            data_row(
                "WARN",
                "Command palette density",
                "widgets",
                "needs work",
                false,
                "warning",
            ),
            data_row(
                "ERR",
                "Disabled and invalid states",
                "style",
                "missing",
                false,
                "destructive",
            ),
            data_row(
                "INFO",
                "Token resolution",
                "theme",
                "planned",
                false,
                "info",
            ),
            data_row(
                "OK",
                "Popover elevation",
                "shader",
                "queued",
                false,
                "success",
            ),
        ],
    )
    .width(Size::Fill(1.2))
    .height(Size::Fill(1.0))
}

fn data_row(
    status: &'static str,
    title: &'static str,
    owner: &'static str,
    state: &'static str,
    selected: bool,
    tone: &'static str,
) -> El {
    let status_badge = match tone {
        "success" => badge(status).success(),
        "warning" => badge(status).warning(),
        "destructive" => badge(status).destructive(),
        _ => badge(status).info(),
    };

    let mut row = row([
        status_badge.width(Size::Fixed(70.0)),
        column([
            text(title)
                .font_weight(FontWeight::Medium)
                .ellipsis()
                .width(Size::Fill(1.0)),
            text("Default styling probe.")
                .caption()
                .ellipsis()
                .width(Size::Fill(1.0)),
        ])
        .gap(2.0)
        .width(Size::Fill(1.0)),
        text(owner).caption().ellipsis().width(Size::Fixed(110.0)),
        text(state)
            .label()
            .small()
            .ellipsis()
            .width(Size::Fixed(86.0)),
    ])
    .key(format!("row-{title}"))
    .height(Size::Fixed(52.0))
    .padding(Sides::xy(tokens::SPACE_SM, 0.0))
    .gap(tokens::SPACE_SM)
    .radius(tokens::RADIUS_SM)
    .focusable();

    if selected {
        row = row.selected();
    }

    row
}

fn command_card() -> El {
    card(
        "Command surface",
        [
            text_input("Search commands...", TextSelection::caret(0))
                .key("command-search")
                .width(Size::Fill(1.0)),
            popover_panel([
                menu_row("git-branch", "New branch", "Ctrl+B"),
                menu_row("git-commit", "Commit staged files", "Ctrl+Enter"),
                menu_row("refresh-cw", "Refresh repository", "Ctrl+R"),
                menu_row("alert-circle", "Force push", "Danger"),
            ])
            .width(Size::Fill(1.0)),
            form_probe(),
        ],
    )
    .width(Size::Fill(0.8))
    .height(Size::Fill(1.0))
}

fn menu_row(icon_name: &'static str, label: &'static str, shortcut: &'static str) -> El {
    row([
        icon_slot(icon_name),
        text(label).ellipsis().width(Size::Fill(1.0)),
        mono(shortcut).caption(),
    ])
    .height(Size::Fixed(32.0))
    .padding(Sides::xy(tokens::SPACE_SM, 0.0))
    .radius(tokens::RADIUS_SM)
    .fill(tokens::BG_CARD)
    .focusable()
}

fn icon_slot(icon_name: &'static str) -> El {
    El::new(Kind::Custom("icon_cell"))
        .style_profile(StyleProfile::Surface)
        .child(
            icon(icon_name)
                .color(tokens::TEXT_FOREGROUND)
                .icon_size(tokens::FONT_BASE),
        )
        .align(Align::Center)
        .justify(Justify::Center)
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fixed(26.0))
        .height(Size::Fixed(26.0))
}

fn form_probe() -> El {
    column([
        text("Form state probes").semibold(),
        text_input("Valid input", TextSelection::caret(11))
            .key("valid-input")
            .width(Size::Fill(1.0)),
        text_input("Invalid input", TextSelection::caret(13))
            .key("invalid-input")
            .width(Size::Fill(1.0))
            .invalid(),
        row([
            button("Disabled").secondary().disabled(),
            button("Loading").primary().loading(),
            spacer(),
        ]),
        text("These are currently hand-styled probes; they should become semantic modifiers.")
            .caption()
            .wrap_text()
            .max_lines(2)
            .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_SM)
    .padding(tokens::SPACE_MD)
    .fill(tokens::BG_MUTED)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
}

fn icon_cell(label: &'static str) -> El {
    El::new(Kind::Custom("icon_cell"))
        .style_profile(StyleProfile::Surface)
        .text(label)
        .text_align(TextAlign::Center)
        .font_size(tokens::FONT_XS)
        .font_weight(FontWeight::Semibold)
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fixed(26.0))
        .height(Size::Fixed(26.0))
}
