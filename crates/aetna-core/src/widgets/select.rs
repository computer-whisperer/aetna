//! Select / dropdown menu — a trigger surface that displays the
//! currently chosen value paired with a dropdown popover of options.
//! Authored as two compositional pieces (trigger + menu) so apps place
//! the trigger inline in their layout and compose the menu at the root
//! of the El tree (the popover paradigm — see `widgets/popover.rs`).
//!
//! # Shape
//!
//! ```ignore
//! struct App { color: String, color_open: bool }
//! impl aetna_core::App for App {
//!     fn build(&self) -> El {
//!         let trigger = select_trigger("color", &self.color);
//!         let main = column([row([text("Color"), trigger])]);
//!
//!         let mut layers: Vec<El> = vec![main];
//!         if self.color_open {
//!             layers.push(select_menu("color", [
//!                 ("red", "Red"),
//!                 ("blue", "Blue"),
//!                 ("green", "Green"),
//!             ]));
//!         }
//!         stack(layers)
//!     }
//! }
//! ```
//!
//! # Routed keys
//!
//! - `{key}` — `Click` on the trigger; the app toggles its open flag.
//! - `{key}:dismiss` — `Click` outside the menu (the popover scrim);
//!   the app clears its open flag.
//! - `{key}:option:{value}` — `Click` on an option; the app sets the
//!   selected value and clears its open flag.
//!
//! Apps that share one open slot across several selects can match the
//! `:option:` and `:dismiss` suffixes back to the active select's key.
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface — `Kind::Custom` for
//! the inspector tag, `.focusable()` + `.paint_overflow()` for the
//! focus ring, `.key()` for hit-test routing, and the existing
//! [`crate::widgets::popover`] composition for the dropdown body. An
//! app crate can write an equivalent select against the same public
//! API. See `widget_kit.md`.

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::widgets::popover::{Anchor, menu_item, popover, popover_panel};
use crate::{icon, text};

/// Format the routed key emitted when an option is clicked. Apps that
/// match against the `:option:` suffix can use this helper to produce
/// the same string the widget produces, but the convention is also
/// stable enough to format inline.
pub fn select_option_key(key: &str, value: &impl std::fmt::Display) -> String {
    format!("{key}:option:{value}")
}

/// The trigger surface for a `select`. Visually a button-shaped row
/// of `[ current_label ▼ ]` keyed by `key`. Click emits `Click` on
/// `key`; the app toggles its open flag in `on_event`.
///
/// The trigger is also the anchor key for [`select_menu`] — keep them
/// identical so the menu drops below the trigger.
#[track_caller]
pub fn select_trigger(key: impl Into<String>, current_label: impl Into<String>) -> El {
    let label = text(current_label)
        .label()
        .ellipsis()
        .width(Size::Fill(1.0));
    let chevron = icon("chevron-down")
        .icon_size(16.0)
        .text_color(tokens::TEXT_MUTED_FOREGROUND);
    El::new(Kind::Custom("select_trigger"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Input)
        .focusable()
        .paint_overflow(Sides::all(tokens::FOCUS_RING_WIDTH))
        .key(key)
        .axis(Axis::Row)
        .gap(tokens::SPACE_SM)
        .align(Align::Center)
        .child(label)
        .child(chevron)
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .text_color(tokens::TEXT_FOREGROUND)
        .radius(tokens::RADIUS_MD)
        .height(Size::Fixed(36.0))
        .padding(Sides::xy(tokens::SPACE_MD, 0.0))
}

/// The dropdown popover for a `select`. Render this only while the
/// menu is open; place it at the root of the El tree (e.g. inside a
/// `stack`) so it paints over content and intercepts clicks above
/// siblings.
///
/// `options` is an iterable of `(value, label)` pairs. Each becomes a
/// [`menu_item`] keyed `{key}:option:{value}`. The dismiss scrim
/// emits `{key}:dismiss` (per the popover convention) on click
/// outside.
///
/// The menu anchors below the trigger keyed `key`; if that placement
/// would clip the viewport bottom the popover flips above
/// automatically (see [`crate::anchor_rect`]).
#[track_caller]
pub fn select_menu<I, V, L>(key: impl Into<String>, options: I) -> El
where
    I: IntoIterator<Item = (V, L)>,
    V: std::fmt::Display,
    L: Into<String>,
{
    let key = key.into();
    let items: Vec<El> = options
        .into_iter()
        .map(|(value, label)| menu_item(label).key(select_option_key(&key, &value)))
        .collect();
    popover(key.clone(), Anchor::below_key(key), popover_panel(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_trigger_keys_root_and_carries_chevron() {
        let t = select_trigger("color", "Red");
        assert_eq!(t.key.as_deref(), Some("color"));
        // Trigger is a row of [label, chevron]. The chevron is the
        // last child and carries the chevron-down icon name so visual
        // affordance is unambiguous.
        let chevron = t.children.last().expect("trigger has chevron child");
        assert_eq!(chevron.icon, Some(IconName::ChevronDown));
        // Trigger opts into focus + ring overhead so keyboard users
        // can tab through selects like any other interactive surface.
        assert!(t.focusable, "select_trigger must be focusable");
    }

    #[test]
    fn select_menu_routes_dismiss_and_option_keys() {
        let menu = select_menu("color", [("red", "Red"), ("blue", "Blue")]);
        // Dismiss scrim follows the popover convention: `{key}:dismiss`.
        let scrim = &menu.children[0];
        assert_eq!(scrim.kind, Kind::Scrim);
        assert_eq!(scrim.key.as_deref(), Some("color:dismiss"));
        // Layer wraps the panel; panel children are the menu_items
        // keyed `{key}:option:{value}`.
        let layer = &menu.children[1];
        let panel = &layer.children[0];
        assert_eq!(panel.children.len(), 2);
        assert_eq!(panel.children[0].key.as_deref(), Some("color:option:red"));
        assert_eq!(panel.children[1].key.as_deref(), Some("color:option:blue"));
    }

    #[test]
    fn select_option_key_matches_widget_format() {
        // Apps decoding routed events should use the same helper to
        // avoid format drift.
        assert_eq!(select_option_key("color", &"red"), "color:option:red");
        assert_eq!(
            select_option_key("profile:7", &42u32),
            "profile:7:option:42"
        );
    }

    #[test]
    fn select_menu_anchors_below_trigger_key() {
        // End-to-end layout regression: the menu must look up the
        // trigger's rect via `rect_of_key(key)`, so when the trigger
        // is laid out at (x, y, w, h), the panel lands directly below.
        use crate::layout::layout;
        use crate::state::UiState;
        use crate::tree::stack;
        let trigger = select_trigger("sel", "A");
        let menu = select_menu("sel", [("a", "A"), ("b", "B")]);
        let mut tree = stack([trigger, menu]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 300.0));
        // Trigger laid out by stack at parent origin, height 36.
        let trig_rect = state
            .rect_of_key(&tree, "sel")
            .expect("trigger key resolves");
        // The popover panel sits below the trigger with the standard
        // anchor gap. It's the popover layer's first child.
        let layer = &tree.children[1].children[1];
        let panel = &layer.children[0];
        let panel_rect = state.rect(&panel.computed_id);
        assert!(
            panel_rect.y >= trig_rect.bottom(),
            "panel should sit below trigger; trig.bottom={}, panel.y={}",
            trig_rect.bottom(),
            panel_rect.y,
        );
    }
}
