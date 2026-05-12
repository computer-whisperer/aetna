//! Overlays — dialog, sheet, popover, dropdown_menu, context menu.
//!
//! Floating layers all compose `overlay([scrim, content])` underneath
//! and route a `{key}:dismiss` event when the user clicks outside the
//! panel. The page mounts each layer only when its open flag is set —
//! Showcase contributes them as siblings of the main view via
//! `overlays(main, [layer_a, layer_b, …])`.

use aetna_core::prelude::*;

use super::{Section, Showcase};

const DIALOG_KEY: &str = "ov-dialog";
const SHEET_KEY: &str = "ov-sheet";
const POPOVER_KEY: &str = "ov-popover";
const POPOVER_TRIGGER_KEY: &str = "ov-popover-trigger";
const DROPDOWN_KEY: &str = "ov-dropdown";
const DROPDOWN_TRIGGER_KEY: &str = "ov-dropdown-trigger";
const CONTEXT_MENU_KEY: &str = "ov-context";
const CONTEXT_TARGET_KEY: &str = "ov-context-target";

#[derive(Default)]
pub struct State {
    pub dialog_open: bool,
    pub sheet_open: bool,
    pub popover_open: bool,
    pub dropdown_open: bool,
    pub context_menu_at: Option<(f32, f32)>,
}

pub fn view(state: &State) -> El {
    column([
        h1("Overlays"),
        paragraph(
            "Five floating-layer flavours all sharing the same anatomy: \
             `overlay([scrim, content])`, `{key}:dismiss` on outside \
             click, and the app mounts the layer into a root stack only \
             while open.",
        )
        .muted(),
        section_label("Dialog & sheet"),
        row([
            button("Open dialog").primary().key("ov-open-dialog"),
            button("Open sheet (left)").secondary().key("ov-open-sheet"),
        ])
        .gap(tokens::SPACE_2),
        text(format!(
            "dialog: {}, sheet: {}",
            yn(state.dialog_open),
            yn(state.sheet_open)
        ))
        .small()
        .muted(),
        section_label("Popover & dropdown menu"),
        row([
            button("Show popover").secondary().key(POPOVER_TRIGGER_KEY),
            button("Open dropdown ▾")
                .secondary()
                .key(DROPDOWN_TRIGGER_KEY),
        ])
        .gap(tokens::SPACE_2),
        section_label("Context menu (right-click below)"),
        column([
            text("Right-click anywhere in this panel.").muted(),
            text(match state.context_menu_at {
                Some((x, y)) => format!("last open at ({x:.0}, {y:.0})"),
                None => "no open yet".into(),
            })
            .small()
            .muted(),
        ])
        .gap(tokens::SPACE_2)
        .padding(tokens::SPACE_5)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_MD)
        .key(CONTEXT_TARGET_KEY)
        .height(Size::Fixed(120.0)),
    ])
    .gap(tokens::SPACE_4)
    .height(Size::Hug)
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if matches!(e.kind, UiEventKind::SecondaryClick)
        && e.route() == Some(CONTEXT_TARGET_KEY)
        && let (Some(x), Some(y)) = (e.pointer_x(), e.pointer_y())
    {
        state.context_menu_at = Some((x, y));
        return;
    }
    if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    let dismiss_dialog = format!("{DIALOG_KEY}:dismiss");
    let dismiss_sheet = format!("{SHEET_KEY}:dismiss");
    let dismiss_popover = format!("{POPOVER_KEY}:dismiss");
    let dismiss_dropdown = format!("{DROPDOWN_KEY}:dismiss");
    let dismiss_context = format!("{CONTEXT_MENU_KEY}:dismiss");
    match e.route() {
        Some("ov-open-dialog") => state.dialog_open = true,
        Some("ov-open-sheet") => state.sheet_open = true,
        Some(POPOVER_TRIGGER_KEY) => state.popover_open = !state.popover_open,
        Some(DROPDOWN_TRIGGER_KEY) => state.dropdown_open = !state.dropdown_open,
        Some(k) if k == dismiss_dialog || k == "ov-dialog-save" => state.dialog_open = false,
        Some(k) if k == dismiss_sheet || k == "ov-sheet-reset" || k == "ov-sheet-apply" => {
            state.sheet_open = false
        }
        Some(k) if k == dismiss_popover => state.popover_open = false,
        Some(k) if k == dismiss_dropdown || k.starts_with("ov-dropdown-action:") => {
            state.dropdown_open = false
        }
        Some(k) if k == dismiss_context || k.starts_with("ov-context-action:") => {
            state.context_menu_at = None
        }
        _ => {}
    }
}

