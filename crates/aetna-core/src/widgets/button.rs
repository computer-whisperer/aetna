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
//!
//! # Dogfood note (v0.7.5)
//!
//! This builder uses only the public widget-author surface — `Kind::Custom`
//! for the inspector tag, `.focusable()` to opt into the focus ring,
//! `.paint_overflow()` to give the ring somewhere to render, and
//! `.text_align(TextAlign::Center)` to center the label. An app crate
//! can write an equivalent button against the same API; nothing here
//! reaches into library internals. See `widget_kit.md`.

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn button(label: impl Into<String>) -> El {
    El::new(Kind::Custom("button"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .focusable()
        .paint_overflow(Sides::all(tokens::FOCUS_RING_WIDTH))
        .text(label)
        .text_align(TextAlign::Center)
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
