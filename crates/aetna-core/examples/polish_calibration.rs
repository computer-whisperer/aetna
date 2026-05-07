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
    let viewport = Rect::new(0.0, 0.0, 1180.0, 780.0);
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");

    let variants = [
        ("polish_calibration", Theme::aetna_dark()),
        ("polish_calibration.compact", Theme::aetna_dark().compact()),
        (
            "polish_calibration.comfortable",
            Theme::aetna_dark().comfortable(),
        ),
        (
            "polish_calibration.spacious",
            Theme::aetna_dark().spacious(),
        ),
    ];
    for (name, theme) in variants {
        let mut root = polish_calibration(theme.metrics().layout());
        let bundle =
            render_bundle_themed(&mut root, viewport, Some(env!("CARGO_PKG_NAME")), &theme);
        let written = write_bundle(&bundle, &out_dir, name)?;
        for p in &written {
            println!("wrote {}", p.display());
        }
        if !bundle.lint.findings.is_empty() {
            eprintln!(
                "\nlint findings for {name} ({}):",
                bundle.lint.findings.len()
            );
            eprint!("{}", bundle.lint.text());
        }
    }

    Ok(())
}

fn polish_calibration(layout: LayoutMetrics) -> El {
    row([sidebar(layout), main_panel(layout)])
        .key("metric:root")
        .gap(0.0)
        .fill_size()
        .align(Align::Stretch)
        .fill(tokens::BG_APP)
}

fn sidebar(layout: LayoutMetrics) -> El {
    column([
        column([h2("Aetna"), text("calibration").muted()])
            .key("metric:sidebar.brand")
            .gap(tokens::SPACE_XS)
            .height(Size::Hug),
        spacer().height(Size::Fixed(tokens::SPACE_LG)),
        nav_item("01", "Overview", true),
        nav_item("02", "Commands", false),
        nav_item("03", "Tables", false),
        nav_item("04", "Forms", false),
        spacer(),
        badge("dark theme").muted(),
    ])
    .gap(tokens::SPACE_SM)
    .padding(layout.pane_padding)
    .key("metric:sidebar")
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
    .key(if selected {
        "metric:sidebar.nav.row".to_string()
    } else {
        format!("nav-{label}")
    })
    .metrics_role(MetricsRole::ListItem)
    .align(Align::Center)
    .focusable();

    if selected {
        item = item.current();
    }

    item
}

