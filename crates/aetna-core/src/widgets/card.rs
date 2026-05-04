//! Card — content container with title + body.
//!
//! `card(title, body)` lays out as a column with comfortable padding and
//! gap. The first child is an `h3`-styled title; subsequent children are
//! the body. Cards default to filling the parent's width and hugging
//! their height.

use std::panic::Location;

use super::text::h3;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn card<I, E>(title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut children: Vec<El> = vec![h3(title)];
    children.extend(body.into_iter().map(Into::into));

    El::new(Kind::Card)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Panel)
        .children(children)
        .fill(tokens::BG_CARD)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_LG)
        .shadow(tokens::SHADOW_MD)
        .padding(tokens::SPACE_LG)
        .gap(tokens::SPACE_MD)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
}
