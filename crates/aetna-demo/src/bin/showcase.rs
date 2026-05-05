//! Showcase — one app exercising every Aetna primitive.
//!
//! Sidebar nav switches between five sections: Counter, List, Palette,
//! Picker, Settings. Each section's state persists across switches.
//!
//! Run: `cargo run -p aetna-demo --bin showcase`
//!
//! See `aetna_fixtures::showcase` for the full module docs.

use aetna_core::Rect;
use aetna_fixtures::Showcase;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    aetna_demo::run("Aetna — showcase", viewport, Showcase::new())
}
