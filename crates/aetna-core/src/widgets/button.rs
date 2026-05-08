//! Button component.
//!
//! Default `button("Save")` is the secondary style. Apply variants from
//! [`crate::style`] to opt into others:
//!
//! - `.primary()` — filled accent color, semibold text.
//! - `.secondary()` — secondary surface (the default look).
//! - `.ghost()` — no fill, no border, muted text.
//! - `.outline()` — outline-only.
//! - `.destructive()` — solid red, contrasting text.
//!
//! Buttons hug their text width and default to [`tokens::CONTROL_HEIGHT`]
//! — the same height used by `select`, `text_input`, and tab triggers,
//! so they line up in form rows. Override `.width(Size::Fill(1.0))` to
//! stretch; the label stays horizontally centered.
//!
//! # Dogfood note
//!
//! This builder uses only the public widget-author surface — `Kind::Custom`
//! for the inspector tag, `.focusable()` to opt into the focus ring,
//! `.paint_overflow()` to give the ring somewhere to render, and
//! `.text_align(TextAlign::Center)` to center the label. An app crate
//! can write an equivalent button against the same API; nothing here
//! reaches into library internals. See `widget_kit.md`.

use std::panic::Location;

use crate::cursor::Cursor;
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;
use crate::{IntoIconSource, icon, text};

#[track_caller]
pub fn button(label: impl Into<String>) -> El {
    El::new(Kind::Custom("button"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::Button)
        .surface_role(SurfaceRole::Raised)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .cursor(Cursor::Pointer)
        .text(label)
        .text_align(TextAlign::Center)
        .text_role(TextRole::Label)
        .fill(tokens::SECONDARY)
        .stroke(tokens::BORDER)
        .text_color(tokens::SECONDARY_FOREGROUND)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Hug)
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_MD, 0.0))
}

#[track_caller]
pub fn icon_button(source: impl IntoIconSource) -> El {
    El::new(Kind::Custom("icon_button"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::IconButton)
        .surface_role(SurfaceRole::Raised)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .cursor(Cursor::Pointer)
        .icon_source(source)
        .icon_size(tokens::ICON_SM)
        .icon_stroke_width(2.0)
        .fill(tokens::SECONDARY)
        .stroke(tokens::BORDER)
        .text_color(tokens::SECONDARY_FOREGROUND)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
}

#[track_caller]
pub fn button_with_icon(source: impl IntoIconSource, label: impl Into<String>) -> El {
    El::new(Kind::Custom("button_with_icon"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Solid)
        .metrics_role(MetricsRole::Button)
        .surface_role(SurfaceRole::Raised)
        .focusable()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .cursor(Cursor::Pointer)
        .axis(Axis::Row)
        .default_gap(tokens::SPACE_SM)
        .align(Align::Center)
        .justify(Justify::Center)
        .child(
            icon(source)
                .icon_size(tokens::ICON_SM)
                .color(tokens::SECONDARY_FOREGROUND),
        )
        .child(text(label).label())
        .fill(tokens::SECONDARY)
        .stroke(tokens::BORDER)
        .text_color(tokens::SECONDARY_FOREGROUND)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Hug)
        .default_height(Size::Fixed(tokens::CONTROL_HEIGHT))
        .default_padding(Sides::xy(tokens::SPACE_MD, 0.0))
}
