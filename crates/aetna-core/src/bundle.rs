//! Render bundle — one call produces every artifact the agent loop needs.
//!
//! A [`Bundle`] is the textual + visual representation of a rendered tree:
//!
//! - `svg` — visual fixture (also convertible to PNG via `tools/svg_to_png.sh`).
//! - `tree_dump` — semantic walk of the laid-out tree with rects and source.
//! - `draw_ops` — flat draw-op IR (the same one a wgpu backend consumes).
//! - `shader_manifest` — every shader used by this tree, with uniform values.
//! - `lint` — findings: raw values in user code, overflows, duplicate IDs.
//!
//! [`render_bundle`] runs layout + draw-op resolution + dump + lint in one
//! call so a single `cargo run --example X` produces everything needed
//! to verify intent without further round-trips.
//!
//! In v0.1 the SVG output is approximate (stock shaders rendered best-
//! effort, custom shaders as placeholder rects). When the wgpu renderer
//! lands, that becomes the source of truth for visual fidelity; SVG
//! stays as a layout/structure debugging artifact.

use std::path::Path;

use crate::draw_ops;
use crate::inspect;
use crate::ir::DrawOp;
use crate::layout;
use crate::lint::{LintReport, lint};
use crate::manifest;
use crate::state::UiState;
use crate::svg::svg_from_ops;
use crate::tokens;
use crate::tree::{El, Rect};

/// Everything an agent loop wants from a single render.
#[derive(Clone, Debug)]
pub struct Bundle {
    /// SVG source (approximate — see crate-level docs).
    pub svg: String,
    /// Semantic tree dump — grep-able, source-mapped.
    pub tree_dump: String,
    /// Flat draw-op list — the same IR a wgpu backend would consume.
    pub draw_ops: Vec<DrawOp>,
    /// Shader manifest — usage + resolved uniforms per draw.
    pub shader_manifest: String,
    /// Findings from the lint pass.
    pub lint: LintReport,
}

/// Lay out, resolve to draw ops, dump, lint.
///
/// `library_marker` filters lint findings whose source path contains
/// the marker — typically `"crates/aetna-core/src"` to ignore values
/// inside library defaults. Pass `None` to see everything.
///
/// Constructs a fresh [`UiState`] internally — bundle artifacts are a
/// snapshot of the tree at rest, with no hover/press/focus state. For
/// fixtures that need to demonstrate non-trivial state (a scroll
/// position, a hovered button), see [`render_bundle_with`].
pub fn render_bundle(root: &mut El, viewport: Rect, library_marker: Option<&str>) -> Bundle {
    render_bundle_with(root, &mut UiState::new(), viewport, library_marker)
}

/// Same as [`render_bundle`], but threads a caller-built [`UiState`]
/// through the pipeline. Use this when the fixture wants to seed
/// runtime state (scroll offsets, hovered/focused trackers) before
/// snapshotting — the layout pass reads it, and the resulting bundle
/// reflects the seeded state.
///
/// Seed scroll offsets by calling [`crate::layout::assign_ids`] first
/// to populate `computed_id`, then inserting into `ui_state.scroll_offsets`.
pub fn render_bundle_with(
    root: &mut El,
    ui_state: &mut UiState,
    viewport: Rect,
    library_marker: Option<&str>,
) -> Bundle {
    layout::layout(root, ui_state, viewport);
    let draw_ops = draw_ops::draw_ops(root, ui_state);
    let svg = svg_from_ops(viewport.w, viewport.h, &draw_ops, tokens::BG_APP);
    let tree_dump = inspect::dump_tree(root, ui_state);
    let shader_manifest = manifest::shader_manifest(&draw_ops);
    let lint = lint(root, ui_state, library_marker);
    Bundle {
        svg,
        tree_dump,
        draw_ops,
        shader_manifest,
        lint,
    }
}

/// Write a bundle to disk under `dir`, naming files `{name}.{ext}`.
///
/// Files written:
/// - `{name}.svg`
/// - `{name}.tree.txt`
/// - `{name}.draw_ops.txt`
/// - `{name}.shader_manifest.txt`
/// - `{name}.lint.txt`
pub fn write_bundle(
    bundle: &Bundle,
    dir: &Path,
    name: &str,
) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();

    let svg = dir.join(format!("{name}.svg"));
    std::fs::write(&svg, &bundle.svg)?;
    written.push(svg);

    let tree = dir.join(format!("{name}.tree.txt"));
    std::fs::write(&tree, &bundle.tree_dump)?;
    written.push(tree);

    let draw_ops_path = dir.join(format!("{name}.draw_ops.txt"));
    std::fs::write(&draw_ops_path, manifest::draw_ops_text(&bundle.draw_ops))?;
    written.push(draw_ops_path);

    let manifest_path = dir.join(format!("{name}.shader_manifest.txt"));
    std::fs::write(&manifest_path, &bundle.shader_manifest)?;
    written.push(manifest_path);

    let lint = dir.join(format!("{name}.lint.txt"));
    std::fs::write(&lint, bundle.lint.text())?;
    written.push(lint);

    Ok(written)
}
