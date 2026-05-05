# aetna-wgpu

`wgpu` backend for Aetna.

Most applications should not start here. Implement `aetna_core::App` and
run it through `aetna-winit-wgpu` for a native window.

Use this crate directly when you are writing a custom host or embedding
Aetna into an existing `wgpu` render loop:

1. Create a `Runner` with the target texture format.
2. Register any app shaders.
3. Forward pointer, keyboard, text-input, modifier, and wheel events to
   the runner.
4. Call `prepare` with a fresh `El` tree before drawing.
5. Call `render` when Aetna owns pass boundaries, especially for
   backdrop-sampling shaders; call `draw` only inside a pass you own and
   only when backdrop sampling is not needed.

Coordinates passed to interaction methods are logical pixels. Render
targets are physical pixels; pass the host scale factor to `prepare`.
