//! attempt_2 — an LLM-native UI library.
//!
//! Build a UI by constructing an [`El`] tree, then hand it to a backend.
//! The tree is the contract: layout is a pass over it, render commands
//! are the handoff to backends, and a backend can be any of SVG, HTML,
//! native GPU, or game-engine HUD.
//!
//! # Quick example
//!
//! ```
//! use attempt_2::*;
//!
//! let ui = column([
//!     h1("Hello"),
//!     card("Greeting", [
//!         text("Welcome to the demo."),
//!         row([spacer(), button("OK").primary()]),
//!     ]),
//! ]);
//! ```
//!
//! # Pipeline
//!
//! ```text
//! builders → El tree → layout(viewport) → RenderCmd → backend (SVG, HTML, wgpu, ...)
//! ```
//!
//! Read [`tree`] first to understand the data structure. Then [`theme`]
//! for the token vocabulary and [`style`] for the chainable styling
//! modifiers. The component modules ([`button`], [`card`], etc.) are
//! short and self-contained — read one and you know the pattern for
//! all of them.

pub mod theme;
pub mod tree;
pub mod style;
pub mod layout;
pub mod render;

pub mod text;
pub mod button;
pub mod badge;
pub mod card;

// Prelude — re-exports for `use attempt_2::*;`.
pub use theme::{Theme, theme};
pub use tree::{
    El, Kind, Color, Size, Sides, Rect, Axis, Align, Justify, FontWeight,
    column, row, stack, spacer, divider,
};
pub use layout::layout;
pub use render::{RenderCmd, render_commands, render_svg, svg_from_commands};

pub use text::{text, h1, h2, h3, mono};
pub use button::button;
pub use badge::badge;
pub use card::card;
