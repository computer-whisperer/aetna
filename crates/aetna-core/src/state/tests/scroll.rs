use super::support::*;

#[test]
fn hit_test_through_scrolled_content() {
    // Three 60px buttons in a 100px-tall scroll viewport. The
    // second button is initially below the visible area.
    // After scrolling 60px, button[1] is now at the top.
    let mut tree = scroll([
        button("zero").key("b0").height(Size::Fixed(60.0)),
        button("one").key("b1").height(Size::Fixed(60.0)),
        button("two").key("b2").height(Size::Fixed(60.0)),
    ])
    .key("list")
    .height(Size::Fixed(100.0));
    let mut state = UiState::new();
    assign_ids(&mut tree);
    state.scroll.offsets.insert(tree.computed_id.clone(), 60.0);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));

    // Buttons hug their text width — click at b1's center after the
    // scroll shift to land inside its actual rect.
    let r1 = find_rect(&tree, &state, "b1").expect("b1 rect");
    let hit = hit_test(&tree, &state, (r1.center_x(), r1.center_y()));
    assert_eq!(hit.as_deref(), Some("b1"));

    // b0 has been scrolled above the viewport — clicking where it
    // would now sit (above y=0) misses it.
    let r0 = find_rect(&tree, &state, "b0").expect("b0 rect");
    assert!(
        r0.bottom() <= 0.0,
        "b0 should be above the viewport, was {:?}",
        r0
    );
}
#[test]
fn pointer_wheel_routes_to_deepest_scrollable() {
    // Outer scroll containing an inner scroll. Wheel events at the
    // inner's center should target the inner.
    let mut tree = scroll([
        button("above").key("above").height(Size::Fixed(40.0)),
        scroll([button("inner-row")
            .key("inner-row")
            .height(Size::Fixed(60.0))])
        .key("inner")
        .height(Size::Fixed(100.0)),
    ])
    .key("outer")
    .height(Size::Fixed(300.0));
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 200.0, 300.0));

    let inner_rect = find_rect(&tree, &state, "inner-row").expect("inner row rect");
    let routed = state.pointer_wheel(&tree, (inner_rect.center_x(), inner_rect.center_y()), 30.0);
    assert!(routed, "wheel should route to a scrollable");
    // Inner's id includes its key.
    let inner_id = find_id_for_kind(&tree, "inner").expect("inner id");
    assert!(
        state.scroll.offsets.contains_key(&inner_id),
        "expected inner offset, got {:?}",
        state.scroll.offsets.keys().collect::<Vec<_>>()
    );
}
