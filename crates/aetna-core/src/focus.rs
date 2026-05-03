//! Linear focus traversal — collects the focusable keyed nodes into the
//! order Tab/Shift-Tab walks. Ancestors with `clip` shrink the visible
//! rect so a focusable that's been scrolled out of view is dropped.
//!
//! Reads computed rects from [`UiState::computed_rects`]; the tree
//! itself only carries identity (`computed_id`).

use crate::event::UiTarget;
use crate::state::UiState;
use crate::tree::{El, Rect};

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
