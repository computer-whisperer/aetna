//! Showcase — sixteen pages across six groups, demoing every shadcn-shaped
//! widget and every system-level capability (theme swap, animation,
//! hotkeys, custom shaders, overlays, toasts).
//!
//! Run: `cargo run -p aetna-examples --bin showcase`
//!
//! See `aetna_fixtures::showcase` for the full module docs.

use aetna_core::Rect;
use aetna_fixtures::Showcase;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    aetna_winit_wgpu::run("Aetna — showcase", viewport, Showcase::new())
}
