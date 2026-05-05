//! Windowed wgpu fixture for the vector-icon glass material.
//!
//! Run: `cargo run -p aetna-examples --bin icon_gallery_glass`

use aetna_core::Rect;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 840.0, 680.0);
    aetna_winit_wgpu::run(
        "Aetna - vector icon glass",
        viewport,
        aetna_fixtures::GlassIconGallery,
    )
}
