//! Flex-style layout pass over the [`El`] tree.
//!
//! - `Fixed(px)` — exact size on its axis.
//! - `Hug` — intrinsic size (text width, sum of children, etc.).
//! - `Fill(weight)` — share leftover space proportionally.
//!
//! Cross-axis behavior is governed by the parent's [`Align`]; main-axis
//! distribution by [`Justify`] (or insert a [`spacer`]).
//!
//! The layout pass also assigns each node a stable path-based
//! [`El::computed_id`]: `root.0.card[account].2.button` — a node's ID is
//! parent-id + dot + role-or-key + sibling-index. IDs survive minor
//! refactors and are usable as patch / lint / draw-op targets.
//!
//! v5.0 step 7: rects no longer live on `El` — the layout pass writes
//! them to [`UiState::computed_rects`], keyed by `computed_id`. The
//! container rect flows down the recursion as a parameter; child rects
//! are computed per-axis and inserted into the side map. Scroll offsets
//! likewise read/write [`UiState::scroll_offsets`] directly.
//!
//! Text intrinsic measurement uses bundled-font glyph advances via
//! [`crate::text::metrics`]. Full shaping still belongs to the renderer
//! for now; this keeps layout/lint/SVG close enough to glyphon output
//! without committing to the final text stack.

use std::sync::Arc;

use crate::state::UiState;
use crate::text::metrics as text_metrics;
use crate::tree::*;

/// v0.5 — second escape hatch: author-supplied layout function.
///
/// When set on a node via [`El::layout`], the layout pass calls this
/// function instead of running the column/row/overlay distribution for
/// that node's direct children. The function returns one [`Rect`] per
/// child (in source order), positioned anywhere inside the container.
/// The library still recurses into each child (so descendants lay out
/// normally) and still drives hit-test, focus, animation, scroll —
/// those all read from [`UiState::computed_rects`], which receives the
/// rects this function produces.
///
/// Authors typically write a free `fn(LayoutCtx) -> Vec<Rect>` and
/// pass it directly: `column(children).layout(my_layout)`.
///
/// ## What you get
///
/// - [`LayoutCtx::container`] — the rect available for placement
///   (parent rect minus this node's padding).
/// - [`LayoutCtx::children`] — read-only slice of the node's children;
///   index here matches the index in your returned `Vec<Rect>`.
/// - [`LayoutCtx::measure`] — call to get a child's intrinsic
///   `(width, height)` if you need it for sizing decisions.
///
/// ## v0.5 scope limits (will panic)
///
/// - The custom-layout node itself must size with [`Size::Fixed`] or
///   [`Size::Fill`] on both axes. `Size::Hug` requires a separate
///   intrinsic callback that's deferred.
/// - The returned `Vec<Rect>` length must equal `children.len()`.
#[derive(Clone)]
pub struct LayoutFn(pub Arc<dyn Fn(LayoutCtx) -> Vec<Rect> + Send + Sync>);

impl LayoutFn {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(LayoutCtx) -> Vec<Rect> + Send + Sync + 'static,
    {
        LayoutFn(Arc::new(f))
    }
}

impl std::fmt::Debug for LayoutFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LayoutFn(<fn>)")
    }
}

/// v0.5 — virtualized list state attached to a [`Kind::VirtualList`]
/// node. Holds the row count, fixed row height, and the closure that
/// realizes a row by global index. Set via [`crate::virtual_list`];
/// the layout pass calls `build_row(i)` only for indices whose rect
/// intersects the viewport.
///
/// ## Scope (v0.5 step 2)
///
/// - **Fixed row height** — every row is `row_height` logical pixels
///   tall. Variable-height rows would require an estimated height +
///   measure cache; deferred to a later slice.
/// - **Vertical only** — the v0.5 fixture is feed/chat_log-shaped,
///   which is always vertical. A horizontal variant can come later.
/// - **No row pooling** — visible rows are rebuilt from scratch each
///   layout pass. Fine for thousands of items; if it bottlenecks we
///   add a pool keyed by stable row keys.
#[derive(Clone)]
pub struct VirtualItems {
    pub count: usize,
    pub row_height: f32,
    pub build_row: Arc<dyn Fn(usize) -> El + Send + Sync>,
}

impl VirtualItems {
    pub fn new<F>(count: usize, row_height: f32, build_row: F) -> Self
    where
        F: Fn(usize) -> El + Send + Sync + 'static,
    {
        assert!(
            row_height > 0.0,
            "VirtualItems::new requires row_height > 0.0 (got {row_height})"
        );
        VirtualItems {
            count,
            row_height,
            build_row: Arc::new(build_row),
        }
    }
}

impl std::fmt::Debug for VirtualItems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualItems")
            .field("count", &self.count)
            .field("row_height", &self.row_height)
            .field("build_row", &"<fn>")
            .finish()
    }
}

