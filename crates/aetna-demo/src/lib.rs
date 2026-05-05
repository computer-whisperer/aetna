//! aetna-demo — demo binaries and compatibility exports.
//!
//! Reusable fixture `App`s live in `aetna-fixtures`. The native winit
//! + wgpu host lives in `aetna-winit-wgpu`. This crate keeps the
//! historical `aetna_demo::run` and fixture re-exports so existing demo
//! bins stay small while ownership is split into reusable library
//! crates.

pub use aetna_fixtures::{
    GlassIconGallery, IconGallery, LiquidGlassLab, ReliefIconGallery, Showcase,
};
pub use aetna_winit_wgpu::{
    HostConfig, WinitWgpuApp, run, run_host_app, run_host_app_with_config, run_with_config,
};

pub mod icon_gallery {
    pub use aetna_fixtures::icon_gallery::*;
}

pub mod liquid_glass_lab {
    pub use aetna_fixtures::liquid_glass_lab::*;
}

pub mod showcase {
    pub use aetna_fixtures::showcase::*;
}

pub mod text_quality {
    pub use aetna_fixtures::text_quality::*;
}
