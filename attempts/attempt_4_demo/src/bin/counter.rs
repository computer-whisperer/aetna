//! Counter — the v0.2 proof point.
//!
//! Validates the load-bearing claim from `LIBRARY_VISION.md`: a real
//! interactive native app fits in an [`App`] impl with a pure `build`
//! and a plain `&mut self` event handler. Hover and press visuals are
//! applied automatically by the library; the author never writes
//! `.hovered()` or `.pressed()`.
//!
//! Mouse over a button — it lightens. Press it — it darkens. Release —
//! the count updates. That's the entire round-trip:
//!
//! ```text
//! pointer event ─▶ winit ─▶ host runner ─▶ ui.pointer_*()
//!                                              ▼
//!                                         hit-test against
//!                                         last laid-out tree
//!                                              ▼
//! UiEvent ◀── ui.pointer_up() ◀────── click matched
//!     │
//!     ▼
//! app.on_event() ─▶ self.value += 1 ─▶ request_redraw
//!                                              │
//!                                              ▼
//!                                     app.build() returns
//!                                     a fresh tree from
//!                                     the new state
//! ```
//!
//! Run: `cargo run -p attempt_4_demo --bin counter`

use attempt_4::*;

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
            .muted(),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL)
        .align(Align::Center)
    }

    fn on_event(&mut self, event: UiEvent) {
        match (event.kind, event.key.as_deref()) {
            (UiEventKind::Click, Some("inc")) => self.value += 1,
            (UiEventKind::Click, Some("dec")) => self.value -= 1,
            (UiEventKind::Click, Some("reset")) => self.value = 0,
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 480.0, 280.0);
    attempt_4_demo::run("attempt_4 — counter", viewport, Counter { value: 0 })
}