/// Context handed to a [`LayoutFn`]. Marked `#[non_exhaustive]` so
/// future fields (intrinsic-at-width, scroll context, …) can be added
/// without breaking author code that currently reads `container` /
/// `children` / `measure`.
#[non_exhaustive]
pub struct LayoutCtx<'a> {
    /// Inner rect of the parent (after padding) — the area available
    /// for child placement. Children may be positioned anywhere; the
    /// library does not clamp returned rects to this region.
    pub container: Rect,
    /// Direct children of the node, in source order. Read-only — return
    /// positions through your `Vec<Rect>`.
    pub children: &'a [El],
    /// Intrinsic `(width, height)` for any child. Wrapped text returns
    /// its unwrapped width here; if you need width-dependent wrapping
    /// you'll need to size the child with `Fixed` / `Fill` instead.
    pub measure: &'a dyn Fn(&El) -> (f32, f32),
    /// Look up any keyed node's laid-out rect. Returns `None` when the
    /// key is absent from the tree, when the node hasn't been laid out
    /// yet (siblings later in source order), or when the key was used
    /// on a node without a recorded rect. Used by widgets like
    /// [`crate::widgets::popover::popover`] to position children
    /// relative to elements outside their own subtree.
    pub rect_of_key: &'a dyn Fn(&str) -> Option<Rect>,
}

/// Lay out the whole tree into the given viewport rect.
pub fn layout(root: &mut El, ui_state: &mut UiState, viewport: Rect) {
    assign_id(root, "root");
    ui_state
        .computed_rects
        .insert(root.computed_id.clone(), viewport);
    rebuild_key_index(root, ui_state);
    layout_children(root, viewport, ui_state);
}

/// Walk the tree once and refresh `ui_state.layout_key_index` so
/// `LayoutCtx::rect_of_key` can resolve `key → computed_id` without
/// re-scanning the tree per lookup. First key wins — duplicate keys
/// are an author bug, but we don't want to crash layout over it.
fn rebuild_key_index(root: &El, ui_state: &mut UiState) {
    ui_state.layout_key_index.clear();
    fn visit(node: &El, index: &mut std::collections::HashMap<String, String>) {
        if let Some(key) = &node.key {
            index
                .entry(key.clone())
                .or_insert_with(|| node.computed_id.clone());
        }
        for c in &node.children {
            visit(c, index);
        }
    }
    visit(root, &mut ui_state.layout_key_index);
}

/// Assign every node's `computed_id` without positioning anything else.
/// Useful when callers need to read or seed side-map entries (e.g.,
/// scroll offsets) before `layout` runs.
pub fn assign_ids(root: &mut El) {
    assign_id(root, "root");
}

fn assign_id(node: &mut El, path: &str) {
    node.computed_id = path.to_string();
    for (i, c) in node.children.iter_mut().enumerate() {
        let role = role_token(&c.kind);
        let suffix = match (&c.key, role) {
            (Some(k), r) => format!("{r}[{k}]"),
            (None, r) => format!("{r}.{i}"),
        };
        let child_path = format!("{path}.{suffix}");
        assign_id(c, &child_path);
    }
}

fn role_token(k: &Kind) -> &'static str {
    match k {
        Kind::Group => "group",
        Kind::Card => "card",
        Kind::Button => "button",
        Kind::Badge => "badge",
        Kind::Text => "text",
        Kind::Heading => "heading",
        Kind::Spacer => "spacer",
        Kind::Divider => "divider",
        Kind::Overlay => "overlay",
        Kind::Scrim => "scrim",
        Kind::Modal => "modal",
        Kind::Scroll => "scroll",
        Kind::VirtualList => "virtual_list",
        Kind::Inlines => "inlines",
        Kind::HardBreak => "hard_break",
        Kind::Custom(name) => name,
    }
}

fn layout_children(node: &mut El, node_rect: Rect, ui_state: &mut UiState) {
    if matches!(node.kind, Kind::Inlines) {
        // The paragraph paints as a single AttributedText DrawOp;
        // child Text/HardBreak nodes are aggregated by draw_ops::
        // push_node and don't paint independently. Give each child a
        // zero-size rect so the rest of the engine (hit-test, focus,
        // animation, lint) treats them as non-paint pseudo-nodes. The
        // paragraph's hit-test target is the Inlines node itself,
        // sized by node_rect.
        for c in &mut node.children {
            ui_state.computed_rects.insert(
                c.computed_id.clone(),
                Rect::new(node_rect.x, node_rect.y, 0.0, 0.0),
            );
            // Recurse so descendants of Text/HardBreak nodes (rare —
            // these are leaves in practice — but keeping the invariant
            // simple) still get their rects assigned.
            layout_children(c, Rect::new(node_rect.x, node_rect.y, 0.0, 0.0), ui_state);
        }
        return;
    }
    if let Some(items) = node.virtual_items.clone() {
        layout_virtual(node, node_rect, items, ui_state);
        return;
    }
    if let Some(layout_fn) = node.layout_override.clone() {
        layout_custom(node, node_rect, layout_fn, ui_state);
        if node.scrollable {
            apply_scroll_offset(node, node_rect, ui_state);
        }
        return;
    }
    match node.axis {
        Axis::Overlay => {
            let inner = node_rect.inset(node.padding);
            for c in &mut node.children {
                let c_rect = overlay_rect(c, inner, node.align, node.justify);
                ui_state
                    .computed_rects
                    .insert(c.computed_id.clone(), c_rect);
                layout_children(c, c_rect, ui_state);
            }
        }
        Axis::Column => layout_axis(node, node_rect, true, ui_state),
        Axis::Row => layout_axis(node, node_rect, false, ui_state),
    }
    if node.scrollable {
        apply_scroll_offset(node, node_rect, ui_state);
    }
}

