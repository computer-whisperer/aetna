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
//! Text intrinsic measurement is approximate (`chars × font_size × 0.56`).
//! Good enough for SVG fixtures; will be replaced when glyphon-based
//! shaping lands.

use crate::state::UiState;
use crate::tree::*;

/// Lay out the whole tree into the given viewport rect.
pub fn layout(root: &mut El, ui_state: &mut UiState, viewport: Rect) {
    assign_id(root, "root");
    ui_state
        .computed_rects
        .insert(root.computed_id.clone(), viewport);
    layout_children(root, viewport, ui_state);
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
        Kind::Custom(name) => name,
    }
}

fn layout_children(node: &mut El, node_rect: Rect, ui_state: &mut UiState) {
    match node.axis {
        Axis::Overlay => {
            let inner = node_rect.inset(node.padding);
            for c in &mut node.children {
                let c_rect = overlay_rect(c, inner, node.align, node.justify);
                ui_state.computed_rects.insert(c.computed_id.clone(), c_rect);
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
    let free_after_used = if fill_weight_total == 0.0 { remaining } else { 0.0 };
    let mut cursor = match node.justify {
        Justify::Start => 0.0,
        Justify::Center => free_after_used * 0.5,
        Justify::End => free_after_used,
        Justify::SpaceBetween => 0.0,
    };
    let between_extra = if matches!(node.justify, Justify::SpaceBetween) && n > 1 && fill_weight_total == 0.0 {
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
        let cross_size = match cross_intent {
            Size::Fixed(v) => v,
            Size::Hug => cross_intrinsic,
            Size::Fill(_) => match node.align {
                Align::Stretch => cross_extent,
                _ => cross_intrinsic.min(cross_extent),
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
        ui_state.computed_rects.insert(c.computed_id.clone(), c_rect);
        layout_children(c, c_rect, ui_state);

        cursor += main_size + node.gap + if i + 1 < n { between_extra } else { 0.0 };
    }
}

enum MainSize { Resolved(f32), Fill(f32) }

fn main_size_of(c: &El, iw: f32, ih: f32, vertical: bool) -> MainSize {
    let s = if vertical { c.height } else { c.width };
    let intr = if vertical { ih } else { iw };
    match s {
        Size::Fixed(v) => MainSize::Resolved(v),
        Size::Hug => MainSize::Resolved(intr),
        Size::Fill(w) => MainSize::Fill(w),
    }
}

fn child_intrinsic(c: &El, vertical: bool, parent_cross_extent: f32, parent_align: Align) -> (f32, f32) {
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

/// Approximate intrinsic (width, height) for hugging layouts.
pub fn intrinsic(c: &El) -> (f32, f32) {
    intrinsic_constrained(c, None)
}

fn intrinsic_constrained(c: &El, available_width: Option<f32>) -> (f32, f32) {
    if let Some(text) = &c.text {
        let char_w = c.font_size * char_width_factor(c.font_mono);
        let line_h = c.font_size * 1.4;
        let unwrapped_w = text
            .split('\n')
            .map(|line| line.chars().count() as f32 * char_w)
            .fold(0.0, f32::max);
        let content_available = match c.text_wrap {
            TextWrap::NoWrap => None,
            TextWrap::Wrap => available_width
                .or(match c.width {
                    Size::Fixed(v) => Some(v),
                    Size::Fill(_) | Size::Hug => None,
                })
                .map(|w| (w - c.padding.left - c.padding.right).max(char_w)),
        };
        let line_count = content_available
            .map(|w| wrapped_line_count(text, w, char_w))
            .unwrap_or_else(|| text.split('\n').count().max(1));
        let w = content_available
            .map(|w| unwrapped_w.min(w) + c.padding.left + c.padding.right)
            .unwrap_or(unwrapped_w + c.padding.left + c.padding.right);
        let h = line_count as f32 * line_h + c.padding.top + c.padding.bottom;
        return apply_min(c, w, h);
    }
    match c.axis {
        Axis::Overlay => {
            let mut w: f32 = 0.0;
            let mut h: f32 = 0.0;
            for ch in &c.children {
                let child_available = available_width.map(|w| (w - c.padding.left - c.padding.right).max(0.0));
                let (cw, chh) = intrinsic_constrained(ch, child_available);
                w = w.max(cw);
                h = h.max(chh);
            }
            apply_min(c, w + c.padding.left + c.padding.right, h + c.padding.top + c.padding.bottom)
        }
        Axis::Column => {
            let mut w: f32 = 0.0;
            let mut h: f32 = c.padding.top + c.padding.bottom;
            let n = c.children.len();
            let child_available = available_width.map(|w| (w - c.padding.left - c.padding.right).max(0.0));
            for (i, ch) in c.children.iter().enumerate() {
                let (cw, chh) = intrinsic_constrained(ch, child_available);
                w = w.max(cw);
                h += chh;
                if i + 1 < n { h += c.gap; }
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
                if i + 1 < n { w += c.gap; }
                h = h.max(chh);
            }
            apply_min(c, w, h + c.padding.top + c.padding.bottom)
        }
    }
}

pub(crate) fn estimated_text_size(c: &El, available_width: Option<f32>) -> Option<(f32, f32)> {
    c.text.as_ref()?;
    Some(intrinsic_constrained(c, available_width))
}

fn apply_min(c: &El, mut w: f32, mut h: f32) -> (f32, f32) {
    if let Size::Fixed(v) = c.width { w = v; }
    if let Size::Fixed(v) = c.height { h = v; }
    (w, h)
}

fn char_width_factor(mono: bool) -> f32 {
    // Conservative-leaning estimate. cosmic-text's actual run width for
    // typical sans-serif sits around 0.56–0.62 of size depending on the
    // glyph mix; pick the upper end so layout reserves enough width for
    // text that the wgpu glyph run won't overflow visibly. The SVG
    // fixture path also benefits — fewer false-positive overflow lints.
    if mono { 0.62 } else { 0.60 }
}

fn wrapped_line_count(text: &str, max_width: f32, char_width: f32) -> usize {
    let max_chars = (max_width / char_width).floor().max(1.0) as usize;
    let mut total = 0;
    for paragraph in text.split('\n') {
        let mut line_len = 0usize;
        let mut saw_word = false;
        for word in paragraph.split_whitespace() {
            saw_word = true;
            let word_len = word.chars().count();
            if word_len > max_chars {
                if line_len > 0 {
                    total += 1;
                    line_len = 0;
                }
                total += (word_len + max_chars - 1) / max_chars;
                continue;
            }
            let next_len = if line_len == 0 { word_len } else { line_len + 1 + word_len };
            if next_len > max_chars {
                total += 1;
                line_len = word_len;
            } else {
                line_len = next_len;
            }
        }
        if saw_word {
            total += 1;
        } else {
            total += 1;
        }
    }
    total.max(1)
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
        let mut root = column([crate::text::text("hi").width(Size::Fixed(40.0)).height(Size::Fixed(20.0))])
            .justify(Justify::Center)
            .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        // Expected: 100 - 20 = 80 leftover; centered → starts at y=40.
        assert!((child_rect.y - 40.0).abs() < 0.5,
            "expected y≈40, got {}", child_rect.y);
    }

    #[test]
    fn justify_end_pushes_to_bottom() {
        let mut root = column([crate::text::text("hi").width(Size::Fixed(40.0)).height(Size::Fixed(20.0))])
            .justify(Justify::End)
            .height(Size::Fill(1.0));
        let mut state = UiState::new();
        layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        let child_rect = state.rect(&root.children[0].computed_id);
        assert!((child_rect.y - 80.0).abs() < 0.5,
            "expected y≈80, got {}", child_rect.y);
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
        assert!((child_rect.x - 200.0).abs() < 0.5, "expected x≈200, got {}", child_rect.x);
        assert!(child_rect.y > 100.0 && child_rect.y < 200.0, "expected centered y, got {}", child_rect.y);
    }

    #[test]
    fn scroll_offset_translates_children_and_clamps_to_content() {
        // Six 50px-tall rows in a 200px-tall scroll viewport.
        // Content height = 6*50 + 5*gap_default = 300 + 5*12 = 360 px.
        // Visible viewport (no padding) = 200 px → max_offset = 160.
        let mut root = scroll((0..6).map(|i| {
            crate::text::text(format!("row {i}")).height(Size::Fixed(50.0))
        }))
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
        state.scroll_offsets.insert(root.computed_id.clone(), 9999.0);
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
        let mut tiny = scroll([crate::text::text("just one row").height(Size::Fixed(20.0))])
            .height(Size::Fixed(200.0));
        let mut tiny_state = UiState::new();
        assign_ids(&mut tiny);
        tiny_state.scroll_offsets.insert(tiny.computed_id.clone(), 50.0);
        layout(&mut tiny, &mut tiny_state, Rect::new(0.0, 0.0, 300.0, 200.0));
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
