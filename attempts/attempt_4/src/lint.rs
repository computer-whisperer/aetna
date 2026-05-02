//! Lint pass — surfaces the kind of issues an LLM iterating on a UI
//! benefits from knowing about, with provenance so the report only
//! flags things the user code can fix.
//!
//! Categories:
//!
//! - **Raw colors / sizes:** values that aren't tokenized. Often fine
//!   inside library code but a smell in user code.
//! - **Overflow:** child rects extending past their parent.
//! - **Duplicate IDs:** two nodes with the same computed ID (only
//!   possible via explicit `.key(...)` collisions; pure path IDs are
//!   unique by construction).
//!
//! Provenance: every finding records the source location of the offending
//! node. By default, findings whose source path is inside this crate's
//! `src/` directory are filtered out — those represent library-internal
//! defaults the user can't fix. The crate path filter is configurable.

use std::fmt::Write as _;

use crate::tree::*;

/// A single lint finding.
#[derive(Clone, Debug)]
pub struct Finding {
    pub kind: FindingKind,
    pub node_id: String,
    pub source: Source,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindingKind {
    RawColor,
    Overflow,
    DuplicateId,
}

#[derive(Clone, Debug, Default)]
pub struct LintReport {
    pub findings: Vec<Finding>,
}

impl LintReport {
    pub fn text(&self) -> String {
        if self.findings.is_empty() {
            return "no findings\n".to_string();
        }
        let mut s = String::new();
        for f in &self.findings {
            let _ = writeln!(
                s,
                "{kind:?} node={id} {source} :: {msg}",
                kind = f.kind,
                id = f.node_id,
                source = if f.source.line == 0 {
                    "<no-source>".to_string()
                } else {
                    format!("{}:{}", short_path(f.source.file), f.source.line)
                },
                msg = f.message,
            );
        }
        s
    }
}

/// Run the lint pass. Findings whose source path contains
/// `library_path_marker` are filtered out so the report focuses on
/// user code. Pass `Some("attempts/attempt_4/src")` (or similar) to
/// scrub library-internal raw values; pass `None` to see everything.
pub fn lint(root: &El, library_path_marker: Option<&str>) -> LintReport {
    let mut r = LintReport::default();
    let mut seen_ids: std::collections::BTreeMap<String, usize> = Default::default();
    walk(root, &mut r, &mut seen_ids, library_path_marker);
    for (id, n) in seen_ids {
        if n > 1 {
            r.findings.push(Finding {
                kind: FindingKind::DuplicateId,
                node_id: id.clone(),
                source: Source::default(),
                message: format!("{n} nodes share id {id}"),
            });
        }
    }
    r
}

fn walk(
    n: &El,
    r: &mut LintReport,
    seen: &mut std::collections::BTreeMap<String, usize>,
    lib_marker: Option<&str>,
) {
    *seen.entry(n.computed_id.clone()).or_default() += 1;

    let from_user = match lib_marker {
        Some(marker) => !n.source.file.contains(marker),
        None => true,
    };

    if from_user {
        if let Some(c) = n.fill && c.token.is_none() && c.a > 0 {
            r.findings.push(Finding {
                kind: FindingKind::RawColor,
                node_id: n.computed_id.clone(),
                source: n.source,
                message: format!("fill is a raw rgba({},{},{},{}) — use a token", c.r, c.g, c.b, c.a),
            });
        }
        if let Some(c) = n.stroke && c.token.is_none() && c.a > 0 {
            r.findings.push(Finding {
                kind: FindingKind::RawColor,
                node_id: n.computed_id.clone(),
                source: n.source,
                message: format!("stroke is a raw rgba({},{},{},{}) — use a token", c.r, c.g, c.b, c.a),
            });
        }
        if let Some(c) = n.text_color && c.token.is_none() && c.a > 0 {
            r.findings.push(Finding {
                kind: FindingKind::RawColor,
                node_id: n.computed_id.clone(),
                source: n.source,
                message: format!("text_color is a raw rgba({},{},{},{}) — use a token", c.r, c.g, c.b, c.a),
            });
        }
    }

    // Overflow: child rect extends past parent.
    for c in &n.children {
        if !rect_contains(n.computed, c.computed, 0.5) {
            let dx_left = (n.computed.x - c.computed.x).max(0.0);
            let dx_right = (c.computed.right() - n.computed.right()).max(0.0);
            let dy_top = (n.computed.y - c.computed.y).max(0.0);
            let dy_bottom = (c.computed.bottom() - n.computed.bottom()).max(0.0);
            r.findings.push(Finding {
                kind: FindingKind::Overflow,
                node_id: c.computed_id.clone(),
                source: c.source,
                message: format!(
                    "child overflows parent {parent_id} by L={dx_left:.0} R={dx_right:.0} T={dy_top:.0} B={dy_bottom:.0}",
                    parent_id = n.computed_id,
                ),
            });
        }
        walk(c, r, seen, lib_marker);
    }
}

fn rect_contains(parent: Rect, child: Rect, tol: f32) -> bool {
    child.x >= parent.x - tol
        && child.y >= parent.y - tol
        && child.right() <= parent.right() + tol
        && child.bottom() <= parent.bottom() + tol
}

fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.split(['/', '\\']).collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        p.to_string()
    }
}
