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