pub fn dialog_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::Overlays && app.overlays.dialog_open).then(dialog_content_factory)
}

pub fn sheet_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::Overlays && app.overlays.sheet_open).then(sheet_content_factory)
}

pub fn popover_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::Overlays && app.overlays.popover_open).then(|| {
        popover(
            POPOVER_KEY,
            Anchor::below_key(POPOVER_TRIGGER_KEY),
            popover_panel([column([
                text("Information").bold(),
                paragraph(
                    "Popovers are non-modal — outside clicks dismiss; \
                         interactive content survives.",
                )
                .muted(),
            ])
            .padding(tokens::SPACE_3)
            .gap(tokens::SPACE_2)]),
        )
    })
}

pub fn dropdown_layer(app: &Showcase) -> Option<El> {
    (app.section == Section::Overlays && app.overlays.dropdown_open).then(|| {
        dropdown_menu(
            DROPDOWN_KEY,
            DROPDOWN_TRIGGER_KEY,
            [
                dropdown_menu_label("Workspace"),
                dropdown_menu_item_with_icon_and_shortcut(IconName::Plus, "New project", "⌘N")
                    .key("ov-dropdown-action:new"),
                dropdown_menu_item_with_icon_and_shortcut(IconName::FileText, "Duplicate", "⌘D")
                    .key("ov-dropdown-action:duplicate"),
                dropdown_menu_item_with_icon_and_shortcut(IconName::Settings, "Settings", "⌘,")
                    .key("ov-dropdown-action:settings"),
                dropdown_menu_separator(),
                dropdown_menu_item_with_icon_and_shortcut(IconName::Search, "Find", "⌘F")
                    .key("ov-dropdown-action:find"),
                dropdown_menu_item_with_icon(IconName::X, "Sign out")
                    .key("ov-dropdown-action:signout"),
            ],
        )
    })
}

pub fn context_menu_layer(app: &Showcase) -> Option<El> {
    if app.section != Section::Overlays {
        return None;
    }
    app.overlays.context_menu_at.map(|(x, y)| {
        context_menu(
            CONTEXT_MENU_KEY,
            (x, y),
            [
                menu_item("Copy").key("ov-context-action:copy"),
                menu_item("Cut").key("ov-context-action:cut"),
                menu_item("Paste").key("ov-context-action:paste"),
                menu_item("Duplicate").key("ov-context-action:duplicate"),
                menu_item("Delete").key("ov-context-action:delete"),
            ],
        )
    })
}

fn dialog_content_factory() -> El {
    dialog(
        DIALOG_KEY,
        [
            dialog_header([
                dialog_title("Confirm changes"),
                dialog_description(
                    "Settings have been edited. Save now to keep your changes, \
                     or close this dialog to keep editing.",
                ),
            ]),
            dialog_footer([
                button("Cancel")
                    .key(format!("{DIALOG_KEY}:dismiss"))
                    .ghost(),
                button("Save").key("ov-dialog-save").primary(),
            ]),
        ],
    )
}

fn sheet_content_factory() -> El {
    sheet(
        SHEET_KEY,
        SheetSide::Left,
        [
            sheet_header([
                sheet_title("Filter"),
                sheet_description(
                    "Narrow the list with a few filters. Edge-attached so \
                     it stays out of the main content's way.",
                ),
            ]),
            paragraph(
                "Sheets are useful for navigation drawers, secondary detail \
                 inspectors, and filter panes — anywhere a full modal would \
                 feel disruptive but a transient surface still wants its own \
                 dedicated space.",
            ),
            sheet_footer([
                button("Reset").key("ov-sheet-reset").ghost(),
                button("Apply").key("ov-sheet-apply").primary(),
            ]),
        ],
    )
}

fn yn(b: bool) -> &'static str {
    if b { "open" } else { "closed" }
}

fn section_label(s: &str) -> El {
    h3(s).label()
}
