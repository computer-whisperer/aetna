//! Runtime color palette — the swappable rgba backing for color tokens.
//!
//! Tokens (e.g. [`crate::tokens::BG_CARD`]) are [`Color`] values carrying
//! both a fallback rgba and a `token: Some("bg-card")` name. The renderer
//! consults the active [`crate::Theme`]'s palette at paint time, looking
//! up that name to pick the rgba for the currently-active palette. Apps
//! swap palettes at runtime by returning a different [`crate::Theme`] from
//! [`crate::event::App::theme`] each frame.
//!
//! Direct token references swap perfectly across palettes.
//! [`Color::with_alpha`] is alpha-only, so it keeps the token name and
//! resolves cleanly (the user's alpha override survives). The rgb-
//! modifying ops [`Color::darken`]/[`Color::lighten`]/[`Color::mix`]
//! strip the token name — once you've derived rgb from a token, swapping
//! the palette would silently discard the derivation, so derived colors
//! opt out of resolution and render exactly as computed. State animations
//! (hover lighten, press darken, focus mix) all flow through this path.
//!
//! Consequence: a derived color computed against the dark palette renders
//! with its dark-derived rgb even when the active palette is light.
//! `theme.rs:apply_role_material` is the one library site that runs an
//! rgb op against a token (`BG_MUTED.darken(0.08)` for the Sunken role);
//! a tighter fix later is to add a `bg-sunken` token to `Palette` so the
//! role can reference a resolvable name. State animations don't need this
//! — they want the per-frame derivation to win, not the palette default.
//!
//! Tokens whose rgba is theme-invariant (e.g. `text-on-solid-dark`) are not
//! members of `Palette` — `lookup` returns `None` for them and the renderer
//! falls back to the baked rgba, which is correct.

use crate::tree::Color;

/// Runtime backing for the design-token color vocabulary.
///
/// One field per theme-variant token. Theme-invariant tokens
/// (`text-on-solid-dark`, `text-on-solid-light`) keep their compile-time
/// rgba and are not represented here.
#[derive(Clone, Debug)]
pub struct Palette {
    // Backgrounds
    pub bg_app: Color,
    pub bg_card: Color,
    pub bg_muted: Color,
    pub bg_raised: Color,
    pub overlay_scrim: Color,

    // Text
    pub text_foreground: Color,
    pub text_muted_foreground: Color,
    pub link_foreground: Color,

    // Borders
    pub border: Color,
    pub border_strong: Color,

    // Status
    pub success: Color,
    pub warning: Color,
    pub destructive: Color,
    pub info: Color,

    // Accents
    pub primary: Color,
    pub primary_hover: Color,

    // Scrollbar
    pub scrollbar_thumb_fill: Color,
    pub scrollbar_thumb_fill_active: Color,

    // State
    pub focus_ring: Color,
    pub selection_bg: Color,
    pub selection_bg_unfocused: Color,
}

impl Palette {
    /// The Aetna Dark palette — historical default, matches the
    /// `#[cfg(not(feature = "light_theme"))]` branch of `tokens.rs`.
    pub const fn aetna_dark() -> Self {
        Self {
            bg_app: Color::token("bg-app", 14, 16, 22, 255),
            bg_card: Color::token("bg-card", 23, 26, 33, 255),
            bg_muted: Color::token("bg-muted", 32, 36, 45, 255),
            bg_raised: Color::token("bg-raised", 41, 47, 58, 255),
            overlay_scrim: Color::token("overlay-scrim", 3, 6, 12, 178),

            text_foreground: Color::token("text-foreground", 232, 238, 246, 255),
            text_muted_foreground: Color::token("text-muted-foreground", 148, 160, 176, 255),
            link_foreground: Color::token("link-foreground", 96, 165, 250, 255),

            border: Color::token("border", 50, 58, 72, 255),
            border_strong: Color::token("border-strong", 80, 96, 118, 255),

            success: Color::token("success", 80, 210, 140, 255),
            warning: Color::token("warning", 245, 190, 85, 255),
            destructive: Color::token("destructive", 245, 95, 110, 255),
            info: Color::token("info", 92, 170, 255, 255),

            primary: Color::token("primary", 92, 170, 255, 255),
            primary_hover: Color::token("primary-hover", 110, 184, 255, 255),

            scrollbar_thumb_fill: Color::token("scrollbar-thumb", 148, 160, 176, 130),
            scrollbar_thumb_fill_active: Color::token(
                "scrollbar-thumb-active",
                200,
                210,
                224,
                220,
            ),

            focus_ring: Color::token("focus-ring", 92, 170, 255, 200),
            selection_bg: Color::token("selection-bg", 92, 170, 255, 96),
            selection_bg_unfocused: Color::token("selection-bg-unfocused", 160, 160, 160, 64),
        }
    }

