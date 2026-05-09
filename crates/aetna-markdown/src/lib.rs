//! Markdown → Aetna `El` transformer.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//! use aetna_markdown::md;
//!
//! let tree: El = md("# Hi\n\nHello **world** with [a link](https://aetna.dev).");
//! ```
//!
//! Markdown is defined as a transformation to HTML, and Aetna's widget
//! kit already echoes most of HTML's shape (`text_runs` ≈ `<p>`,
//! `hard_break` ≈ `<br>`, span modifiers ≈ inline tags, `bullet_list`
//! ≈ `<ul>`, `code_block` ≈ `<pre><code>`, …). The transformer walks
//! `pulldown-cmark`'s streaming `Event` API and assembles an `El` tree
//! out of those primitives — a column of blocks an author would have
//! written by hand. The rendered output behaves like any other Aetna
//! tree: themed surfaces, selection, hit-test, layout, lint.
//!
//! Supported today:
//!
//! - Headings `#` … `###` (and h4–h6 clamped to h3).
//! - Paragraphs with inline emphasis, strong, code, link, hard / soft
//!   breaks. Soft breaks render as a space (CommonMark default).
//! - Bulleted (`-` / `*`) and numbered (`1.`) lists, including nested.
//! - Block quotes.
//! - Fenced and indented code blocks.
//! - Horizontal rules.
//! - Inline + block images render as block-level [`aetna_core::image`]
//!   today. Inline-image-in-Inlines is a Phase 2 follow-up.
//!
//! Deferred:
//!
//! - Tables (need a thin wrapper over `widgets::table`).
//! - Footnotes, task lists, raw HTML, math (`$…$` / `$$…$$`).
//! - Syntax highlighting inside code blocks.

mod transformer;

pub use transformer::md;
