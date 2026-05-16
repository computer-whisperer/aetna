# Aetna iOS Showcase

This folder tracks the native iOS packaging plan for
`crates/aetna-ios-showcase`. The Rust crate builds as a `staticlib` and
exports `start_winit_app()`, which an Xcode app target should call from
its Objective-C or Swift entry point.

The Rust side is intentionally the same shape as Android:

- `crates/aetna-ios` is the reusable host wrapper.
- `crates/aetna-ios-showcase` is the app-specific entry crate.
- `aetna-winit-wgpu` owns the winit event loop, wgpu surface, device,
  queue, input mapping, and IME visibility.
- The app still owns normal `aetna_core::App` state and rendering
  declarations.

## Build From macOS

Install the iOS Rust target that matches the Xcode destination:

```bash
rustup target add aarch64-apple-ios
rustup target add aarch64-apple-ios-sim
```

Build the Rust static library:

```bash
cargo build -p aetna-ios-showcase --release --target aarch64-apple-ios
```

For the simulator on Apple Silicon:

```bash
cargo build -p aetna-ios-showcase --release --target aarch64-apple-ios-sim
```

Link the resulting archive into an Xcode iOS app target:

```text
target/aarch64-apple-ios/release/libaetna_ios_showcase.a
```

The Xcode target needs to link the Apple frameworks used by winit and
wgpu's Metal backend, typically:

```text
UIKit
Foundation
QuartzCore
Metal
CoreGraphics
CoreFoundation
```

Declare the Rust entry point in a bridging header or Objective-C source:

```c
void start_winit_app(void);
```

Then call `start_winit_app()` once from the app entry path.

## Current Limitations

This is compile and packaging groundwork. The Linux development
environment cannot link or run iOS binaries because it does not have
Xcode's SDKs. Before calling iOS support complete, verify on Apple
hardware or simulator:

- first frame presents through Metal,
- touch down/move/up route to widgets,
- text input shows the soft keyboard and commits text,
- safe area and rotation sizes are sane,
- suspend/resume does not present to a stale surface,
- link opening and clipboard behavior are acceptable.
