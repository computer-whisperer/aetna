//! Native Vulkan backend for custom Aetna hosts.
//!
//! Most applications should implement `aetna_core::App` and run it
//! through `aetna-winit-wgpu`. Use this crate directly when you are
//! validating backend parity or embedding Aetna into an existing Vulkan
//! renderer built on `vulkano`.
//!
//! The public entry point is [`Runner`]. Its surface mirrors
//! `aetna-wgpu::Runner` where the GPU APIs allow it: the host owns the
//! window, device, queue, swapchain, and event loop; the runner owns
//! Aetna interaction state, layout/draw-op preparation, Vulkan
//! pipelines, text atlas images, and icon rendering.
//!
//! WGSL remains the shader source language. This backend uses [`naga`]
//! to compile WGSL to SPIR-V when building pipelines so custom shader
//! fixtures can be shared with the `wgpu` backend.

mod icon;
mod instance;
pub mod naga_compile;
mod pipeline;
pub mod runner;
mod text;

pub use naga_compile::{CompileError, wgsl_to_spirv};
pub use runner::{PrepareResult, PrepareTimings, Runner};
