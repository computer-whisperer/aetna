# aetna-winit-wgpu

Optional native desktop host for Aetna apps using `winit` and `wgpu`.

Use this crate when you want the host to own the window, surface,
swapchain, input mapping, IME forwarding, animation redraws, and MSAA
target management:

```rust
use aetna_core::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 480.0);
    aetna_winit_wgpu::run("My Aetna App", viewport, MyApp::default())
}
```

For apps with external live state, put per-frame refresh in
`App::before_build`, then use `run_with_config` and
`HostConfig::with_redraw_interval` to choose a host cadence. For custom
render-loop integration, bypass this crate and call `aetna-wgpu::Runner`
directly.
