//! Dump bundle artifacts (SVG, tree dump, draw_ops, lint, manifest)
//! for every section of the Showcase fixture. One-shot diagnostic
//! used to validate layout intent — the SVG and tree dump together
//! make layout regressions obvious without needing a window.
//!
//! Usage: `cargo run -p aetna-tools --bin dump_showcase_bundles`
//!
//! Output: `crates/aetna-fixtures/out/showcase_<section>.*` plus
//! named stateful scenes such as open overlay menus.

use std::path::PathBuf;

use aetna_core::prelude::{Rect, render_bundle, write_bundle};
use aetna_core::{App, BuildCx};
use aetna_fixtures::{Showcase, showcase::Section};

fn main() -> std::io::Result<()> {
    // Match the windowed Showcase's viewport so the same layout math
    // runs (the bug, if any, won't depend on the viewport size).
    let viewport = Rect::new(0.0, 0.0, 900.0, 640.0);
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/aetna-fixtures/out");
    let mut finding_count = 0;

    for scene in showcase_bundle_scenes() {
        let mut app = scene.app;
        app.before_build();
        let theme = app.theme();
        let cx = BuildCx::new(&theme);
        let mut tree = app.build(&cx);

        let bundle = render_bundle(&mut tree, viewport);

        let name = scene.name;
        let written = write_bundle(&bundle, &out_dir, &name)?;
        for p in &written {
            println!("wrote {}", p.display());
        }
        if !bundle.lint.findings.is_empty() {
            eprintln!("\n[{name}] lint findings ({}):", bundle.lint.findings.len());
            eprint!("{}", bundle.lint.text());
            finding_count += bundle.lint.findings.len();
        }
    }

    if finding_count > 0 {
        return Err(std::io::Error::other(format!(
            "showcase bundle lint found {finding_count} finding(s)"
        )));
    }

    Ok(())
}

struct ShowcaseBundleScene {
    name: String,
    app: Showcase,
}

fn showcase_bundle_scenes() -> Vec<ShowcaseBundleScene> {
    let mut scenes = Section::ALL
        .into_iter()
        .map(|section| ShowcaseBundleScene {
            name: format!("showcase_{}", section.slug()),
            app: Showcase::with_section(section),
        })
        .collect::<Vec<_>>();

    scenes.push(ShowcaseBundleScene {
        name: "showcase_overlays_dropdown".into(),
        app: Showcase::with_overlay_dropdown_open(),
    });

    scenes.push(ShowcaseBundleScene {
        name: "showcase_overlays_context_menu".into(),
        app: Showcase::with_overlay_context_menu_at(560.0, 450.0),
    });

    scenes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showcase_bundle_scenes_include_open_overlay_menus() {
        let names = showcase_bundle_scenes()
            .into_iter()
            .map(|scene| scene.name)
            .collect::<Vec<_>>();

        assert!(
            names
                .iter()
                .any(|name| name == "showcase_overlays_dropdown")
        );
        assert!(
            names
                .iter()
                .any(|name| name == "showcase_overlays_context_menu")
        );
    }
}
