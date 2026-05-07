//! density_calibration — side-by-side component density fixture.
//!
//! Run:
//! `cargo run -p aetna-core --example density_calibration`

use aetna_core::prelude::*;

fn main() -> std::io::Result<()> {
    let mut root = density_calibration();
    let viewport = Rect::new(0.0, 0.0, 1180.0, 840.0);
    let bundle = render_bundle(&mut root, viewport, Some(env!("CARGO_PKG_NAME")));

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "density_calibration")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}

fn density_calibration() -> El {
    column([
        row([
            column([
                h1("Density calibration").display().font_size(24.0),
                text("Compact, comfortable, and spacious surfaces using theme metrics roles.")
                    .caption(),
            ])
            .gap(tokens::SPACE_XS)
            .height(Size::Hug),
            spacer(),
            button("Default").secondary(),
            button("Primary").primary(),
        ])
        .height(Size::Fixed(56.0))
        .align(Align::Center)
        .gap(tokens::SPACE_SM),
        row([
            density_column("Compact", Density::Compact, ComponentSize::Sm),
            density_column("Comfortable", Density::Comfortable, ComponentSize::Md),
            density_column("Spacious", Density::Spacious, ComponentSize::Lg),
        ])
        .gap(tokens::SPACE_MD)
        .height(Size::Fill(1.0))
        .align(Align::Stretch),
    ])
    .padding(tokens::SPACE_XL)
    .gap(tokens::SPACE_LG)
    .fill_size()
    .fill(tokens::BG_APP)
}

fn density_column(title: &'static str, density: Density, size: ComponentSize) -> El {
    column([
        card(
            title,
            [
                row([
                    button("Button").size(size),
                    button("Ghost").ghost().size(size),
                    icon_button("settings").secondary().size(size),
                ])
                .gap(tokens::SPACE_SM)
                .align(Align::Center),
                text_input(
                    "Search documents...",
                    &Selection::default(),
                    &format!("{title}:search"),
                )
                .size(size),
                tabs_list(
                    format!("{title}:tabs"),
                    &"overview",
                    [
                        ("overview", "Overview"),
                        ("activity", "Activity"),
                        ("settings", "Settings"),
                    ],
                )
                .size(size)
                .density(density),
            ],
        )
        .density(density),
        card(
            "List",
            [
                list_item("git-branch", "Branch created", "2 min ago", density),
                list_item("git-commit", "Commit staged", "12 min ago", density),
                list_item("refresh-cw", "Repository synced", "1 hr ago", density),
            ],
        )
        .density(density),
        card(
            "Table",
            [
                table_header(density),
                divider(),
                table_row("Settings", "core", badge("Ready").success(), density),
                table_row("Commands", "widgets", badge("Warn").warning(), density),
            ],
        )
        .density(density),
    ])
    .gap(tokens::SPACE_MD)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn list_item(
    icon_name: &'static str,
    title: &'static str,
    meta: &'static str,
    density: Density,
) -> El {
    row([
        icon_slot(icon_name),
        text(title)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
        text(meta).caption().ellipsis().width(Size::Fixed(62.0)),
    ])
    .metrics_role(MetricsRole::ListItem)
    .density(density)
    .align(Align::Center)
    .fill(tokens::BG_CARD)
    .focusable()
}

fn table_header(density: Density) -> El {
    row([
        text("Surface").caption().width(Size::Fill(1.0)),
        text("Owner").caption().width(Size::Fixed(64.0)),
        text("State").caption().width(Size::Fixed(70.0)),
    ])
    .metrics_role(MetricsRole::TableHeader)
    .density(density)
    .align(Align::Center)
}

fn table_row(surface: &'static str, owner: &'static str, status: El, density: Density) -> El {
    row([
        text(surface).label().ellipsis().width(Size::Fill(1.0)),
        text(owner).caption().ellipsis().width(Size::Fixed(64.0)),
        status.width(Size::Fixed(70.0)),
    ])
    .metrics_role(MetricsRole::TableRow)
    .density(density)
    .align(Align::Center)
    .fill(tokens::BG_CARD)
    .focusable()
}

fn icon_slot(icon_name: &'static str) -> El {
    stack([icon(icon_name)
        .icon_size(14.0)
        .color(tokens::TEXT_FOREGROUND)])
    .align(Align::Center)
    .justify(Justify::Center)
    .fill(tokens::BG_MUTED)
    .radius(tokens::RADIUS_SM)
    .width(Size::Fixed(26.0))
    .height(Size::Fixed(26.0))
}
