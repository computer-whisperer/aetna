//! attempt_3 — an LLM-native UI library, designed agent-loop-first.
//!
//! Build a UI by constructing an [`El`] tree, then hand it to
//! [`render_bundle`] to get every artifact an agent loop needs in one
//! call: SVG fixture, semantic tree dump, render-command IR, lint
//! report. Each artifact is grep-able text — designed for the LLM to
//! reason about symbolically rather than guessing pixel positions.
//!
//! # Quick example
//!
//! ```
//! use attempt_3::*;
//!
//! let mut ui = column([
//!     h1("Hello"),
//!     card("Greeting", [
//!         text("Welcome to attempt_3."),
//!         row([spacer(), button("OK").primary()]),
//!     ]),
//! ]);
//! let bundle = render_bundle(&mut ui, Rect::new(0.0, 0.0, 720.0, 400.0), Some("attempts/attempt_3/src"));
//! assert!(!bundle.svg.is_empty());
//! ```
//!
//! # What's different from attempt_2
//!
//! - **Source mapping is automatic** via `#[track_caller]` on every
//!   constructor. No `src_here!` macro at call sites.
//! - **Style modifiers dispatch on [`StyleProfile`]**, not on
//!   [`Kind`]. Adding a new component is a self-contained file change.
//! - **Tokens are `pub const`s**, not a `OnceLock<Theme>`. No hidden
//!   global to initialize.
//! - **Bundle pipeline** produces the agent's reading material in one
//!   call — visual + semantic + IR + lint.
//! - **Patch API** ([`Patch`], [`patch::apply`]) for surgical edits
//!   without rewriting source files.
//! - **Interaction states** ([`InteractionState`]) carried on every
//!   element; renderer applies hover/press/focus/disabled/loading
//!   visual deltas. Build state-matrix fixtures by setting state on
//!   the element you want to demonstrate.
//!
//! # Pipeline
//!
//! ```text
//! builders → El tree → render_bundle(viewport)
//!                          ├─ layout      (mutates computed rects + ids)
//!                          ├─ render      (RenderCmd IR)
//!                          ├─ inspect     (tree dump text)
//!                          ├─ lint        (with provenance)
//!                          └─ SVG backend
//! ```

pub mod tokens;
pub mod tree;
pub mod style;
pub mod layout;
pub mod render;
pub mod inspect;
pub mod lint;
pub mod patch;
pub mod bundle;

pub mod text;
pub mod button;
pub mod badge;
pub mod card;

// Prelude — for `use attempt_3::*;`.
pub use tree::{
    El, Kind, Color, Size, Sides, Rect, Axis, Align, Justify, FontWeight,
    InteractionState, Source,
    column, row, stack, spacer, divider,
};
pub use style::StyleProfile;
pub use layout::layout;
pub use render::{RenderCmd, TextAnchor, render_commands, render_svg, svg_from_commands};
pub use inspect::dump_tree;
pub use lint::{LintReport, Finding, FindingKind, lint};
pub use patch::{Patch, PatchError, apply, apply_all};
pub use bundle::{Bundle, render_bundle, write_bundle};

pub use text::{text, h1, h2, h3, mono};
pub use button::button;
pub use badge::badge;
pub use card::card;
