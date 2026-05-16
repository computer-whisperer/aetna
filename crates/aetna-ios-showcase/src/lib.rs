//! iOS entry point for the Aetna showcase demo.

#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn start_winit_app() {
    let viewport = aetna_core::Rect::new(0.0, 0.0, 900.0, 640.0);
    if let Err(err) = aetna_ios::run("Aetna showcase", viewport, aetna_fixtures::Showcase::new()) {
        eprintln!("aetna-ios-showcase: {err}");
    }
}

#[cfg(not(target_os = "ios"))]
pub fn ios_showcase_entry_is_ios_only() {}
