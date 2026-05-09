//! Page-chrome widgets — breadcrumb, pagination, toolbar.
//!
//! Static fixture covering the small navigational primitives that
//! cluster around list views and content pages. Each is too small to
//! justify its own page but collectively they're what most paged
//! interfaces reach for.

use aetna_core::prelude::*;

#[derive(Default)]
pub struct State;

pub fn view() -> El {
    column([
        h1("Page chrome"),
        paragraph(
            "Small navigation widgets used together on most paged list \
             views. None of these own state — the app provides the path \
             segments, page tokens, and toolbar groups; the widgets give \
             them their canonical visual form.",
        )
        .muted(),
        section_label("Breadcrumb"),
        breadcrumb_list([
            breadcrumb_item(breadcrumb_link("Home")),
            breadcrumb_separator(),
            breadcrumb_item(breadcrumb_link("Documents")),
            breadcrumb_separator(),
            breadcrumb_item(breadcrumb_link("Reports")),
            breadcrumb_separator(),
            breadcrumb_item(breadcrumb_page("Q1 summary")),
        ]),
        separator(),
        section_label("Pagination"),
        pagination_content([
            pagination_item(pagination_previous()),
            pagination_item(pagination_link("1", false)),
            pagination_item(pagination_link("2", true)),
            pagination_item(pagination_link("3", false)),
            pagination_item(pagination_ellipsis()),
            pagination_item(pagination_link("9", false)),
            pagination_item(pagination_next()),
        ]),
        separator(),
        section_label("Toolbar"),
        toolbar([
            toolbar_title("Document"),
            toolbar_description("draft.md"),
            spacer(),
            toolbar_group([
                button("Format").ghost().key("page-chrome-format"),
                button("Outline").ghost().key("page-chrome-outline"),
            ]),
            vertical_separator(),
            toolbar_group([
                button("Share").secondary().key("page-chrome-share"),
                button("Publish").primary().key("page-chrome-publish"),
            ]),
        ]),
        section_label("Vertical separators in toolbars"),
        row([
            text("File").label(),
            vertical_separator(),
            text("Edit").label(),
            vertical_separator(),
            text("View").label(),
            vertical_separator(),
            text("Help").label(),
        ])
        .gap(tokens::SPACE_3)
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_4)
    .height(Size::Hug)
}

fn section_label(s: &str) -> El {
    text(s).label().muted()
}
