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
//! v5.3 step 4 (current): GPU-agnostic Runner skeleton in place — input
//! plumbing, layout/animation passes, hit-test wiring, and the public
//! method surface. No vulkano resources are owned yet (step 5 wires up
//! pipelines + buffers; step 6 the text atlas; step 7 the custom-shader
//! pipeline build).

pub mod naga_compile;
pub mod runner;

pub use naga_compile::{CompileError, wgsl_to_spirv};
pub use runner::{PrepareResult, PrepareTimings, Runner};
