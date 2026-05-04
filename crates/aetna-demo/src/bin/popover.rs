//! Popover — smoke test for the v0.9 anchored popover widget.
//!
//! Three patterns in one window:
//!
//! - **Dropdown near the top** — opens below the trigger.
//! - **Dropdown near the bottom** — auto-flips above the trigger when
//!   below would clip against the viewport.
//! - **Context menu** — right-click anywhere in the marked region; the
//!   menu anchors at the click point.
//! - **Tooltip** — `popover_panel` without a scrim, hovers next to a
//!   help icon while pressed.
//!
//! Run interactively:
//!
//! ```text
//! cargo run -p aetna-demo --bin popover
//! ```
//!
//! Things to try in the window:
//!
//! - Click "Color" to open the top dropdown; click an item to select.
//! - Click "Edit" near the bottom — the menu opens above the trigger.
//! - Click anywhere outside an open menu to dismiss; or press `Escape`.
//! - Right-click in the gray "context region" panel to open a context
//!   menu at the click position. Right-click near the bottom-right
//!   corner — the menu clamps inside the viewport.
//! - Press and hold the help icon to show a tooltip.

use aetna_core::*;

#[derive(Default)]
struct Demo {
    color: Option<&'static str>,
    color_open: bool,
    edit_open: bool,
    context_open: bool,
    context_point: (f32, f32),
    last_action: Option<String>,
    tooltip_open: bool,
}

const COLORS: &[&str] = &["Red", "Green", "Blue", "Yellow"];
const EDIT_ACTIONS: &[&str] = &["Cut", "Copy", "Paste", "Select All"];
const CTX_ACTIONS: &[&str] = &["Inspect", "Copy link", "Save as…"];

impl App for Demo {
    fn build(&self) -> El {
        let main = column([
            h2("Popover demo"),
            text(self.summary()).muted(),
            spacer().height(Size::Fixed(tokens::SPACE_LG)),
            row([
                column([
                    text("Top dropdown").muted().font_size(tokens::FONT_SM),
                    button(self.color.unwrap_or("Color"))
                        .key("color-trigger")
                        .secondary(),
                ])
                .gap(tokens::SPACE_XS)
                .height(Size::Hug),
                spacer(),
                column([
                    text("Tooltip on press").muted().font_size(tokens::FONT_SM),
                    button("?").key("help").ghost(),
                ])
                .gap(tokens::SPACE_XS)
                .height(Size::Hug),
            ]),
            spacer().height(Size::Fixed(tokens::SPACE_MD)),
            context_region(),
            spacer(),
            // Bottom-anchored trigger — its dropdown will need to flip above.
            row([spacer(), button("Edit ▾").key("edit-trigger").secondary()]),
        ])
        .padding(tokens::SPACE_XL)
        .gap(tokens::SPACE_MD);

        let mut layers: Vec<El> = vec![main];

        if self.color_open {
            layers.push(dropdown(
                "color-menu",
                "color-trigger",
                COLORS
                    .iter()
                    .map(|c| menu_item(*c).key(format!("color:{c}"))),
            ));
        }
        if self.edit_open {
            layers.push(dropdown(
                "edit-menu",
                "edit-trigger",
                EDIT_ACTIONS
                    .iter()
                    .map(|a| menu_item(*a).key(format!("edit:{a}"))),
            ));
        }
        if self.context_open {
            layers.push(context_menu(
                "ctx-menu",
                self.context_point,
                CTX_ACTIONS
                    .iter()
                    .map(|a| menu_item(*a).key(format!("ctx:{a}"))),
            ));
        }
        if self.tooltip_open {
            layers.push(tooltip_layer());
        }

        stack(layers)
    }

    fn on_event(&mut self, event: UiEvent) {
        // Open / close logic.
        match (&event.kind, event.key.as_deref()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("color-trigger")) => {
                self.color_open = !self.color_open;
                self.edit_open = false;
                self.context_open = false;
                return;
            }
            (UiEventKind::Click | UiEventKind::Activate, Some("edit-trigger")) => {
                self.edit_open = !self.edit_open;
                self.color_open = false;
                self.context_open = false;
                return;
            }
            (UiEventKind::SecondaryClick, Some("ctx-region")) => {
                if let Some(p) = event.pointer {
                    self.context_point = p;
                    self.context_open = true;
                    self.color_open = false;
                    self.edit_open = false;
                }
                return;
            }
            (UiEventKind::PointerDown, Some("help")) => {
                self.tooltip_open = true;
                return;
            }
            (UiEventKind::PointerUp, Some("help")) => {
                self.tooltip_open = false;
                return;
            }
            (UiEventKind::Escape, _) => {
                self.close_all_menus();
                return;
            }
            _ => {}
        }

        // Dismiss-via-outside-click: any popover's scrim emits
        // `{key}:dismiss` on click.
        if matches!(event.kind, UiEventKind::Click)
            && let Some(key) = event.key.as_deref()
        {
            match key {
                "color-menu:dismiss" => {
                    self.color_open = false;
                    return;
                }
                "edit-menu:dismiss" => {
                    self.edit_open = false;
                    return;
                }
                "ctx-menu:dismiss" => {
                    self.context_open = false;
                    return;
                }
                _ => {}
            }
        }

        // Item routing — menu items carry `{family}:{label}` keys.
        if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate)
            && let Some(key) = event.key.as_deref()
        {
            if let Some(c) = key.strip_prefix("color:") {
                self.color = COLORS.iter().copied().find(|x| *x == c);
                self.color_open = false;
                self.last_action = Some(format!("Picked color: {c}"));
                return;
            }
            if let Some(a) = key.strip_prefix("edit:") {
                self.edit_open = false;
                self.last_action = Some(format!("Edit: {a}"));
                return;
            }
            if let Some(a) = key.strip_prefix("ctx:") {
                self.context_open = false;
                self.last_action = Some(format!("Context: {a}"));
            }
        }
    }
}

impl Demo {
    fn close_all_menus(&mut self) {
        self.color_open = false;
        self.edit_open = false;
        self.context_open = false;
    }

    fn summary(&self) -> String {
        match &self.last_action {
            Some(a) => format!("Last action: {a}"),
            None => "Click a trigger or right-click the gray region.".to_string(),
        }
    }
}

fn context_region() -> El {
    El::new(Kind::Card)
        .key("ctx-region")
        .style_profile(StyleProfile::Surface)
        .child(
            text("Right-click anywhere in this region")
                .muted()
                .center_text(),
        )
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_MD)
        .padding(tokens::SPACE_LG)
        .height(Size::Fixed(80.0))
        .width(Size::Fill(1.0))
        .axis(Axis::Overlay)
        .align(Align::Center)
        .justify(Justify::Center)
}

fn tooltip_layer() -> El {
    // The tooltip uses popover_panel directly (no scrim). Wrap it in
    // a layer that fills the viewport and anchors to the help button.
    let panel = popover_panel([text("Show tooltip while pressed").font_size(tokens::FONT_SM)])
        .padding(tokens::SPACE_SM);
    El::new(Kind::Custom("tooltip_layer"))
        .child(panel)
        .fill_size()
        .layout(|ctx| {
            let (w, h) = (ctx.measure)(&ctx.children[0]);
            let rect = anchor_rect(
                &Anchor::below_key("help"),
                (w, h),
                ctx.container,
                ctx.rect_of_key,
            );
            vec![rect]
        })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 480.0);
    aetna_demo::run("Aetna — popover smoke test", viewport, Demo::default())
}
