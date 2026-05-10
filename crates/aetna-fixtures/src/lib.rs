//! Backend-neutral Aetna fixture apps.
//!
//! This crate intentionally contains only `aetna-core` scene/app code:
//! no winit, no wgpu/vulkano setup, no browser entry point. Backend
//! demo crates and web targets import these fixtures so parity tests
//! exercise the same `App` or `El` across renderers.
//!
//! If you are learning the app-facing API, start with
//! [`showcase::Showcase`]. It demonstrates the normal shape:
//!
//! - import `aetna_core::prelude::*`,
//! - implement `App`,
//! - return controlled widget state from `build`,
//! - route `UiEvent` values in `on_event`,
//! - declare app shaders through `App::shaders`.
//!
//! This crate is not a host. Use `aetna-winit-wgpu` for a native
//! desktop window, or a backend runner such as `aetna-wgpu::Runner` for
//! custom host integration.

pub mod hero;
pub mod icon_gallery;
pub mod liquid_glass_lab;
pub mod showcase;
pub mod text_quality;

pub use hero::HeroDemo;
pub use icon_gallery::{GlassIconGallery, IconGallery, ReliefIconGallery};
pub use liquid_glass_lab::LiquidGlassLab;
pub use showcase::Showcase;
