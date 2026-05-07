//! Flex-style layout pass over the [`El`] tree.
//!
//! Sizing per axis:
//! - `Fixed(px)` — exact size on its axis.
//! - `Hug` — intrinsic size (text width, sum of children, etc.). Default.
//! - `Fill(weight)` — share leftover main-axis space proportionally.
//!
//! Defaults match CSS flex's `flex: 0 1 auto`: children content-size
//! on the main axis, defer to the parent's [`Align`] on the cross
//! axis. `Align::Stretch` (the column / scroll default) stretches both
//! `Hug` and `Fill` children to the container's full cross extent —
//! the analog of CSS `align-items: stretch`. `Align::Center | Start |
//! End` shrinks them to intrinsic so the alignment can actually
//! position them — matching CSS's behavior when align-items is
//! non-stretch. Main-axis distribution is governed by [`Justify`] (or
//! insert a [`spacer`]).
//!
//! The layout pass also assigns each node a stable path-based
//! [`El::computed_id`]: `root.0.card[account].2.button` — a node's ID is
//! parent-id + dot + role-or-key + sibling-index. IDs survive minor
//! refactors and are usable as patch / lint / draw-op targets.
//!
//! Rects do not live on `El` — the layout pass writes them to
//! `UiState`'s computed-rect side map, keyed by `computed_id`. The
//! container rect flows down the recursion as a parameter; child rects
//! are computed per-axis and inserted into the side map. Scroll offsets
//! likewise read/write `UiState`'s scroll-offset side map directly.
//!
//! Text intrinsic measurement uses bundled-font glyph advances via
//! [`crate::text::metrics`]. Full shaping still belongs to the renderer
//! for now; this keeps layout/lint/SVG close enough to glyphon output
//! without committing to the final text stack.

use std::sync::Arc;

use crate::state::UiState;
use crate::text::metrics as text_metrics;
use crate::tree::*;

/// Second escape hatch: author-supplied layout function.
///
/// When set on a node via [`El::layout`], the layout pass calls this
/// function instead of running the column/row/overlay distribution for
/// that node's direct children. The function returns one [`Rect`] per
/// child (in source order), positioned anywhere inside the container.
/// The library still recurses into each child (so descendants lay out
/// normally) and still drives hit-test, focus, animation, scroll —
/// those all read from `UiState`'s computed-rect side map, which receives the
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
/// ## Scope limits (will panic)
///
/// - The custom-layout node itself must size with [`Size::Fixed`] or
///   [`Size::Fill`] on both axes. `Size::Hug` would require a separate
///   intrinsic callback and is not yet supported.
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

/// Virtualized list state attached to a [`Kind::VirtualList`] node.
/// Holds the row count, fixed row height, and the closure that realizes
/// a row by global index. Set via [`crate::virtual_list`]; the layout
/// pass calls `build_row(i)` only for indices whose rect intersects the
/// viewport.
///
/// ## Current scope
///
/// - **Fixed row height** — every row is `row_height` logical pixels
///   tall. Variable-height rows would require an estimated height +
///   measure cache; not yet supported.
/// - **Vertical only** — feed/chat-log-shaped lists are the target.
///   A horizontal variant can come later.
/// - **No row pooling** — visible rows are rebuilt from scratch each
///   layout pass. Fine for thousands of items; if it bottlenecks we
///   add a pool keyed by stable row keys.
#[derive(Clone)]
#[non_exhaustive]
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
    /// Look up a node's laid-out rect by its `computed_id`. Same
    /// semantics as [`Self::rect_of_key`] but skips the `key →
    /// computed_id` translation — useful for runtime-synthesized
    /// layers (tooltips, focus rings) that anchor to a node the
    /// library already knows by id.
    pub rect_of_id: &'a dyn Fn(&str) -> Option<Rect>,
}

/// Lay out the whole tree into the given viewport rect.
pub fn layout(root: &mut El, ui_state: &mut UiState, viewport: Rect) {
    assign_id(root, "root");
    ui_state
        .layout
        .computed_rects
        .insert(root.computed_id.clone(), viewport);
    rebuild_key_index(root, ui_state);
    // Per-scrollable scratch is rebuilt every layout — entries for
    // scrollables that disappeared mid-frame must not leave stale
    // thumb rects behind for hit-test or paint to find.
    ui_state.scroll.metrics.clear();
    ui_state.scroll.thumb_rects.clear();
    ui_state.scroll.thumb_tracks.clear();
    layout_children(root, viewport, ui_state);
}

