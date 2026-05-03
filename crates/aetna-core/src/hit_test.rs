//! Pointer hit-testing and scroll routing on a laid-out tree.
//!
//! All entry points walk children in reverse paint order (top-most
//! visual first), respecting the inherited clip stack so a button outside
//! its scroll viewport can't be clicked. Only nodes with `key.is_some()`
//! are hit-test targets — author intent is "I tagged it with a key, it's
//! interactive."
//!
//! Reads computed rects from [`UiState::computed_rects`] (populated by
//! the layout pass) — the tree carries identity (`computed_id`) but not
//! geometry.

use crate::event::UiTarget;
use crate::state::UiState;
use crate::tree::{El, Rect};

/// Find the topmost keyed node whose laid-out rect contains `point`
/// (logical pixels). Returns `None` if the point hits no keyed node.
pub fn hit_test(root: &El, ui_state: &UiState, point: (f32, f32)) -> Option<String> {
    hit_test_target(root, ui_state, point).map(|target| target.key)
}

/// Find the topmost keyed node and return full target metadata.
pub fn hit_test_target(root: &El, ui_state: &UiState, point: (f32, f32)) -> Option<UiTarget> {
    match hit_test_rec(root, ui_state, point, None) {
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
) -> Hit {
    if let Some(clip) = inherited_clip {
        if !clip.contains(point.0, point.1) {
            return Hit::Miss;
        }
    }
    let computed = ui_state.rect(&node.computed_id);
    if !computed.contains(point.0, point.1) {
        return Hit::Miss;
    }
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(computed),
        }
    } else {
        inherited_clip
    };
    // Children paint last → are on top → check first.
    for child in node.children.iter().rev() {
        match hit_test_rec(child, ui_state, point, child_clip) {
            Hit::Target(target) => return Hit::Target(target),
            Hit::Blocked => return Hit::Blocked,
            Hit::Miss => {}
        }
    }
    // No child hit. Self counts only if it has a key.
    if let Some(key) = &node.key {
        return Hit::Target(UiTarget {
            key: key.clone(),
            node_id: node.computed_id.clone(),
            rect: computed,
        });
    }
    if node.block_pointer {
        return Hit::Blocked;
    }
    Hit::Miss
}

/// Return the `computed_id` of the deepest scrollable container whose
/// laid-out rect contains `point`, respecting clipping ancestors.
/// Used to route wheel events.
pub(crate) fn scroll_target_at(root: &El, ui_state: &UiState, point: (f32, f32)) -> Option<String> {
    let mut hit = None;
    scroll_target_rec(root, ui_state, point, None, &mut hit);
    hit
}

fn scroll_target_rec(
    node: &El,
    ui_state: &UiState,
    point: (f32, f32),
    inherited_clip: Option<Rect>,
    out: &mut Option<String>,
) {
    if let Some(clip) = inherited_clip {
        if !clip.contains(point.0, point.1) {
            return;
        }
    }
    let computed = ui_state.rect(&node.computed_id);
    if !computed.contains(point.0, point.1) {
        return;
    }
    if node.scrollable {
        *out = Some(node.computed_id.clone());
    }
    let child_clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(computed),
        }
    } else {
        inherited_clip
    };
    for c in &node.children {
        scroll_target_rec(c, ui_state, point, child_clip, out);
    }
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
    fn unkeyed_blocking_node_stops_fallthrough() {
        use crate::tree::stack;
        let mut tree = stack([
            El::new(Kind::Scrim)
                .key("dismiss")
                .fill(crate::tokens::OVERLAY_SCRIM),
            El::new(Kind::Modal)
                .block_pointer()
                .width(Size::Fixed(100.0))
                .height(Size::Fixed(100.0)),
        ])
        .align(Align::Center)
        .justify(Justify::Center);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 300.0));

        assert!(hit_test(&tree, &state, (150.0, 150.0)).is_none());
        assert_eq!(
            hit_test(&tree, &state, (10.0, 10.0)).as_deref(),
            Some("dismiss")
        );
    }
}
