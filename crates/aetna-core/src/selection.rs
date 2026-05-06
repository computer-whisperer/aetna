//! Library-level text selection model.
//!
//! Selection is a single, application-owned [`Selection`] value that
//! identifies *which* keyed text-bearing element holds the active
//! selection and *where* in that element's text the anchor and head
//! sit. The library enforces the single-selection invariant by
//! emitting `SelectionChanged` events; the app folds them into its
//! `Selection` field the same way it folds `apply_event` results into
//! a [`crate::widgets::text_input::TextSelection`] today.
//!
//! # Model
//!
//! - [`Selection`] — the slot, holds an `Option<SelectionRange>`.
//! - [`SelectionRange`] — anchor + head, both [`SelectionPoint`].
//! - [`SelectionPoint`] — `(key, byte)`. The key references the same
//!   widget-key form that `focus_order` already uses; the byte indexes
//!   into that element's text content.
//!
//! When `anchor.key == head.key` the selection lives entirely inside
//! one leaf — equivalent to a [`crate::widgets::text_input::TextSelection`]
//! over that leaf's text. When they differ, the selection spans
//! multiple leaves in document order.
//!
//! # Key requirement
//!
//! Selectable leaves must carry an explicit `.key(...)` — same
//! convention as focusable widgets. Without a key the leaf is silently
//! excluded from `selection_order` because nothing could survive a
//! tree rebuild as a stable identity. See [`crate::tree::El::selectable`].

use crate::tree::{El, Kind};
use crate::widgets::text_input::TextSelection;

/// The application's single selection slot. `None` means nothing is
/// selected. The library emits `SelectionChanged` events that fold
/// into this; widgets read it back to draw highlight bands and route
/// editing operations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub range: Option<SelectionRange>,
}

/// A non-empty selection range. `anchor` is where the user started
/// (pointer-down); `head` is where they ended up (pointer current /
/// last move). The pair may be in tree order or reversed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionRange {
    pub anchor: SelectionPoint,
    pub head: SelectionPoint,
}

/// A point inside a selectable leaf's text content. `key` is the
/// widget key that owns the leaf; `byte` is a byte offset into that
/// leaf's text (clamped to a UTF-8 char boundary by anything that
/// reads or writes it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionPoint {
    pub key: String,
    pub byte: usize,
}

impl SelectionPoint {
    pub fn new(key: impl Into<String>, byte: usize) -> Self {
        Self {
            key: key.into(),
            byte,
        }
    }
}

impl Selection {
    /// A collapsed caret at `(key, byte)`. Convenience for tests and
    /// app-side initialization.
    pub fn caret(key: impl Into<String>, byte: usize) -> Self {
        let pt = SelectionPoint::new(key, byte);
        Self {
            range: Some(SelectionRange {
                anchor: pt.clone(),
                head: pt,
            }),
        }
    }

    /// True when there is no active selection.
    pub fn is_empty(&self) -> bool {
        self.range.is_none()
    }

    /// True when the selection lives entirely inside `key` — both
    /// anchor and head reference it. False for cross-element
    /// selections and for the empty selection.
    pub fn is_within(&self, key: &str) -> bool {
        match &self.range {
            Some(r) => r.anchor.key == key && r.head.key == key,
            None => false,
        }
    }

    /// True when `key` is the anchor's key (the originating leaf).
    pub fn anchored_at(&self, key: &str) -> bool {
        self.range.as_ref().is_some_and(|r| r.anchor.key == key)
    }

    /// View the selection through one leaf's lens: returns
    /// `Some(TextSelection)` only when the selection lives entirely
    /// inside `key`. Cross-element selections return `None` here —
    /// callers that need a per-leaf slice for a spanned leaf should
    /// instead consult the document-order range.
    pub fn within(&self, key: &str) -> Option<TextSelection> {
        let r = self.range.as_ref()?;
        if r.anchor.key == key && r.head.key == key {
            Some(TextSelection {
                anchor: r.anchor.byte,
                head: r.head.byte,
            })
        } else {
            None
        }
    }

    /// Replace this selection's slice for `key` from a freshly
    /// produced [`TextSelection`]. Used by editable widgets after
    /// folding an event: take the slice via [`Self::within`], let the
    /// widget mutate it, and write it back. No-op when the selection
    /// isn't currently within `key`.
    pub fn set_within(&mut self, key: &str, sel: TextSelection) {
        let Some(r) = self.range.as_mut() else { return };
        if r.anchor.key == key && r.head.key == key {
            r.anchor.byte = sel.anchor;
            r.head.byte = sel.head;
        }
    }

    /// Clear the selection.
    pub fn clear(&mut self) {
        self.range = None;
    }
}

