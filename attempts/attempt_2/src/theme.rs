//! Theme tokens — shadcn/Tailwind-shaped vocabulary.
//!
//! Naming intentionally shadows shadcn so LLM training transfers verbatim:
//! `bg.card`, `text.muted_foreground`, `border.default`, `radius.md`,
//! `space.lg`, etc.
//!
//! Right now there's a single curated dark theme returned from [`theme`].
//! Adding a light theme is a matter of adding another constructor; the
//! token *shape* stays fixed so user code never has to change.
//!
//! Tokens are exposed as plain values (`f32`, [`Color`]) — no wrapper
//! types. Colors carry their token name as metadata so backends and
//! lint passes can resolve them by name (e.g. CSS variables in HTML).

use std::sync::OnceLock;

use crate::tree::Color;

/// The full theme.
#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Backgrounds,
    pub text: Texts,
    pub border: Borders,
    pub status: Statuses,
    pub accent: Accents,
    pub space: Space,
    pub radius: Radius,
    pub shadow: Shadow,
    pub font: FontSize,
}

#[derive(Clone, Debug)]
pub struct Backgrounds {
    /// App-level background.
    pub app: Color,
    /// Card surface, slightly raised.
    pub card: Color,
    /// Muted surface (input fields, chips, secondary buttons).
    pub muted: Color,
    /// Hover/raised affordance.
    pub raised: Color,
}

#[derive(Clone, Debug)]
pub struct Texts {
    /// Default body text.
    pub foreground: Color,
    /// Secondary, lower-emphasis text.
    pub muted_foreground: Color,
    /// Text on top of `accent.primary` fill.
    pub primary_foreground: Color,
    /// Text on top of `status.destructive` fill.
    pub destructive_foreground: Color,
}

#[derive(Clone, Debug)]
pub struct Borders {
    pub default: Color,
    pub strong: Color,
}

#[derive(Clone, Debug)]
pub struct Statuses {
    pub success: Color,
    pub warning: Color,
    pub destructive: Color,
    pub info: Color,
}

#[derive(Clone, Debug)]
pub struct Accents {
    pub primary: Color,
    pub primary_hover: Color,
}

#[derive(Clone, Debug)]
pub struct Space {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

#[derive(Clone, Debug)]
pub struct Radius {
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub pill: f32,
}

#[derive(Clone, Debug)]
pub struct Shadow {
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
}

#[derive(Clone, Debug)]
pub struct FontSize {
    pub xs: f32,
    pub sm: f32,
    pub base: f32,
    pub lg: f32,
    pub xl: f32,
    pub xxl: f32,
}

impl Theme {
    /// The canonical dark theme.
    pub fn dark() -> Self {
        Self {
            bg: Backgrounds {
                app: Color::token("bg-app", 14, 16, 22, 255),
                card: Color::token("bg-card", 23, 26, 33, 255),
                muted: Color::token("bg-muted", 32, 36, 45, 255),
                raised: Color::token("bg-raised", 41, 47, 58, 255),
            },
            text: Texts {
                foreground: Color::token("text-foreground", 232, 238, 246, 255),
                muted_foreground: Color::token("text-muted-foreground", 148, 160, 176, 255),
                primary_foreground: Color::token("text-primary-foreground", 8, 16, 25, 255),
                destructive_foreground: Color::token("text-destructive-foreground", 30, 5, 8, 255),
            },
            border: Borders {
                default: Color::token("border", 50, 58, 72, 255),
                strong: Color::token("border-strong", 80, 96, 118, 255),
            },
            status: Statuses {
                success: Color::token("success", 80, 210, 140, 255),
                warning: Color::token("warning", 245, 190, 85, 255),
                destructive: Color::token("destructive", 245, 95, 110, 255),
                info: Color::token("info", 92, 170, 255, 255),
            },
            accent: Accents {
                primary: Color::token("primary", 92, 170, 255, 255),
                primary_hover: Color::token("primary-hover", 110, 184, 255, 255),
            },
            space: Space { xs: 4.0, sm: 8.0, md: 12.0, lg: 18.0, xl: 28.0 },
            radius: Radius { sm: 4.0, md: 8.0, lg: 12.0, pill: 999.0 },
            shadow: Shadow { sm: 4.0, md: 12.0, lg: 24.0 },
            font: FontSize { xs: 11.0, sm: 12.0, base: 14.0, lg: 16.0, xl: 20.0, xxl: 26.0 },
        }
    }
}

static THEME: OnceLock<Theme> = OnceLock::new();

/// Access the active theme. Defaults to [`Theme::dark`].
///
/// All component constructors call this implicitly so user code never
/// has to thread `&theme` through every call.
pub fn theme() -> &'static Theme {
    THEME.get_or_init(Theme::dark)
}
