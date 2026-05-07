//! dashboard_01_calibration — Aetna fixture paired with the shadcn
//! dashboard-01-style reference.
//!
//! Run:
//! `cargo run -p aetna-core --example dashboard_01_calibration`

use aetna_core::prelude::*;

fn main() -> std::io::Result<()> {
    let viewport = Rect::new(0.0, 0.0, 1180.0, 780.0);
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");

    let variants = [
        ("dashboard_01_calibration", Theme::aetna_dark()),
        (
            "dashboard_01_calibration.compact",
            Theme::aetna_dark().compact(),
        ),
        (
            "dashboard_01_calibration.comfortable",
            Theme::aetna_dark().comfortable(),
        ),
        (
            "dashboard_01_calibration.spacious",
            Theme::aetna_dark().spacious(),
        ),
    ];
    for (name, theme) in variants {
        let mut root = dashboard_01_calibration();
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

fn dashboard_01_calibration() -> El {
    row([dashboard_sidebar(), dashboard_main()])
        .gap(0.0)
        .fill_size()
        .align(Align::Stretch)
        .fill(tokens::BG_APP)
}

fn dashboard_sidebar() -> El {
    column([
        row([
            icon_cell("A"),
            column([
                text("Acme Inc.")
                    .semibold()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                text("Enterprise")
                    .caption()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
            ])
            .gap(2.0)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_SM)
        .height(Size::Fixed(44.0))
        .align(Align::Center),
        section_label("Platform"),
        side_item("layout-dashboard", "Dashboard", true),
        side_item("activity", "Lifecycle", false),
        side_item("bar-chart", "Analytics", false),
        side_item("folder", "Projects", false),
        spacer().height(Size::Fixed(tokens::SPACE_LG)),
        section_label("Documents"),
        side_item("file-text", "Data library", false),
        side_item("download", "Reports", false),
        side_item("users", "Team", false),
        spacer(),
        row([
            icon_cell("AK"),
            column([
                text("Alicia Koch")
                    .semibold()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                text("alicia@example.com")
                    .caption()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
            ])
            .gap(2.0)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_SM)
        .height(Size::Fixed(50.0))
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_SM)
    .padding(Sides::xy(tokens::SPACE_MD, tokens::SPACE_SM))
    .width(Size::Fixed(244.0))
    .height(Size::Fill(1.0))
    .fill(tokens::BG_CARD)
    .stroke(tokens::BORDER)
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
            .icon_size(15.0)
            .width(Size::Fixed(18.0)),
        text(label)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(32.0))
    .padding(Sides::xy(tokens::SPACE_SM, 0.0))
    .align(Align::Center)
    .radius(tokens::RADIUS_SM)
    .focusable();

    if selected {
        item = item.current();
    } else {
        item = item.color(tokens::TEXT_MUTED_FOREGROUND);
    }

    item
}

fn dashboard_main() -> El {
    column([
        dashboard_header(),
        column([
            row([
                metric_card(
                    "bar-chart",
                    "Total Revenue",
                    "$1,250.00",
                    "+12.5%",
                    "Trending up this month",
                    true,
                ),
                metric_card(
                    "users",
                    "New Customers",
                    "1,234",
                    "-20%",
                    "Acquisition needs attention",
                    false,
                ),
                metric_card(
                    "folder",
                    "Active Accounts",
                    "45,678",
                    "+12.5%",
                    "Strong user retention",
                    true,
                ),
                metric_card(
                    "activity",
                    "Growth Rate",
                    "4.5%",
                    "+4.5%",
                    "Meets growth projections",
                    true,
                ),
            ])
            .gap(tokens::SPACE_MD),
            row([chart_card(), sales_card()])
                .gap(tokens::SPACE_MD)
                .height(Size::Fixed(305.0))
                .align(Align::Stretch),
            documents_card(),
        ])
        .gap(tokens::SPACE_MD)
        .padding(tokens::SPACE_LG)
        .height(Size::Fill(1.0)),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn dashboard_header() -> El {
    row([
        icon_button("menu").ghost(),
        divider().width(Size::Fixed(1.0)).height(Size::Fixed(22.0)),
        h3("Documents"),
        spacer(),
        text_input("Search...", &Selection::default(), "dashboard-search")
            .width(Size::Fixed(260.0)),
        icon_button("plus").ghost(),
        icon_button("bell").ghost(),
    ])
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(56.0))
    .padding(Sides::xy(tokens::SPACE_MD, 0.0))
    .align(Align::Center)
    .stroke(tokens::BORDER)
}

fn metric_card(
    icon_name: &'static str,
    title: &'static str,
    value: &'static str,
    delta: &'static str,
    note: &'static str,
    positive: bool,
) -> El {
    let badge = if positive {
        badge(delta).success()
    } else {
        badge(delta).warning()
    };
    column([
        row([
            row([
                icon(icon_name)
                    .color(tokens::TEXT_MUTED_FOREGROUND)
                    .icon_size(14.0),
                text(title).caption().ellipsis().width(Size::Fill(1.0)),
            ])
            .gap(tokens::SPACE_XS)
            .width(Size::Fill(1.0))
            .align(Align::Center),
            badge,
        ])
        .gap(tokens::SPACE_SM)
        .align(Align::Center),
        h2(value).display().font_size(24.0).ellipsis(),
        text(note).caption().ellipsis().width(Size::Fill(1.0)),
    ])
    .style_profile(StyleProfile::Surface)
    .metrics_role(MetricsRole::Card)
    .surface_role(SurfaceRole::Panel)
    .fill(tokens::BG_CARD)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
    .shadow(tokens::SHADOW_MD)
    .padding(tokens::SPACE_MD)
    .gap(tokens::SPACE_SM)
    .width(Size::Fill(1.0))
    .height(Size::Fixed(126.0))
}

fn chart_card() -> El {
    card(
        "Visitors for the last 6 months",
        [
            text("Total visitors by channel.").caption(),
            row(chart_bars())
                .gap(2.0)
                .height(Size::Fixed(150.0))
                .align(Align::End),
        ],
    )
    .padding(tokens::SPACE_MD)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn chart_bars() -> Vec<El> {
    [
        48.0, 72.0, 56.0, 90.0, 64.0, 80.0, 108.0, 84.0, 122.0, 96.0, 136.0, 118.0,
    ]
    .into_iter()
    .flat_map(|height| {
        [
            bar(height, tokens::TEXT_MUTED_FOREGROUND),
            bar((height - 28.0_f32).max(24.0), tokens::BORDER_STRONG),
        ]
    })
    .collect()
}

fn bar(height: f32, color: Color) -> El {
    El::new(Kind::Custom("chart_bar"))
        .fill(color)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(height))
}

fn sales_card() -> El {
    card(
        "Recent Sales",
        [
            text("You made 265 sales this month.").caption(),
            sale_row("OM", "Olivia Martin", "olivia@example.com", "+$1,999.00"),
            sale_row("JL", "Jackson Lee", "jackson@example.com", "+$39.00"),
            sale_row("IN", "Isabella Nguyen", "isabella@example.com", "+$299.00"),
            sale_row("WK", "William Kim", "will@example.com", "+$99.00"),
        ],
    )
    .padding(tokens::SPACE_MD)
    .width(Size::Fixed(330.0))
    .height(Size::Fill(1.0))
}

fn sale_row(
    initials: &'static str,
    name: &'static str,
    email: &'static str,
    amount: &'static str,
) -> El {
    row([
        icon_cell(initials),
        column([
            text(name).semibold().ellipsis().width(Size::Fill(1.0)),
            text(email).caption().ellipsis().width(Size::Fill(1.0)),
        ])
        .gap(2.0)
        .height(Size::Hug)
        .width(Size::Fill(1.0)),
        text(amount).label().small(),
    ])
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(42.0))
    .align(Align::Center)
}

fn documents_card() -> El {
    card(
        "Documents",
        [
            row([
                text("Header").caption().width(Size::Fill(1.7)),
                text("Section Type").caption().width(Size::Fill(1.0)),
                text("Status").caption().width(Size::Fixed(112.0)),
                text("Target").caption().width(Size::Fixed(70.0)),
                text("Limit").caption().width(Size::Fixed(70.0)),
                text("Reviewer").caption().width(Size::Fixed(140.0)),
            ])
            .height(Size::Fixed(32.0)),
            divider(),
            document_row(
                "Cover page",
                "Cover page",
                "In Process",
                "18",
                "5",
                "Eddie Lake",
                "info",
            ),
            document_row(
                "Table of contents",
                "Table of contents",
                "Done",
                "29",
                "24",
                "Eddie Lake",
                "success",
            ),
        ],
    )
    .padding(tokens::SPACE_MD)
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(174.0))
}

fn document_row(
    header: &'static str,
    section: &'static str,
    status: &'static str,
    target: &'static str,
    limit: &'static str,
    reviewer: &'static str,
    tone: &'static str,
) -> El {
    let status_badge = match tone {
        "success" => badge(status).success(),
        _ => badge(status).info(),
    };
    row([
        text(header)
            .label()
            .small()
            .ellipsis()
            .width(Size::Fill(1.7)),
        text(section).caption().ellipsis().width(Size::Fill(1.0)),
        status_badge.width(Size::Fixed(112.0)),
        text(target).label().small().width(Size::Fixed(70.0)),
        text(limit).label().small().width(Size::Fixed(70.0)),
        text(reviewer)
            .caption()
            .ellipsis()
            .width(Size::Fixed(140.0)),
    ])
    .gap(tokens::SPACE_SM)
    .height(Size::Fixed(36.0))
    .align(Align::Center)
}

fn icon_cell(label: &'static str) -> El {
    El::new(Kind::Custom("icon_cell"))
        .style_profile(StyleProfile::Surface)
        .text(label)
        .text_align(TextAlign::Center)
        .font_size(tokens::FONT_XS)
        .font_weight(FontWeight::Semibold)
        .fill(tokens::BG_MUTED)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fixed(30.0))
        .height(Size::Fixed(30.0))
}
