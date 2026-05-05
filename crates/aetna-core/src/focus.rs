//! Linear focus traversal — collects the focusable keyed nodes into the
//! order Tab/Shift-Tab walks. Ancestors with `clip` shrink the visible
//! rect so a focusable that's been scrolled out of view is dropped.
//!
//! Reads computed rects from `UiState`'s layout side map; the tree
//! itself only carries identity (`computed_id`).

use crate::event::UiTarget;
use crate::state::UiState;
use crate::tree::{El, Rect};

/// Find the focusable siblings inside the focused element's nearest
/// `arrow_nav_siblings` parent, returning them in tree order (so an
/// arrow-key handler can index them directly). Returns `None` when no
/// such parent contains the focused element — that's the signal that
/// arrow keys should fall through to the default `KeyDown` path.
///
/// Sibling collection mirrors [`focus_order`]: only `focusable` keyed
/// nodes that survive the inherited clip are included. The returned
/// list always contains the currently-focused element when one
/// matches; callers locate it by `node_id` to compute next / prev /
/// first / last.
pub fn arrow_nav_group(root: &El, ui_state: &UiState, focused_id: &str) -> Option<Vec<UiTarget>> {
    find_group(root, ui_state, None, focused_id)
}

fn find_group(
    node: &El,
    ui_state: &UiState,
    inherited_clip: Option<Rect>,
    focused_id: &str,
) -> Option<Vec<UiTarget>> {
    let computed = ui_state.rect(&node.computed_id);
    let clip = if node.clip {
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

    // If this node is an arrow-navigable parent, check whether the
    // focused element is one of its direct children. If so, this is
    // the group to return — collect its focusable siblings.
    if node.arrow_nav_siblings && node.children.iter().any(|c| c.computed_id == focused_id) {
        let mut siblings: Vec<UiTarget> = Vec::new();
        for child in &node.children {
            collect_focusable_self(child, ui_state, clip, &mut siblings);
        }
        return Some(siblings);
    }

    // Otherwise, recurse — the focused element might be inside a
    // deeper arrow-navigable group.
    for child in &node.children {
        if let Some(group) = find_group(child, ui_state, clip, focused_id) {
            return Some(group);
        }
    }
    None
}

/// Append `node`'s [`UiTarget`] if it's focusable, keyed, and inside
/// the visible clip. Mirrors the per-node rule used by [`focus_order`]
/// without recursing into descendants — the arrow-nav group is
/// strictly the immediate children of the navigable parent.
fn collect_focusable_self(
    node: &El,
    ui_state: &UiState,
    clip: Option<Rect>,
    out: &mut Vec<UiTarget>,
) {
    let computed = ui_state.rect(&node.computed_id);
    if node.focusable
        && let Some(key) = &node.key
        && clip
            .map(|c| c.intersect(computed).is_some())
            .unwrap_or(true)
    {
        out.push(UiTarget {
            key: key.clone(),
            node_id: node.computed_id.clone(),
            rect: computed,
        });
    }
}

/// Collect focusable, keyed nodes in tree order (Tab walks forward,
/// Shift-Tab walks backward). Nodes outside their inherited clip are
/// skipped.
pub fn focus_order(root: &El, ui_state: &UiState) -> Vec<UiTarget> {
    let mut out = Vec::new();
    collect_focus(root, ui_state, None, &mut out);
    out
}

fn collect_focus(
    node: &El,
    ui_state: &UiState,
    inherited_clip: Option<Rect>,
    out: &mut Vec<UiTarget>,
) {
    let computed = ui_state.rect(&node.computed_id);
    let clip = if node.clip {
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
    if node.focusable
        && let Some(key) = &node.key
        && clip
            .map(|c| c.intersect(computed).is_some())
            .unwrap_or(true)
    {
        out.push(UiTarget {
            key: key.clone(),
            node_id: node.computed_id.clone(),
            rect: computed,
        });
    }
    for child in &node.children {
        collect_focus(child, ui_state, clip, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::state::UiState;
    use crate::tree::*;
    use crate::{button, column, row};

    #[test]
    fn focus_order_collects_keyed_focusable_nodes() {
        let mut tree = column([
            crate::text("0"),
            row([button("-").key("dec"), button("+").key("inc")]),
        ])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let order = focus_order(&tree, &state);
        let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, vec!["dec", "inc"]);
    }
}