/// Walk `tree` and return the substring covered by `selection`.
/// Returns `None` for an empty selection or when the selection
/// references a key with no matching keyed text leaf in the tree.
///
/// For single-leaf selections (the only kind P1a renders) the
/// returned string is `value[lo..hi]` for that leaf. Cross-leaf
/// selections walk in tree order: anchor leaf from anchor.byte to
/// end, every leaf strictly between anchor and head fully, head leaf
/// up to head.byte. Joined with `\n` between leaves.
pub fn selected_text(tree: &El, selection: &Selection) -> Option<String> {
    let r = selection.range.as_ref()?;
    if r.anchor.key == r.head.key {
        let value = find_keyed_text(tree, &r.anchor.key)?;
        let lo = r.anchor.byte.min(r.head.byte).min(value.len());
        let hi = r.anchor.byte.max(r.head.byte).min(value.len());
        if lo >= hi {
            return None;
        }
        return Some(value[lo..hi].to_string());
    }
    // Cross-leaf walk in tree order.
    let mut leaves: Vec<(String, String)> = Vec::new();
    collect_keyed_text_leaves(tree, &mut leaves);
    let anchor_idx = leaves.iter().position(|(k, _)| *k == r.anchor.key)?;
    let head_idx = leaves.iter().position(|(k, _)| *k == r.head.key)?;
    let (lo_idx, lo_byte, hi_idx, hi_byte) = if anchor_idx <= head_idx {
        (anchor_idx, r.anchor.byte, head_idx, r.head.byte)
    } else {
        (head_idx, r.head.byte, anchor_idx, r.anchor.byte)
    };
    let mut out = String::new();
    for (i, (_, value)) in leaves.iter().enumerate().skip(lo_idx).take(hi_idx - lo_idx + 1) {
        let start = if i == lo_idx {
            lo_byte.min(value.len())
        } else {
            0
        };
        let end = if i == hi_idx {
            hi_byte.min(value.len())
        } else {
            value.len()
        };
        if start >= end {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&value[start..end]);
    }
    if out.is_empty() { None } else { Some(out) }
}

fn find_keyed_text(node: &El, key: &str) -> Option<String> {
    if matches!(node.kind, Kind::Text | Kind::Heading)
        && node.key.as_deref() == Some(key)
        && let Some(t) = &node.text
    {
        return Some(t.clone());
    }
    node.children.iter().find_map(|c| find_keyed_text(c, key))
}

fn collect_keyed_text_leaves(node: &El, out: &mut Vec<(String, String)>) {
    if matches!(node.kind, Kind::Text | Kind::Heading)
        && let (Some(k), Some(t)) = (&node.key, &node.text)
    {
        out.push((k.clone(), t.clone()));
    }
    for c in &node.children {
        collect_keyed_text_leaves(c, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_selection_has_no_views() {
        let sel = Selection::default();
        assert!(sel.is_empty());
        assert!(!sel.is_within("name"));
        assert!(sel.within("name").is_none());
    }

    #[test]
    fn caret_constructor_is_within_its_key() {
        let sel = Selection::caret("name", 3);
        assert!(!sel.is_empty());
        assert!(sel.is_within("name"));
        assert!(!sel.is_within("email"));
        let view = sel.within("name").expect("within name");
        assert_eq!(view, TextSelection::caret(3));
    }

    #[test]
    fn within_returns_none_for_cross_element_selection() {
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("para_a", 0),
                head: SelectionPoint::new("para_b", 5),
            }),
        };
        // Cross-element: neither lens reveals the full selection.
        assert!(sel.within("para_a").is_none());
        assert!(sel.within("para_b").is_none());
        // But the originating-leaf check still works.
        assert!(sel.anchored_at("para_a"));
        assert!(!sel.anchored_at("para_b"));
    }

    #[test]
    fn set_within_writes_back_a_modified_slice() {
        let mut sel = Selection::caret("name", 0);
        let mut view = sel.within("name").expect("caret");
        view.head = 5; // simulate widget editing the slice
        sel.set_within("name", view);
        let view_back = sel.within("name").expect("still within name");
        assert_eq!(view_back, TextSelection::range(0, 5));
    }

    #[test]
    fn set_within_is_a_noop_when_selection_is_not_in_key() {
        let mut sel = Selection::caret("name", 0);
        sel.set_within("email", TextSelection::range(0, 9));
        // Selection unchanged.
        assert_eq!(sel.within("name"), Some(TextSelection::caret(0)));
        assert!(sel.within("email").is_none());
    }

    #[test]
    fn selected_text_returns_single_leaf_substring() {
        let tree = crate::widgets::text::text("Hello, world!").key("p");
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("p", 7),
                head: SelectionPoint::new("p", 12),
            }),
        };
        assert_eq!(selected_text(&tree, &sel).as_deref(), Some("world"));
    }

    #[test]
    fn selected_text_walks_tree_order_for_cross_leaf_selection() {
        let tree = crate::column([
            crate::widgets::text::text("alpha").key("a"),
            crate::widgets::text::text("bravo").key("b"),
            crate::widgets::text::text("charlie").key("c"),
        ]);
        // Anchor inside "alpha" at byte 2, head inside "charlie" at
        // byte 4 — should yield "pha\nbravo\nchar" (joined by newline
        // between leaves; full middle leaf included).
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("a", 2),
                head: SelectionPoint::new("c", 4),
            }),
        };
        assert_eq!(
            selected_text(&tree, &sel).as_deref(),
            Some("pha\nbravo\nchar")
        );
    }

    #[test]
    fn selected_text_returns_none_for_empty_or_unknown_keys() {
        let tree = crate::widgets::text::text("hi").key("p");
        assert!(selected_text(&tree, &Selection::default()).is_none());
        let unknown = Selection::caret("missing", 0);
        assert!(selected_text(&tree, &unknown).is_none());
    }
}
