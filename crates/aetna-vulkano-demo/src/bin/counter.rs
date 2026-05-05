//! Counter — v5.3 step 5 acceptance fixture.
//!
//! Same App impl as `examples/src/bin/counter.rs` (the v0.2 wgpu
//! proof point), driven through `aetna-vulkano` instead. Side-by-side
//! with the wgpu version this is the visual A/B test for whether the
//! `aetna-core` ↔ backend boundary actually holds across two GPU APIs.
//!
//! Step 5 only renders rect-shaped surfaces — text comes in step 6 —
//! so the buttons appear as solid rounded rectangles without their
//! "−" / "Reset" / "+" labels and the count number isn't visible.
//! Hover, press, focus, and clicks still work end-to-end.
//!
//! Duplicated rather than `pub use`-imported so this backend acceptance
//! fixture remains self-contained.

use aetna_core::*;

struct Counter {
    value: i32,
}

impl App for Counter {
    fn build(&self) -> El {
        column([
            h1(format!("{}", self.value)),
            row([
                button("−").key("dec").secondary(),
                button("Reset").key("reset").ghost(),
                button("+").key("inc").primary(),
            ])
            .gap(tokens::SPACE_MD),
            text(if self.value == 0 {
                "Click + or − to change the count.".to_string()
            } else {
                format!("You have clicked +/− a net {} times.", self.value)
            })
            .center_text()
            .muted(),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL)
        .align(Align::Center)
    }

    fn on_event(&mut self, event: UiEvent) {
        match (event.kind, event.key.as_deref()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("inc")) => self.value += 1,
            (UiEventKind::Click | UiEventKind::Activate, Some("dec")) => self.value -= 1,
            (UiEventKind::Click | UiEventKind::Activate, Some("reset")) => self.value = 0,
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 480.0, 280.0);
    aetna_vulkano_demo::run("Aetna — counter (vulkano)", viewport, Counter { value: 0 })
}
