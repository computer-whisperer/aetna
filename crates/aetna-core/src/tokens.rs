//! Design tokens — the named, finite vocabulary of values everything else
//! refers to.
//!
//! Tokens are public `const` values, not a runtime struct. That means:
//!
//! - Reading the file once gives you the entire vocabulary.
//! - Components reference tokens by name (`tokens::BG_CARD`) directly,
//!   no theme handle threaded through every call.
//! - Constants are inlined and indistinguishable from any other f32/Color
//!   at runtime — there's no `OnceLock` to initialize.
//!
//! Naming intentionally shadows shadcn/Tailwind so LLM training transfers:
//! `BG_CARD`, `TEXT_MUTED_FOREGROUND`, `RADIUS_LG`, `SPACE_MD`, etc.
//!
//! [`Color`] tokens flow into shader uniforms at render time — the
//! `token` metadata field stays attached so the shader manifest and
//! tree dump can show "fill=bg-card" rather than rgba bytes.

use crate::tree::Color;

// ---- Backgrounds ----
pub const BG_APP: Color = Color::token("bg-app", 14, 16, 22, 255);
pub const BG_CARD: Color = Color::token("bg-card", 23, 26, 33, 255);
pub const BG_MUTED: Color = Color::token("bg-muted", 32, 36, 45, 255);
pub const BG_RAISED: Color = Color::token("bg-raised", 41, 47, 58, 255);
pub const OVERLAY_SCRIM: Color = Color::token("overlay-scrim", 3, 6, 12, 178);

// ---- Text ----
pub const TEXT_FOREGROUND: Color = Color::token("text-foreground", 232, 238, 246, 255);
pub const TEXT_MUTED_FOREGROUND: Color = Color::token("text-muted-foreground", 148, 160, 176, 255);

// ---- Borders ----
pub const BORDER: Color = Color::token("border", 50, 58, 72, 255);
pub const BORDER_STRONG: Color = Color::token("border-strong", 80, 96, 118, 255);

// ---- Status colors ----
pub const SUCCESS: Color = Color::token("success", 80, 210, 140, 255);
pub const WARNING: Color = Color::token("warning", 245, 190, 85, 255);
pub const DESTRUCTIVE: Color = Color::token("destructive", 245, 95, 110, 255);
pub const INFO: Color = Color::token("info", 92, 170, 255, 255);

// ---- Accents ----
pub const PRIMARY: Color = Color::token("primary", 92, 170, 255, 255);
pub const PRIMARY_HOVER: Color = Color::token("primary-hover", 110, 184, 255, 255);

// ---- Solid-foreground (text-on-solid-fill colors) ----
pub const TEXT_ON_SOLID_DARK: Color = Color::token("text-on-solid-dark", 8, 16, 25, 255);
pub const TEXT_ON_SOLID_LIGHT: Color = Color::token("text-on-solid-light", 250, 250, 252, 255);

// ---- Spacing ----
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 18.0;
pub const SPACE_XL: f32 = 28.0;

// ---- Pinned-pane sizing ----
//
// Conventional starting widths for a resizable sidebar (file tree,
// settings nav, inspector). Sourced from VS Code (~240px), Slack
// (~270px), and Finder (~250px) — wide enough that label content
// stays readable, narrow enough that the main work area still
// dominates. `_MIN` is the floor below which file/label content
// truncates badly; `_MAX` is the ceiling above which a sidebar
// stops being a sidebar.
pub const SIDEBAR_WIDTH: f32 = 256.0;
pub const SIDEBAR_WIDTH_MIN: f32 = 180.0;
pub const SIDEBAR_WIDTH_MAX: f32 = 480.0;

// ---- Radius ----
pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;
pub const RADIUS_PILL: f32 = 999.0;

