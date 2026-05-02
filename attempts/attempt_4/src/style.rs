//! Style modifier methods on [`El`] — kind-aware via [`StyleProfile`].
//!
//! Each component declares its [`StyleProfile`] in its constructor.
//! Style modifiers (`.primary`, `.success`, `.muted`, etc.) dispatch
//! on the profile, not on `Kind`. That means adding a new component
//! is a self-contained file change: declare a profile, the existing
//! modifier vocabulary just works.
//!
//! Profile semantics:
//!
//! - [`StyleProfile::Solid`] — color modifiers produce solid fills
//!   (Button, Toggle thumb, …).
//! - [`StyleProfile::Tinted`] — color modifiers produce tinted alpha
//!   fills with status-colored text (Badge, highlighted Card, …).
//! - [`StyleProfile::Surface`] — color modifiers tint a subtle bg;
//!   `.muted` swaps to a neutral surface (Card, TextField, Select, …).
//! - [`StyleProfile::TextOnly`] — color modifiers only change text color
//!   (Text, Heading, …).
//!
//! Modifier groups in this file:
//!
//! - **Color/status:** `primary`, `success`, `warning`, `destructive`, `info`
//! - **Surface variants:** `secondary`, `ghost`, `outline`, `muted`
//! - **Text shape:** `bold`, `semibold`, `small`, `xsmall`, `color`

use crate::tokens;
use crate::tree::*;

/// How a component reacts to style/color modifiers.
///
/// Set once in the component's constructor; the modifier methods dispatch
/// on this rather than on [`Kind`], so adding a new component never
/// requires editing this file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StyleProfile {
    Solid,
    Tinted,
    Surface,
    #[default]
    TextOnly,
}

impl El {
    // ===== Color / status (profile-aware) =====

    pub fn primary(self) -> Self { tint(self, tokens::PRIMARY) }
    pub fn success(self) -> Self { tint(self, tokens::SUCCESS) }
    pub fn warning(self) -> Self { tint(self, tokens::WARNING) }
    pub fn destructive(self) -> Self { tint(self, tokens::DESTRUCTIVE) }
    pub fn info(self) -> Self { tint(self, tokens::INFO) }

    // ===== Surface variants =====

    /// Default-styled secondary surface. This is the default look for
    /// `button(...)`; calling `.secondary()` makes intent explicit.
    pub fn secondary(mut self) -> Self {
        self.fill = Some(tokens::BG_MUTED);
        self.stroke = Some(tokens::BORDER);
        self.stroke_width = 1.0;
        self.text_color = Some(tokens::TEXT_FOREGROUND);
        self.font_weight = FontWeight::Medium;
        self
    }

    /// No fill, no border. Low-emphasis actions like "Cancel" alongside
    /// a primary "Save".
    pub fn ghost(mut self) -> Self {
        self.fill = None;
        self.stroke = None;
        self.stroke_width = 0.0;
        self.text_color = Some(tokens::TEXT_MUTED_FOREGROUND);
        self
    }

    /// Outline-only style: no fill, prominent border.
    pub fn outline(mut self) -> Self {
        self.fill = None;
        self.stroke = Some(tokens::BORDER_STRONG);
        self.stroke_width = 1.0;
        self.text_color = Some(tokens::TEXT_FOREGROUND);
        self
    }

    /// Muted/neutral emphasis. On surface profiles this swaps to a
    /// neutral background; on text-only profiles it switches the text
    /// color to muted-foreground.
    pub fn muted(mut self) -> Self {
        match self.style_profile {
            StyleProfile::Solid | StyleProfile::Tinted | StyleProfile::Surface => {
                self.fill = Some(tokens::BG_MUTED);
                self.stroke = Some(tokens::BORDER);
                self.stroke_width = 1.0;
                self.text_color = Some(tokens::TEXT_MUTED_FOREGROUND);
            }
            StyleProfile::TextOnly => {
                self.text_color = Some(tokens::TEXT_MUTED_FOREGROUND);
            }
        }
        self
    }

    // ===== Text shape =====

    pub fn bold(mut self) -> Self { self.font_weight = FontWeight::Bold; self }
    pub fn semibold(mut self) -> Self { self.font_weight = FontWeight::Semibold; self }
    pub fn small(mut self) -> Self { self.font_size = tokens::FONT_SM; self }
    pub fn xsmall(mut self) -> Self { self.font_size = tokens::FONT_XS; self }
    /// Set an explicit text color.
    pub fn color(mut self, c: Color) -> Self { self.text_color = Some(c); self }
}

fn tint(mut el: El, c: Color) -> El {
    match el.style_profile {
        StyleProfile::Solid => {
            el.fill = Some(c);
            el.stroke = Some(c);
            el.stroke_width = 1.0;
            el.text_color = Some(text_on_solid(c));
            el.font_weight = FontWeight::Semibold;
        }
        StyleProfile::Tinted => {
            el.fill = Some(c.with_alpha(38));
            el.stroke = Some(c.with_alpha(120));
            el.stroke_width = 1.0;
            el.text_color = Some(c);
        }
        StyleProfile::Surface => {
            el.fill = Some(c.with_alpha(38));
            el.stroke = Some(c.with_alpha(120));
            el.stroke_width = 1.0;
            el.text_color = Some(c);
        }
        StyleProfile::TextOnly => {
            el.text_color = Some(c);
        }
    }
    el
}

/// Pick a contrasting text color for a solid background fill.
///
/// Rec. 601 luminance threshold tuned so light/saturated fills (accent
/// blue, success green, warning yellow) get dark text, and darker
/// saturated fills (destructive red) get light text.
fn text_on_solid(c: Color) -> Color {
    let lum = 0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32;
    if lum > 150.0 {
        tokens::TEXT_ON_SOLID_DARK
    } else {
        tokens::TEXT_ON_SOLID_LIGHT
    }
}
