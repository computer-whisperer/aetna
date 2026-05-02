//! Button component.
//!
//! Default `button("Save")` is the secondary style. Apply variant
//! modifiers from [`crate::style`] to opt into other shadcn-flavored
//! variants:
//!
//! - `.primary()` — filled accent color, semibold text.
//! - `.secondary()` — muted surface (this is also the default look).
//! - `.ghost()` — no fill, no border, muted text.
//! - `.outline()` — outline-only.
//! - `.destructive()` — red fill, destructive text.
//!
//! Buttons hug their text width and have a fixed comfortable height.
//! To stretch a button to fill its container, override with
//! `.width(Size::Fill(1.0))` — the label remains horizontally centered.

use crate::theme::theme;
use crate::tree::*;

/// A button labeled with `label`. Defaults to the secondary variant.
pub fn button(label: impl Into<String>) -> El {
    let t = theme();
    El::new(Kind::Button)
        .text(label)
        .fill(t.bg.muted)
        .stroke(t.border.default)
        .text_color(t.text.foreground)
        .radius(t.radius.md)
        .font_size(t.font.base)
        .font_weight(FontWeight::Medium)
        .width(Size::Hug)
        .height(Size::Fixed(36.0))
        .padding(Sides::xy(t.space.md, 0.0))
}
