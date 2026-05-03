//! aetna-vulkano — Vulkan backend for Aetna, peer to `aetna-wgpu`.
//!
//! v5.3 introduces this crate as the second GPU backend so the
//! `aetna-core` ↔ backend boundary gets exercised against a wholly
//! separate GPU API. The wgpu side stays unchanged.
//!
//! Shape (per V5_3.md): mirror `aetna-wgpu`'s `Runner` surface, with
//! GPU-typed parameters swapped for `vulkano`'s. WGSL stays the source
//! shader language; `naga` transpiles to SPIR-V at pipeline build time.
//!
//! v5.3 step 6 (current): text rendering is in. `Runner` owns a
//! `TextPaint` that mirrors the core-side `GlyphAtlas` to per-page
//! Vulkan images (R8 UNORM), records glyph quads from `DrawOp::GlyphRun`,
//! and uploads dirty atlas regions via a one-shot command buffer in
//! `prepare()`. `draw()` rebinds the page descriptor set per text run.
//! Counter renders fully with text labels and the live count value.

mod instance;
pub mod naga_compile;
mod pipeline;
pub mod runner;
mod text;

pub use naga_compile::{CompileError, wgsl_to_spirv};
pub use runner::{PrepareResult, PrepareTimings, Runner};
