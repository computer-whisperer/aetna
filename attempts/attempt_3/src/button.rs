//! Button component.
//!
//! Default `button("Save")` is the secondary style. Apply variants from
//! [`crate::style`] to opt into others:
//!
//! - `.primary()` — filled accent color, semibold text.
//! - `.secondary()` — muted surface (the default look).
//! - `.ghost()` — no fill, no border, muted text.
//! - `.outline()` — outline-only.
//! - `.destructive()` — solid red, contrasting text.
//!
//! Buttons hug their text width and have a fixed comfortable height.
//! Override with `.width(Size::Fill(1.0))` to stretch — the label stays
//! horizontally centered.

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn button(label: impl Into<String>) -> El {
    El::new(Kind::Button)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .text(label)
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .text_color(tokens::TEXT_FOREGROUND)
        .radius(tokens::RADIUS_MD)
        .font_size(tokens::FONT_BASE)
        .font_weight(FontWeight::Medium)
        .width(Size::Hug)
        .height(Size::Fixed(36.0))
        .padding(Sides::xy(tokens::SPACE_MD, 0.0))
}
