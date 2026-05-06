//! NotoColorEmoji bundled for Aetna.
//!
//! `NOTO_COLOR_EMOJI` exposes the CBDT color-bitmap font that
//! `aetna-fonts` re-exports when the `emoji` feature is enabled.
//! Most consumers should depend on `aetna-fonts` (with the default
//! features) rather than this crate directly.
//!
//! Color rendering needs aetna-core's RGBA atlas path; if you load
//! this directly into a non-aetna `fontdb`, color glyphs render as
//! B&W silhouettes.

#![no_std]

pub static NOTO_COLOR_EMOJI: &[u8] = include_bytes!("../fonts/NotoColorEmoji.ttf");
