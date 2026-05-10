//! Pointer hit-testing and scroll routing on a laid-out tree.
//!
//! All entry points walk children in reverse paint order (top-most
//! visual first), respecting the inherited clip stack so a button outside
//! its scroll viewport can't be clicked. Only nodes with `key.is_some()`
//! are hit-test targets — author intent is "I tagged it with a key, it's
//! interactive."
//!
//! This keyed-only rule is also what gates
//! [`crate::tree::El::tooltip`]: an unkeyed leaf with `.tooltip()` is
//! silently dead because hover never lands on it. The bundle lint
//! flags this case as
//! [`crate::bundle::lint::FindingKind::DeadTooltip`].
//!
//! Reads computed rects from `UiState`'s layout side map (populated by
//! the layout pass) — the tree carries identity (`computed_id`) but not
//! geometry. Paint-time transforms (`translate`, `scale`) are then
//! applied in the same way `draw_ops::push_node` applies them, so
//! hit-testing matches what the user sees. Parent rects are *not*
//! barriers: a child can paint outside its parent (a swatch lifting on
//! `.scale(1.15)`) and still be hittable. Only `clip()` (an explicit
//! author-declared boundary) gates descent into descendants.

use crate::event::UiTarget;
use crate::selection::SelectionPoint;
use crate::state::UiState;
use crate::text::metrics;
use crate::tree::{El, Kind, Rect};

/// Find the topmost keyed node whose laid-out rect contains `point`
/// (logical pixels). Returns `None` if the point hits no keyed node.
pub fn hit_test(root: &El, ui_state: &UiState, point: (f32, f32)) -> Option<String> {
    hit_test_target(root, ui_state, point).map(|target| target.key)
}

/// Find the topmost keyed node and return full target metadata.
pub fn hit_test_target(root: &El, ui_state: &UiState, point: (f32, f32)) -> Option<UiTarget> {
    match hit_test_rec(root, ui_state, point, None, (0.0, 0.0)) {
        Hit::Target(target) => Some(target),
        Hit::Blocked | Hit::Miss => None,
    }
}

enum Hit {
    Target(UiTarget),
    Blocked,
    Miss,
}

fn hit_test_rec(
    node: &El,
    ui_state: &UiState,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    inherited_translate: (f32, f32),
) -> Hit {
    if let Some(clip) = inherited_clip
        && !clip.contains(point.0, point.1)
    {
        return Hit::Miss;
    }
    // Mirror `draw_ops::push_node`: translate accumulates through the
    // subtree; scale applies to this node only and doesn't propagate.
    // Hit-testing must use the same painted rect that the user sees, or
    // clicks on a translated card land on whatever sibling occupies the
    // un-translated layout slot.
    let total_translate = (
        inherited_translate.0 + node.translate.0,
        inherited_translate.1 + node.translate.1,
    );
    let computed = ui_state.rect(&node.computed_id);
    let translated_rect = translated(computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, node.scale);
    // We do NOT early-return on `!painted_rect.contains(point)`.
    // A child can paint outside its parent's rect (the palette
    // swatches `.scale(1.15).translate(0, -8)` lift over the row's
    // computed bounds) and the only hard boundary is `inherited_clip`.
    // The painted-rect containment is checked below for self-as-target.
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(painted_rect)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(painted_rect),
        }
    } else {
        inherited_clip
    };
    // Children paint last → are on top → check first.
    for child in node.children.iter().rev() {
        match hit_test_rec(child, ui_state, point, child_clip, total_translate) {
            Hit::Target(target) => return Hit::Target(target),
            Hit::Blocked => return Hit::Blocked,
            Hit::Miss => {}
        }
    }
    // No child hit. Self counts only if its painted rect contains the
    // point AND it's keyed (author tagged it interactive).
    if !painted_rect.contains(point.0, point.1) {
        return Hit::Miss;
    }
    if let Some(key) = &node.key {
        return Hit::Target(UiTarget {
            key: key.clone(),
            node_id: node.computed_id.clone(),
            rect: painted_rect,
            tooltip: node.tooltip.clone(),
        });
    }
    if node.block_pointer {
        return Hit::Blocked;
    }
    Hit::Miss
}

/// Find the topmost selectable + keyed text leaf containing `point`
/// and return a [`SelectionPoint`] resolved against the leaf's text
/// content (one byte offset per Unicode scalar boundary).
///
/// Returns `None` when the point misses every selectable leaf, or
/// when the hit leaf has no text. Walks the same tree the focus
/// hit-test walks, with the same clip / translate rules — so a
/// selectable paragraph that's been scrolled out of view is correctly
/// excluded.
pub fn selection_point_at(
    root: &El,
    ui_state: &UiState,
    point: (f32, f32),
) -> Option<SelectionPoint> {
    let mut hit: Option<SelectableHit<'_>> = None;
    selectable_rec(root, ui_state, point, None, (0.0, 0.0), &mut hit);
    let SelectableHit { node, painted } = hit?;
    let key = node.key.clone()?;
    let value = node.text.as_deref()?;
    let local_x = (point.0 - painted.x).max(0.0);
    let local_y = (point.1 - painted.y).clamp(0.0, painted.h.max(1.0) - 1.0);
    let geometry = metrics::TextGeometry::new_with_family(
        value,
        node.font_size,
        node.font_family,
        node.font_weight,
        node.font_mono,
        node.text_wrap,
        Some(painted.w),
    );
    let byte = match geometry.hit_byte(local_x, local_y) {
        Some(byte) => byte.min(value.len()),
        None => {
            if local_x <= 0.0 {
                0
            } else {
                value.len()
            }
        }
    };
    Some(SelectionPoint { key, byte })
}

