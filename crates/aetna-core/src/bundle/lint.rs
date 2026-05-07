//! Lint pass — surfaces the kind of issues an LLM iterating on a UI
//! benefits from knowing about, with provenance so the report only
//! flags things the user code can fix.
//!
//! Categories:
//!
//! - **Raw colors / sizes:** values that aren't tokenized. Often fine
//!   inside library code but a smell in user code.
//! - **Overflow:** child rects extending past their parent, or text
//!   exceeding its container's padded content region (centered text
//!   that spills past the padding reads as visually off-center, even
//!   when it nominally fits inside the outer rect).
//! - **Duplicate IDs:** two nodes with the same computed ID (only
//!   possible via explicit `.key(...)` collisions; pure path IDs are
//!   unique by construction).
//!
//! Provenance: every finding records the source location of the
//! offending node (via `#[track_caller]` propagation up to the user's
//! call site). The lint accepts an optional `app_path_marker` —
//! findings are kept only when there's a user-source ancestor with a
//! source path containing this marker. The recommended idiom is to
//! pass `Some(env!("CARGO_PKG_NAME"))` so the filter scopes to the
//! calling crate without having to type out a workspace path. Pass
//! `None` to see everything (including anything that fell through
//! `track_caller`).
//!
//! Overflow findings (rect and text) walk up to the nearest
//! user-source ancestor for attribution. `#[track_caller]` doesn't
//! propagate through closures, so a widget like `tabs_list` that
//! builds children inside `.map(...)` records widget-internal source
//! on those children. The lint detects the issue at the leaf but
//! blames the user's call site, since that's where the fix lives
//! (the user supplied the offending content). Raw-color findings are
//! still self-attributed — those are intentional inside widgets and
//! should only fire from user code directly.

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
/// are kept only if the offending node has a user-source ancestor
/// (or is itself user-source) whose source path contains `m`. The
/// recommended idiom is `Some(env!("CARGO_PKG_NAME"))` —
/// `Location::caller()` records workspace-relative paths like
/// `crates/your-app/src/...`, which contain the package name as a
/// directory component. Pass `None` to see every finding (including
/// any that fell through `track_caller` propagation).
pub fn lint(root: &El, ui_state: &UiState, app_path_marker: Option<&str>) -> LintReport {
    let mut r = LintReport::default();
    let mut seen_ids: std::collections::BTreeMap<String, usize> = Default::default();
    walk(
        root,
        None,
        None,
        ui_state,
        &mut r,
        &mut seen_ids,
        app_path_marker,
    );
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

fn is_from_user(source: Source, app_marker: Option<&str>) -> bool {
    match app_marker {
        Some(marker) => source.file.contains(marker),
        None => true,
    }
}

fn walk(
    n: &El,
    parent_kind: Option<&Kind>,
    parent_blame: Option<Source>,
    ui_state: &UiState,
    r: &mut LintReport,
    seen: &mut std::collections::BTreeMap<String, usize>,
    app_marker: Option<&str>,
) {
    *seen.entry(n.computed_id.clone()).or_default() += 1;
    let computed = ui_state.rect(&n.computed_id);

    let from_user_self = is_from_user(n.source, app_marker);
    // Nearest user-source location attributable to this node — itself
    // when self is from user code, otherwise the closest ancestor's
    // user source. Used by overflow findings so widget-composed leaves
    // (e.g. `tab_trigger` built inside `tabs_list`'s `.map(...)`
    // closure, where `Location::caller()` resolves inside aetna-core)
    // still blame the user code that supplied the offending content.
    let self_blame = if from_user_self {
        Some(n.source)
    } else {
        parent_blame
    };

    // Children of an Inlines paragraph are encoded into one
    // AttributedText draw op by draw_ops; their individual rects are
    // intentionally zero-size. Skip the per-text overflow + per-child
    // overflow checks for them — the paragraph as a whole holds the
    // rect, so any overflow lint applies at the Inlines node level.
    let inside_inlines = matches!(parent_kind, Some(Kind::Inlines));

    // Raw colors are intentional inside library widgets; only flag
    // them when the node is itself in user code.
    if from_user_self {
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
    }

    // Text overflow: detect at the node itself (with the node's own
    // padding-aware content region — text_w includes padding so the
    // check fires when the text exceeds the padded content area, not
    // just the bare rect). Attribute to the nearest user-source
    // ancestor so closure-built widget leaves still blame user code.
    if n.text.is_some()
        && !inside_inlines
        && let Some(blame) = self_blame
    {
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
                    source: blame,
                    message,
                });
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
        let from_user_child = is_from_user(c.source, app_marker);
        let child_blame = if from_user_child {
            Some(c.source)
        } else {
            self_blame
        };

        let c_rect = ui_state.rect(&c.computed_id);
        if !suppress_overflow
            && !rect_contains(computed, c_rect, 0.5)
            && let Some(blame) = child_blame
        {
            let dx_left = (computed.x - c_rect.x).max(0.0);
            let dx_right = (c_rect.right() - computed.right()).max(0.0);
            let dy_top = (computed.y - c_rect.y).max(0.0);
            let dy_bottom = (c_rect.bottom() - computed.bottom()).max(0.0);
            r.findings.push(Finding {
                kind: FindingKind::Overflow,
                node_id: c.computed_id.clone(),
                source: blame,
                message: format!(
                    "child overflows parent {parent_id} by L={dx_left:.0} R={dx_right:.0} T={dy_top:.0} B={dy_bottom:.0}",
                    parent_id = n.computed_id,
                ),
            });
        }
        walk(c, Some(&n.kind), child_blame, ui_state, r, seen, app_marker);
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

    #[test]
    fn padding_aware_text_overflow_fires_when_text_spills_past_padded_region() {
        // Box is wide enough for the bare text (66 ≤ 80) but padding
        // eats so much that the text spills past the padded content
        // area (66 > 80 - 40). Centered text in this state visually
        // reads as off-center — the lint must flag it even though the
        // text would technically fit inside the outer rect.
        //
        // Wrap in a row so the inner Fixed(80) is honored; the layout
        // pass forces the root rect to the viewport regardless of its
        // own size, so a single-node test would mis-measure.
        let leaf = crate::text("dashboard")
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(28.0))
            .padding(Sides::xy(20.0, 0.0));
        let root = crate::row([leaf]);

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
    fn overflow_findings_attribute_to_nearest_user_source_ancestor() {
        // Simulate the closure-built-widget pattern: a root in user
        // code whose child is constructed with an aetna-internal
        // source (mimicking what `tabs_list` does when it builds tab
        // triggers inside `.map(|...| tab_trigger(...))` — the closure
        // boundary breaks `#[track_caller]` so the trigger's recorded
        // source points inside aetna-core, not at the user's call).
        let user_source = Source {
            file: "crates/test_app/src/screen.rs",
            line: 42,
        };
        let widget_source = Source {
            file: "crates/aetna-core/src/widgets/tabs.rs",
            line: 200,
        };

        let mut leaf = crate::text("A very long dashboard label")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0));
        leaf.source = widget_source;

        let mut root = crate::row([leaf])
            .width(Size::Fixed(160.0))
            .height(Size::Fixed(48.0));
        root.source = user_source;

        let mut ui_state = UiState::new();
        layout::layout(&mut root, &mut ui_state, Rect::new(0.0, 0.0, 160.0, 48.0));
        let report = lint(&root, &ui_state, Some("crates/test_app/src"));

        let text_overflow = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::TextOverflow)
            .unwrap_or_else(|| panic!("expected TextOverflow finding\n{}", report.text()));
        // Detection happens at the leaf, attribution walks up to the
        // user-source ancestor.
        assert_eq!(text_overflow.source.file, user_source.file);
        assert_eq!(text_overflow.source.line, user_source.line);
    }

    #[test]
    fn overflow_finding_self_attributes_when_node_is_already_user_source() {
        // Sanity check: when the offending node itself is in user
        // code, the finding still points at it (no spurious walk to
        // an ancestor with a different line number).
        let mut node = crate::text("A very long dashboard label")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0));
        let user_source = Source {
            file: "crates/test_app/src/screen.rs",
            line: 99,
        };
        node.source = user_source;

        let mut ui_state = UiState::new();
        layout::layout(&mut node, &mut ui_state, Rect::new(0.0, 0.0, 160.0, 48.0));
        let report = lint(&node, &ui_state, Some("crates/test_app/src"));

        let text_overflow = report
            .findings
            .iter()
            .find(|f| f.kind == FindingKind::TextOverflow)
            .unwrap_or_else(|| panic!("expected TextOverflow finding\n{}", report.text()));
        assert_eq!(text_overflow.source.line, user_source.line);
    }

    #[test]
    fn overflow_finding_suppressed_when_no_user_ancestor_exists() {
        // Without any user-source ancestor in the chain, the lint
        // marker filter is restored: there's nothing the user can
        // act on, so the finding is silently dropped.
        let widget_source = Source {
            file: "crates/aetna-core/src/widgets/tabs.rs",
            line: 200,
        };
        let mut leaf = crate::text("A very long dashboard label")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0));
        leaf.source = widget_source;

        let mut wrapper = crate::row([leaf])
            .width(Size::Fixed(160.0))
            .height(Size::Fixed(48.0));
        wrapper.source = widget_source;

        let mut ui_state = UiState::new();
        layout::layout(
            &mut wrapper,
            &mut ui_state,
            Rect::new(0.0, 0.0, 160.0, 48.0),
        );
        let report = lint(&wrapper, &ui_state, Some("crates/test_app/src"));

        assert!(
            !report
                .findings
                .iter()
                .any(|f| f.kind == FindingKind::TextOverflow || f.kind == FindingKind::Overflow),
            "{}",
            report.text()
        );
    }
}