/// Walk the tree once and refresh `ui_state.layout.key_index` so
/// `LayoutCtx::rect_of_key` can resolve `key → computed_id` without
/// re-scanning the tree per lookup. First key wins — duplicate keys
/// are an author bug, but we don't want to crash layout over it.
fn rebuild_key_index(root: &El, ui_state: &mut UiState) {
    ui_state.layout.key_index.clear();
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
    visit(root, &mut ui_state.layout.key_index);
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
        Kind::Image => "image",
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
            ui_state.layout.computed_rects.insert(
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
                    .layout
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
    let key_index = &ui_state.layout.key_index;
    let computed_rects = &ui_state.layout.computed_rects;
    let rect_of_key = |key: &str| -> Option<Rect> {
        let id = key_index.get(key)?;
        computed_rects.get(id).copied()
    };
    let rect_of_id = |id: &str| -> Option<Rect> { computed_rects.get(id).copied() };
    let rects = (layout_fn.0)(LayoutCtx {
        container: inner,
        children: &node.children,
        measure: &measure,
        rect_of_key: &rect_of_key,
        rect_of_id: &rect_of_id,
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
            .layout
            .computed_rects
            .insert(c.computed_id.clone(), c_rect);
        layout_children(c, c_rect, ui_state);
    }
}

/// Virtualized list realization. Reads the stored scroll offset,
/// clamps it to the available range, computes the visible row index
/// range, calls `build_row(i)` for each, and lays them out at the
/// scroll-shifted Y positions. Replaces both the column distribution
/// and the scroll post-pass for `Kind::VirtualList` nodes.
fn layout_virtual(node: &mut El, node_rect: Rect, items: VirtualItems, ui_state: &mut UiState) {
    let inner = node_rect.inset(node.padding);
    let total_h = items.count as f32 * items.row_height;
    let max_offset = (total_h - inner.h).max(0.0);
    let stored = ui_state
        .scroll
        .offsets
        .get(&node.computed_id)
        .copied()
        .unwrap_or(0.0);
    let offset = stored.clamp(0.0, max_offset);
    ui_state
        .scroll
        .offsets
        .insert(node.computed_id.clone(), offset);
    ui_state.scroll.metrics.insert(
        node.computed_id.clone(),
        crate::state::ScrollMetrics {
            viewport_h: inner.h,
            content_h: total_h,
            max_offset,
        },
    );

    write_thumb_rect(node, inner, total_h, max_offset, offset, ui_state);

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
            .layout
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
            .scroll
            .offsets
            .insert(node.computed_id.clone(), 0.0);
        ui_state.scroll.metrics.insert(
            node.computed_id.clone(),
            crate::state::ScrollMetrics {
                viewport_h: inner.h,
                content_h: 0.0,
                max_offset: 0.0,
            },
        );
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
        .scroll
        .offsets
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
        .scroll
        .offsets
        .insert(node.computed_id.clone(), clamped);
    ui_state.scroll.metrics.insert(
        node.computed_id.clone(),
        crate::state::ScrollMetrics {
            viewport_h: inner.h,
            content_h,
            max_offset,
        },
    );

    write_thumb_rect(node, inner, content_h, max_offset, clamped, ui_state);
}

/// Compute and store the scrollbar thumb + track rects for `node`
/// when the author opted into a visible scrollbar AND content
/// overflows. Both rects are anchored to the right edge of `inner`.
/// The visible thumb is `SCROLLBAR_THUMB_WIDTH` wide and tracks the
/// scroll offset; the track is `SCROLLBAR_HITBOX_WIDTH` wide and
/// covers the full inner height so a press above/below the thumb
/// can page-scroll.
fn write_thumb_rect(
    node: &El,
    inner: Rect,
    content_h: f32,
    max_offset: f32,
    offset: f32,
    ui_state: &mut UiState,
) {
    if !node.scrollbar || max_offset <= 0.0 || inner.h <= 0.0 || content_h <= 0.0 {
        return;
    }
    let thumb_w = crate::tokens::SCROLLBAR_THUMB_WIDTH;
    let track_w = crate::tokens::SCROLLBAR_HITBOX_WIDTH;
    let track_inset = crate::tokens::SCROLLBAR_TRACK_INSET;
    let min_thumb_h = crate::tokens::SCROLLBAR_THUMB_MIN_H;
    let thumb_h = ((inner.h * inner.h / content_h).max(min_thumb_h)).min(inner.h);
    let track_remaining = (inner.h - thumb_h).max(0.0);
    let thumb_y = inner.y + track_remaining * (offset / max_offset);
    let thumb_x = inner.right() - thumb_w - track_inset;
    let track_x = inner.right() - track_w - track_inset;
    ui_state.scroll.thumb_rects.insert(
        node.computed_id.clone(),
        Rect::new(thumb_x, thumb_y, thumb_w, thumb_h),
    );
    ui_state.scroll.thumb_tracks.insert(
        node.computed_id.clone(),
        Rect::new(track_x, inner.y, track_w, inner.h),
    );
}

fn shift_subtree_y(node: &El, dy: f32, ui_state: &mut UiState) {
    if let Some(rect) = ui_state.layout.computed_rects.get_mut(&node.computed_id) {
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
        // CSS-flex parity for cross-axis sizing: `Size::Fixed` is an
        // explicit author override and always wins. Otherwise the
        // parent's `Align` decides — `Stretch` (the column default)
        // stretches non-fixed children to the container, `Center` /
        // `Start` / `End` shrink to intrinsic so the alignment can
        // actually position them. This collapses Hug and Fill on the
        // cross axis (both are "follow align-items"), the same way
        // CSS flex doesn't distinguish between them on the cross axis.
        let cross_size = match cross_intent {
            Size::Fixed(v) => v,
            Size::Hug | Size::Fill(_) => match node.align {
                Align::Stretch => cross_extent,
                Align::Start | Align::Center | Align::End => cross_intrinsic,
            },
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
            .layout
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
    // Wrap-text height depends on width, so constrain the intrinsic
    // measurement to the width the child will actually be laid out at
    // — same shape as `child_intrinsic` does for column/row children.
    // Without this, a Fixed-width modal with a wrappable paragraph
    // measures as a single-line block and the modal's Hug height ends
    // up shorter than the actual content needs, eating bottom padding.
    let constrained_width = match c.width {
        Size::Fixed(v) => Some(v),
        Size::Fill(_) | Size::Hug => Some(parent.w),
    };
    let (iw, ih) = intrinsic_constrained(c, constrained_width);
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
        // Custom-layout nodes don't define an intrinsic. Authors must
        // size them with `Fixed` or `Fill` on both axes; the returned
        // (0.0, 0.0) is replaced by `apply_min` for `Fixed` and is
        // unread for `Fill` (parent's distribution decides).
        if matches!(c.width, Size::Hug) || matches!(c.height, Size::Hug) {
            panic!(
                "layout_override on {:?} requires Size::Fixed or Size::Fill on both axes; \
                 Size::Hug is not supported for custom layouts",
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
    if let Some(img) = &c.image {
        // Natural pixel size as a logical-pixel intrinsic. Authors who
        // want a different sized box set `.width()` / `.height()`;
        // the projection inside that box is decided by `image_fit`.
        let w = img.width() as f32 + c.padding.left + c.padding.right;
        let h = img.height() as f32 + c.padding.top + c.padding.bottom;
        return apply_min(c, w, h);
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
        let layout = text_metrics::layout_text_with_line_height(
            &display,
            c.font_size,
            c.line_height,
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
    Some(text_metrics::layout_text_with_line_height(
        &display,
        c.font_size,
        c.line_height,
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
    let line_height = inline_paragraph_line_height(node);
    let unwrapped = text_metrics::layout_text_with_line_height(
        &concat,
        size,
        line_height,
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
    let layout = text_metrics::layout_text_with_line_height(
        &concat,
        size,
        line_height,
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

fn inline_paragraph_line_height(node: &El) -> f32 {
    let mut line_height: f32 = node.line_height;
    let mut max_size: f32 = node.font_size;
    for c in &node.children {
        if matches!(c.kind, Kind::Text) && c.font_size >= max_size {
            max_size = c.font_size;
            line_height = c.line_height;
        }
    }
    line_height
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UiState;

    /// CSS-flex parity: a `Size::Fill` child of a column with
    /// `align(Center)` should shrink to its intrinsic cross-axis size
    /// and be horizontally centered, matching `align-items: center`
    /// in CSS flex (which causes flex items to lose their stretch).
    #[test]
    fn align_center_shrinks_fill_child_to_intrinsic() {
        // Column with align(Center). Inner row has the default
        // El::new width = Fill(1.0); without Proposal B it would
        // claim the full 200px and align would be a no-op.
        let mut root = column([crate::row([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])])
        .align(Align::Center)
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let row_rect = state.rect(&root.children[0].computed_id);
        // Row's intrinsic width = 40 (single fixed child). 200 - 40 = 160
        // leftover; centered → row starts at x=80.
        assert!(
            (row_rect.x - 80.0).abs() < 0.5,
            "expected x≈80 (centered), got {}",
            row_rect.x
        );
        assert!(
            (row_rect.w - 40.0).abs() < 0.5,
            "expected w≈40 (shrunk to intrinsic), got {}",
            row_rect.w
        );
    }

    /// `align(Stretch)` (the default) preserves Fill stretching: a
    /// Fill-width child still claims the full cross axis.
    #[test]
    fn align_stretch_preserves_fill_stretch() {
        let mut root = column([crate::row([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])])
        .align(Align::Stretch)
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let row_rect = state.rect(&root.children[0].computed_id);
        assert!(
            (row_rect.x - 0.0).abs() < 0.5 && (row_rect.w - 200.0).abs() < 0.5,
            "expected stretched (x=0, w=200), got x={} w={}",
            row_rect.x,
            row_rect.w
        );
    }

    /// When all children are Hug-sized, `Justify::Center` should split
    /// the leftover space symmetrically across the main axis.
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

    /// CSS `justify-content: space-between`: when no main-axis Fill
    /// children claim the slack, the leftover space is distributed
    /// evenly *between* (not around) the children — outer edges flush.
    #[test]
    fn justify_space_between_distributes_evenly() {
        let row_child = || {
            crate::widgets::text::text("x")
                .width(Size::Fixed(20.0))
                .height(Size::Fixed(20.0))
        };
        let mut root = column([row_child(), row_child(), row_child()])
            .justify(Justify::SpaceBetween)
            .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 200.0));
        // Used main = 3 * 20 = 60. Leftover = 140 over (n-1) = 2 gaps
        // → 70 between. Positions: 0, 90, 180.
        let y0 = state.rect(&root.children[0].computed_id).y;
        let y1 = state.rect(&root.children[1].computed_id).y;
        let y2 = state.rect(&root.children[2].computed_id).y;
        assert!(
            y0.abs() < 0.5,
            "first child should be flush at y=0, got {y0}"
        );
        assert!(
            (y1 - 90.0).abs() < 0.5,
            "middle child should be at y≈90, got {y1}"
        );
        assert!(
            (y2 - 180.0).abs() < 0.5,
            "last child should be flush at y≈180, got {y2}"
        );
    }

    /// CSS `flex: <weight>`: when multiple `Size::Fill` children share
    /// a container, the available space is distributed in proportion
    /// to their weights.
    #[test]
    fn fill_weight_distributes_proportionally() {
        let big = crate::widgets::text::text("big")
            .width(Size::Fixed(40.0))
            .height(Size::Fill(2.0));
        let small = crate::widgets::text::text("small")
            .width(Size::Fixed(40.0))
            .height(Size::Fill(1.0));
        let mut root = column([big, small]).height(Size::Fixed(300.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 300.0));
        // Total weight = 3, available = 300. Big = 200, small = 100.
        let big_h = state.rect(&root.children[0].computed_id).h;
        let small_h = state.rect(&root.children[1].computed_id).h;
        assert!(
            (big_h - 200.0).abs() < 0.5,
            "Fill(2.0) should claim 2/3 of 300 ≈ 200, got {big_h}"
        );
        assert!(
            (small_h - 100.0).abs() < 0.5,
            "Fill(1.0) should claim 1/3 of 300 ≈ 100, got {small_h}"
        );
    }

    /// `padding` on a `Hug`-sized container is included in the
    /// container's intrinsic — matching CSS `box-sizing: content-box`
    /// where padding adds to the rendered size.
    #[test]
    fn padding_on_hug_includes_in_intrinsic() {
        let root = column([crate::widgets::text::text("x")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(40.0))])
        .padding(Sides::all(20.0));
        let (w, h) = intrinsic(&root);
        // 40 content + 2*20 padding on each axis = 80.
        assert!((w - 80.0).abs() < 0.5, "expected intrinsic w≈80, got {w}");
        assert!((h - 80.0).abs() < 0.5, "expected intrinsic h≈80, got {h}");
    }

    /// Cross-axis `Align::End` on a row pins children to the bottom
    /// edge — CSS `align-items: flex-end`. Mirror of `justify_end`
    /// but on the cross axis instead of the main axis.
    #[test]
    fn align_end_pins_to_cross_axis_far_edge() {
        let mut root = crate::row([crate::widgets::text::text("hi")
            .width(Size::Fixed(40.0))
            .height(Size::Fixed(20.0))])
        .align(Align::End)
        .width(Size::Fixed(200.0))
        .height(Size::Fixed(100.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        // Row cross axis = height. End → child y = 100 - 20 = 80.
        assert!(
            (child_rect.y - 80.0).abs() < 0.5,
            "expected y≈80 (pinned to bottom), got {}",
            child_rect.y
        );
    }

    #[test]
    fn overlay_can_center_hug_child() {
        let mut root = stack([crate::titled_card("Dialog", [crate::text("Body")])
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
        // Content height = 6 * 50 + 5 * 12 (gap) = 360 px. Visible
        // viewport (no padding) = 200 px → max_offset = 160.
        let mut root = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .key("list")
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        assign_ids(&mut root);
        state.scroll.offsets.insert(root.computed_id.clone(), 80.0);

        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        // Offset is in range, applied verbatim.
        let stored = state
            .scroll
            .offsets
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
            .scroll
            .offsets
            .insert(root.computed_id.clone(), 9999.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let stored = state
            .scroll
            .offsets
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
            .scroll
            .offsets
            .insert(tiny.computed_id.clone(), 50.0);
        layout(
            &mut tiny,
            &mut tiny_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        assert_eq!(
            tiny_state
                .scroll
                .offsets
                .get(&tiny.computed_id)
                .copied()
                .unwrap_or(0.0),
            0.0
        );
    }

    #[test]
    fn scrollbar_thumb_size_and_position_track_overflow() {
        // 6 rows x 50px + 5 gaps x 12 = 360 content; 200 viewport.
        // viewport/content = 200/360 ≈ 0.555 → thumb_h ≈ 111.1.
        let mut root = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let metrics = state
            .scroll
            .metrics
            .get(&root.computed_id)
            .copied()
            .expect("scrollable should have metrics");
        assert!((metrics.viewport_h - 200.0).abs() < 0.01);
        assert!((metrics.content_h - 360.0).abs() < 0.01);
        assert!((metrics.max_offset - 160.0).abs() < 0.01);

        let thumb = state
            .scroll
            .thumb_rects
            .get(&root.computed_id)
            .copied()
            .expect("scrollable with scrollbar() and overflow gets a thumb");
        // viewport^2 / content_h = 200^2 / 360 = 111.11..
        assert!((thumb.h - 111.111).abs() < 0.5, "thumb h = {}", thumb.h);
        assert!((thumb.w - crate::tokens::SCROLLBAR_THUMB_WIDTH).abs() < 0.01);
        // At offset 0, thumb sits at the top of the inner rect.
        assert!(thumb.y.abs() < 0.01);
        // Right-anchored: thumb_x + thumb_w + track_inset == viewport_right.
        assert!(
            (thumb.x + thumb.w + crate::tokens::SCROLLBAR_TRACK_INSET - 300.0).abs() < 0.01,
            "thumb anchored at {} (expected {})",
            thumb.x,
            300.0 - thumb.w - crate::tokens::SCROLLBAR_TRACK_INSET
        );

        // Slide to half — thumb should be at half the track_remaining.
        state.scroll.offsets.insert(root.computed_id.clone(), 80.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let thumb = state
            .scroll
            .thumb_rects
            .get(&root.computed_id)
            .copied()
            .unwrap();
        let track_remaining = 200.0 - thumb.h;
        let expected_y = track_remaining * (80.0 / 160.0);
        assert!(
            (thumb.y - expected_y).abs() < 0.5,
            "thumb at half-scroll y = {} (expected {expected_y})",
            thumb.y,
        );
    }

    #[test]
    fn scrollbar_track_is_wider_than_thumb_and_full_height() {
        // The track is the click hitbox: wider than the visible
        // thumb (Fitts's law) and tall enough to detect track
        // clicks above and below the thumb for paging.
        let mut root = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .gap(12.0)
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

        let thumb = state
            .scroll
            .thumb_rects
            .get(&root.computed_id)
            .copied()
            .unwrap();
        let track = state
            .scroll
            .thumb_tracks
            .get(&root.computed_id)
            .copied()
            .unwrap();
        // Track wider than thumb on the same right edge.
        assert!(track.w > thumb.w, "track.w {} thumb.w {}", track.w, thumb.w);
        assert!(
            (track.right() - thumb.right()).abs() < 0.01,
            "track and thumb must share the right edge",
        );
        // Track spans the full inner viewport (so above/below thumb
        // are both inside it for click-to-page).
        assert!(
            (track.h - 200.0).abs() < 0.01,
            "track height = {} (expected 200)",
            track.h,
        );
    }

    #[test]
    fn scrollbar_thumb_absent_when_disabled_or_no_overflow() {
        // Same scrollable, but author opted out — no thumb_rect.
        let mut suppressed = scroll(
            (0..6)
                .map(|i| crate::widgets::text::text(format!("row {i}")).height(Size::Fixed(50.0))),
        )
        .no_scrollbar()
        .height(Size::Fixed(200.0));
        let mut state = UiState::new();
        layout(
            &mut suppressed,
            &mut state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        assert!(
            !state
                .scroll
                .thumb_rects
                .contains_key(&suppressed.computed_id)
        );

        // Same scrollable, content fits → no thumb either.
        let mut tiny = scroll([crate::widgets::text::text("one row").height(Size::Fixed(20.0))])
            .height(Size::Fixed(200.0));
        let mut tiny_state = UiState::new();
        layout(
            &mut tiny,
            &mut tiny_state,
            Rect::new(0.0, 0.0, 300.0, 200.0),
        );
        assert!(
            !tiny_state
                .scroll
                .thumb_rects
                .contains_key(&tiny.computed_id)
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
        state.scroll.offsets.insert(root.computed_id.clone(), 120.0);
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
            .scroll
            .offsets
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
            .scroll
            .offsets
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
            .scroll
            .offsets
            .insert(root.computed_id.clone(), 9999.0);
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
        let stored = state
            .scroll
            .offsets
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

    #[test]
    fn overlay_child_with_wrapped_text_measures_against_its_resolved_width() {
        // Regression: overlay_rect used to call `intrinsic(c)` with no
        // width hint, so a Fixed-width modal containing a wrappable
        // paragraph measured the paragraph as a single line — leaving
        // the modal's Hug height short by the wrapped lines and
        // crowding the buttons against the bottom edge of the panel
        // (rumble cert-pending modal showed this).
        //
        // The fix: pass the child's resolved width as the available
        // width for intrinsic measurement, mirroring what column/row
        // already do.
        const PANEL_W: f32 = 240.0;
        const PADDING: f32 = 18.0;
        const GAP: f32 = 12.0;

        let panel = column([
            crate::paragraph(
                "A long enough warning paragraph that it has to wrap onto a second line \
                 inside this narrow panel.",
            ),
            crate::widgets::button::button("OK").key("ok"),
        ])
        .width(Size::Fixed(PANEL_W))
        .height(Size::Hug)
        .padding(Sides::all(PADDING))
        .gap(GAP)
        .align(Align::Stretch);

        let mut root = crate::stack([panel])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 800.0, 600.0));

        let panel_rect = state.rect(&root.children[0].computed_id);
        assert_eq!(panel_rect.w, PANEL_W, "panel keeps its Fixed width");

        let para_rect = state.rect(&root.children[0].children[0].computed_id);
        let button_rect = state.rect(&root.children[0].children[1].computed_id);

        // Paragraph wrapped to ≥ 2 lines (exact line count depends on
        // glyph metrics; just guard against the single-line bug).
        assert!(
            para_rect.h > crate::tokens::FONT_BASE * 1.4,
            "paragraph should wrap to multiple lines inside the Fixed-width panel; \
             got h={}",
            para_rect.h
        );

        // Panel height must accommodate top padding + paragraph +
        // gap + button + bottom padding. The bug was that the panel
        // came out exactly `padding + gap + 1-line-paragraph + button`
        // — short by the second wrap line — and the button overshot
        // the inner area, leaving zero pixels of bottom padding.
        let bottom_padding = (panel_rect.y + panel_rect.h) - (button_rect.y + button_rect.h);
        assert!(
            (bottom_padding - PADDING).abs() < 0.5,
            "expected {PADDING}px between button and panel bottom, got {bottom_padding}",
        );
    }
}
