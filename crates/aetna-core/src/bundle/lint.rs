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
//! Provenance: every finding records the source location of the
//! offending node (via `#[track_caller]` propagation up to the user's
//! call site). The lint accepts an optional `app_path_marker` —
//! findings are kept only when the source path contains this marker.
//! The recommended idiom is to pass `Some(env!("CARGO_PKG_NAME"))`
//! so the filter scopes to the calling crate without having to type
//! out a workspace path. Pass `None` to see everything (including
//! anything that fell through `track_caller`).

use std::fmt::Write as _;

use crate::layout;
use crate::state::UiState;
use crate::tree::*;

/// A single lint finding.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Finding {
    pub kind: FindingKind,
    pub node_id: String,
    pub source: Source,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FindingKind {
    RawColor,
    Overflow,
    TextOverflow,
    DuplicateId,
}

#[derive(Clone, Debug, Default)]
#[non_exhaustive]
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

/// Run the lint pass. When `app_path_marker` is `Some(m)`, findings
/// are kept only if the source path of the offending node contains
/// `m`. The recommended idiom is `Some(env!("CARGO_PKG_NAME"))` —
/// `Location::caller()` records workspace-relative paths like
/// `crates/your-app/src/...`, which contain the package name as a
/// directory component. Pass `None` to see every finding (including
/// any that fell through `track_caller` propagation).
pub fn lint(root: &El, ui_state: &UiState, app_path_marker: Option<&str>) -> LintReport {
    let mut r = LintReport::default();
    let mut seen_ids: std::collections::BTreeMap<String, usize> = Default::default();
    walk(root, None, ui_state, &mut r, &mut seen_ids, app_path_marker);
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
    parent_kind: Option<&Kind>,
    ui_state: &UiState,
    r: &mut LintReport,
    seen: &mut std::collections::BTreeMap<String, usize>,
    app_marker: Option<&str>,
) {
    *seen.entry(n.computed_id.clone()).or_default() += 1;
    let computed = ui_state.rect(&n.computed_id);

    let from_user = match app_marker {
        Some(marker) => n.source.file.contains(marker),
        None => true,
    };
    // Children of an Inlines paragraph are encoded into one
    // AttributedText draw op by draw_ops; their individual rects are
    // intentionally zero-size. Skip the per-text overflow + per-child
    // overflow checks for them — the paragraph as a whole holds the
    // rect, so any overflow lint applies at the Inlines node level.
    let inside_inlines = matches!(parent_kind, Some(Kind::Inlines));

    if from_user {
        if let Some(c) = n.fill
            && c.token.is_none()
            && c.a > 0
        {
            r.findings.push(Finding {
                kind: FindingKind::RawColor,
                node_id: n.computed_id.clone(),
                source: n.source,
                message: format!(
                    "fill is a raw rgba({},{},{},{}) — use a token",
                    c.r, c.g, c.b, c.a
                ),
            });
        }
        if let Some(c) = n.stroke
            && c.token.is_none()
            && c.a > 0
        {
            r.findings.push(Finding {
                kind: FindingKind::RawColor,
                node_id: n.computed_id.clone(),
                source: n.source,
                message: format!(
                    "stroke is a raw rgba({},{},{},{}) — use a token",
                    c.r, c.g, c.b, c.a
                ),
            });
        }
        if let Some(c) = n.text_color
            && c.token.is_none()
            && c.a > 0
        {
            r.findings.push(Finding {
                kind: FindingKind::RawColor,
                node_id: n.computed_id.clone(),
                source: n.source,
                message: format!(
                    "text_color is a raw rgba({},{},{},{}) — use a token",
                    c.r, c.g, c.b, c.a
                ),
            });
        }
        if n.text.is_some() && !inside_inlines {
            let available_width = match n.text_wrap {
                TextWrap::NoWrap => None,
                TextWrap::Wrap => Some(computed.w),
            };
            if let Some(text_layout) = layout::text_layout(n, available_width) {
                let text_w = text_layout.width + n.padding.left + n.padding.right;
                let text_h = text_layout.height + n.padding.top + n.padding.bottom;
                let raw_overflow_x = (text_w - computed.w).max(0.0);
                let overflow_x = if matches!(
                    (n.text_wrap, n.text_overflow),
                    (TextWrap::NoWrap, TextOverflow::Ellipsis)
                ) {
                    0.0
                } else {
                    raw_overflow_x
                };
                let overflow_y = (text_h - computed.h).max(0.0);
                if overflow_x > 0.5 || overflow_y > 0.5 {
                    let is_clipped_nowrap = overflow_x > 0.5
                        && matches!(
                            (n.text_wrap, n.text_overflow),
                            (TextWrap::NoWrap, TextOverflow::Clip)
                        );
                    let kind = if is_clipped_nowrap {
                        FindingKind::TextOverflow
                    } else {
                        FindingKind::Overflow
                    };
                    let message = if kind == FindingKind::TextOverflow {
                        format!(
                            "nowrap text exceeds its box by X={overflow_x:.0}; use .ellipsis(), wrap_text(), or a wider box"
                        )
                    } else {
                        format!(
                            "text content exceeds its box by X={overflow_x:.0} Y={overflow_y:.0}; use paragraph()/wrap_text(), a wider box, or explicit clipping"
                        )
                    };
                    r.findings.push(Finding {
                        kind,
                        node_id: n.computed_id.clone(),
                        source: n.source,
                        message,
                    });
                }
            }
        }
    }

    // Overflow: child rect extends past parent. Scrollable parents
    // overflow their content on the main axis by design — that's the
    // whole point — so don't flag children of a scroll viewport.
    // Inlines parents intentionally zero-size their children (the
    // paragraph paints them as one AttributedText), so per-child rect
    // checks would always fire — suppress. The runtime-synthesized
    // toast_stack uses a custom layout that pins cards to the
    // viewport regardless of its own (parent-allocated) rect, so its
    // children naturally extend past the layer's bounds — also
    // suppress.
    let suppress_overflow = n.scrollable
        || matches!(n.kind, Kind::Inlines)
        || matches!(n.kind, Kind::Custom("toast_stack"));
    for c in &n.children {
        let c_rect = ui_state.rect(&c.computed_id);
        if !suppress_overflow && !rect_contains(computed, c_rect, 0.5) {
            let dx_left = (computed.x - c_rect.x).max(0.0);
            let dx_right = (c_rect.right() - computed.right()).max(0.0);
            let dy_top = (computed.y - c_rect.y).max(0.0);
            let dy_bottom = (c_rect.bottom() - computed.bottom()).max(0.0);
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
        walk(c, Some(&n.kind), ui_state, r, seen, app_marker);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lint_one(mut root: El) -> LintReport {
        let mut ui_state = UiState::new();
        layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 160.0, 48.0));
        lint(&root, &ui_state, None)
    }

    #[test]
    fn clipped_nowrap_text_reports_text_overflow() {
        let root = crate::text("A very long dashboard label")
            .width(Size::Fixed(42.0))
            .height(Size::Fixed(20.0));

        let report = lint_one(root);

        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::TextOverflow),
            "{}",
            report.text()
        );
    }

    #[test]
    fn ellipsis_nowrap_text_satisfies_horizontal_overflow_policy() {
        let root = crate::text("A very long dashboard label")
            .ellipsis()
            .width(Size::Fixed(42.0))
            .height(Size::Fixed(20.0));

        let report = lint_one(root);

        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.kind == FindingKind::TextOverflow),
            "{}",
            report.text()
        );
    }
}