fn layout_custom(node: &mut El, node_rect: Rect, layout_fn: LayoutFn, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    let measure = |c: &El| intrinsic(c);
    // Split-borrow `ui_state` so the `rect_of_key` closure reads the
    // key index + computed rects while the surrounding function still
    // holds the mutable borrow needed to insert this node's children
    // back into `computed_rects` afterwards.
    let key_index = &ui_state.layout_key_index;
    let computed_rects = &ui_state.computed_rects;
    let rect_of_key = |key: &str| -> Option<Rect> {
        let id = key_index.get(key)?;
        computed_rects.get(id).copied()
    };
    let rects = (layout_fn.0)(LayoutCtx {
        container: inner,
        children: &node.children,
        measure: &measure,
        rect_of_key: &rect_of_key,
    });
    assert_eq!(
        rects.len(),
        node.children.len(),
        "LayoutFn for {:?} returned {} rects for {} children",
        node.computed_id,
        rects.len(),
        node.children.len(),
    );
    for (c, c_rect) in node.children.iter_mut().zip(rects) {
        ui_state
            .computed_rects
            .insert(c.computed_id.clone(), c_rect);
        layout_children(c, c_rect, ui_state);
    }
}

/// v0.5 — virtualized list realization. Reads the stored scroll offset,
/// clamps it to the available range, computes the visible row index
/// range, calls `build_row(i)` for each, and lays them out at the
/// scroll-shifted Y positions. Replaces both the column distribution
/// and the scroll post-pass for `Kind::VirtualList` nodes.
fn layout_virtual(node: &mut El, node_rect: Rect, items: VirtualItems, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    let total_h = items.count as f32 * items.row_height;
    let max_offset = (total_h - inner.h).max(0.0);
    let stored = ui_state
        .scroll_offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let offset = stored.clamp(0.0, max_offset);
    ui_state
        .scroll_offsets
        .insert(node.computed_id.clone(), offset);

    if items.count == 0 {
        node.children.clear();
        return;
    }

    // Visible index range — `start` floors, `end` ceils, both clamped.
    let start = (offset / items.row_height).floor() as usize;
    let end = (((offset + inner.h) / items.row_height).ceil() as usize).min(items.count);

    let mut realized: Vec<El> = (start..end).map(|i| (items.build_row)(i)).collect();
    for (vis_i, child) in realized.iter_mut().enumerate() {
        let global_i = start + vis_i;
        let role = role_token(&child.kind);
        let suffix = match (&child.key, role) {
            (Some(k), r) => format!("{r}[{k}]"),
            (None, r) => format!("{r}.{global_i}"),
        };
        let child_path = format!("{}.{}", node.computed_id, suffix);
        assign_id(child, &child_path);

        let row_y = inner.y + global_i as f32 * items.row_height - offset;
        let c_rect = Rect::new(inner.x, row_y, inner.w, items.row_height);
        ui_state
            .computed_rects
            .insert(child.computed_id.clone(), c_rect);
        layout_children(child, c_rect, ui_state);
    }
    node.children = realized;
}

/// Scrollable post-pass: measure content height from the laid-out
/// children's stored rects, clamp the scroll offset to the available
/// range, and shift every descendant rect by `-offset`.
///
/// Children should size with `Hug` or `Fixed` on the main axis —
/// `Fill` children would absorb the viewport's height and there would
/// be nothing to scroll.
fn apply_scroll_offset(node: &El, node_rect: Rect, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    if node.children.is_empty() {
        ui_state
            .scroll_offsets
            .insert(node.computed_id.clone(), 0.0);
        return;
    }
    let content_bottom = node
        .children
        .iter()
        .map(|c| ui_state.rect(&c.computed_id).bottom())
        .fold(f32::NEG_INFINITY, f32::max);
    let content_h = (content_bottom - inner.y).max(0.0);
    let max_offset = (content_h - inner.h).max(0.0);
    let stored = ui_state
        .scroll_offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let clamped = stored.clamp(0.0, max_offset);
    if clamped > 0.0 {
        for c in &node.children {
            shift_subtree_y(c, -clamped, ui_state);
        }
    }
    ui_state
        .scroll_offsets
        .insert(node.computed_id.clone(), clamped);
}

fn shift_subtree_y(node: &El, dy: f32, ui_state: &mut UiState) {
    if let Some(rect) = ui_state.computed_rects.get_mut(&node.computed_id) {
        rect.y += dy;
    }
    for c in &node.children {
        shift_subtree_y(c, dy, ui_state);
    }
}

