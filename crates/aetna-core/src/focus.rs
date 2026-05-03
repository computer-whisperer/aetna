//! Linear focus traversal — collects the focusable keyed nodes into the
//! order Tab/Shift-Tab walks. Ancestors with `clip` shrink the visible
//! rect so a focusable that's been scrolled out of view is dropped.
//!
//! Rich composites can layer roving focus on top later; this is the
//! library default.

use crate::event::UiTarget;
use crate::tree::{El, Rect};

/// Collect focusable, keyed nodes in tree order (Tab walks forward,
/// Shift-Tab walks backward). Nodes outside their inherited clip are
/// skipped.
pub fn focus_order(root: &El) -> Vec<UiTarget> {
    let mut out = Vec::new();
    collect_focus(root, None, &mut out);
    out
}

fn collect_focus(node: &El, inherited_clip: Option<Rect>, out: &mut Vec<UiTarget>) {
    let clip = if node.clip {
        match inherited_clip {
            Some(clip) => Some(
                clip.intersect(node.computed)
                    .unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            ),
            None => Some(node.computed),
        }
    } else {
        inherited_clip
    };
    if node.focusable {
        if let Some(key) = &node.key {
            if clip
                .map(|c| c.intersect(node.computed).is_some())
                .unwrap_or(true)
            {
                out.push(UiTarget {
                    key: key.clone(),
                    node_id: node.computed_id.clone(),
                    rect: node.computed,
                });
            }
        }
    }
    for child in &node.children {
        collect_focus(child, clip, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::layout;
    use crate::tree::*;
    use crate::{button, column, row};

    #[test]
    fn focus_order_collects_keyed_focusable_nodes() {
        let mut tree = column([
            crate::text("0"),
            row([button("-").key("dec"), button("+").key("inc")]),
        ])
        .padding(20.0);
        layout(&mut tree, Rect::new(0.0, 0.0, 400.0, 200.0));

        let order = focus_order(&tree);
        let keys: Vec<&str> = order.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, vec!["dec", "inc"]);
    }
}
