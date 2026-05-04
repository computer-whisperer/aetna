//! Windowed wgpu fixture for the vector-icon relief material.
//!
//! Run: `cargo run -p aetna-demo --bin icon_gallery_relief`

use aetna_core::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 880.0, 620.0);
    aetna_demo::run_with_icon_material(
        "Aetna — vector icon relief",
        viewport,
        aetna_demo::IconGallery,
        VectorIconMaterial::Relief,
    )
}
