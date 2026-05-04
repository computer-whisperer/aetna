//! Badge — small status pill.
//!
//! Default style is `info` (accent-tinted). Apply status modifiers from
//! [`crate::style`]: `.success()`, `.warning()`, `.destructive()`,
//! `.info()`, `.muted()`.

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn badge(label: impl Into<String>) -> El {
    El::new(Kind::Badge)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Tinted)
        .text(label)
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        .font_size(tokens::FONT_SM)
        .text_color(tokens::INFO)
        .fill(tokens::INFO.with_alpha(38))
        .stroke(tokens::INFO.with_alpha(120))
        .radius(tokens::RADIUS_PILL)
        .width(Size::Hug)
        .height(Size::Fixed(22.0))
        .padding(Sides::xy(tokens::SPACE_SM, 0.0))
}
