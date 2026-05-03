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
//! v5.3 step 5 (current): Runner owns the rect-shaped pipelines + per-
//! frame instance buffer + descriptor sets. `prepare()` walks
//! `DrawOp::Quad` runs and packs them; `draw()` records draws into the
//! host's primary command-buffer builder. Solid rounded rectangles
//! render through this path; text comes in step 6.

mod instance;
pub mod naga_compile;
mod pipeline;
pub mod runner;

pub use naga_compile::{CompileError, wgsl_to_spirv};
pub use runner::{PrepareResult, PrepareTimings, Runner};
