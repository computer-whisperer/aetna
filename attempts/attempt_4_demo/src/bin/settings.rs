//! Run attempt_4's `settings` fixture against the real wgpu backend in
//! a winit window.
//!
//! v0.1 paints `stock::rounded_rect` quads only — text, focus rings, and
//! shadow are not yet wired through the wgpu path. You'll see the cards,
//! buttons, and badges as the right shapes/colors but without their
//! labels. That's expected; visual parity with the SVG path comes back
//! in the next slice when `stock::text_sdf` lands.

use attempt_4::*;

fn settings() -> El {
    column([
        h1("Settings"),
        card("Account", [
            row([text("Email"), spacer(), text("user@example.com").muted()]),
            row([text("Two-factor authentication"), spacer(), badge("Enabled").success()]),
            row([text("Recovery codes"), spacer(), button("Generate").secondary()]),
        ]),
        card("Appearance", [
            row([text("Theme"), spacer(), button("Dark").secondary()]),
            row([text("Compact mode"), spacer(), badge("Off").muted()]),
            row([text("Font size"), spacer(), text("14")]),
        ]),
        card("Danger zone", [
            row([
                column([
                    text("Delete account").bold(),
                    text("Permanently remove your account and all data.").muted().small(),
                ])
                .gap(tokens::SPACE_XS)
                .align(Align::Start)
                .width(Size::Hug),
                spacer(),
                button("Delete").destructive(),
            ]),
        ]),
        row([spacer(), button("Cancel").ghost(), button("Save").primary()]),
    ])
    .gap(tokens::SPACE_LG)
    .padding(tokens::SPACE_XL)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 760.0);
    attempt_4_demo::run("attempt_4 — settings", viewport, settings)
}
