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

## Profiling

Build with `wasm-pack build --target web --features profiling` to route
every `profile_span!` call through `tracing-wasm`. Spans land on the
browser's User Timing API (`performance.measure`); record a profile in
DevTools → Performance and the Aetna spans appear as labeled measures
in the flamegraph alongside the page's frame/script work. Off in
release — the feature gates both the dep and the spans.
