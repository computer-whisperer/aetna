//! Render bundle — one call produces every artifact the agent loop needs.
//!
//! A [`Bundle`] is the textual + visual representation of a rendered tree:
//!
//! - `svg` — visual fixture (also convertible to PNG via tools/svg_to_png.sh).
//! - `tree_dump` — semantic walk of the laid-out tree with rects and source.
//! - `commands` — flat render-command IR (the same one a wgpu/vulkano
//!   backend would consume).
//! - `lint` — findings: raw values in user code, overflows, duplicate IDs.
//!
//! [`render_bundle`] runs layout + render + dump + lint in one call so a
//! single `cargo run --example X` produces everything needed to verify
//! intent without further round-trips.

use std::path::Path;

use crate::inspect;
use crate::layout;
use crate::lint::{LintReport, lint};
use crate::render::{RenderCmd, render_commands, render_svg};
use crate::tree::{El, Rect};

/// Everything an agent loop wants from a single render.
#[derive(Clone, Debug)]
pub struct Bundle {
    /// SVG source.
    pub svg: String,
    /// Semantic tree dump — grep-able, source-mapped.
    pub tree_dump: String,
    /// Flat render-command list — the same IR a GPU backend would consume.
    pub commands: Vec<RenderCmd>,
    /// Findings from the lint pass.
    pub lint: LintReport,
}

/// Lay out, render, dump, and lint the tree.
///
/// `library_marker` filters lint findings whose source path contains
/// the marker — typically `"attempts/attempt_3/src"` to ignore values
/// inside library defaults. Pass `None` to see everything.
pub fn render_bundle(
    root: &mut El,
    viewport: Rect,
    library_marker: Option<&str>,
) -> Bundle {
    layout::layout(root, viewport);
    let svg = render_svg(root, viewport.w, viewport.h);
    let commands = render_commands(root);
    let tree_dump = inspect::dump_tree(root);
    let lint = lint(root, library_marker);
    Bundle { svg, tree_dump, commands, lint }
}

/// Write a bundle to disk under `dir`, naming files `{name}.{ext}`.
///
/// Files written:
/// - `{name}.svg`
/// - `{name}.tree.txt`
/// - `{name}.lint.txt`
/// - `{name}.commands.txt` (debug-formatted — JSON later if useful)
pub fn write_bundle(bundle: &Bundle, dir: &Path, name: &str) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    let svg = dir.join(format!("{name}.svg"));
    std::fs::write(&svg, &bundle.svg)?;
    written.push(svg);

    let tree = dir.join(format!("{name}.tree.txt"));
    std::fs::write(&tree, &bundle.tree_dump)?;
    written.push(tree);

    let lint = dir.join(format!("{name}.lint.txt"));
    std::fs::write(&lint, bundle.lint.text())?;
    written.push(lint);

    let cmds_path = dir.join(format!("{name}.commands.txt"));
    let mut cmds_text = String::new();
    use std::fmt::Write as _;
    for c in &bundle.commands {
        let _ = writeln!(cmds_text, "{c:?}");
    }
    std::fs::write(&cmds_path, cmds_text)?;
    written.push(cmds_path);

    Ok(written)
}
