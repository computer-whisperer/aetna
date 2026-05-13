//! Page-chrome widgets — breadcrumb, pagination, toolbar, menubar.
//!
//! Static fixture covering the small navigational primitives that
//! cluster around list views and content pages. Each is too small to
//! justify its own page but collectively they're what most paged
//! interfaces reach for.

use aetna_core::prelude::*;

#[derive(Default)]
pub struct State {
    pub open_menu: Option<String>,
    pub last_menu_action: Option<String>,
}

const MENUBAR_KEY: &str = "page-menubar";

pub fn view(state: &State) -> El {
    column([
        h1("Page chrome"),
        paragraph(
            "Small navigation widgets used together on most paged list \
             views. None of these own state — the app provides the path \
             segments, page tokens, and toolbar groups; the widgets give \
             them their canonical visual form.",
        )
        .muted(),
        section_label("Menubar"),
        menubar_demo(state),
        separator(),
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

pub fn on_event(state: &mut State, event: UiEvent) {
    if menubar::apply_event(&mut state.open_menu, &event, MENUBAR_KEY) {
        return;
    }
    if !matches!(event.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    if let Some(route) = event.route()
        && let Some(action) = route.strip_prefix("page-menubar-action:")
    {
        state.last_menu_action = Some(action.to_string());
        state.open_menu = None;
    }
}

pub fn layer(state: &State) -> Option<El> {
    let open = state.open_menu.as_deref()?;
    let items = match open {
        "file" => vec![
            menubar_item_with_icon_and_shortcut(IconName::FileText, "New file", "Ctrl+N")
                .key("page-menubar-action:new-file"),
            menubar_item_with_icon_and_shortcut(IconName::Folder, "Open project", "Ctrl+O")
                .key("page-menubar-action:open-project"),
            menubar_separator(),
            menubar_item_with_shortcut("Save", "Ctrl+S").key("page-menubar-action:save"),
        ],
        "view" => vec![
            menubar_item_with_icon(IconName::Search, "Command palette")
                .key("page-menubar-action:command-palette"),
            menubar_item_with_icon(IconName::Settings, "Preferences")
                .key("page-menubar-action:preferences"),
        ],
        "help" => vec![
            menubar_label("Resources"),
            menubar_item([menubar_item_label("Documentation")])
                .key("page-menubar-action:documentation"),
            menubar_item([menubar_item_label("Keyboard shortcuts")])
                .key("page-menubar-action:shortcuts"),
        ],
        _ => return None,
    };
    Some(menubar_menu(MENUBAR_KEY, open, items))
}

fn menubar_demo(state: &State) -> El {
    let open = state.open_menu.as_deref();
    let action = state
        .last_menu_action
        .as_deref()
        .unwrap_or("choose an item from File, View, or Help");
    column([
        menubar([
            menubar_trigger(MENUBAR_KEY, "file", "File", open == Some("file")),
            menubar_trigger(MENUBAR_KEY, "view", "View", open == Some("view")),
            menubar_trigger(MENUBAR_KEY, "help", "Help", open == Some("help")),
        ]),
        text(format!("last action: {action}")).small().muted(),
    ])
    .gap(tokens::SPACE_2)
}

fn section_label(s: &str) -> El {
    text(s).label().muted()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn menubar_open_pick_and_dismiss_cycle() {
        let mut state = State::default();

        on_event(&mut state, click("page-menubar:menu:file"));
        assert_eq!(state.open_menu.as_deref(), Some("file"));
        assert!(layer(&state).is_some());

        on_event(&mut state, click("page-menubar-action:save"));
        assert_eq!(state.open_menu, None);
        assert_eq!(state.last_menu_action.as_deref(), Some("save"));

        on_event(&mut state, click("page-menubar:menu:view"));
        assert_eq!(state.open_menu.as_deref(), Some("view"));
        on_event(&mut state, click("page-menubar:menu:view:dismiss"));
        assert_eq!(state.open_menu, None);
    }
}
