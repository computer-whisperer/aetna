//! Scroll fixture for v0.3 scroll substrate.
//!
//! Demonstrates: a vertical scroll viewport with content taller than
//! its visible rect, content clipped by the viewport, and a non-zero
//! scroll offset reflected in the laid-out tree (children translated up,
//! topmost rows clipped by the scissor).
//!
//! Run: `cargo run -p aetna-core --example scroll_list`

use aetna_core::*;

fn scroll_list_fixture(scroll_y: f32) -> El {
    let rows: Vec<El> = (0..20)
        .map(|i| {
            row([
                badge(format!("#{i}")).info(),
                text(format!("Notification {i}")).bold(),
                spacer(),
                text(format!("{}m ago", i + 1)).muted(),
            ])
            .gap(tokens::SPACE_SM)
            .height(Size::Fixed(44.0))
            .padding(Sides::xy(tokens::SPACE_MD, tokens::SPACE_SM))
        })
        .collect();

    let mut list = scroll(rows)
        .key("notifications")
        .height(Size::Fixed(420.0))
        .padding(tokens::SPACE_SM);
    list.scroll_offset_y = scroll_y;

    column([
        h2("Notifications"),
        text("Roll the wheel inside the panel to scroll. The content is taller than the viewport.")
            .muted(),
        list,
    ])
    .gap(tokens::SPACE_LG)
    .padding(tokens::SPACE_XL)
}

fn main() -> std::io::Result<()> {
    // Scroll part-way down so the artifact actually shows the offset
    // applied — the top rows clip and middle rows fill the viewport.
    let mut root = scroll_list_fixture(220.0);
    let viewport = Rect::new(0.0, 0.0, 720.0, 600.0);
    let bundle = render_bundle(&mut root, viewport, Some("crates/aetna-core/src"));

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "scroll_list")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
