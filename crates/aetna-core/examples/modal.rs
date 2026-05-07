//! Modal fixture for the overlay substrate.
//!
//! Demonstrates: regular content behind an overlay, keyed dismiss scrim,
//! centered blocking modal panel, and action buttons with normal keys.
//! Run: `cargo run -p aetna-core --example modal`

use aetna_core::prelude::*;

fn modal_fixture() -> El {
    stack([
        column([
            h1("Account"),
            titled_card(
                "Profile",
                [
                    row([text("Email"), spacer(), text("user@example.com").muted()]),
                    row([text("Plan"), spacer(), badge("Pro").info()]),
                ],
            ),
            titled_card(
                "Danger zone",
                [row([
                    column([
                        text("Delete account").bold(),
                        text("Remove this account and all associated data.")
                            .muted()
                            .small(),
                    ])
                    .gap(tokens::SPACE_XS)
                    .align(Align::Start)
                    .width(Size::Hug),
                    spacer(),
                    button("Delete").destructive().key("open-delete"),
                ])],
            ),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL),
        modal(
            "delete-account",
            "Delete account?",
            [
                text("Permanent action. Export data first.").muted(),
                row([
                    spacer(),
                    button("Cancel").ghost().key("cancel-delete"),
                    button("Delete").destructive().key("confirm-delete"),
                ])
                .gap(tokens::SPACE_SM),
            ],
        ),
    ])
}

fn main() -> std::io::Result<()> {
    let mut root = modal_fixture();
    let viewport = Rect::new(0.0, 0.0, 720.0, 560.0);
    let bundle = render_bundle(&mut root, viewport, Some(env!("CARGO_PKG_NAME")));

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "modal")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