fn layout_axis(node: &mut El, node_rect: Rect, vertical: bool, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    let n = node.children.len();
    if n == 0 {
        return;
    }

    let total_gap = node.gap * n.saturating_sub(1) as f32;
    let main_extent = if vertical { inner.h } else { inner.w };
    let cross_extent = if vertical { inner.w } else { inner.h };

    let intrinsics: Vec<(f32, f32)> = node
        .children
        .iter()
        .map(|c| child_intrinsic(c, vertical, cross_extent, node.align))
        .collect();

    let mut consumed = 0.0;
    let mut fill_weight_total = 0.0;
    for (c, (iw, ih)) in node.children.iter().zip(intrinsics.iter()) {
        match main_size_of(c, *iw, *ih, vertical) {
            MainSize::Resolved(v) => consumed += v,
            MainSize::Fill(w) => fill_weight_total += w.max(0.001),
        }
    }
    let remaining = (main_extent - consumed - total_gap).max(0.0);

    // Free space after children + gaps. When there are Fill children they
    // claim it all, so justify is moot; otherwise this is what center/end
    // distribute around.
    let free_after_used = if fill_weight_total == 0.0 {
        remaining
    } else {
        0.0
    };
    let mut cursor = match node.justify {
        Justify::Start => 0.0,
        Justify::Center => free_after_used * 0.5,
        Justify::End => free_after_used,
        Justify::SpaceBetween => 0.0,
    };
    let between_extra =
        if matches!(node.justify, Justify::SpaceBetween) && n > 1 && fill_weight_total == 0.0 {
            remaining / (n - 1) as f32
        } else {
            0.0
        };

    for (i, (c, (iw, ih))) in node.children.iter_mut().zip(intrinsics).enumerate() {
        let main_size = match main_size_of(c, iw, ih, vertical) {
            MainSize::Resolved(v) => v,
            MainSize::Fill(w) => remaining * w.max(0.001) / fill_weight_total.max(0.001),
        };

        let cross_intent = if vertical { c.width } else { c.height };
        let cross_intrinsic = if vertical { iw } else { ih };
        // `Size::Fill` on the cross axis always claims the parent's
        // full extent — there is no slack left to position, so the
        // parent's `Align` becomes a no-op for that child. `Align`
        // only positions Hug/Fixed children that are smaller than the
        // container.
        let cross_size = match cross_intent {
            Size::Fixed(v) => v,
            Size::Hug => cross_intrinsic,
            Size::Fill(_) => cross_extent,
        };

        let cross_off = match node.align {
            Align::Start | Align::Stretch => 0.0,
            Align::Center => (cross_extent - cross_size) * 0.5,
            Align::End => cross_extent - cross_size,
        };

        let c_rect = if vertical {
            Rect::new(inner.x + cross_off, inner.y + cursor, cross_size, main_size)
        } else {
            Rect::new(inner.x + cursor, inner.y + cross_off, main_size, cross_size)
        };
        ui_state
            .computed_rects
            .insert(c.computed_id.clone(), c_rect);
        layout_children(c, c_rect, ui_state);

        cursor += main_size + node.gap + if i + 1 < n { between_extra } else { 0.0 };
    }
}

enum MainSize {
    Resolved(f32),
    Fill(f32),
}

fn main_size_of(c: &El, iw: f32, ih: f32, vertical: bool) -> MainSize {
    let s = if vertical { c.height } else { c.width };
    let intr = if vertical { ih } else { iw };
    match s {
        Size::Fixed(v) => MainSize::Resolved(v),
        Size::Hug => MainSize::Resolved(intr),
        Size::Fill(w) => MainSize::Fill(w),
    }
}

fn child_intrinsic(
    c: &El,
    vertical: bool,
    parent_cross_extent: f32,
    parent_align: Align,
) -> (f32, f32) {
    if !vertical {
        return intrinsic(c);
    }
    let available_width = match c.width {
        Size::Fixed(v) => Some(v),
        Size::Fill(_) => Some(parent_cross_extent),
        Size::Hug => match parent_align {
            Align::Stretch => Some(parent_cross_extent),
            Align::Start | Align::Center | Align::End => Some(parent_cross_extent),
        },
    };
    intrinsic_constrained(c, available_width)
}

fn overlay_rect(c: &El, parent: Rect, align: Align, justify: Justify) -> Rect {
    let (iw, ih) = intrinsic(c);
    let w = match c.width {
        Size::Fixed(v) => v,
        Size::Hug => iw.min(parent.w),
        Size::Fill(_) => parent.w,
    };
    let h = match c.height {
        Size::Fixed(v) => v,
        Size::Hug => ih.min(parent.h),
        Size::Fill(_) => parent.h,
    };
    let x = match align {
        Align::Start | Align::Stretch => parent.x,
        Align::Center => parent.x + (parent.w - w) * 0.5,
        Align::End => parent.right() - w,
    };
    let y = match justify {
        Justify::Start | Justify::SpaceBetween => parent.y,
        Justify::Center => parent.y + (parent.h - h) * 0.5,
        Justify::End => parent.bottom() - h,
    };
    Rect::new(x, y, w, h)
}

/// Intrinsic (width, height) for hugging layouts.
pub fn intrinsic(c: &El) -> (f32, f32) {
    intrinsic_constrained(c, None)
}

