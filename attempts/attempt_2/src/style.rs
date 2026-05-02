//! Style modifier methods on [`El`].
//!
//! These are kind-aware fluent methods. The same modifier name produces
//! the right look depending on what it's applied to:
//!
//! - `button("X").primary()` — solid accent fill (shadcn primary button).
//! - `badge("X").primary()` — tinted accent pill (shadcn primary badge).
//! - `text("X").primary()` — accent-colored text.
//!
//! That kind-aware dispatch is what lets the LLM type one familiar
//! modifier (`.primary()`, `.success()`, `.destructive()`) without
//! having to remember which trait owns which method.
//!
//! Implemented as inherent methods on `El` so there's no trait
//! disambiguation to learn. Read this file once and you have the
//! full styling vocabulary.
//!
//! Modifier groups:
//!
//! - **Color/status:** `primary`, `success`, `warning`, `destructive`, `info`
//! - **Surface variants:** `secondary`, `ghost`, `outline`, `muted`
//! - **Text shape:** `bold`, `semibold`, `small`, `xsmall`, `color`

use crate::theme::theme;
use crate::tree::*;

impl El {
    // ===== Color / status (kind-aware) =====
    //
    // On Button: solid fill, picked text color for contrast.
    // On Badge or Card: tinted fill at low alpha + status-colored text.
    // On plain text or other elements: just sets text color.

    pub fn primary(self) -> Self { tint(self, theme().accent.primary) }
    pub fn success(self) -> Self { tint(self, theme().status.success) }
    pub fn warning(self) -> Self { tint(self, theme().status.warning) }
    pub fn destructive(self) -> Self { tint(self, theme().status.destructive) }
    pub fn info(self) -> Self { tint(self, theme().status.info) }

    // ===== Surface variants =====

    /// Default-styled secondary surface (muted bg, default border).
    /// This is the default look for a `button(...)` so calling
    /// `.secondary()` is rarely necessary, but it makes intent explicit.
    pub fn secondary(mut self) -> Self {
        let t = theme();
        self.fill = Some(t.bg.muted);
        self.stroke = Some(t.border.default);
        self.stroke_width = 1.0;
        self.text_color = Some(t.text.foreground);
        self.font_weight = FontWeight::Medium;
        self
    }

    /// No fill, no border. Useful for low-emphasis actions like
    /// "Cancel" alongside a primary "Save".
    pub fn ghost(mut self) -> Self {
        self.fill = None;
        self.stroke = None;
        self.stroke_width = 0.0;
        self.text_color = Some(theme().text.muted_foreground);
        self
    }

    /// Outline-only style: no fill, prominent border.
    pub fn outline(mut self) -> Self {
        let t = theme();
        self.fill = None;
        self.stroke = Some(t.border.strong);
        self.stroke_width = 1.0;
        self.text_color = Some(t.text.foreground);
        self
    }

    /// Apply muted/secondary visual emphasis.
    ///
    /// On surface elements (Button, Badge, Card) this switches to the
    /// muted background with default border. On plain text it switches
    /// to muted-foreground color only.
    pub fn muted(mut self) -> Self {
        let t = theme();
        if matches!(self.kind, Kind::Card | Kind::Button | Kind::Badge) {
            self.fill = Some(t.bg.muted);
            self.stroke = Some(t.border.default);
            self.stroke_width = 1.0;
            self.text_color = Some(t.text.muted_foreground);
        } else {
            self.text_color = Some(t.text.muted_foreground);
        }
        self
    }

    // ===== Text shape =====

    pub fn bold(mut self) -> Self {
        self.font_weight = FontWeight::Bold;
        self
    }

    pub fn semibold(mut self) -> Self {
        self.font_weight = FontWeight::Semibold;
        self
    }

    pub fn small(mut self) -> Self {
        self.font_size = theme().font.sm;
        self
    }

    pub fn xsmall(mut self) -> Self {
        self.font_size = theme().font.xs;
        self
    }

    /// Set an explicit text color (e.g. `.color(theme().status.success)`).
    pub fn color(mut self, c: Color) -> Self {
        self.text_color = Some(c);
        self
    }
}

fn tint(mut el: El, c: Color) -> El {
    match el.kind {
        Kind::Button => {
            el.fill = Some(c);
            el.stroke = Some(c);
            el.stroke_width = 1.0;
            el.text_color = Some(text_on_solid(c));
            el.font_weight = FontWeight::Semibold;
        }
        Kind::Badge | Kind::Card => {
            el.fill = Some(c.with_alpha(38));
            el.stroke = Some(c.with_alpha(120));
            el.stroke_width = 1.0;
            el.text_color = Some(c);
        }
        _ => {
            el.text_color = Some(c);
        }
    }
    el
}

/// Pick a contrasting text color for a solid background fill.
///
/// Uses Rec. 601 luminance with a threshold tuned so light/saturated
/// fills (accent blue, success green, warning yellow) get dark text,
/// and darker saturated fills (destructive red) get light text.
fn text_on_solid(c: Color) -> Color {
    let lum = 0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32;
    if lum > 150.0 {
        Color::token("text-on-solid-dark", 8, 16, 25, 255)
    } else {
        Color::token("text-on-solid-light", 250, 250, 252, 255)
    }
}