/// Inner state carried while walking for a selectable target. We
/// keep a borrow of the El so the caller can read `text` / font
/// params after the walk completes — saving a second tree walk.
struct SelectableHit<'a> {
    node: &'a El,
    painted: Rect,
}

fn selectable_rec<'a>(
    node: &'a El,
    ui_state: &UiState,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    inherited_translate: (f32, f32),
    out: &mut Option<SelectableHit<'a>>,
) {
    if let Some(clip) = inherited_clip
        && !clip.contains(point.0, point.1)
    {
        return;
    }
    let total_translate = (
        inherited_translate.0 + node.translate.0,
        inherited_translate.1 + node.translate.1,
    );
    let computed = ui_state.rect(&node.computed_id);
    let translated_rect = translated(computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, node.scale);
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(painted_rect)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(painted_rect),
        }
    } else {
        inherited_clip
    };
    // Children paint on top → check first.
    for child in node.children.iter().rev() {
        selectable_rec(child, ui_state, point, child_clip, total_translate, out);
        if out.is_some() {
            return;
        }
    }
    // Self counts only if it's a selectable + keyed text leaf and the
    // point lands inside its painted rect.
    if node.selectable
        && node.key.is_some()
        && matches!(node.kind, Kind::Text | Kind::Heading)
        && painted_rect.contains(point.0, point.1)
    {
        *out = Some(SelectableHit {
            node,
            painted: painted_rect,
        });
    }
}

/// Return the `computed_id` of the deepest scrollable container whose
/// laid-out rect contains `point`, respecting clipping ancestors.
/// Used to route wheel events.
pub(crate) fn scroll_target_at(root: &El, ui_state: &UiState, point: (f32, f32)) -> Option<String> {
    let mut hit = None;
    scroll_target_rec(root, ui_state, point, None, (0.0, 0.0), &mut hit);
    hit
}

fn scroll_target_rec(
    node: &El,
    ui_state: &UiState,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    inherited_translate: (f32, f32),
    out: &mut Option<String>,
) {
    if let Some(clip) = inherited_clip
        && !clip.contains(point.0, point.1)
    {
        return;
    }
    let total_translate = (
        inherited_translate.0 + node.translate.0,
        inherited_translate.1 + node.translate.1,
    );
    let computed = ui_state.rect(&node.computed_id);
    let translated_rect = translated(computed, total_translate);
    let painted_rect = scaled_around_center(translated_rect, node.scale);
    // Self counts as a scroll target only if its painted rect contains
    // the point — but we still recurse into children regardless, since
    // a child can paint outside its parent (translate/scale).
    if node.scrollable && painted_rect.contains(point.0, point.1) {
        *out = Some(node.computed_id.clone());
    }
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(painted_rect)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(painted_rect),
        }
    } else {
        inherited_clip
    };
    for c in &node.children {
        scroll_target_rec(c, ui_state, point, child_clip, total_translate, out);
    }
}

fn translated(r: Rect, offset: (f32, f32)) -> Rect {
    if offset.0 == 0.0 && offset.1 == 0.0 {
        return r;
    }
    Rect::new(r.x + offset.0, r.y + offset.1, r.w, r.h)
}

