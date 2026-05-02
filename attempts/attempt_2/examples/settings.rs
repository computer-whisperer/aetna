//! settings — a moderately rich UI fixture exercising the attempt_2 API.
//!
//! Renders to `out/settings.svg`. The intent is that the source of this
//! file should look like the kind of code an LLM produces naturally when
//! asked for a settings page in JSX.

use attempt_2::*;

fn settings() -> El {
    let t = theme();

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
                ]).gap(t.space.xs).align(Align::Start),
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
    .gap(t.space.lg)
    .padding(t.space.xl)
}

fn main() -> std::io::Result<()> {
    let mut root = settings();

    let width = 720.0;
    let height = 760.0;
    layout(&mut root, Rect::new(0.0, 0.0, width, height));

    let svg = render_svg(&root, width, height);
    let out_dir = format!("{}/out", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&out_dir)?;
    let path = format!("{out_dir}/settings.svg");
    std::fs::write(&path, svg)?;
    println!("wrote {path}");
    Ok(())
}
