//! Run Aetna's `settings` fixture against the real wgpu backend in
//! a winit window.
//!
//! This is a static fixture — no buttons are keyed and `on_event` is a
//! no-op — so the library's hover/press visuals don't kick in. It exists
//! as a readable parity baseline against `out/settings.wgpu.png`. The
//! counter demo (`bin/counter.rs`) is the interactive proof point.

use aetna_core::prelude::*;

struct Settings;

impl App for Settings {
    fn build(&self, _cx: &BuildCx) -> El {
        column([
            h1("Settings"),
            card(
                "Account",
                [
                    row([text("Email"), text("user@example.com").muted()])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                    row([
                        text("Two-factor authentication"),
                        badge("Enabled").success(),
                    ])
                    .align(Align::Center)
                    .justify(Justify::SpaceBetween),
                    row([text("Recovery codes"), button("Generate").secondary()])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                ],
            ),
            card(
                "Appearance",
                [
                    row([text("Theme"), button("Dark").secondary()])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                    row([text("Compact mode"), badge("Off").muted()])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                    row([text("Font size"), text("14")])
                        .align(Align::Center)
                        .justify(Justify::SpaceBetween),
                ],
            ),
            card(
                "Danger zone",
                [row([
                    column([
                        text("Delete account").bold(),
                        text("Permanently remove your account and all data.")
                            .muted()
                            .small(),
                    ])
                    .gap(tokens::SPACE_XS)
                    .align(Align::Start),
                    button("Delete").destructive(),
                ])
                .align(Align::Center)
                .justify(Justify::SpaceBetween)],
            ),
            row([button("Cancel").ghost(), button("Save").primary()])
                .gap(tokens::SPACE_SM)
                .justify(Justify::End),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 760.0);
    aetna_winit_wgpu::run("Aetna — settings", viewport, Settings)
}
