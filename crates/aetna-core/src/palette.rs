//! Runtime color palette — the swappable rgba backing for color tokens.
//!
//! Tokens (e.g. [`crate::tokens::CARD`]) are [`Color`] values carrying
//! both a fallback rgba and a `token: Some("card")` name. The renderer
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
//! rgb op against a token (`MUTED.darken(0.08)` for the Sunken role);
//! a tighter fix later is to add an input/container token to `Palette` so the
//! role can reference a resolvable name. State animations don't need this
//! — they want the per-frame derivation to win, not the palette default.
//!
//! The core vocabulary is intentionally close to shadcn/ui: background /
//! foreground pairs for surfaces and actions, plus border, input, ring,
//! and semantic status roles. Link, scrollbar, overlay, and selection
//! tokens are component/domain extensions.

use crate::tree::Color;

/// Runtime backing for the design-token color vocabulary.
///
/// One field per theme-variant token.
#[derive(Clone, Debug)]
pub struct Palette {
    // Core shadcn-shaped semantic colors.
    pub background: Color,
    pub foreground: Color,

    pub card: Color,
    pub card_foreground: Color,

    pub popover: Color,
    pub popover_foreground: Color,

    pub primary: Color,
    pub primary_foreground: Color,

    pub secondary: Color,
    pub secondary_foreground: Color,

    pub muted: Color,
    pub muted_foreground: Color,

    pub accent: Color,
    pub accent_foreground: Color,

    pub destructive: Color,
    pub destructive_foreground: Color,

    pub border: Color,
    pub input: Color,
    pub ring: Color,

    pub success: Color,
    pub success_foreground: Color,
    pub warning: Color,
    pub warning_foreground: Color,
    pub info: Color,
    pub info_foreground: Color,

    // Extensions.
    pub overlay_scrim: Color,
    pub link_foreground: Color,

    pub scrollbar_thumb_fill: Color,
    pub scrollbar_thumb_fill_active: Color,

    pub selection_bg: Color,
    pub selection_bg_unfocused: Color,
}

impl Palette {
    /// The Aetna Dark palette — historical default. These rgba values
    /// also serve as the compile-time fallback baked into the constants
    /// in [`crate::tokens`].
    pub const fn aetna_dark() -> Self {
        Self {
            background: Color::token("background", 14, 16, 22, 255),
            foreground: Color::token("foreground", 232, 238, 246, 255),

            card: Color::token("card", 23, 26, 33, 255),
            card_foreground: Color::token("card-foreground", 232, 238, 246, 255),

            popover: Color::token("popover", 23, 26, 33, 255),
            popover_foreground: Color::token("popover-foreground", 232, 238, 246, 255),

            primary: Color::token("primary", 92, 170, 255, 255),
            primary_foreground: Color::token("primary-foreground", 8, 16, 25, 255),

            secondary: Color::token("secondary", 32, 36, 45, 255),
            secondary_foreground: Color::token("secondary-foreground", 232, 238, 246, 255),

            muted: Color::token("muted", 32, 36, 45, 255),
            muted_foreground: Color::token("muted-foreground", 148, 160, 176, 255),

            accent: Color::token("accent", 41, 47, 58, 255),
            accent_foreground: Color::token("accent-foreground", 232, 238, 246, 255),

            destructive: Color::token("destructive", 245, 95, 110, 255),
            destructive_foreground: Color::token("destructive-foreground", 8, 16, 25, 255),

            border: Color::token("border", 50, 58, 72, 255),
            input: Color::token("input", 50, 58, 72, 255),
            ring: Color::token("ring", 92, 170, 255, 200),

            success: Color::token("success", 80, 210, 140, 255),
            success_foreground: Color::token("success-foreground", 8, 16, 25, 255),
            warning: Color::token("warning", 245, 190, 85, 255),
            warning_foreground: Color::token("warning-foreground", 8, 16, 25, 255),
            info: Color::token("info", 92, 170, 255, 255),
            info_foreground: Color::token("info-foreground", 8, 16, 25, 255),

            overlay_scrim: Color::token("overlay-scrim", 3, 6, 12, 178),
            link_foreground: Color::token("link-foreground", 96, 165, 250, 255),

            scrollbar_thumb_fill: Color::token("scrollbar-thumb", 148, 160, 176, 130),
            scrollbar_thumb_fill_active: Color::token("scrollbar-thumb-active", 200, 210, 224, 220),

            selection_bg: Color::token("selection-bg", 92, 170, 255, 96),
            selection_bg_unfocused: Color::token("selection-bg-unfocused", 160, 160, 160, 64),
        }
    }

