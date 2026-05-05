# aetna-fixtures

Backend-neutral Aetna fixture apps and render trees.

This crate is useful as source material when learning Aetna's app API:
it contains realistic `aetna_core::App` implementations without any
windowing, GPU setup, or browser glue.

Use it when validating a backend or host:

```rust
use aetna_fixtures::Showcase;

let app = Showcase::new();
```

For normal application code, depend on `aetna-core` and import
`aetna_core::prelude::*`. For a native desktop host, add
`aetna-winit-wgpu`.
