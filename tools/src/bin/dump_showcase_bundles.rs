//! Dump bundle artifacts (SVG, tree dump, draw_ops, lint, manifest)
//! for every section of the Showcase fixture. One-shot diagnostic
//! used to validate layout intent — the SVG and tree dump together
//! make layout regressions obvious without needing a window.
//!
//! Usage: `cargo run -p aetna-tools --bin dump_showcase_bundles`
//!
//! Output: `crates/aetna-fixtures/out/showcase_<section>.*`.

use std::path::PathBuf;

use aetna_core::App;
use aetna_core::prelude::{Rect, render_bundle, write_bundle};
use aetna_fixtures::{Showcase, showcase::Section};

fn main() -> std::io::Result<()> {
    // Match the windowed Showcase's viewport so the same layout math
    // runs (the bug, if any, won't depend on the viewport size).
    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/aetna-fixtures/out");

    for section in [
        Section::Counter,
        Section::List,
        Section::Palette,
        Section::Picker,
        Section::Settings,
        Section::Glass,
    ] {
        let mut app = Showcase::with_section(section);
        app.before_build();
        let mut tree = app.build();

        let bundle = render_bundle(&mut tree, viewport, Some("crates/aetna-core/src"));

        let name = format!("showcase_{}", section_slug(section));
        let written = write_bundle(&bundle, &out_dir, &name)?;
        for p in &written {
            println!("wrote {}", p.display());
        }
        if !bundle.lint.findings.is_empty() {
            eprintln!("\n[{name}] lint findings ({}):", bundle.lint.findings.len());
            eprint!("{}", bundle.lint.text());
        }
    }
    Ok(())
}

fn section_slug(s: Section) -> &'static str {
    match s {
        Section::Counter => "counter",
        Section::List => "list",
        Section::Palette => "palette",
        Section::Picker => "picker",
        Section::Settings => "settings",
        Section::Glass => "glass",
    }
}
