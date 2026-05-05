//! aetna-core — backend-agnostic UI library core.
//!
//! See `SHADER_VISION.md` and `LIBRARY_VISION.md` at the repo root for
//! the design intent. This crate is the verbatim port of v0.4
//! (`attempts/attempt_4/src/`) into the canonical Aetna layout.
//! Subsequent v5.0 commits split modules and move library bookkeeping
//! off `El` into `UiState` side maps.
//!
//! # Quick example
//!
//! ```
//! use aetna_core::*;
//!
//! let mut ui = column([
//!     h1("Hello"),
//!     card("Greeting", [
//!         text("Welcome to Aetna."),
//!         row([spacer(), button("OK").primary()]),
//!     ]),
//! ]);
//! let bundle = render_bundle(&mut ui, Rect::new(0.0, 0.0, 720.0, 400.0), Some("crates/aetna-core/src"));
//! assert!(!bundle.svg.is_empty());
//! ```
//!
//! # What's different from attempt_3
//!
//! - **Draw-op IR** ([`DrawOp`]) replaces `RenderCmd::Rect/Text`. Every
//!   visual fact resolves to a `Quad` or `GlyphRun` bound to a
//!   [`ShaderHandle`] and a [`UniformBlock`].
//! - **Stock shaders** — the surface paint goes through
//!   `stock::rounded_rect` (handles fill+stroke+radius+shadow plus the
//!   focus ring as uniforms); text through `stock::text_sdf`.
//!   Discipline: uniform proliferation, not shader proliferation.
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

pub mod anim;
pub mod bundle;
pub mod draw_ops;
pub mod event;
pub mod focus;
pub mod hit_test;
pub mod icon_msdf;
pub mod icon_msdf_atlas;
pub mod icons;
pub mod ir;
pub mod layout;
pub mod paint;
pub mod runtime;
pub mod shader;
pub mod state;
pub mod style;
pub mod text;
pub mod theme;
pub mod tokens;
pub mod tree;
pub mod vector;
pub mod widgets;

// Prelude — for `use aetna_core::*;`.
pub use anim::{AnimProp, AnimValue, Animation, SpringConfig, Timing, TweenConfig};
pub use bundle::artifact::{
    Bundle, render_bundle, render_bundle_themed, render_bundle_with, render_bundle_with_theme,
    write_bundle,
};
pub use bundle::inspect::dump_tree;
pub use bundle::lint::{Finding, FindingKind, LintReport, lint};
pub use bundle::manifest::{draw_ops_text, shader_manifest};
pub use bundle::svg::svg_from_ops;
pub use draw_ops::{draw_ops, draw_ops_with_theme};
pub use event::{
    App, AppShader, KeyChord, KeyModifiers, KeyPress, PointerButton, UiEvent, UiEventKind, UiKey,
    UiTarget,
};
pub use focus::focus_order;
pub use hit_test::{hit_test, hit_test_target};
pub use icons::{
    IconStroke, IntoIconName, all_icon_names, icon, icon_path, icon_strokes, icon_vector_asset,
};
pub use ir::{DrawOp, TextAnchor};
pub use layout::{LayoutCtx, LayoutFn, VirtualItems, layout};
pub use shader::{ShaderBinding, ShaderHandle, StockShader, UniformBlock, UniformValue};
pub use state::{AnimationMode, UiState, WidgetState};
pub use style::StyleProfile;
pub use text::atlas::{
    AtlasPage, AtlasRect, GlyphAtlas, GlyphKey, GlyphSlot, RunStyle, ShapedGlyph, ShapedRun,
};
pub use text::metrics::{
    MeasuredText, TextHit, TextLayout, TextLine, caret_xy, hit_text, layout_text, line_height,
    line_width, measure_text, selection_rects, wrap_lines,
};
pub use theme::Theme;
pub use tree::{
    Align, Axis, Color, El, FontWeight, IconName, InteractionState, Justify, Kind, Rect, Sides,
    Size, Source, SurfaceRole, TextAlign, TextOverflow, TextRole, TextWrap, column, divider,
    hard_break, row, scroll, spacer, stack, text_runs, virtual_list,
};
pub use vector::{
    IconMaterial, VectorAsset, VectorColor, VectorFill, VectorFillRule, VectorLineCap,
    VectorLineJoin, VectorMesh, VectorMeshOptions, VectorMeshRun, VectorMeshVertex,
    VectorParseError, VectorPath, VectorSegment, VectorStroke, append_vector_asset_mesh,
    parse_svg_asset, tessellate_vector_asset,
};

pub use widgets::badge::badge;
pub use widgets::button::{button, button_with_icon, icon_button};
pub use widgets::card::card;
pub use widgets::overlay::{modal, modal_panel, overlay, scrim};
pub use widgets::popover::{
    Anchor, Side, anchor_rect, context_menu, dropdown, menu_item, popover, popover_panel,
};
pub use widgets::select::{select_menu, select_option_key, select_trigger};
pub use widgets::slider::slider;
pub use widgets::text::{h1, h2, h3, mono, paragraph, text};
pub use widgets::text_area::text_area;
pub use widgets::text_input::{ClipboardKind, TextSelection, text_input};
