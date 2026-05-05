# aetna-web

Browser wasm entry point for the shared Aetna `Showcase` fixture.

This crate is not the general app-author API. For normal UI code, use
`aetna-core::prelude::*`. For a native desktop window, use
`aetna-winit-wgpu`.

`aetna-web` exists to package the browser host path:

- `wasm-pack build --target web`
- a `<canvas id="aetna_canvas">` supplied by the host page
- `winit`'s web event loop
- `aetna-wgpu` rendering through browser WebGPU/WebGL support
- the same `aetna_fixtures::Showcase` app used by native parity demos
