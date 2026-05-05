//! Overlay and modal primitives.
//!
//! These are ordinary [`El`] trees, not a hidden retained overlay stack.
//! That keeps the agent loop simple: the scrim, panel, buttons, source
//! locations, draw ops, and hit-test keys all appear in the same artifacts
//! as the rest of the UI.

use std::panic::Location;

use super::text::h3;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

/// A full-size overlay layer. Children share the overlay rect and are
/// centered by default; put a full-size scrim first and the floating
/// surface after it.
#[track_caller]
pub fn overlay<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Overlay)
        .at_loc(Location::caller())
        .children(children)
        .fill_size()
        .align(Align::Center)
        .justify(Justify::Center)
        .axis(Axis::Overlay)
        .clip()
}

/// Compose a main view with optional floating layers, filtering out
/// `None`s. A thin sugar wrapper over [`crate::stack`] for the
/// recurring "main + maybe-modal + maybe-popover" pattern at the El
/// root:
///
/// ```ignore
/// overlays(main_view, [
///     self.modal_open.then(|| modal("confirm", "Delete?", [...])),
///     self.menu_open.then(|| dropdown("menu", "trigger", [...])),
/// ])
/// ```
///
/// Equivalent to building a `Vec<El>` by hand and pushing only when
/// each `Option` is `Some`. Layers paint in the order given (last on
/// top); hit-testing visits them in reverse.
#[track_caller]
pub fn overlays<I>(main: impl Into<El>, layers: I) -> El
where
    I: IntoIterator<Item = Option<El>>,
{
    let mut children: Vec<El> = Vec::new();
    children.push(main.into());
    children.extend(layers.into_iter().flatten());
    crate::stack(children)
}

/// A full-size modal scrim. The key should route to dismiss behavior in
/// the app's event handler.
#[track_caller]
pub fn scrim(key: impl Into<String>) -> El {
    El::new(Kind::Scrim)
        .at_loc(Location::caller())
        .key(key)
        .fill(tokens::OVERLAY_SCRIM)
        .fill_size()
}

/// A centered modal with a keyed dismiss scrim.
///
/// Keys:
/// - `{key}:dismiss` — emitted when the user clicks outside the panel.
/// - Child controls keep their own keys, e.g. `button("Delete").key("confirm")`.
#[track_caller]
pub fn modal<I, E>(key: impl Into<String>, title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let key = key.into();
    overlay([
        scrim(format!("{key}:dismiss")),
        modal_panel(title, body).block_pointer(),
    ])
}

#[track_caller]
pub fn modal_panel<I, E>(title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = vec![h3(title)];
    children.extend(body.into_iter().map(Into::into));

    El::new(Kind::Modal)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Popover)
        .children(children)
        .fill(tokens::BG_CARD)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_LG)
        .shadow(tokens::SHADOW_LG)
        .padding(tokens::SPACE_LG)
        .gap(tokens::SPACE_MD)
        .width(Size::Fixed(420.0))
        .height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
        .clip()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::button::button;

    #[test]
    fn overlays_filters_none_layers_in_order() {
        let main = button("main").key("main");
        let one = button("one").key("one");
        let two = button("two").key("two");
        let stacked = overlays(main, [None, Some(one), None, Some(two)]);
        let keys: Vec<_> = stacked
            .children
            .iter()
            .map(|c| c.key.clone().unwrap_or_default())
            .collect();
        assert_eq!(keys, vec!["main", "one", "two"]);
    }

    #[test]
    fn overlays_with_no_layers_is_just_main_in_a_stack() {
        let stacked = overlays(button("main").key("main"), std::iter::empty::<Option<El>>());
        assert_eq!(stacked.children.len(), 1);
        assert_eq!(stacked.children[0].key.as_deref(), Some("main"));
    }
}
