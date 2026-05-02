//! Badge component — a small pill used to convey status.
//!
//! Default style is `info` (accent-tinted). Apply status modifiers from
//! [`crate::style`]: `.success()`, `.warning()`, `.destructive()`,
//! `.info()`, or `.muted()` for a neutral off/disabled look.
//!
//! Each status variant uses a tinted fill (status color at low alpha)
//! plus a status-colored border and text — readable against a card surface.

use crate::theme::theme;
use crate::tree::*;

/// A badge labeled with `label`. Defaults to the `info` variant.
pub fn badge(label: impl Into<String>) -> El {
    let t = theme();
    El::new(Kind::Badge)
        .text(label)
        .font_size(t.font.sm)
        .font_weight(FontWeight::Medium)
        .text_color(t.status.info)
        .fill(t.status.info.with_alpha(38))
        .stroke(t.status.info.with_alpha(120))
        .radius(t.radius.pill)
        .width(Size::Hug)
        .height(Size::Fixed(22.0))
        .padding(Sides::xy(t.space.sm, 0.0))
}