fn intrinsic_constrained(c: &El, available_width: Option<f32>) -> (f32, f32) {
    if c.layout_override.is_some() {
        // v0.5: custom-layout nodes don't define an intrinsic. Authors
        // must size them with `Fixed` or `Fill` on both axes; the
        // returned (0.0, 0.0) is replaced by `apply_min` for `Fixed`
        // and is unread for `Fill` (parent's distribution decides).
        if matches!(c.width, Size::Hug) || matches!(c.height, Size::Hug) {
            panic!(
                "layout_override on {:?} requires Size::Fixed or Size::Fill on both axes; \
                 Size::Hug is not supported for custom layouts in v0.5",
                c.computed_id,
            );
        }
        return apply_min(c, 0.0, 0.0);
    }
    if c.virtual_items.is_some() {
        // VirtualList sizes the whole viewport (the parent decides) and
        // realizes only on-screen rows. Hug-sizing it would mean
        // "shrink to fit all rows", defeating virtualization. Same
        // shape as the layout_override guard.
        if matches!(c.width, Size::Hug) || matches!(c.height, Size::Hug) {
            panic!(
                "virtual_list on {:?} requires Size::Fixed or Size::Fill on both axes; \
                 Size::Hug would defeat virtualization",
                c.computed_id,
            );
        }
        return apply_min(c, 0.0, 0.0);
    }
    if matches!(c.kind, Kind::Inlines) {
        return inline_paragraph_intrinsic(c, available_width);
    }
    if matches!(c.kind, Kind::HardBreak) {
        // HardBreak is meaningful only inside Inlines (where draw_ops
        // encodes it as `\n` in the attributed text). Outside Inlines
        // it's a no-op layout-wise.
        return apply_min(c, 0.0, 0.0);
    }
    if c.icon.is_some() {
        return apply_min(
            c,
            c.font_size + c.padding.left + c.padding.right,
            c.font_size + c.padding.top + c.padding.bottom,
        );
    }
    if let Some(text) = &c.text {
        let unwrapped = text_metrics::layout_text(
            text,
            c.font_size,
            c.font_weight,
            c.font_mono,
            TextWrap::NoWrap,
            None,
        );
        let content_available = match c.text_wrap {
            TextWrap::NoWrap => None,
            TextWrap::Wrap => available_width
                .or(match c.width {
                    Size::Fixed(v) => Some(v),
                    Size::Fill(_) | Size::Hug => None,
                })
                .map(|w| (w - c.padding.left - c.padding.right).max(1.0)),
        };
        let display = display_text_for_measure(c, text, content_available);
        let layout = text_metrics::layout_text(
            &display,
            c.font_size,
            c.font_weight,
            c.font_mono,
            c.text_wrap,
            content_available,
        );
        let w = content_available
            .map(|available| unwrapped.width.min(available) + c.padding.left + c.padding.right)
            .unwrap_or(layout.width + c.padding.left + c.padding.right);
        let h = layout.height + c.padding.top + c.padding.bottom;
        return apply_min(c, w, h);
    }
    match c.axis {
        Axis::Overlay => {
            let mut w: f32 = 0.0;
            let mut h: f32 = 0.0;
            for ch in &c.children {
                let child_available =
                    available_width.map(|w| (w - c.padding.left - c.padding.right).max(0.0));
                let (cw, chh) = intrinsic_constrained(ch, child_available);
                w = w.max(cw);
                h = h.max(chh);
            }
            apply_min(
                c,
                w + c.padding.left + c.padding.right,
                h + c.padding.top + c.padding.bottom,
            )
        }
        Axis::Column => {
            let mut w: f32 = 0.0;
            let mut h: f32 = c.padding.top + c.padding.bottom;
            let n = c.children.len();
            let child_available =
                available_width.map(|w| (w - c.padding.left - c.padding.right).max(0.0));
            for (i, ch) in c.children.iter().enumerate() {
                let (cw, chh) = intrinsic_constrained(ch, child_available);
                w = w.max(cw);
                h += chh;
                if i + 1 < n {
                    h += c.gap;
                }
            }
            apply_min(c, w + c.padding.left + c.padding.right, h)
        }
        Axis::Row => {
            let mut w: f32 = c.padding.left + c.padding.right;
            let mut h: f32 = 0.0;
            let n = c.children.len();
            for (i, ch) in c.children.iter().enumerate() {
                let (cw, chh) = intrinsic(ch);
                w += cw;
                if i + 1 < n {
                    w += c.gap;
                }
                h = h.max(chh);
            }
            apply_min(c, w, h + c.padding.top + c.padding.bottom)
        }
    }
}

pub(crate) fn text_layout(
    c: &El,
    available_width: Option<f32>,
) -> Option<text_metrics::TextLayout> {
    let text = c.text.as_ref()?;
    let content_available = match c.text_wrap {
        TextWrap::NoWrap => None,
        TextWrap::Wrap => available_width
            .or(match c.width {
                Size::Fixed(v) => Some(v),
                Size::Fill(_) | Size::Hug => None,
            })
            .map(|w| (w - c.padding.left - c.padding.right).max(1.0)),
    };
    let display = display_text_for_measure(c, text, content_available);
    Some(text_metrics::layout_text(
        &display,
        c.font_size,
        c.font_weight,
        c.font_mono,
        c.text_wrap,
        content_available,
    ))
}

fn display_text_for_measure(c: &El, text: &str, available_width: Option<f32>) -> String {
    if let (TextWrap::Wrap, Some(max_lines), Some(width)) =
        (c.text_wrap, c.text_max_lines, available_width)
    {
        text_metrics::clamp_text_to_lines(
            text,
            c.font_size,
            c.font_weight,
            c.font_mono,
            width,
            max_lines,
        )
    } else {
        text.to_string()
    }
}

fn apply_min(c: &El, mut w: f32, mut h: f32) -> (f32, f32) {
    if let Size::Fixed(v) = c.width {
        w = v;
    }
    if let Size::Fixed(v) = c.height {
        h = v;
    }
    (w, h)
}

/// Approximate intrinsic measurement for `Kind::Inlines` paragraphs.
///
/// The paragraph paints through cosmic-text's rich-text shaping (which
/// resolves bold/italic/mono runs against fontdb), but layout needs a
/// width and height *before* we get to the renderer. We concatenate
/// the runs' text into one string and call `text_metrics::layout_text`
/// at the dominant font size — same approximation the lint pass uses
/// for single-style text. Bold/italic widths are slightly different
/// from regular; for body-text paragraphs that difference is well
/// under one wrap-line and we accept it. If a fixture wraps within
/// 1-2 characters of a boundary the rendered glyphs may straddle the
/// laid-out rect by a fraction of a glyph.
fn inline_paragraph_intrinsic(node: &El, available_width: Option<f32>) -> (f32, f32) {
    let concat = concat_inline_text(&node.children);
    let size = inline_paragraph_size(node);
    let unwrapped = text_metrics::layout_text(
        &concat,
        size,
        FontWeight::Regular,
        false,
        TextWrap::NoWrap,
        None,
    );
    let content_available = match node.text_wrap {
        TextWrap::NoWrap => None,
        TextWrap::Wrap => available_width
            .or(match node.width {
                Size::Fixed(v) => Some(v),
                Size::Fill(_) | Size::Hug => None,
            })
            .map(|w| (w - node.padding.left - node.padding.right).max(1.0)),
    };
    let layout = text_metrics::layout_text(
        &concat,
        size,
        FontWeight::Regular,
        false,
        node.text_wrap,
        content_available,
    );
    let w = content_available
        .map(|av| unwrapped.width.min(av) + node.padding.left + node.padding.right)
        .unwrap_or(layout.width + node.padding.left + node.padding.right);
    let h = layout.height + node.padding.top + node.padding.bottom;
    apply_min(node, w, h)
}