fn scaled_around_center(r: Rect, s: f32) -> Rect {
    if (s - 1.0).abs() < f32::EPSILON {
        return r;
    }
    let cx = r.center_x();
    let cy = r.center_y();
    let w = r.w * s;
    let h = r.h * s;
    Rect::new(cx - w * 0.5, cy - h * 0.5, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::tree::*;
    use crate::{button, column, row};

    fn lay_out_counter() -> (El, UiState) {
        let mut tree = column([
            crate::text("0"),
            row([button("-").key("dec"), button("+").key("inc")]),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        (tree, state)
    }

    fn find_rect(node: &El, state: &UiState, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) {
            return Some(state.rect(&node.computed_id));
        }
        node.children.iter().find_map(|c| find_rect(c, state, key))
    }

    fn find_text_rect(node: &El, state: &UiState) -> Option<Rect> {
        if matches!(node.kind, Kind::Text) {
            return Some(state.rect(&node.computed_id));
        }
        node.children.iter().find_map(|c| find_text_rect(c, state))
    }

    #[test]
    fn hit_test_finds_keyed_button() {
        let (tree, state) = lay_out_counter();
        for key in &["dec", "inc"] {
            let r = find_rect(&tree, &state, key).expect("button rect");
            let center = (r.x + r.w * 0.5, r.y + r.h * 0.5);
            let hit = hit_test(&tree, &state, center);
            assert_eq!(hit.as_deref(), Some(*key));
        }
    }

    #[test]
    fn hit_test_misses_unkeyed_text() {
        let (tree, state) = lay_out_counter();
        let r = find_text_rect(&tree, &state).expect("text rect");
        let center = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        assert!(hit_test(&tree, &state, center).is_none());
    }

    #[test]
    fn hit_test_outside_returns_none() {
        let (tree, state) = lay_out_counter();
        assert!(hit_test(&tree, &state, (-10.0, -10.0)).is_none());
        assert!(hit_test(&tree, &state, (9999.0, 9999.0)).is_none());
    }

    #[test]
    fn hit_test_respects_clipping_ancestor() {
        let mut tree = column([row([
            button("-").key("visible"),
            button("+").key("clipped").width(Size::Fixed(240.0)),
        ])
        .clip()
        .width(Size::Fixed(80.0))]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));

        let clipped = find_rect(&tree, &state, "clipped").expect("clipped button rect");
        assert!(hit_test(&tree, &state, (clipped.center_x(), clipped.center_y())).is_none());
    }

    #[test]
    fn hit_test_follows_ancestor_translate() {
        // A keyed button inside a column that is translated horizontally
        // by 120 px must be hit-testable at its translated location, and
        // the un-translated layout slot should miss. This guards against
        // a regression where `.translate()` (paint-time) shifts visuals
        // but hit-testing still uses layout rects, causing clicks on the
        // visually-shifted widget to land on whatever sibling occupies
        // the original layout slot.
        let mut tree = row([
            column([button("A").key("a")]).translate(120.0, 0.0),
            button("B").key("b"),
        ]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));

        let untranslated = find_rect(&tree, &state, "a").expect("a layout rect");
        let translated_center = (untranslated.center_x() + 120.0, untranslated.center_y());
        let untranslated_center = (untranslated.center_x(), untranslated.center_y());

        assert_eq!(
            hit_test(&tree, &state, translated_center).as_deref(),
            Some("a"),
            "click at translated location should hit the translated button"
        );
        // The original layout slot may still belong to an ancestor row,
        // but it must not return "a" — that would be the bug.
        assert_ne!(
            hit_test(&tree, &state, untranslated_center).as_deref(),
            Some("a"),
            "click at the un-translated layout slot must not hit the translated button"
        );
    }

    #[test]
    fn hit_test_child_lifted_above_parent_still_hits() {
        // Reproduces the palette swatch bug: a child uses
        // `.scale(1.15).translate(0, -8)` so its painted rect lifts
        // above the parent row's layout rect. A click on the lifted
        // top edge must still find the child — the parent row's bounds
        // should not be a hit-test boundary, since only `clip()` is.
        let mut tree = row([crate::titled_card("c", [crate::text("body")])
            .key("swatch")
            .width(Size::Fixed(120.0))
            .height(Size::Fixed(120.0))
            .scale(1.15)
            .translate(0.0, -20.0)]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let layout_rect = find_rect(&tree, &state, "swatch").expect("swatch rect");
        // Painted top is roughly: layout.y - 20 (translate) - layout.h * 0.075 (scale lift).
        let painted_top_y = layout_rect.y - 20.0 - layout_rect.h * 0.075 + 1.0;
        let painted_top_x = layout_rect.center_x();
        assert_eq!(
            hit_test(&tree, &state, (painted_top_x, painted_top_y)).as_deref(),
            Some("swatch"),
            "click on lifted top of scaled+translated child should hit"
        );
    }

    #[test]
    fn hit_test_translate_inherits_to_descendants() {
        // Ancestor translate should propagate through a chain of children.
        let mut tree = column([row([button("X").key("x")]).translate(0.0, 50.0)]);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let pre = find_rect(&tree, &state, "x").expect("x layout rect");
        let translated = (pre.center_x(), pre.center_y() + 50.0);
        assert_eq!(
            hit_test(&tree, &state, translated).as_deref(),
            Some("x"),
            "ancestor translate must accumulate to descendants"
        );
    }

    #[test]
    fn unkeyed_blocking_node_stops_fallthrough() {
        use crate::tree::stack;
        let mut tree = stack([
            El::new(Kind::Scrim)
                .key("dismiss")
                .fill(crate::tokens::OVERLAY_SCRIM)
                .fill_size(),
            El::new(Kind::Modal)
                .block_pointer()
                .width(Size::Fixed(100.0))
                .height(Size::Fixed(100.0)),
        ])
        .align(Align::Center)
        .justify(Justify::Center)
        .fill_size();
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 300.0));

        assert!(hit_test(&tree, &state, (150.0, 150.0)).is_none());
        assert_eq!(
            hit_test(&tree, &state, (10.0, 10.0)).as_deref(),
            Some("dismiss")
        );
    }
}
