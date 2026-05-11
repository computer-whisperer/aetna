//! Sidebar + content frame.
//!
//! The shell wraps every page in a sidebar / content row plus the set
//! of floating layers each page contributes. The sidebar uses the
//! proper `sidebar_*` widgets — `sidebar_header`, `sidebar_group`,
//! `sidebar_group_label`, `sidebar_menu`, `sidebar_menu_button` — so
//! the panel chrome and group-of-buttons spacing come from one source.

use aetna_core::prelude::*;

use super::{Group, Section, Showcase, theme_choice::ThemeChoice};

pub const THEME_PICKER_KEY: &str = "theme-picker";
pub const DIAGNOSTICS_TOGGLE_KEY: &str = "diagnostics-toggle";

/// Build the main view (sidebar + content) plus the list of floating
/// layers each page contributes. `Showcase::build` wraps the result in
/// `overlays(main, layers)`.
pub fn frame(app: &Showcase, body: El) -> (El, Vec<Option<El>>) {
    let main = row([sidebar_chrome(app), content(body)]);
    // Each page contributes zero or one floating layer; theme picker is
    // always available across pages. `Showcase::build` wraps the result
    // in `overlays(main, layers)` and inactive layers drop out.
    let layers = vec![
        app.theme_picker_open.then(theme_picker_menu),
        super::overlays::dialog_layer(app),
        super::overlays::sheet_layer(app),
        super::overlays::popover_layer(app),
        super::overlays::context_menu_layer(app),
        super::overlays::dropdown_layer(app),
        super::text_inputs::region_layer(app),
        super::text_inputs::command_layer(app),
        super::tabs_accordion::actions_layer(app),
    ];
    (main, layers)
}

fn sidebar_chrome(app: &Showcase) -> El {
    let groups = Group::ALL
        .iter()
        .copied()
        .map(|g| group_block(app.section, g));

    sidebar([
        sidebar_header([
            text("Aetna").bold().font_size(18.0),
            text("showcase").muted().small(),
        ]),
        theme_picker(app.theme_choice),
        // Wrap the nav groups in a column with right-padding equal to
        // the scrollbar's hitbox width so the thumb sits in a reserved
        // gutter to the right of the buttons. Putting the padding
        // *inside* the scroll (rather than on the sidebar) keeps the
        // thumb at the scroll's right edge while pulling the focusable
        // buttons inward — fixing the `ScrollbarObscuresFocusable`
        // findings the lint flagged on every nav item.
        scroll([column(groups.collect::<Vec<_>>())
            .gap(tokens::SPACE_3)
            .padding(Sides::right(tokens::SCROLLBAR_HITBOX_WIDTH))])
        .key("nav-scroll")
        .height(Size::Fill(1.0)),
        diagnostics_toggle(app.diagnostics_visible),
    ])
}

fn group_block(active: Section, group: Group) -> El {
    let buttons: Vec<El> = group
        .sections()
        .into_iter()
        .map(|s| sidebar_menu_button(s.label(), s == active).key(s.nav_key()))
        .collect();
    sidebar_group([sidebar_group_label(group.label()), sidebar_menu(buttons)])
}

fn theme_picker(active: ThemeChoice) -> El {
    column([
        text("Theme").caption().muted(),
        select_trigger(THEME_PICKER_KEY, active.label()),
    ])
    .gap(tokens::SPACE_1)
    .padding(Sides::xy(0.0, tokens::SPACE_2))
}

/// Footer row that toggles the host-diagnostics overlay. Sits below the
/// nav scroll so it stays pinned to the bottom of the sidebar even when
/// the section list is long enough to scroll.
fn diagnostics_toggle(active: bool) -> El {
    row([
        text("Debug overlay").label().width(Size::Fill(1.0)),
        switch(active).key(DIAGNOSTICS_TOGGLE_KEY),
    ])
    .gap(tokens::SPACE_3)
    .padding(Sides::xy(0.0, tokens::SPACE_2))
    .align(Align::Center)
}

fn theme_picker_menu() -> El {
    let options = ThemeChoice::ALL
        .iter()
        .map(|c| (c.token(), c.label()))
        .collect::<Vec<_>>();
    select_menu(THEME_PICKER_KEY, options)
}

fn content(body: El) -> El {
    column([body])
        .padding(tokens::SPACE_7)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}
