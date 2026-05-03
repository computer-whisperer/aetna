//! Native shim for the shared `aetna-web` Counter app.
//!
//! Mirrors the role of `whisper-agent-desktop`'s `main.rs` — a thin
//! native wrapper that imports the shared library crate and hands off
//! to the platform's runner. Same `Counter` runs in the browser via
//! the `wasm-pack` build of `aetna-web`'s `cdylib`; this bin proves
//! the shared `App` impl is target-portable.
//!
//! Usage: `cargo run -p aetna-web --bin aetna-counter-native`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    aetna_web::launch_native()
}
