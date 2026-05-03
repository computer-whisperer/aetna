//! attempt_4 — shader-first LLM-native UI library.
//!
//! See `SHADER_VISION.md` next to this crate's manifest for the design
//! intent and the v0.1 scope.
//!
//! # Quick example
//!
//! ```
//! use attempt_4::*;
//!
//! let mut ui = column([
//!     h1("Hello"),
//!     card("Greeting", [
//!         text("Welcome to attempt_4."),
//!         row([spacer(), button("OK").primary()]),
//!     ]),
//! ]);
//! let bundle = render_bundle(&mut ui, Rect::new(0.0, 0.0, 720.0, 400.0), Some("attempts/attempt_4/src"));
//! assert!(!bundle.svg.is_empty());
//! ```
//!
//! # What's different from attempt_3
//!
//! - **Draw-op IR** ([`DrawOp`]) replaces `RenderCmd::Rect/Text`. Every
//!   visual fact resolves to a `Quad` or `GlyphRun` bound to a
//!   [`ShaderHandle`] and a [`UniformBlock`].
//! - **Stock shaders** — the surface paint goes through
//!   `stock::rounded_rect` (handles fill+stroke+radius+shadow as
//!   uniforms); text through `stock::text_sdf`; focus rings through
//!   `stock::focus_ring`. Discipline: uniform proliferation, not shader
//!   proliferation.
//! - **Custom shader override** ([`El::shader_override`]) — a user crate
//!   can bind its own shader for the surface paint, replacing the
//!   implicit `rounded_rect`. v0.1 ships no custom shaders, but the
//!   substrate supports them.
//! - **Bundle artifacts** add `draw_ops.txt` and `shader_manifest.txt`.
//! - **`Justify::Center` / `Justify::End` fixed** (regression test in
//!   `layout::tests`).
//!
//! # Pipeline
//!
//! ```text
//! builders → El tree → render_bundle(viewport)
//!                          ├─ layout      (mutates computed rects + ids)
//!                          ├─ draw_ops    (resolve to DrawOp IR)
//!                          ├─ inspect     (tree dump text)
//!                          ├─ manifest    (shader manifest + draw-op text)
//!                          ├─ lint        (with provenance)
//!                          └─ svg         (approximate fallback)
//! ```

pub mod tokens;
pub mod tree;
pub mod style;
pub mod layout;
pub mod shader;
pub mod ir;
pub mod draw_ops;
pub mod event;
pub mod svg;
pub mod inspect;
pub mod lint;
pub mod manifest;
pub mod bundle;

pub mod text;
pub mod button;
pub mod badge;
pub mod card;
pub mod overlay;

pub mod wgpu_render;
pub use wgpu_render::UiRenderer;

// Prelude — for `use attempt_4::*;`.
pub use tree::{
    El, Kind, Color, Size, Sides, Rect, Axis, Align, Justify, FontWeight,
    InteractionState, Source, TextAlign, TextWrap,
    column, row, scroll, stack, spacer, divider,
};
pub use style::StyleProfile;
pub use layout::layout;
pub use shader::{ShaderHandle, StockShader, ShaderBinding, UniformBlock, UniformValue};
pub use ir::{DrawOp, TextAnchor};
pub use draw_ops::draw_ops;
pub use event::{
    App, KeyChord, KeyModifiers, KeyPress, UiEvent, UiEventKind, UiKey, UiTarget, focus_order,
    hit_test, hit_test_target,
};
pub use svg::svg_from_ops;
pub use inspect::dump_tree;
pub use lint::{LintReport, Finding, FindingKind, lint};
pub use manifest::{shader_manifest, draw_ops_text};
pub use bundle::{Bundle, render_bundle, write_bundle};

pub use text::{text, paragraph, h1, h2, h3, mono};
pub use button::button;
pub use badge::badge;
pub use card::card;
pub use overlay::{modal, modal_panel, overlay, scrim};
