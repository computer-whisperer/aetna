//! Slider keyboard — the controlled `slider::apply_event` helper.
//!
//! A focused [`slider`] receives `KeyDown` events; passing them to
//! [`slider::apply_event`] folds them back into a normalized value
//! using the standard ARIA range pattern: `ArrowUp` / `ArrowRight`
//! step up by `step`, `ArrowDown` / `ArrowLeft` step down, `PageUp`
//! / `PageDown` adjust by `page_step`, `Home` / `End` jump to the
//! ends.
//!
//! Run: `cargo run -p aetna-examples --bin slider_keyboard`
//!
//! Things to try:
//! - Press `Tab` to focus the slider, then arrow keys to move.
//! - Drag the thumb with the pointer — the same `value` field
//!   updates, so keyboard and pointer stay in sync.
//! - Hold `Shift` is not used here; it's `PageUp` / `PageDown` for
//!   the coarse step.

use aetna_core::prelude::*;
use aetna_core::widgets::slider;

struct VolumeDemo {
    value: f32,
}

impl App for VolumeDemo {
    fn build(&self) -> El {
        column([
            h2("Slider keyboard demo"),
            text(format!("{:.0}%", self.value * 100.0))
                .muted()
                .center_text(),
            slider(self.value, tokens::PRIMARY)
                .key("vol")
                .width(Size::Fixed(280.0)),
            text("Tab to focus · Arrows step 5% · PgUp/Dn 25% · Home/End jump")
                .muted()
                .center_text()
                .font_size(tokens::FONT_SM),
        ])
        .gap(tokens::SPACE_MD)
        .padding(tokens::SPACE_XL)
        .align(Align::Center)
        .justify(Justify::Center)
    }

    fn on_event(&mut self, event: UiEvent) {
        // Pointer drag: the existing `normalized_from_event` helper.
        if matches!(
            event.kind,
            UiEventKind::PointerDown | UiEventKind::Drag | UiEventKind::Click
        ) && event.route() == Some("vol")
            && let (Some(rect), Some(x)) = (event.target_rect(), event.pointer_x())
        {
            self.value = slider::normalized_from_event(rect, x);
            return;
        }
        // Keyboard: the new `apply_event` helper.
        slider::apply_event(&mut self.value, &event, "vol", 0.05, 0.25);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 480.0, 280.0);
    aetna_winit_wgpu::run(
        "Aetna — slider keyboard",
        viewport,
        VolumeDemo { value: 0.5 },
    )
}
