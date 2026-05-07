//! settings_calibration — Aetna fixture paired with the shadcn
//! settings/form reference.
//!
//! Run:
//! `cargo run -p aetna-core --example settings_calibration`

use aetna_core::prelude::*;

fn main() -> std::io::Result<()> {
    let viewport = Rect::new(0.0, 0.0, 1180.0, 780.0);
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");

    let variants = [
        ("settings_calibration", Theme::aetna_dark()),
        (
            "settings_calibration.compact",
            Theme::aetna_dark().compact(),
        ),
        (
            "settings_calibration.comfortable",
            Theme::aetna_dark().comfortable(),
        ),
        (
            "settings_calibration.spacious",
            Theme::aetna_dark().spacious(),
        ),
    ];
    for (name, theme) in variants {
        let mut root = settings_calibration();
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

fn settings_calibration() -> El {
    row([settings_sidebar(), settings_main()])
        .gap(0.0)
        .fill_size()
        .align(Align::Stretch)
        .fill(tokens::BG_APP)
}

fn settings_sidebar() -> El {
    column([
        row([
            icon_slot("settings"),
            column([
                text("Workspace")
                    .semibold()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                text("Settings").caption().ellipsis().width(Size::Fill(1.0)),
            ])
            .gap(2.0)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_SM)
        .height(Size::Fixed(44.0))
        .align(Align::Center),
        section_label("Personal"),
        side_item("users", "Profile", false),
        side_item("settings", "Account", true),
        side_item("alert-circle", "Security", false),
        side_item("bell", "Notifications", false),
        spacer().height(Size::Fixed(tokens::SPACE_LG)),
        section_label("Workspace"),
        side_item("file-text", "Billing", false),
        side_item("bar-chart", "Appearance", false),
        side_item("activity", "Integrations", false),
        spacer(),
        column([text("Changes sync after save.").caption().wrap_text()])
            .padding(tokens::SPACE_SM)
            .fill(tokens::BG_MUTED)
            .radius(tokens::RADIUS_MD),
    ])
    .gap(tokens::SPACE_SM)
    .padding(Sides::xy(tokens::SPACE_MD, tokens::SPACE_SM))
    .width(Size::Fixed(244.0))
    .height(Size::Fill(1.0))
    .fill(tokens::BG_CARD)
    .stroke(tokens::BORDER)
}

fn settings_main() -> El {
    column([
        settings_header(),
        row([settings_nav_card(), settings_body(), settings_aside()])
            .gap(tokens::SPACE_MD)
            .padding(tokens::SPACE_MD)
            .height(Size::Fill(1.0))
            .align(Align::Stretch),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn settings_header() -> El {
    row([
        icon_button("menu").ghost(),
        divider().width(Size::Fixed(1.0)).height(Size::Fixed(22.0)),
        h3("Settings"),
        spacer(),
        button("Reset").secondary(),
        button("Save changes").primary(),
    ])
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(56.0))
    .padding(Sides::xy(tokens::SPACE_MD, 0.0))
    .align(Align::Center)
    .stroke(tokens::BORDER)
}

fn settings_nav_card() -> El {
    column([
        settings_nav_item("Account", true),
        settings_nav_item("Security", false),
        settings_nav_item("Notifications", false),
        settings_nav_item("Appearance", false),
        settings_nav_item("Billing", false),
    ])
    .gap(tokens::SPACE_XS)
    .padding(tokens::SPACE_XS)
    .width(Size::Fixed(220.0))
    .height(Size::Fill(1.0))
    .style_profile(StyleProfile::Surface)
    .surface_role(SurfaceRole::Panel)
    .fill(tokens::BG_CARD)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
    .shadow(tokens::SHADOW_MD)
}

fn settings_nav_item(label: &'static str, selected: bool) -> El {
    let mut item = row([
        El::new(Kind::Custom("nav-dot"))
            .fill(tokens::TEXT_MUTED_FOREGROUND)
            .radius(tokens::RADIUS_PILL)
            .width(Size::Fixed(6.0))
            .height(Size::Fixed(6.0)),
        text(label)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .metrics_role(MetricsRole::ListItem)
    .align(Align::Center)
    .focusable();

    if selected {
        item = item.current();
    } else {
        item = item.color(tokens::TEXT_MUTED_FOREGROUND);
    }

    item
}

fn settings_body() -> El {
    column([
        column([
            h1("Account").heading(),
            text("Manage identity, workspace defaults, and security preferences.").caption(),
        ])
        .gap(tokens::SPACE_XS)
        .height(Size::Hug),
        profile_card(),
        preferences_card(),
    ])
    .gap(tokens::SPACE_MD)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn profile_card() -> El {
    card(
        "Profile",
        [
            text("This information appears in audit logs and shared documents.")
                .caption()
                .wrap_text()
                .width(Size::Fill(1.0)),
            row([
                setting_field("Display name", "Alicia Koch", "display-name"),
                setting_field("Email", "alicia@example.com", "email"),
            ])
            .gap(tokens::SPACE_MD),
            row([
                setting_select("Role", "Workspace admin", "role"),
                setting_select("Region", "US East", "region"),
            ])
            .gap(tokens::SPACE_MD),
        ],
    )
}

fn setting_field(label: &'static str, value: &'static str, key: &'static str) -> El {
    column([
        text(label).label(),
        text_input(value, &Selection::caret(key, value.len()), key).width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_XS)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn setting_select(label: &'static str, value: &'static str, key: &'static str) -> El {
    column([
        text(label).label(),
        select_trigger(key, value).width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_XS)
    .width(Size::Fill(1.0))
    .height(Size::Hug)
}

fn preferences_card() -> El {
    card(
        "Preferences",
        [
            text("Defaults used when creating new dashboards and exports.").caption(),
            preference_row(
                "Compact navigation",
                "Use tighter rows in the sidebar and command menus.",
                switch(true).key("compact-navigation"),
            ),
            divider(),
            preference_row(
                "Email summaries",
                "Send a daily digest when documents change.",
                switch(false).key("email-summaries"),
            ),
            divider(),
            preference_row(
                "Require approval",
                "Route external sharing through an owner review.",
                checkbox(true).key("approval-required"),
            ),
        ],
    )
}

fn preference_row(title: &'static str, description: &'static str, control: El) -> El {
    row([
        column([
            text(title).semibold().ellipsis().width(Size::Fill(1.0)),
            text(description)
                .caption()
                .ellipsis()
                .width(Size::Fill(1.0)),
        ])
        .gap(2.0)
        .width(Size::Fill(1.0))
        .height(Size::Hug),
        control,
    ])
    .gap(tokens::SPACE_MD)
    .height(Size::Fixed(52.0))
    .align(Align::Center)
}

fn settings_aside() -> El {
    column([security_card(), scale_card()])
        .gap(tokens::SPACE_MD)
        .width(Size::Fixed(300.0))
        .height(Size::Fill(1.0))
}

fn security_card() -> El {
    card(
        "Security",
        [
            text("Two-factor authentication is enabled for all privileged users.")
                .caption()
                .wrap_text()
                .width(Size::Fill(1.0)),
            compact_stat("Passkeys", "2 registered", badge("On").success()),
            compact_stat("Sessions", "3 active", button("Review").secondary()),
        ],
    )
    .width(Size::Fill(1.0))
}

fn scale_card() -> El {
    card(
        "Interface scale",
        [
            text("Reference captures keep browser zoom fixed and vary root UI scale.")
                .caption()
                .wrap_text()
                .width(Size::Fill(1.0)),
            row([text("Dense").caption(), spacer(), text("Default").caption()]),
            slider(0.66, tokens::PRIMARY)
                .key("interface-scale")
                .width(Size::Fill(1.0)),
        ],
    )
    .width(Size::Fill(1.0))
}

fn compact_stat(title: &'static str, detail: &'static str, control: El) -> El {
    row([
        column([
            text(title).semibold().ellipsis().width(Size::Fill(1.0)),
            text(detail).caption().ellipsis().width(Size::Fill(1.0)),
        ])
        .gap(2.0)
        .width(Size::Fill(1.0))
        .height(Size::Hug),
        control,
    ])
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(44.0))
    .align(Align::Center)
}

fn section_label(label: &'static str) -> El {
    text(label)
        .caption()
        .height(Size::Fixed(22.0))
        .padding(Sides::xy(tokens::SPACE_SM, 0.0))
}

fn side_item(icon_name: &'static str, label: &'static str, selected: bool) -> El {
    let mut item = row([
        icon(icon_name)
            .color(tokens::TEXT_MUTED_FOREGROUND)
            .icon_size(tokens::FONT_LG)
            .width(Size::Fixed(tokens::FONT_LG)),
        text(label)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .metrics_role(MetricsRole::ListItem)
    .compact()
    .align(Align::Center)
    .focusable();

    if selected {
        item = item.current();
    } else {
        item = item.color(tokens::TEXT_MUTED_FOREGROUND);
    }

    item
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
        .width(Size::Fixed(30.0))
        .height(Size::Fixed(30.0))
}
