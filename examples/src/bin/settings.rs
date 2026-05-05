//! Run Aetna's `settings` fixture against the real wgpu backend in
//! a winit window.
//!
//! This is a static fixture — no buttons are keyed and `on_event` is a
//! no-op — so v0.2's hover/press visuals don't kick in. It exists as a
//! readable parity baseline against `out/settings.wgpu.png`. The
//! counter demo (`bin/counter.rs`) is the v0.2 interactive proof point.

use aetna_core::*;

struct Settings;

impl App for Settings {
    fn build(&self) -> El {
        column([
            h1("Settings"),
            card(
                "Account",
                [
                    row([text("Email"), spacer(), text("user@example.com").muted()]),
                    row([
                        text("Two-factor authentication"),
                        spacer(),
                        badge("Enabled").success(),
                    ]),
                    row([
                        text("Recovery codes"),
                        spacer(),
                        button("Generate").secondary(),
                    ]),
                ],
            ),
            card(
                "Appearance",
                [
                    row([text("Theme"), spacer(), button("Dark").secondary()]),
                    row([text("Compact mode"), spacer(), badge("Off").muted()]),
                    row([text("Font size"), spacer(), text("14")]),
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
                    .align(Align::Start)
                    .width(Size::Hug),
                    spacer(),
                    button("Delete").destructive(),
                ])],
            ),
            row([spacer(), button("Cancel").ghost(), button("Save").primary()]),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 760.0);
    aetna_winit_wgpu::run("Aetna — settings", viewport, Settings)
}
