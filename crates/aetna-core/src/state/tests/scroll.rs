use super::support::*;
use crate::scroll::{ScrollAlignment, ScrollRequest};
use crate::tree::{virtual_list, virtual_list_dyn};

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

fn fixed_list_root(count: usize, row_height: f32) -> El {
    virtual_list(count, row_height, |i| {
        crate::widgets::text::text(format!("r{i}"))
    })
    .key("list")
}

fn dyn_list_root(count: usize, est: f32, row_h: f32) -> El {
    virtual_list_dyn(count, est, move |i| {
        column([crate::widgets::text::text(format!("r{i}"))])
            .key(format!("row-{i}"))
            .height(Size::Fixed(row_h))
    })
    .key("dyn-list")
}

#[test]
fn scroll_request_start_aligns_row_top_to_viewport_top() {
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new("list", 10, ScrollAlignment::Start)]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    let stored = state.scroll_offset(&tree.computed_id);
    assert!(
        (stored - 500.0).abs() < 0.5,
        "expected offset 500, got {stored}"
    );
}

#[test]
fn scroll_request_end_aligns_row_bottom_to_viewport_bottom() {
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new("list", 10, ScrollAlignment::End)]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 350.0).abs() < 0.5);
}

#[test]
fn scroll_request_center_centres_row_in_viewport() {
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new(
        "list",
        10,
        ScrollAlignment::Center,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 425.0).abs() < 0.5);
}

#[test]
fn scroll_request_visible_is_noop_when_row_already_in_viewport() {
    // 50 rows × 50px, viewport 200px, current offset 100 → rows 2..6
    // visible. Requesting row 3 with Visible should leave offset
    // unchanged.
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    assign_ids(&mut tree);
    state.scroll.offsets.insert(tree.computed_id.clone(), 100.0);
    state.push_scroll_requests(vec![ScrollRequest::new(
        "list",
        3,
        ScrollAlignment::Visible,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 100.0).abs() < 0.5);
}

#[test]
fn scroll_request_visible_scrolls_min_distance_for_offscreen_row() {
    // Same 50 × 50px list; current offset 100; row 20 is below the
    // viewport (rows 2..6 visible). Visible alignment → scroll just
    // enough to put row 20's bottom at the viewport bottom: 21*50 -
    // 200 = 850.
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    assign_ids(&mut tree);
    state.scroll.offsets.insert(tree.computed_id.clone(), 100.0);
    state.push_scroll_requests(vec![ScrollRequest::new(
        "list",
        20,
        ScrollAlignment::Visible,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 850.0).abs() < 0.5);
}

#[test]
fn scroll_request_clamps_to_max_offset() {
    // Resolved offset can land past content end (e.g. End on the last
    // row); write_virtual_scroll_state must clamp it to [0, max].
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new("list", 49, ScrollAlignment::End)]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 2300.0).abs() < 0.5);
}

#[test]
fn scroll_request_unknown_list_drops_silently() {
    // Request targets a list that's not in the tree; offset must
    // remain at its initial value, and the queue must be empty after
    // layout (clean drop, not a leak).
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new(
        "no-such-list",
        10,
        ScrollAlignment::Start,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));
    state.clear_pending_scroll_requests();

    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
    assert!(state.scroll.pending_requests.is_empty());
}

#[test]
fn scroll_request_out_of_range_row_drops_silently() {
    // count = 50, request row 999. The matching list takes the
    // request from the queue (so it doesn't leak), then drops it
    // because the row index is past the end.
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new(
        "list",
        999,
        ScrollAlignment::Start,
    )]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 0.0).abs() < 0.5);
    assert!(state.scroll.pending_requests.is_empty());
}

#[test]
fn scroll_request_first_frame_works_without_prior_layout() {
    // The whole point of the deferred design: the app pushes the
    // request before any layout has run, the very first layout pass
    // resolves it correctly because viewport and row geometry are
    // available right then.
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![ScrollRequest::new("list", 25, ScrollAlignment::Start)]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 1250.0).abs() < 0.5);
}

#[test]
fn scroll_request_resolves_against_dynamic_list_with_warm_cache() {
    // Measured rows use the cached height; unmeasured rows above the
    // target fall back to the configured estimate. The expected
    // offset is recomputed below from the actual measured_count
    // because layout decides how many rows fit in the first frame.
    let mut tree = dyn_list_root(50, 50.0, 30.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    let measured_count = state
        .scroll
        .measured_row_heights
        .get(&tree.computed_id)
        .map(|m| m.len())
        .unwrap_or(0);
    assert!(
        measured_count >= 7,
        "expected first frame to measure ≥7 rows, got {measured_count}"
    );
    let id = tree.computed_id.clone();

    let mut tree2 = dyn_list_root(50, 50.0, 30.0);
    state.push_scroll_requests(vec![ScrollRequest::new(
        "dyn-list",
        10,
        ScrollAlignment::Start,
    )]);
    layout(&mut tree2, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    let expected = (measured_count.min(10)) as f32 * 30.0
        + (10_usize.saturating_sub(measured_count)) as f32 * 50.0;
    let stored = state.scroll_offset(&id);
    assert!(
        (stored - expected).abs() < 0.5,
        "expected offset {expected}, got {stored}"
    );
}

#[test]
fn scroll_request_last_match_wins_when_multiple_target_same_list() {
    let mut tree = fixed_list_root(50, 50.0);
    let mut state = UiState::new();
    state.push_scroll_requests(vec![
        ScrollRequest::new("list", 5, ScrollAlignment::Start),
        ScrollRequest::new("list", 30, ScrollAlignment::Start),
    ]);
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 300.0, 200.0));

    assert!((state.scroll_offset(&tree.computed_id) - 1500.0).abs() < 0.5);
}
