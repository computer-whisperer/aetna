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
//! v5.3 step 2 (current): the WGSL → SPIR-V helper module is in place
//! and unit-tested against both stock shaders. Subsequent steps wire up
//! the Vulkan device/pipelines and the paint-stream loop.

pub mod naga_compile;

pub use naga_compile::{CompileError, wgsl_to_spirv};
