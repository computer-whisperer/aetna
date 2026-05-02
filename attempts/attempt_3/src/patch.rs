//! Patch protocol — typed structural edits on an [`El`] tree.
//!
//! Patches let an agent make surgical changes ("set the fill of node X")
//! without rewriting source files. Each [`Patch`] addresses a node by its
//! computed ID (the same ID that appears in the tree dump and lint output).
//!
//! Apply patches between layout passes. Layout assigns IDs from path +
//! role + sibling index, so a patch issued against ID `root.card[account]
//! .row.0.button.2` keeps working as long as the tree's structure around
//! that path doesn't change.
//!
//! Today's operations are intentionally narrow — enough to demonstrate the
//! pattern. Add more as the need is concrete.

use crate::tree::*;

/// A typed structural edit on a tree.
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // Patch isn't perf-critical; no need to box El
pub enum Patch {
    SetFill { id: String, color: Option<Color> },
    SetStroke { id: String, color: Option<Color> },
    SetStrokeWidth { id: String, width: f32 },
    SetText { id: String, text: String },
    SetTextColor { id: String, color: Color },
    SetState { id: String, state: InteractionState },
    SetGap { id: String, gap: f32 },
    SetPadding { id: String, padding: Sides },
    SetWidth { id: String, width: Size },
    SetHeight { id: String, height: Size },
    /// Replace the children of `id` entirely with `children`.
    ReplaceChildren { id: String, children: Vec<El> },
    /// Append a child to `id`.
    AppendChild { id: String, child: El },
    /// Remove a node by id (it becomes a no-op group with no fill/stroke).
    /// Keeps the tree shape stable for sibling indices.
    Remove { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchError {
    NodeNotFound(String),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::NodeNotFound(id) => write!(f, "node not found: {id}"),
        }
    }
}

impl std::error::Error for PatchError {}

impl Patch {
    pub fn id(&self) -> &str {
        match self {
            Patch::SetFill { id, .. }
            | Patch::SetStroke { id, .. }
            | Patch::SetStrokeWidth { id, .. }
            | Patch::SetText { id, .. }
            | Patch::SetTextColor { id, .. }
            | Patch::SetState { id, .. }
            | Patch::SetGap { id, .. }
            | Patch::SetPadding { id, .. }
            | Patch::SetWidth { id, .. }
            | Patch::SetHeight { id, .. }
            | Patch::ReplaceChildren { id, .. }
            | Patch::AppendChild { id, .. }
            | Patch::Remove { id } => id,
        }
    }
}

/// Apply a single patch. Returns an error if the target ID is not found.
///
/// Patches mutate the tree in place. They do *not* re-run layout — the
/// caller should call [`crate::layout::layout`] again before re-rendering.
pub fn apply(root: &mut El, patch: &Patch) -> Result<(), PatchError> {
    let id = patch.id();
    let node = find_mut(root, id).ok_or_else(|| PatchError::NodeNotFound(id.to_string()))?;
    match patch {
        Patch::SetFill { color, .. } => node.fill = *color,
        Patch::SetStroke { color, .. } => {
            node.stroke = *color;
            if color.is_some() && node.stroke_width == 0.0 {
                node.stroke_width = 1.0;
            }
        }
        Patch::SetStrokeWidth { width, .. } => node.stroke_width = *width,
        Patch::SetText { text, .. } => node.text = Some(text.clone()),
        Patch::SetTextColor { color, .. } => node.text_color = Some(*color),
        Patch::SetState { state, .. } => node.state = *state,
        Patch::SetGap { gap, .. } => node.gap = *gap,
        Patch::SetPadding { padding, .. } => node.padding = *padding,
        Patch::SetWidth { width, .. } => node.width = *width,
        Patch::SetHeight { height, .. } => node.height = *height,
        Patch::ReplaceChildren { children, .. } => node.children = children.clone(),
        Patch::AppendChild { child, .. } => node.children.push(child.clone()),
        Patch::Remove { .. } => {
            node.fill = None;
            node.stroke = None;
            node.text = None;
            node.children.clear();
        }
    }
    Ok(())
}

/// Apply a slice of patches in order, stopping at the first error.
pub fn apply_all(root: &mut El, patches: &[Patch]) -> Result<(), PatchError> {
    for p in patches {
        apply(root, p)?;
    }
    Ok(())
}

fn find_mut<'a>(root: &'a mut El, id: &str) -> Option<&'a mut El> {
    if root.computed_id == id {
        return Some(root);
    }
    for c in &mut root.children {
        if let Some(found) = find_mut(c, id) {
            return Some(found);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn fixture() -> El {
        column([
            text("hello").key("greeting"),
            button("Save").primary().key("save"),
        ])
    }

    fn lay_out(t: &mut El) {
        crate::layout::layout(t, Rect::new(0.0, 0.0, 320.0, 80.0));
    }

    #[test]
    fn patch_text() {
        let mut t = fixture();
        lay_out(&mut t);
        let save_id = "root.button[save]";
        apply(&mut t, &Patch::SetText { id: save_id.into(), text: "Apply".into() }).unwrap();
        let n = find_mut(&mut t, save_id).unwrap();
        assert_eq!(n.text.as_deref(), Some("Apply"));
    }

    #[test]
    fn patch_state_then_layout() {
        let mut t = fixture();
        lay_out(&mut t);
        let save_id = "root.button[save]";
        apply(&mut t, &Patch::SetState { id: save_id.into(), state: InteractionState::Disabled }).unwrap();
        crate::layout::layout(&mut t, Rect::new(0.0, 0.0, 320.0, 80.0));
        let n = find_mut(&mut t, save_id).unwrap();
        assert_eq!(n.state, InteractionState::Disabled);
    }

    #[test]
    fn unknown_id_errors() {
        let mut t = fixture();
        lay_out(&mut t);
        let err = apply(&mut t, &Patch::SetText { id: "nope".into(), text: "x".into() });
        assert!(matches!(err, Err(PatchError::NodeNotFound(_))));
    }
}
