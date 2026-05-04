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
//! The runner owns text and vector-icon painters alongside the
//! rect/custom-shader path. Text mirrors the core-side `GlyphAtlas` to
//! per-page Vulkan images; icons use the shared SVG/vector mesh and
//! stock icon-material shaders.

mod icon;
mod instance;
pub mod naga_compile;
mod pipeline;
pub mod runner;
mod text;

pub use naga_compile::{CompileError, wgsl_to_spirv};
pub use runner::{PrepareResult, PrepareTimings, Runner};
