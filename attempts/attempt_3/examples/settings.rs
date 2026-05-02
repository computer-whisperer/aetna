//! settings — moderately rich UI fixture.
//!
//! Demonstrates: cards, button variants, badges, status colors, ghost
//! buttons in a save-row pattern. Should look idiomatic to an LLM that
//! has seen shadcn/Tailwind in training.
//!
//! Produces a full agent bundle: SVG, tree dump, render commands, lint.
//! Run: `cargo run -p attempt_3 --example settings`

use attempt_3::*;

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

        row([
            spacer(),
            button("Cancel").ghost(),
            button("Save").primary(),
        ]),
    ])
    .gap(tokens::SPACE_LG)
    .padding(tokens::SPACE_XL)
}

fn main() -> std::io::Result<()> {
    let mut root = settings();

    let viewport = Rect::new(0.0, 0.0, 720.0, 760.0);
    let bundle = render_bundle(&mut root, viewport, Some("attempts/attempt_3/src"));

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "settings")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