    /// Replace `c`'s rgb with this palette's value for its token name,
    /// keeping the token name and the input alpha. Colors with no token
    /// name pass through unchanged — that includes raw `Color::rgba`
    /// values *and* the output of rgb-modifying ops (which strip the
    /// token name; see [`Color`] module docs). Colors whose token isn't
    /// a palette member (theme-invariant tokens like `text-on-solid-dark`,
    /// unknown tokens) also pass through unchanged.
    ///
    /// Alpha is taken from the input so [`Color::with_alpha`] overrides
    /// survive resolution.
    pub fn resolve(&self, c: Color) -> Color {
        match c.token.and_then(|name| self.lookup(name)) {
            Some(swap) => Color {
                r: swap.r,
                g: swap.g,
                b: swap.b,
                a: c.a,
                token: c.token,
            },
            None => c,
        }
    }

    /// Resolve a token name to its rgba in this palette. Returns `None`
    /// for theme-invariant tokens (the renderer falls back to the
    /// `Color`'s baked rgba) and for unknown names.
    pub fn lookup(&self, token: &str) -> Option<Color> {
        Some(match token {
            "bg-app" => self.bg_app,
            "bg-card" => self.bg_card,
            "bg-muted" => self.bg_muted,
            "bg-raised" => self.bg_raised,
            "overlay-scrim" => self.overlay_scrim,
            "text-foreground" => self.text_foreground,
            "text-muted-foreground" => self.text_muted_foreground,
            "link-foreground" => self.link_foreground,
            "border" => self.border,
            "border-strong" => self.border_strong,
            "success" => self.success,
            "warning" => self.warning,
            "destructive" => self.destructive,
            "info" => self.info,
            "primary" => self.primary,
            "primary-hover" => self.primary_hover,
            "scrollbar-thumb" => self.scrollbar_thumb_fill,
            "scrollbar-thumb-active" => self.scrollbar_thumb_fill_active,
            "focus-ring" => self.focus_ring,
            "selection-bg" => self.selection_bg,
            "selection-bg-unfocused" => self.selection_bg_unfocused,
            _ => return None,
        })
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::aetna_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_lookup_round_trips() {
        let p = Palette::aetna_dark();
        let bg = p.lookup("bg-app").expect("bg-app present");
        assert_eq!(bg, p.bg_app);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let p = Palette::aetna_dark();
        assert!(p.lookup("not-a-token").is_none());
    }

    #[test]
    fn theme_invariant_tokens_not_in_palette() {
        // text-on-solid-* are intentionally not palette members.
        let p = Palette::aetna_dark();
        assert!(p.lookup("text-on-solid-dark").is_none());
        assert!(p.lookup("text-on-solid-light").is_none());
    }

    #[test]
    fn resolve_passes_through_unrecognized() {
        let p = Palette::aetna_dark();
        let raw = Color::rgba(1, 2, 3, 4);
        assert_eq!(p.resolve(raw), raw);
        let invariant = Color::token("text-on-solid-dark", 8, 16, 25, 255);
        assert_eq!(p.resolve(invariant), invariant);
    }

    #[test]
    fn resolve_preserves_alpha_override() {
        let p = Palette::aetna_dark();
        let translucent = p.bg_card.with_alpha(120);
        let resolved = p.resolve(translucent);
        // rgb tracks the palette's bg-card, alpha tracks the override.
        assert_eq!((resolved.r, resolved.g, resolved.b), (p.bg_card.r, p.bg_card.g, p.bg_card.b));
        assert_eq!(resolved.a, 120);
        assert_eq!(resolved.token, Some("bg-card"));
    }

    #[test]
    fn rgb_ops_strip_token_so_resolve_passes_through() {
        // State animations (hover lighten, press darken) build colors
        // via .darken/.lighten/.mix on token-tagged base colors. Those
        // ops strip the token, so resolve passes them through unchanged
        // and the per-frame derivation wins — which is what we want.
        let p = Palette::aetna_dark();
        let darkened = p.bg_muted.darken(0.5);
        assert_eq!(darkened.token, None);
        let resolved = p.resolve(darkened);
        assert_eq!(resolved, darkened);
    }
}