    /// The Aetna Light palette — tuned to mirror shadcn's light baseline.
    /// Token names, alphas, and downstream role assignments match
    /// [`Self::aetna_dark`]; only the literal rgb values shift.
    pub const fn aetna_light() -> Self {
        Self {
            background: Color::token("background", 247, 248, 251, 255),
            foreground: Color::token("foreground", 19, 24, 33, 255),

            card: Color::token("card", 255, 255, 255, 255),
            card_foreground: Color::token("card-foreground", 19, 24, 33, 255),

            popover: Color::token("popover", 255, 255, 255, 255),
            popover_foreground: Color::token("popover-foreground", 19, 24, 33, 255),

            primary: Color::token("primary", 37, 99, 235, 255),
            primary_foreground: Color::token("primary-foreground", 250, 250, 252, 255),

            secondary: Color::token("secondary", 240, 242, 247, 255),
            secondary_foreground: Color::token("secondary-foreground", 19, 24, 33, 255),

            muted: Color::token("muted", 240, 242, 247, 255),
            muted_foreground: Color::token("muted-foreground", 96, 110, 130, 255),

            accent: Color::token("accent", 255, 255, 255, 255),
            accent_foreground: Color::token("accent-foreground", 19, 24, 33, 255),

            destructive: Color::token("destructive", 220, 38, 38, 255),
            destructive_foreground: Color::token("destructive-foreground", 250, 250, 252, 255),

            border: Color::token("border", 220, 224, 232, 255),
            input: Color::token("input", 220, 224, 232, 255),
            ring: Color::token("ring", 37, 99, 235, 200),

            success: Color::token("success", 22, 163, 74, 255),
            success_foreground: Color::token("success-foreground", 250, 250, 252, 255),
            warning: Color::token("warning", 217, 119, 6, 255),
            warning_foreground: Color::token("warning-foreground", 250, 250, 252, 255),
            info: Color::token("info", 37, 99, 235, 255),
            info_foreground: Color::token("info-foreground", 250, 250, 252, 255),

            overlay_scrim: Color::token("overlay-scrim", 12, 18, 32, 110),
            link_foreground: Color::token("link-foreground", 37, 99, 235, 255),

            scrollbar_thumb_fill: Color::token("scrollbar-thumb", 100, 116, 139, 90),
            scrollbar_thumb_fill_active: Color::token("scrollbar-thumb-active", 71, 85, 105, 220),

            selection_bg: Color::token("selection-bg", 37, 99, 235, 64),
            selection_bg_unfocused: Color::token("selection-bg-unfocused", 100, 116, 139, 56),
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
            "background" => self.background,
            "foreground" => self.foreground,
            "card" => self.card,
            "card-foreground" => self.card_foreground,
            "popover" => self.popover,
            "popover-foreground" => self.popover_foreground,
            "primary" => self.primary,
            "primary-foreground" => self.primary_foreground,
            "secondary" => self.secondary,
            "secondary-foreground" => self.secondary_foreground,
            "muted" => self.muted,
            "muted-foreground" => self.muted_foreground,
            "accent" => self.accent,
            "accent-foreground" => self.accent_foreground,
            "destructive" => self.destructive,
            "destructive-foreground" => self.destructive_foreground,
            "border" => self.border,
            "input" => self.input,
            "ring" => self.ring,
            "success" => self.success,
            "success-foreground" => self.success_foreground,
            "warning" => self.warning,
            "warning-foreground" => self.warning_foreground,
            "info" => self.info,
            "info-foreground" => self.info_foreground,
            "overlay-scrim" => self.overlay_scrim,
            "link-foreground" => self.link_foreground,
            "scrollbar-thumb" => self.scrollbar_thumb_fill,
            "scrollbar-thumb-active" => self.scrollbar_thumb_fill_active,
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
        let bg = p.lookup("background").expect("background present");
        assert_eq!(bg, p.background);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        let p = Palette::aetna_dark();
        assert!(p.lookup("not-a-token").is_none());
    }

    #[test]
    fn removed_legacy_tokens_not_in_palette() {
        let p = Palette::aetna_dark();
        assert!(p.lookup("bg-app").is_none());
        assert!(p.lookup("bg-card").is_none());
        assert!(p.lookup("bg-muted").is_none());
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
        let translucent = p.card.with_alpha(120);
        let resolved = p.resolve(translucent);
        // rgb tracks the palette's card, alpha tracks the override.
        assert_eq!(
            (resolved.r, resolved.g, resolved.b),
            (p.card.r, p.card.g, p.card.b)
        );
        assert_eq!(resolved.a, 120);
        assert_eq!(resolved.token, Some("card"));
    }

    #[test]
    fn aetna_light_differs_from_aetna_dark() {
        let dark = Palette::aetna_dark();
        let light = Palette::aetna_light();
        // background is one of the tokens that visibly inverts.
        assert_ne!(
            (dark.background.r, dark.background.g, dark.background.b),
            (light.background.r, light.background.g, light.background.b),
        );
        // Text foreground also inverts.
        assert_ne!(
            (dark.foreground.r, dark.foreground.g, dark.foreground.b),
            (light.foreground.r, light.foreground.g, light.foreground.b),
        );
        // Token names match — same vocabulary, different rgb.
        assert_eq!(dark.background.token, light.background.token);
    }

    #[test]
    fn resolve_against_light_swaps_rgb() {
        let light = Palette::aetna_light();
        // A token-tagged color authored against dark values resolves to
        // the light palette's rgb.
        let dark_card = Color::token("card", 23, 26, 33, 255);
        let resolved = light.resolve(dark_card);
        assert_eq!(
            (resolved.r, resolved.g, resolved.b),
            (light.card.r, light.card.g, light.card.b),
        );
    }

    #[test]
    fn rgb_ops_strip_token_so_resolve_passes_through() {
        // State animations (hover lighten, press darken) build colors
        // via .darken/.lighten/.mix on token-tagged base colors. Those
        // ops strip the token, so resolve passes them through unchanged
        // and the per-frame derivation wins — which is what we want.
        let p = Palette::aetna_dark();
        let darkened = p.muted.darken(0.5);
        assert_eq!(darkened.token, None);
        let resolved = p.resolve(darkened);
        assert_eq!(resolved, darkened);
    }
}