// ---- Scrollbar thumb (overlay indicator on scrollable viewports) ----
/// Visible thumb width when idle. Kept thin so it doesn't crowd
/// content; the hitbox is wider so the thumb still feels grabbable
/// (Fitts's law).
pub const SCROLLBAR_THUMB_WIDTH: f32 = 6.0;
/// Visible thumb width while hovered or being dragged. The thumb
/// expands toward the viewport interior so the cursor sits inside
/// it instead of pinning the right edge.
pub const SCROLLBAR_THUMB_WIDTH_ACTIVE: f32 = 10.0;
/// Track / thumb hitbox width — the column on the right edge that
/// accepts pointer presses for thumb-grab and click-to-page. Always
/// wider than the visible thumb (idle or active) so grabbing a
/// thin idle thumb is still easy. Matches the shadcn ScrollArea
/// track width convention.
pub const SCROLLBAR_HITBOX_WIDTH: f32 = 14.0;
pub const SCROLLBAR_TRACK_INSET: f32 = 2.0;
pub const SCROLLBAR_THUMB_MIN_H: f32 = 24.0;
/// Idle thumb fill — subtle on bg-app/bg-card.
pub const SCROLLBAR_THUMB_FILL: Color = Color::token("scrollbar-thumb", 148, 160, 176, 130);
/// Active (hovered or dragged) thumb fill — fully opaque accent.
pub const SCROLLBAR_THUMB_FILL_ACTIVE: Color =
    Color::token("scrollbar-thumb-active", 200, 210, 224, 220);

// ---- Shadow (passed to renderer as a "level"; backend interprets) ----
pub const SHADOW_SM: f32 = 4.0;
pub const SHADOW_MD: f32 = 12.0;
pub const SHADOW_LG: f32 = 24.0;

// ---- Font sizes ----
pub const FONT_XS: f32 = 11.0;
pub const FONT_SM: f32 = 12.0;
pub const FONT_BASE: f32 = 14.0;
pub const FONT_LG: f32 = 16.0;
pub const FONT_XL: f32 = 20.0;
pub const FONT_XXL: f32 = 26.0;

// ---- State styling ----
//
// Visual deltas applied when an element is in a non-default interaction
// state. Renderer consumes these.

/// How much to darken a fill on press, as a 0..1 factor.
pub const PRESS_DARKEN: f32 = 0.12;
/// How much to lighten a fill on hover, as a 0..1 factor.
pub const HOVER_LIGHTEN: f32 = 0.06;
/// Peak alpha contribution from a fully-eased hover envelope on a
/// surface with no resting fill (`.ghost()`, inactive tab triggers,
/// `.outline()`). Hover/press envelopes only modulate an existing
/// fill, so without a synthesized state-only fill these surfaces show
/// no interaction feedback. Mirrors the shadcn idiom
/// `hover:bg-accent active:bg-accent/80` — transparent at rest, a faint
/// raised surface fades in on interaction.
pub const STATE_FILL_HOVER_ALPHA: f32 = 0.40;
/// Additional peak alpha contribution from a fully-eased press
/// envelope. Sums with [`STATE_FILL_HOVER_ALPHA`] (clamped to 1.0) so
/// a press while hovered reads slightly more committed than hover
/// alone, but still quieter than the active/current treatment.
pub const STATE_FILL_PRESS_ALPHA: f32 = 0.25;
/// Opacity multiplier when an element is disabled.
pub const DISABLED_ALPHA: f32 = 0.5;
/// Focus ring color (typically a tinted accent).
pub const FOCUS_RING: Color = Color::token("focus-ring", 92, 170, 255, 200);
/// Focus ring outset (additional stroke beyond the element bounds).
pub const FOCUS_RING_WIDTH: f32 = 2.0;
/// Background tint for selected text in `text_input` / `text_area`.
/// Tinted accent at low alpha so glyphs stay readable through the
/// selection rectangle.
pub const SELECTION_BG: Color = Color::token("selection-bg", 92, 170, 255, 96);
/// Selection-band fill applied while a text input lacks focus. A
/// neutral, low-saturation cousin of [`SELECTION_BG`]; the painter
/// lerps from this toward `SELECTION_BG` as the input regains focus
/// (see [`crate::tree::El::dim_fill`]). Matches the macOS convention
/// where unfocused selection reads as gray rather than blue.
pub const SELECTION_BG_UNFOCUSED: Color =
    Color::token("selection-bg-unfocused", 160, 160, 160, 64);
