//! App and widget author prelude.
//!
//! Import this in ordinary Aetna applications:
//!
//! ```
//! use aetna_core::prelude::*;
//! ```
//!
//! The prelude intentionally avoids backend internals such as draw-op
//! packing, glyph atlases, MSDF pages, and runner-core state. Use the
//! explicit modules for those advanced surfaces.

pub use crate::anim::{SpringConfig, Timing, TweenConfig};
pub use crate::bundle::artifact::{
    Bundle, render_bundle, render_bundle_themed, render_bundle_with, render_bundle_with_theme,
    write_bundle,
};
pub use crate::bundle::inspect::dump_tree;
pub use crate::bundle::lint::{Finding, FindingKind, LintReport, lint};
pub use crate::bundle::manifest::{draw_ops_text, shader_manifest};
pub use crate::bundle::svg::svg_from_ops;
pub use crate::event::{
    App, AppShader, KeyChord, KeyModifiers, KeyPress, PointerButton, UiEvent, UiEventKind, UiKey,
    UiTarget,
};
pub use crate::icons::{IntoIconName, all_icon_names, icon};
pub use crate::layout::{LayoutCtx, LayoutFn};
pub use crate::shader::{ShaderBinding, UniformBlock, UniformValue};
pub use crate::style::StyleProfile;
pub use crate::text::metrics::{
    TextHit, TextLayout, TextLine, caret_xy, hit_text, layout_text, line_height, line_width,
    measure_text, selection_rects, wrap_lines,
};
pub use crate::theme::Theme;
pub use crate::tokens;
pub use crate::tree::{
    Align, Axis, Color, El, FontWeight, IconName, InteractionState, Justify, Kind, Rect, Sides,
    Size, Source, SurfaceRole, TextAlign, TextOverflow, TextRole, TextWrap, column, divider,
    hard_break, row, scroll, spacer, stack, text_runs, virtual_list,
};
pub use crate::vector::IconMaterial;
pub use crate::widgets::badge::badge;
pub use crate::widgets::button::{button, button_with_icon, icon_button};
pub use crate::widgets::card::card;
pub use crate::widgets::overlay::{modal, modal_panel, overlay, overlays, scrim};
pub use crate::widgets::popover::{
    Anchor, Side, anchor_rect, context_menu, dropdown, menu_item, popover, popover_panel,
};
pub use crate::widgets::select::{
    self, SelectAction, select_menu, select_option_key, select_trigger,
};
pub use crate::widgets::slider::{self, SliderAction, slider};
pub use crate::widgets::text::{h1, h2, h3, mono, paragraph, text};
pub use crate::widgets::text_area::{self, text_area};
pub use crate::widgets::text_input::{self, ClipboardKind, TextSelection, text_input};
