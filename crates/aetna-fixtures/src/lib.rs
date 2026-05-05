//! Backend-neutral Aetna fixture apps.
//!
//! This crate intentionally contains only `aetna-core` scene/app code:
//! no winit, no wgpu/vulkano setup, no browser entry point. Backend
//! demo crates and web targets import these fixtures so parity tests
//! exercise the same `App` or `El` across renderers.

pub mod icon_gallery;
pub mod liquid_glass_lab;
pub mod showcase;
pub mod text_quality;

pub use icon_gallery::{GlassIconGallery, IconGallery, ReliefIconGallery};
pub use liquid_glass_lab::LiquidGlassLab;
pub use showcase::Showcase;
