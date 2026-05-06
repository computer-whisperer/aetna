//! App and widget author prelude.
//!
//! Import this in ordinary Aetna applications:
//!
//! ```
//! use aetna_core::prelude::*;
//! ```
//!
//! The prelude intentionally avoids backend-implementer surface
//! (`runtime::RunnerCore`, `paint::*`, `text::atlas::*`, vector mesh
//! tessellation) and frame-internal hit-test/focus helpers. Reach for
//! those via their explicit modules when needed.

pub use crate::anim::{AnimProp, AnimValue, Animation, SpringConfig, Timing, TweenConfig};
pub use crate::bundle::artifact::{
    Bundle, render_bundle, render_bundle_themed, render_bundle_with, render_bundle_with_theme,
    write_bundle,
};
pub use crate::bundle::inspect::dump_tree;
pub use crate::bundle::lint::{Finding, FindingKind, LintReport, lint};
pub use crate::bundle::manifest::{draw_ops_text, shader_manifest};
pub use crate::bundle::svg::svg_from_ops;
pub use crate::cursor::Cursor;
pub use crate::event::{
    App, AppShader, KeyChord, KeyModifiers, KeyPress, PointerButton, UiEvent, UiEventKind, UiKey,
    UiTarget,
};
pub use crate::icons::{all_icon_names, icon};
pub use crate::image::{Image, ImageFit};
pub use crate::ir::{DrawOp, TextAnchor};
pub use crate::layout::{LayoutCtx, LayoutFn, VirtualItems};
pub use crate::shader::{ShaderBinding, ShaderHandle, StockShader, UniformBlock, UniformValue};
pub use crate::state::{AnimationMode, WidgetState};
pub use crate::style::StyleProfile;
pub use crate::svg_icon::{IconSource, IntoIconSource, SvgIcon};
pub use crate::text::metrics::{
    MeasuredText, TextHit, TextLayout, TextLine, caret_xy, hit_text, layout_text, line_height,
    line_width, measure_text, selection_rects, wrap_lines,
};
pub use crate::theme::Theme;
pub use crate::toast::{Toast, ToastLevel, ToastSpec};
pub use crate::tokens;
pub use crate::tree::{
    Align, Axis, Color, El, FontWeight, IconName, InteractionState, Justify, Kind, Rect, Sides,
    Size, Source, SurfaceRole, TextAlign, TextOverflow, TextRole, TextWrap, column, divider,
    hard_break, image, row, scroll, spacer, stack, text_runs, virtual_list,
};
pub use crate::vector::IconMaterial;
pub use crate::widgets::badge::badge;
pub use crate::widgets::button::{button, button_with_icon, icon_button};
pub use crate::widgets::card::card;
pub use crate::widgets::checkbox::{self, checkbox};
pub use crate::widgets::overlay::{modal, modal_panel, overlay, overlays, scrim};
pub use crate::widgets::popover::{
    Anchor, Side, anchor_rect, context_menu, dropdown, menu_item, popover, popover_panel,
};
pub use crate::widgets::progress::{self, progress};
pub use crate::widgets::radio::{self, RadioAction, radio_group, radio_item, radio_option_key};
pub use crate::widgets::resize_handle::{self, ResizeDrag, ResizeWeightsDrag, resize_handle};
pub use crate::widgets::select::{
    self, SelectAction, select_menu, select_option_key, select_trigger,
};
pub use crate::widgets::slider::{self, SliderAction, slider};
pub use crate::widgets::switch::{self, switch};
pub use crate::widgets::tabs::{self, TabsAction, tab_option_key, tab_trigger, tabs_list};
pub use crate::widgets::text::{h1, h2, h3, mono, paragraph, text};
pub use crate::widgets::text_area::{self, text_area};
pub use crate::widgets::text_input::{
    self, ClipboardKind, MaskMode, TextInputOpts, TextSelection, text_input, text_input_with,
};

pub use crate::selection::{Selection, SelectionPoint, SelectionRange};
