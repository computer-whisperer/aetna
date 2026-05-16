//! Native iOS host shell for Aetna wgpu apps.
//!
//! iOS apps are packaged by Xcode. Downstream crates usually build a
//! Rust `staticlib` with an exported C ABI function, then call that
//! function from the app's Objective-C or Swift entry point. This crate
//! keeps the Rust side aligned with the desktop and Android host APIs:
//! application code owns `App`, while `aetna-winit-wgpu` owns the
//! window, event loop, surface, device/queue, and input translation.

use aetna_core::{App, Rect};
pub use aetna_winit_wgpu::HostConfig;

/// Run an Aetna app in iOS's UIKit/winit event loop.
///
/// This is the iOS equivalent of `aetna_winit_wgpu::run`: app code
/// owns state/build/events; the host owns the platform event loop,
/// surface, device/queue, and input translation.
#[cfg(target_os = "ios")]
pub fn run<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    run_with_config(title, viewport, app, HostConfig::default())
}

/// Run an Aetna app on iOS with explicit host configuration.
#[cfg(target_os = "ios")]
pub fn run_with_config<A: App + 'static>(
    title: &'static str,
    viewport: Rect,
    app: A,
    config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    aetna_winit_wgpu::run_with_config(title, viewport, app, config)
}

/// Non-iOS builds can type-check crates that depend on `aetna-ios`, but
/// cannot start a UIKit application.
#[cfg(not(target_os = "ios"))]
pub fn run<A: App + 'static>(
    _title: &'static str,
    _viewport: Rect,
    _app: A,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("aetna-ios can only run on target_os = \"ios\"".into())
}

/// Non-iOS builds can type-check crates that depend on `aetna-ios`, but
/// cannot start a UIKit application.
#[cfg(not(target_os = "ios"))]
pub fn run_with_config<A: App + 'static>(
    _title: &'static str,
    _viewport: Rect,
    _app: A,
    _config: HostConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    Err("aetna-ios can only run on target_os = \"ios\"".into())
}
