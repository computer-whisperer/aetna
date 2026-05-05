//! Backend-agnostic UI primitives for Aetna apps.
//!
//! Most applications should start with [`prelude`]:
//!
//! ```
//! use aetna_core::prelude::*;
//! ```
//!
//! The app-facing model is deliberately small:
//!
//! 1. Store your application state in a struct.
//! 2. Implement [`App`] for that struct.
//! 3. Refresh external state in [`App::before_build`] when needed.
//! 4. Return a fresh [`El`] tree from [`App::build`].
//! 5. Update your state from routed [`UiEvent`] values in
//!    [`App::on_event`].
//! 6. Run the app through a host crate such as `aetna-winit-wgpu`, or
//!    integrate a backend runner directly in a custom host.
//!
//! # Quick example
//!
//! ```
//! use aetna_core::prelude::*;
//!
//! struct Counter {
//!     value: i32,
//! }
//!
//! impl App for Counter {
//!     fn build(&self) -> El {
//!         column([
//!             h1(format!("{}", self.value)),
//!             row([
//!                 button("-").key("dec"),
//!                 button("+").key("inc").primary(),
//!             ])
//!             .gap(tokens::SPACE_SM),
//!         ])
//!         .gap(tokens::SPACE_MD)
//!         .padding(tokens::SPACE_LG)
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent) {
//!         if event.is_click_or_activate("inc") {
//!             self.value += 1;
//!         } else if event.is_click_or_activate("dec") {
//!             self.value -= 1;
//!         }
//!     }
//! }
//!
//! let mut ui = Counter { value: 0 }.build();
//! let bundle = render_bundle(&mut ui, Rect::new(0.0, 0.0, 720.0, 400.0), None);
//! assert!(!bundle.svg.is_empty());
//! ```
//!
//! # Running a native window
//!
//! In a desktop app, add `aetna-winit-wgpu` and pass your `App` to its
//! host:
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let viewport = Rect::new(0.0, 0.0, 720.0, 480.0);
//!     aetna_winit_wgpu::run("Counter", viewport, Counter { value: 0 })
//! }
//! ```
//!
//! Use `aetna-wgpu::Runner` directly only when you are writing your own
//! host or embedding Aetna into an existing render loop.
//!
//! # Public API layers
//!
//! - [`prelude`] is the app and widget author surface an LLM should
//!   usually import.
//! - [`widgets`] contains controlled widget builders and their helper
//!   modules, such as `text_input::apply_event` and
//!   `slider::normalized_from_event`.
//! - [`bundle`] is for headless artifacts, tests, and design review.
//! - [`ir`], [`paint`], [`runtime`], text atlas, vector mesh, and MSDF
//!   modules are advanced backend/diagnostic surfaces. They are public
//!   because sibling backend crates use them, but ordinary app code
//!   should not start there.
//!
//! # Rendering pipeline
//!
//! Builders produce an [`El`] tree. Layout writes rects into [`UiState`].
//! The draw-op pass resolves visual facts into backend-neutral [`DrawOp`]
//! values. Backend runners turn those draw ops into GPU resources and
//! route pointer/keyboard input back into [`UiEvent`] values.
//!
//! The stock surface shader is `rounded_rect`; text, icons, custom
//! shaders, and backdrop-sampling materials all flow through the same
//! tree and event model.
//!
//! # Packaged examples
//!
//! The crate ships runnable examples under `examples/`. After adding
//! the crate from crates.io, inspect or run these for focused usage
//! patterns: `settings`, `scroll_list`, `virtual_list`, `inline_runs`,
//! `modal`, `custom_shader`, and `circular_layout`.

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
pub mod prelude;
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
pub use widgets::select::{SelectAction, select_menu, select_option_key, select_trigger};
pub use widgets::slider::slider;
pub use widgets::text::{h1, h2, h3, mono, paragraph, text};
pub use widgets::text_area::text_area;
pub use widgets::text_input::{ClipboardKind, TextSelection, text_input};
