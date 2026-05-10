//! Polished end-to-end Aetna demo used by the root README hero shot.
//!
//! Run: `cargo run -p aetna-examples --bin hero`

use aetna_core::Rect;
use aetna_fixtures::HeroDemo;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 1360.0, 820.0);
    aetna_winit_wgpu::run("Aetna — hero demo", viewport, HeroDemo)
}
