//! Text constructors.
//!
//! [`text`] is the default body-text constructor. [`h1`] / [`h2`] / [`h3`]
//! are heading constructors used directly (preferred over chaining
//! `text("X").bold()` since the LLM is more likely to type the dedicated
//! function for a heading).
//!
//! Text-styling modifiers (`.muted()`, `.bold()`, `.semibold()`, `.small()`,
//! `.color(...)`, etc.) are inherent methods on [`El`]; see [`crate::style`]
//! for the full set.

use crate::theme::theme;
use crate::tree::*;

/// Default body text. Hugs its content on both axes.
pub fn text(s: impl Into<String>) -> El {
    let t = theme();
    El::new(Kind::Text)
        .text(s)
        .text_color(t.text.foreground)
        .font_size(t.font.base)
        .hug()
}

/// Top-level page heading.
pub fn h1(s: impl Into<String>) -> El {
    let t = theme();
    El::new(Kind::Heading)
        .text(s)
        .text_color(t.text.foreground)
        .font_size(t.font.xxl)
        .font_weight(FontWeight::Bold)
        .hug()
}

/// Section heading.
pub fn h2(s: impl Into<String>) -> El {
    let t = theme();
    El::new(Kind::Heading)
        .text(s)
        .text_color(t.text.foreground)
        .font_size(t.font.xl)
        .font_weight(FontWeight::Semibold)
        .hug()
}

/// Sub-section heading. Used by [`crate::card::card`] for card titles.
pub fn h3(s: impl Into<String>) -> El {
    let t = theme();
    El::new(Kind::Heading)
        .text(s)
        .text_color(t.text.foreground)
        .font_size(t.font.lg)
        .font_weight(FontWeight::Semibold)
        .hug()
}

/// Monospaced body text. Same size as [`text`], monospace family.
pub fn mono(s: impl Into<String>) -> El {
    text(s).mono()
}