fn main_panel(layout: LayoutMetrics) -> El {
    column([
        toolbar(),
        column([
            row([
                kpi_card("Latency", "42 ms", "-18%", true),
                kpi_card("Runs", "1,284", "+12%", true),
                kpi_card("Errors", "7", "+2", false),
            ])
            .gap(layout.page_gap),
            row([table_card(), command_card()])
                .gap(layout.page_gap)
                .height(Size::Fill(1.0))
                .align(Align::Stretch),
        ])
        .gap(layout.page_gap)
        .height(Size::Fill(1.0))
        .align(Align::Stretch),
    ])
    .padding(layout.page_padding)
    .gap(layout.header_after_gap)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn toolbar() -> El {
    row([
        column([
            h1("Polish calibration").key("metric:page.title"),
            text("A representative app surface for default tuning.")
                .muted()
                .key("metric:page.subtitle"),
        ])
        .gap(tokens::SPACE_SM)
        .height(Size::Hug),
        spacer(),
        button_with_icon("search", "Preview")
            .secondary()
            .key("metric:action.secondary"),
        button_with_icon("upload", "Publish")
            .primary()
            .key("metric:action.primary"),
    ])
    .key("metric:header")
    .gap(tokens::SPACE_SM)
    .height(Size::Hug)
    .align(Align::Start)
}

fn kpi_card(label: &'static str, value: &'static str, delta: &'static str, positive: bool) -> El {
    let delta_badge = if positive {
        badge(delta).success()
    } else {
        badge(delta).destructive()
    };
    let delta_badge = if label == "Latency" {
        delta_badge.key("metric:kpi.badge")
    } else {
        delta_badge
    };
    let value_text = h2(value).display();
    let value_text = if label == "Latency" {
        value_text.key("metric:kpi.value")
    } else {
        value_text
    };
    card([
        card_header([card_title(label)]),
        card_content([
            row([value_text, spacer(), delta_badge]).align(Align::Center),
            text(if positive {
                "Moving in the expected direction"
            } else {
                "Needs visual attention"
            })
            .muted(),
        ]),
    ])
    .key(if label == "Latency" {
        "metric:kpi.card"
    } else {
        label
    })
    .width(Size::Fill(1.0))
}

fn table_card() -> El {
    card([
        card_header([card_title("Reference rows")]),
        card_content([table([
            table_header([table_row([
                table_head("Status").width(Size::Fixed(86.0)),
                table_head("Surface").width(Size::Fill(1.0)),
                table_head("Owner").width(Size::Fixed(110.0)),
                table_head("State").width(Size::Fixed(86.0)),
            ])
            .key("metric:table.header")]),
            divider(),
            table_body([
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
            ])
            .gap(tokens::SPACE_XS)
            .width(Size::Fill(1.0)),
        ])]),
    ])
    .key("metric:table.card")
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
    let status_badge = if selected {
        status_badge.key("metric:table.badge")
    } else {
        status_badge
    };

    let mut row = table_row([
        table_cell(status_badge).width(Size::Fixed(70.0)),
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
        table_cell(text(owner).muted()).width(Size::Fixed(110.0)),
        table_cell(text(state).label().small()).width(Size::Fixed(86.0)),
    ])
    .key(if selected {
        "metric:table.row".to_string()
    } else {
        format!("row-{title}")
    })
    .focusable();

    if selected {
        row = row.selected();
    }

    row
}

fn command_card() -> El {
    card([
        card_header([card_title("Command surface")]),
        card_content([
            text_input(
                "Search commands...",
                &Selection::default(),
                "command-search",
            )
            .key("metric:command.input")
            .width(Size::Fill(1.0)),
            popover_panel([
                menu_row("git-branch", "New branch", "Ctrl+B"),
                menu_row("git-commit", "Commit staged files", "Ctrl+Enter"),
                menu_row("refresh-cw", "Refresh repository", "Ctrl+R"),
                menu_row("alert-circle", "Force push", "Danger"),
            ])
            .width(Size::Fill(1.0)),
            scroll([form_probe()]).key("form-probe-scroll"),
        ])
        .height(Size::Fill(1.0)),
    ])
    .key("metric:command.card")
    .width(Size::Fill(0.8))
    .height(Size::Fill(1.0))
}

fn menu_row(icon_name: &'static str, label: &'static str, shortcut: &'static str) -> El {
    row([
        icon_slot(icon_name),
        text(label).ellipsis().width(Size::Fill(1.0)),
        mono(shortcut).caption(),
    ])
    .key(if label == "New branch" {
        "metric:command.row".to_string()
    } else {
        format!("command-row-{label}")
    })
    .metrics_role(MetricsRole::MenuItem)
    .align(Align::Center)
    .fill(tokens::BG_CARD)
    .focusable()
}

fn icon_slot(icon_name: &'static str) -> El {
    El::new(Kind::Custom("icon_cell"))
        .style_profile(StyleProfile::Surface)
        .child(
            icon(icon_name)
                .color(tokens::TEXT_FOREGROUND)
                .icon_size(tokens::ICON_XS),
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
    form([
        form_item([
            form_label("Valid input"),
            form_control(
                text_input(
                    "Valid input",
                    &Selection::caret("valid-input", 11),
                    "valid-input",
                )
                .key("metric:form.input"),
            ),
            form_description("Default field spacing and helper text."),
        ]),
        form_item([
            form_label("Invalid input"),
            form_control(
                text_input(
                    "Invalid input",
                    &Selection::caret("invalid-input", 13),
                    "invalid-input",
                )
                .invalid(),
            ),
            form_message("This field needs attention."),
        ]),
        row([
            button("Disabled").secondary().disabled(),
            button("Loading").primary().loading(),
            spacer(),
        ]),
    ])
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
        .caption()
        .font_weight(FontWeight::Semibold)
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fixed(26.0))
        .height(Size::Fixed(26.0))
}
