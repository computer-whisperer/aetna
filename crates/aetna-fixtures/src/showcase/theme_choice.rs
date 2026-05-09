//! User-selectable theme — drives [`crate::Showcase::theme`] so the
//! showcase renders against the chosen palette without rebuilding state.
//!
//! Eight themes total: Aetna's own dark/light pair, the shadcn zinc and
//! neutral pairs, and the Radix slate-blue pair. The picker in the
//! sidebar swaps between these live; every page reflects the choice on
//! the next frame because token lookups resolve through the active
//! palette at paint time.

use aetna_core::prelude::Theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeChoice {
    #[default]
    AetnaDark,
    AetnaLight,
    ShadcnZincDark,
    ShadcnZincLight,
    ShadcnNeutralDark,
    ShadcnNeutralLight,
    RadixSlateBlueDark,
    RadixSlateBlueLight,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 8] = [
        ThemeChoice::AetnaDark,
        ThemeChoice::AetnaLight,
        ThemeChoice::ShadcnZincDark,
        ThemeChoice::ShadcnZincLight,
        ThemeChoice::ShadcnNeutralDark,
        ThemeChoice::ShadcnNeutralLight,
        ThemeChoice::RadixSlateBlueDark,
        ThemeChoice::RadixSlateBlueLight,
    ];

    /// Stable identifier used as the value in routed select keys
    /// (`theme-picker:option:<token>`) and serialized state.
    pub fn token(self) -> &'static str {
        match self {
            ThemeChoice::AetnaDark => "aetna-dark",
            ThemeChoice::AetnaLight => "aetna-light",
            ThemeChoice::ShadcnZincDark => "shadcn-zinc-dark",
            ThemeChoice::ShadcnZincLight => "shadcn-zinc-light",
            ThemeChoice::ShadcnNeutralDark => "shadcn-neutral-dark",
            ThemeChoice::ShadcnNeutralLight => "shadcn-neutral-light",
            ThemeChoice::RadixSlateBlueDark => "radix-slate-blue-dark",
            ThemeChoice::RadixSlateBlueLight => "radix-slate-blue-light",
        }
    }

    /// Human-readable label for the dropdown trigger / menu items.
    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::AetnaDark => "Aetna · dark",
            ThemeChoice::AetnaLight => "Aetna · light",
            ThemeChoice::ShadcnZincDark => "shadcn zinc · dark",
            ThemeChoice::ShadcnZincLight => "shadcn zinc · light",
            ThemeChoice::ShadcnNeutralDark => "shadcn neutral · dark",
            ThemeChoice::ShadcnNeutralLight => "shadcn neutral · light",
            ThemeChoice::RadixSlateBlueDark => "Radix slate · dark",
            ThemeChoice::RadixSlateBlueLight => "Radix slate · light",
        }
    }

    pub fn from_token(s: &str) -> Option<ThemeChoice> {
        ThemeChoice::ALL.iter().copied().find(|c| c.token() == s)
    }

    pub fn theme(self) -> Theme {
        match self {
            ThemeChoice::AetnaDark => Theme::aetna_dark(),
            ThemeChoice::AetnaLight => Theme::aetna_light(),
            ThemeChoice::ShadcnZincDark => Theme::shadcn_zinc_dark(),
            ThemeChoice::ShadcnZincLight => Theme::shadcn_zinc_light(),
            ThemeChoice::ShadcnNeutralDark => Theme::shadcn_neutral_dark(),
            ThemeChoice::ShadcnNeutralLight => Theme::shadcn_neutral_light(),
            ThemeChoice::RadixSlateBlueDark => Theme::radix_slate_blue_dark(),
            ThemeChoice::RadixSlateBlueLight => Theme::radix_slate_blue_light(),
        }
    }
}
