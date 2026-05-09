//! Dump bundle artifacts (SVG, tree dump, draw_ops, lint, manifest)
//! for every section of the Showcase fixture. One-shot diagnostic
//! used to validate layout intent — the SVG and tree dump together
//! make layout regressions obvious without needing a window.
//!
//! Usage: `cargo run -p aetna-tools --bin dump_showcase_bundles`
//!
//! Output: `crates/aetna-fixtures/out/showcase_<section>.*`.

use std::path::PathBuf;

use aetna_core::prelude::{Rect, render_bundle, write_bundle};
use aetna_core::{App, BuildCx};
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
        Section::Forms,
        Section::Inputs,
        Section::Tabs,
        Section::EditorTabs,
        Section::Split,
        Section::Glass,
        Section::Surfaces,
        Section::Toasts,
        Section::Images,
        Section::Icons,
        Section::Prose,
        Section::Markdown,
    ] {
        let mut app = Showcase::with_section(section);
        app.before_build();
        let theme = app.theme();
        let cx = BuildCx::new(&theme);
        let mut tree = app.build(&cx);

        // Showcase nodes' source paths point into `aetna-fixtures`
        // (where `Showcase::build` lives) — not this `aetna-tools`
        // bin — so we hardcode the marker. For an app where the
        // dump bin and the app code live in the same crate,
        // `Some(env!("CARGO_PKG_NAME"))` is the recommended idiom.
        let bundle = render_bundle(&mut tree, viewport, Some("aetna-fixtures"));

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
        Section::Forms => "forms",
        Section::Inputs => "inputs",
        Section::Tabs => "tabs",
        Section::EditorTabs => "editor-tabs",
        Section::Split => "split",
        Section::Glass => "glass",
        Section::Surfaces => "surfaces",
        Section::Toasts => "toasts",
        Section::Images => "images",
        Section::Icons => "icons",
        Section::Prose => "prose",
        Section::Markdown => "markdown",
    }
}
