//! Windowed wgpu fixture for SVG-backed vector icons.
//!
//! Run: `cargo run -p aetna-demo --bin icon_gallery`

use aetna_core::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 880.0, 620.0);
    aetna_demo::run(
        "Aetna — vector icons",
        viewport,
        aetna_fixtures::IconGallery,
    )
}
