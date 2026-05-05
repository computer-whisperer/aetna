//! Showcase — the shared `aetna-fixtures::Showcase` app routed through
//! the vulkano backend. v5.4's broader-coverage A/B
//! fixture: every Aetna primitive (sidebar nav, scroll, animation,
//! hotkeys, cards) must produce visually-equivalent output through
//! `aetna-vulkano` as it does through `aetna-wgpu`.
//!
//! Run: `cargo run -p aetna-vulkano-demo --bin showcase`

use aetna_core::Rect;
use aetna_fixtures::Showcase;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    aetna_vulkano_demo::run("Aetna — showcase (vulkano)", viewport, Showcase::new())
}
