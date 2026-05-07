use super::support::*;

#[test]
fn sync_focus_order_preserves_existing_focus_by_node_id() {
    let (tree, mut state) = lay_out_counter();
    state.sync_focus_order(&tree);
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
    state.focus_next();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("dec"));
    state.focus_next();
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));

    let (rebuilt, _) = lay_out_counter();
    state.sync_focus_order(&rebuilt);
    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), Some("inc"));
}
#[test]
fn stale_focus_clears_on_rebuild() {
    let (tree, mut state) = lay_out_counter();
    state.focused = Some(UiTarget {
        key: "gone".into(),
        node_id: "root.missing".into(),
        rect: Rect::default(),
    });

    state.sync_focus_order(&tree);

    assert_eq!(state.focused.as_ref().map(|t| t.key.as_str()), None);
}
