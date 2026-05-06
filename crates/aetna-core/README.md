# aetna-core

![Aetna showcase — Settings section, headless wgpu render](../../assets/showcase_settings.png)

Backend-agnostic UI primitives for Aetna apps.

Start here for application code:

```rust
use aetna_core::prelude::*;

struct Counter {
    value: i32,
}

impl App for Counter {
    fn build(&self) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("-").key("dec"),
                button("+").key("inc").primary(),
            ])
            .gap(tokens::SPACE_SM),
        ])
        .gap(tokens::SPACE_MD)
        .padding(tokens::SPACE_LG)
    }

    fn on_event(&mut self, event: UiEvent) {
        if event.is_click_or_activate("inc") {
            self.value += 1;
        } else if event.is_click_or_activate("dec") {
            self.value -= 1;
        }
    }
}
```

Use `aetna-winit-wgpu` to open a native desktop window. Use
`aetna-wgpu::Runner` directly only when writing a custom host or
embedding Aetna in an existing render loop.

If the UI mirrors external state, refresh it in `App::before_build`.
Hosts call that hook immediately before each `build`.

The app-author surface is `aetna_core::prelude`. Backend and diagnostic
APIs live in explicit modules such as `ir`, `paint`, `runtime`, `bundle`,
`text`, and `vector`.
