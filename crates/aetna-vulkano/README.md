<img src="https://raw.githubusercontent.com/computer-whisperer/aetna/main/assets/aetna_badge_icon.svg" alt="Aetna badge icon" width="96">

# aetna-vulkano

![Showcase — Settings section. The same fixture renders identically through wgpu and vulkano](https://raw.githubusercontent.com/computer-whisperer/aetna/main/assets/showcase_settings.png)

Native Vulkan backend for Aetna using `vulkano`.

Most applications should use `aetna-core` plus `aetna-winit-wgpu`.
Use this crate directly when validating backend parity or writing a
custom Vulkan host.

The public entry point mirrors `aetna-wgpu::Runner` where the GPU API
allows it. A host owns the window, device, queue, swapchain, and event
loop; the runner owns Aetna interaction state, layout/draw-op
preparation, Vulkan pipelines, text atlas images, and icon rendering.

WGSL remains the shader source language. This backend uses `naga` to
compile WGSL to SPIR-V when building pipelines.
