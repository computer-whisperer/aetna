//! Windowed Vulkano fixture for the vector-icon relief material.
//!
//! Run: `cargo run -p aetna-vulkano-demo --bin icon_gallery_relief`

use aetna_core::Rect;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 840.0, 680.0);
    aetna_vulkano_demo::run(
        "Aetna — vector icon relief (vulkano)",
        viewport,
        aetna_fixtures::ReliefIconGallery,
    )
}