/// Walk an Inlines paragraph's children and produce the source-order
/// concatenation that draw_ops will hand to the atlas. `Kind::Text`
/// contributes its `text` field; `Kind::HardBreak` contributes a
/// newline; anything else contributes nothing (an unsupported child
/// kind inside Inlines is a programmer error elsewhere — measurement
/// silently ignores it).
fn concat_inline_text(children: &[El]) -> String {
    let mut s = String::new();
    for c in children {
        match c.kind {
            Kind::Text => {
                if let Some(t) = &c.text {
                    s.push_str(t);
                }
            }
            Kind::HardBreak => s.push('\n'),
            _ => {}
        }
    }
    s
}

/// Pick the font size that drives the paragraph's measurement. We use
/// the maximum across text children rather than the parent's own
/// `font_size`, because builders set sizes on the leaf text nodes.
fn inline_paragraph_size(node: &El) -> f32 {
    let mut size: f32 = node.font_size;
    for c in &node.children {
        if matches!(c.kind, Kind::Text) {
            size = size.max(c.font_size);
        }
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UiState;

    /// Regression test for attempt_3's broken `Justify::Center` (tripped
    /// during the cold-session login fixture). When all children are
    /// Hug-sized, Justify::Center should split the leftover space.
    #[test]
    fn justify_center_centers_hug_children() {
        let mut root = column([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .justify(Justify::Center)
        .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        // Expected: 100 - 20 = 80 leftover; centered → starts at y=40.
        assert!(
            (child_rect.y - 40.0).abs() < 0.5,
            "expected y≈40, got {}",
            child_rect.y
        );
    }

    #[test]
    fn justify_end_pushes_to_bottom() {
        let mut root = column([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .justify(Justify::End)
        .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (child_rect.y - 80.0).abs() < 0.5,
            "expected y≈80, got {}",
            child_rect.y
        );
    }

    #[test]
    fn overlay_can_center_hug_child() {
        let mut root = stack([crate::card("Dialog", [crate::text("Body")])
            .width(Size::Fixed(200.0))
            .height(Size::Hug)])
        .align(Align::Center)
        .justify(Justify::Center);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 400.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (child_rect.x - 200.0).abs() < 0.5,
            "expected x≈200, got {}",
            child_rect.x
        );
        assert!(
            child_rect.y > 100.0 && child_rect.y < 200.0,
            "expected centered y, got {}",
            child_rect.y
        );
    }

    #[test]
    fn scroll_offset_translates_children_and_clamps_to_content() {
        // Six 50px-tall rows in a 200px-tall scroll viewport.
        // Content height = 6*50 + 5*gap_default = 300 + 5*12 = 360 px.
        // Visible viewport (no padding) = 200 px → max_offset = 160.
        let mut root = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .key("list")
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        assign_ids(&mut root);
        state.scroll_offsets.insert(root.computed_id.clone(), 80.0);

        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        // Offset is in range, applied verbatim.
        let stored = state
            .scroll_offsets
            .get(&root.computed_id)
            .copied()
            .unwrap_or(0.0);
        assert!(
            (stored - 80.0).abs() < 0.01,
            "offset clamped unexpectedly: {stored}"
        );
        // First child shifted up by 80.
        let c0 = state.rect(&root.children[0].computed_id);
        assert!(
            (c0.y - (-80.0)).abs() < 0.01,
            "child 0 y = {} (expected -80)",
            c0.y
        );
        // Now overshoot — should clamp to max_offset=160.
        state
            .scroll_offsets
            .insert(root.computed_id.clone(), 9999.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let stored = state
            .scroll_offsets
            .get(&root.computed_id)
            .copied()
            .unwrap_or(0.0);
        assert!(
            (stored - 160.0).abs() < 0.01,
            "overshoot clamped to {stored}"
        );
        // Content fits → offset clamps to 0.
        let mut tiny =
            scroll([crate::widgets::text::text("just one row").height(Size::Fixed(20.0))])
                .height(Size::Fixed(200.0));
        let mut tiny_state = UiState::new();
        assign_ids(&mut tiny);
        tiny_state
            .scroll_offsets
            .insert(tiny.computed_id.clone(), 50.0);
        layout(
            &mut tiny,
            &mut tiny_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        assert_eq!(
            tiny_state
                .scroll_offsets
                .get(&tiny.computed_id)
                .copied()
                .unwrap_or(0.0),
            0.0
        );
    }

    #[test]
    fn layout_override_places_children_at_returned_rects() {
        // A custom layout that just stacks children diagonally inside the container.
        let mut root = column((0..3).map(|i| {
            crate::widgets::text::text(format!("dot {i}"))
                .width(Size::Fixed(20.0))
                .height(Size::Fixed(20.0))
        }))
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0))
        .layout(|ctx| {
            ctx.children
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let off = i as f32 * 30.0;
                    Rect::new(ctx.container.x + off, ctx.container.y + off, 20.0, 20.0)
                })
                .collect()
        });
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let r0 = state.rect(&root.children[0].computed_id);
        let r1 = state.rect(&root.children[1].computed_id);
        let r2 = state.rect(&root.children[2].computed_id);
        assert_eq!((r0.x, r0.y), (0.0, 0.0));
        assert_eq!((r1.x, r1.y), (30.0, 30.0));
        assert_eq!((r2.x, r2.y), (60.0, 60.0));
    }

    #[test]
    fn layout_override_rect_of_key_resolves_earlier_sibling() {
        // The popover-anchor pattern: a custom-laid-out node positions
        // its child by reading another keyed node's rect via the new
        // LayoutCtx::rect_of_key callback. The trigger lives in an
        // earlier sibling so its rect is already in `computed_rects`
        // by the time the popover layer's layout_override runs.
        use crate::tree::stack;
        let trigger_x = 40.0;
        let trigger_y = 20.0;
        let trigger_w = 60.0;
        let trigger_h = 30.0;
        let mut root = stack([
            // Earlier sibling: the trigger.
            crate::widgets::button::button("Open")
                .key("trig")
                .width(Size::Fixed(trigger_w))
                .height(Size::Fixed(trigger_h)),
            // Later sibling: a custom-laid-out container that reads
            // the trigger's rect to position its single child.
            stack([crate::widgets::text::text("popover")
                .width(Size::Fixed(80.0))
                .height(Size::Fixed(20.0))])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .layout(|ctx| {
                let trig = (ctx.rect_of_key)("trig").expect("trigger laid out");
                vec![Rect::new(trig.x, trig.bottom() + 4.0, 80.0, 20.0)]
            }),
        ])
        .padding(Sides::xy(trigger_x, trigger_y));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let popover_layer = &root.children[1];
        let panel_id = &popover_layer.children[0].computed_id;
        let panel_rect = state.rect(panel_id);
        // Anchored to (trigger.x, trigger.bottom() + 4.0). With padding
        // (40, 20) and trigger height 30 → expect (40, 54).
        assert!(
            (panel_rect.x - trigger_x).abs() < 0.01,
            "popover x = {} (expected {trigger_x})",
            panel_rect.x,
        );
        assert!(
            (panel_rect.y - (trigger_y + trigger_h + 4.0)).abs() < 0.01,
            "popover y = {} (expected {})",
            panel_rect.y,
            trigger_y + trigger_h + 4.0,
        );
    }

    #[test]
    fn layout_override_rect_of_key_returns_none_for_missing_key() {
        let mut root = column([crate::widgets::text::text("inner")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0))
        .layout(|ctx| {
            assert!((ctx.rect_of_key)("nope").is_none());
            vec![Rect::new(ctx.container.x, ctx.container.y, 40.0, 20.0)]
        });
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn layout_override_rect_of_key_returns_none_for_later_sibling() {
        // First-frame contract: a custom layout running before its
        // target's sibling has been laid out should see `None`, not a
        // zero rect or a panic. This is what makes the popover pattern
        // (trigger first, popover layer second in source order) the
        // supported shape — the reverse direction simply gets `None`.
        use crate::tree::stack;
        let mut root = stack([
            stack([crate::widgets::text::text("panel")
                .width(Size::Fixed(40.0))
                .height(Size::Fixed(20.0))])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0))
            .layout(|ctx| {
                assert!(
                    (ctx.rect_of_key)("later").is_none(),
                    "later sibling's rect must not be available yet"
                );
                vec![Rect::new(ctx.container.x, ctx.container.y, 40.0, 20.0)]
            }),
            crate::widgets::button::button("after").key("later"),
        ]);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn layout_override_measure_returns_intrinsic() {
        // The custom layout reads `measure` to size each child.
        let mut root = column([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0))
        .layout(|ctx| {
            let (w, h) = (ctx.measure)(&ctx.children[0]);
            assert!((w - 40.0).abs() < 0.01, "measured width {w}");
            assert!((h - 20.0).abs() < 0.01, "measured height {h}");
            vec![Rect::new(ctx.container.x, ctx.container.y, w, h)]
        });
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let r = state.rect(&root.children[0].computed_id);
        assert_eq!((r.w, r.h), (40.0, 20.0));
    }

    #[test]
    #[should_panic(expected = "returned 1 rects for 2 children")]
    fn layout_override_length_mismatch_panics() {
        let mut root = column([
            crate::widgets::text::text("a")
                .width(Size::Fixed(10.0))
                .height(Size::Fixed(10.0)),
            crate::widgets::text::text("b")
                .width(Size::Fixed(10.0))
                .height(Size::Fixed(10.0)),
        ])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0))
        .layout(|ctx| vec![Rect::new(ctx.container.x, ctx.container.y, 10.0, 10.0)]);
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    #[should_panic(expected = "Size::Hug is not supported for custom layouts")]
    fn layout_override_hug_panics() {
        // Hug check fires when the parent's layout pass measures the
        // custom-layout child for sizing — i.e. when a layout_override
        // node is a child of a column/row, not when it's the root.
        let mut root = column([column([crate::widgets::text::text("c")])
            .width(Size::Hug)
            .height(Size::Fixed(200.0))
            .layout(|ctx| vec![Rect::new(ctx.container.x, ctx.container.y, 10.0, 10.0)])])
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
    }

    #[test]
    fn virtual_list_realizes_only_visible_rows() {
        // 100 rows × 50px each in a 200px viewport, offset = 120.
        // Visible range: rows whose y in [-50, 200) → start = floor(120/50) = 2,
        // end = ceil((120+200)/50) = ceil(6.4) = 7. Five rows realized.
        let mut root = crate::tree::virtual_list(100, 50.0, |i| {
            crate::widgets::text::text(format!("row {i}")).key(format!("row-{i}"))
        });
        let mut state = UiState::new();
        assign_ids(&mut root);
        state.scroll_offsets.insert(root.computed_id.clone(), 120.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        assert_eq!(
            root.children.len(),
            5,
            "expected 5 realized rows, got {}",
            root.children.len()
        );
        // Identity check: the first realized row should be the row keyed "row-2".
        assert_eq!(root.children[0].key.as_deref(), Some("row-2"));
        assert_eq!(root.children[4].key.as_deref(), Some("row-6"));
        // Position check: first realized row's y = inner.y + 2*50 - 120 = -20.
        let r0 = state.rect(&root.children[0].computed_id);
        assert!(
            (r0.y - (-20.0)).abs() < 0.5,
            "row 2 expected y≈-20, got {}",
            r0.y
        );
    }

    #[test]
    fn virtual_list_keyed_rows_have_stable_computed_id_across_scroll() {
        let make_root = || {
            crate::tree::virtual_list(50, 50.0, |i| {
                crate::widgets::text::text(format!("row {i}")).key(format!("row-{i}"))
            })
        };

        let mut state = UiState::new();
        let mut root_a = make_root();
        assign_ids(&mut root_a);
        // Scroll so row 5 is visible.
        state
            .scroll_offsets
            .insert(root_a.computed_id.clone(), 250.0);
        layout(&mut root_a, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let id_at_offset_a = root_a
            .children
            .iter()
            .find(|c| c.key.as_deref() == Some("row-5"))
            .unwrap()
            .computed_id
            .clone();

        // Re-layout with a different offset — row 5 is still visible.
        let mut root_b = make_root();
        assign_ids(&mut root_b);
        state
            .scroll_offsets
            .insert(root_b.computed_id.clone(), 200.0);
        layout(&mut root_b, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let id_at_offset_b = root_b
            .children
            .iter()
            .find(|c| c.key.as_deref() == Some("row-5"))
            .unwrap()
            .computed_id
            .clone();

        assert_eq!(
            id_at_offset_a, id_at_offset_b,
            "row-5's computed_id changed when scroll offset moved"
        );
    }

    #[test]
    fn virtual_list_clamps_overshoot_offset() {
        // 10 rows × 50 = 500 content height; viewport 200; max offset = 300.
        let mut root =
            crate::tree::virtual_list(10, 50.0, |i| crate::widgets::text::text(format!("r{i}")));
        let mut state = UiState::new();
        assign_ids(&mut root);
        state
            .scroll_offsets
            .insert(root.computed_id.clone(), 9999.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let stored = state
            .scroll_offsets
            .get(&root.computed_id)
            .copied()
            .unwrap_or(0.0);
        assert!(
            (stored - 300.0).abs() < 0.01,
            "expected clamp to 300, got {stored}"
        );
    }

    #[test]
    fn virtual_list_empty_count_realizes_no_children() {
        let mut root =
            crate::tree::virtual_list(0, 50.0, |i| crate::widgets::text::text(format!("r{i}")));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        assert_eq!(root.children.len(), 0);
    }

    #[test]
    #[should_panic(expected = "row_height > 0.0")]
    fn virtual_list_zero_row_height_panics() {
        let _ = crate::tree::virtual_list(10, 0.0, |i| crate::widgets::text::text(format!("r{i}")));
    }

    #[test]
    #[should_panic(expected = "Size::Hug would defeat virtualization")]
    fn virtual_list_hug_panics() {
        let mut root = column([crate::tree::virtual_list(10, 50.0, |i| {
            crate::widgets::text::text(format!("r{i}"))
        })
        .height(Size::Hug)])
        .width(Size::Fixed(300.0))
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn text_runs_constructor_shape_smoke() {
        let el = crate::tree::text_runs([
            crate::widgets::text::text("Hello, "),
            crate::widgets::text::text("world").bold(),
            crate::tree::hard_break(),
            crate::widgets::text::text("of text").italic(),
        ]);
        assert_eq!(el.kind, Kind::Inlines);
        assert_eq!(el.children.len(), 4);
        assert!(matches!(
            el.children[1].font_weight,
            FontWeight::Bold | FontWeight::Semibold
        ));
        assert_eq!(el.children[2].kind, Kind::HardBreak);
        assert!(el.children[3].text_italic);
    }

    #[test]
    fn wrapped_text_hugs_multiline_height_from_available_width() {
        let mut root = column([crate::paragraph(
            "A longer sentence should wrap into multiple measured lines.",
        )])
        .width(Size::Fill(1.0))
        .height(Size::Hug);

        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 180.0, 200.0));

        let child_rect = state.rect(&root.children[0].computed_id);
        assert_eq!(child_rect.w, 180.0);
        assert!(
            child_rect.h > crate::tokens::FONT_BASE * 1.4,
            "expected multiline paragraph height, got {}",
            child_rect.h
        );
    }
}
