//! Bundled font assets for Aetna.
//!
//! Each font is gated behind a Cargo feature so binary-size-conscious
//! consumers can subset the bundle. The default feature set is generous
//! (Roboto + emoji + symbols) — chosen so that LLM output, which freely
//! reaches for arrows, math operators, dingbats, and box-drawing
//! characters, doesn't render as tofu (`◻`) out of the box.
//!
//! # Feature flags
//!
//! | feature         | adds                                                  | size (raw)   |
//! |-----------------|-------------------------------------------------------|--------------|
//! | `roboto`        | Roboto Regular / Medium / Bold / Italic               | ~1.8 MB      |
//! | `emoji`         | NotoColorEmoji (CBDT color bitmaps)                   | ~10 MB       |
//! | `symbols`       | NotoSansSymbols2 + NotoSansMath                       | ~2.2 MB      |
//! | `cjk` (opt-in)  | NotoSansCJK SC (Simplified Chinese — covers JP/KR)    | ~16 MB       |
//!
//! `default = ["default_fonts"]` and `default_fonts = ["roboto",
//! "emoji", "symbols"]`. `cjk` is **not** in the default set because of
//! its size; downstream UIs that need CJK rendering opt in explicitly:
//!
//! ```toml
//! aetna-fonts = { version = "0.1", features = ["cjk"] }
//! ```
//!
//! To skip the bundled fonts entirely (for example, to ship your own
//! Material Symbols or a brand typeface):
//!
//! ```toml
//! aetna-fonts = { version = "0.1", default-features = false }
//! ```
//!
//! Color emoji (NotoColorEmoji) is rendered through aetna-core's
//! unified RGBA atlas — outline glyphs are stored as
//! `(255, 255, 255, alpha)` and color emoji as native RGBA, so backends
//! sample one texture format and run one shader path.
//!
//! # API
//!
//! Each enabled font exposes a `pub static FOO: &[u8]` constant. The
//! convenience function [`default_fonts`] returns the byte slices for
//! every font that is enabled in the current build, in priority order
//! (sans-serif text first, then symbol/emoji fallbacks, then CJK if
//! opted in). aetna-core's atlas loads them all into its `fontdb` so
//! cosmic-text's per-codepoint fallback can pick from them.

#![no_std]

#[cfg(feature = "roboto")]
pub static ROBOTO_REGULAR: &[u8] = include_bytes!("../fonts/Roboto-Regular.ttf");
#[cfg(feature = "roboto")]
pub static ROBOTO_MEDIUM: &[u8] = include_bytes!("../fonts/Roboto-Medium.ttf");
#[cfg(feature = "roboto")]
pub static ROBOTO_BOLD: &[u8] = include_bytes!("../fonts/Roboto-Bold.ttf");
#[cfg(feature = "roboto")]
pub static ROBOTO_ITALIC: &[u8] = include_bytes!("../fonts/Roboto-Italic.ttf");

#[cfg(feature = "emoji")]
pub static NOTO_COLOR_EMOJI: &[u8] = include_bytes!("../fonts/NotoColorEmoji.ttf");

#[cfg(feature = "symbols")]
pub static NOTO_SANS_SYMBOLS2_REGULAR: &[u8] =
    include_bytes!("../fonts/NotoSansSymbols2-Regular.ttf");
#[cfg(feature = "symbols")]
pub static NOTO_SANS_MATH_REGULAR: &[u8] = include_bytes!("../fonts/NotoSansMath-Regular.ttf");

#[cfg(feature = "cjk")]
pub static NOTO_SANS_CJK_SC_REGULAR: &[u8] = include_bytes!("../fonts/NotoSansCJKsc-Regular.otf");

/// Byte slices for every font enabled in the current build, in priority
/// order: text faces first, then symbol/emoji fallbacks, then CJK.
///
/// aetna-core loads every entry into its `fontdb`. cosmic-text's font
/// matcher then walks the database per codepoint when a primary face
/// lacks a glyph — the order here only documents intent; cosmic-text's
/// fallback is keyed on Unicode coverage, not list position.
pub const DEFAULT_FONTS: &[&[u8]] = &[
    #[cfg(feature = "roboto")]
    ROBOTO_REGULAR,
    #[cfg(feature = "roboto")]
    ROBOTO_MEDIUM,
    #[cfg(feature = "roboto")]
    ROBOTO_BOLD,
    #[cfg(feature = "roboto")]
    ROBOTO_ITALIC,
    #[cfg(feature = "emoji")]
    NOTO_COLOR_EMOJI,
    #[cfg(feature = "symbols")]
    NOTO_SANS_SYMBOLS2_REGULAR,
    #[cfg(feature = "symbols")]
    NOTO_SANS_MATH_REGULAR,
    #[cfg(feature = "cjk")]
    NOTO_SANS_CJK_SC_REGULAR,
];
