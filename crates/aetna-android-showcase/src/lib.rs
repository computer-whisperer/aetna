//! Android entry point for the Aetna showcase demo.

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: aetna_android::AndroidApp) {
    let viewport = aetna_core::Rect::new(0.0, 0.0, 900.0, 640.0);
    if let Err(err) = aetna_android::run(
        app,
        "Aetna showcase",
        viewport,
        aetna_fixtures::Showcase::new(),
    ) {
        eprintln!("aetna-android-showcase: {err}");
    }
}

#[cfg(not(target_os = "android"))]
pub fn android_showcase_entry_is_android_only() {}
