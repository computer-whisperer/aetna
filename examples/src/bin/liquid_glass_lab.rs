//! Windowed liquid-glass material lab.
//!
//! Run: `cargo run -p aetna-examples --bin liquid_glass_lab`

use aetna_core::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    aetna_winit_wgpu::run(
        "Aetna - liquid glass lab",
        Rect::new(0.0, 0.0, 1100.0, 760.0),
        aetna_fixtures::LiquidGlassLab,
    )
}
