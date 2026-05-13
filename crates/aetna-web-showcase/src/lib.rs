//! Browser entry point for the Aetna showcase demo.

#[cfg(target_arch = "wasm32")]
mod entry {
    use aetna_fixtures::Showcase;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(start)]
    pub fn start_web_showcase() {
        let _ = aetna_web::start_with(aetna_web::VIEWPORT, Showcase::new());
    }
}
