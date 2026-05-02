//! button_states — state matrix fixture.
//!
//! Renders the same button in default, hover, press, focus, disabled,
//! and loading states so the LLM agent can verify each state's visual
//! treatment in one screenshot rather than imagining them from one
//! "default" rendering.
//!
//! The pattern: build the same component multiple times with different
//! `.with_state(...)` (or shorthand `.hovered()`, `.pressed()`, etc.).
//! No event runtime is needed — state is a render-time visual delta.
//!
//! Run: `cargo run -p attempt_3 --example button_states`

use attempt_3::*;

fn matrix() -> El {
    column([
        h2("Button states"),
        column([
            row_for_label("primary", [
                button("Default").primary(),
                button("Hover").primary().hovered(),
                button("Press").primary().pressed(),
                button("Focus").primary().focused(),
                button("Disabled").primary().disabled(),
                button("Loading").primary().loading(),
            ]),
            row_for_label("secondary", [
                button("Default").secondary(),
                button("Hover").secondary().hovered(),
                button("Press").secondary().pressed(),
                button("Focus").secondary().focused(),
                button("Disabled").secondary().disabled(),
                button("Loading").secondary().loading(),
            ]),
            row_for_label("destructive", [
                button("Default").destructive(),
                button("Hover").destructive().hovered(),
                button("Press").destructive().pressed(),
                button("Focus").destructive().focused(),
                button("Disabled").destructive().disabled(),
                button("Loading").destructive().loading(),
            ]),
            row_for_label("ghost", [
                button("Default").ghost(),
                button("Hover").ghost().hovered(),
                button("Press").ghost().pressed(),
                button("Focus").ghost().focused(),
                button("Disabled").ghost().disabled(),
                button("Loading").ghost().loading(),
            ]),
        ]).gap(tokens::SPACE_LG),
    ])
    .gap(tokens::SPACE_XL)
    .padding(tokens::SPACE_XL)
}

fn row_for_label<I, E>(label: &'static str, buttons: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column([
        text(label).muted().small(),
        row(buttons).gap(tokens::SPACE_MD),
    ])
    .gap(tokens::SPACE_SM)
    .align(Align::Start)
}

fn main() -> std::io::Result<()> {
    let mut root = matrix();
    let viewport = Rect::new(0.0, 0.0, 900.0, 540.0);
    let bundle = render_bundle(&mut root, viewport, Some("attempts/attempt_3/src"));

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "button_states")?;
    for p in &written {
        println!("wrote {}", p.display());
    }
    Ok(())
}
