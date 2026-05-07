use super::support::*;

#[test]
fn settled_mode_snaps_hover_envelope_to_one() {
    // Headless contract: Settled mode must produce the post-hover
    // envelope on a single prepare. A windowed runner (Live mode)
    // would ease over many frames; the fixture path can't wait.
    let (mut tree, mut state) = lay_out_counter();
    state.set_animation_mode(AnimationMode::Settled);
    state.hovered = Some(target(&tree, &state, "inc"));
    state.apply_to_state();

    let needs_redraw = state.tick_visual_animations(&mut tree, Instant::now());

    assert!(!needs_redraw, "Settled mode should never report in flight");
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::Hover),
        Some(1.0)
    );
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::Press),
        Some(0.0)
    );
    // The build fill stays untouched — the lightening happens in
    // apply_state at draw time, mixing by hover_amount.
}

#[test]
fn live_mode_eases_hover_envelope_over_multiple_ticks() {
    // After a single 8 ms tick the hover envelope should be
    // strictly between 0 and 1 — neither snapped to either end.
    let (mut tree, mut state) = lay_out_counter();
    let t0 = Instant::now();
    state.tick_visual_animations(&mut tree, t0);

    state.hovered = Some(target(&tree, &state, "inc"));
    state.apply_to_state();
    let needs_redraw =
        state.tick_visual_animations(&mut tree, t0 + std::time::Duration::from_millis(8));
    let mid = envelope_for(&tree, &state, "inc", EnvelopeKind::Hover).expect("hover envelope");

    assert!(
        needs_redraw,
        "spring should still be in flight after one 8 ms tick"
    );
    assert!(
        mid > 0.0 && mid < 1.0,
        "expected envelope mid-flight, got {mid}",
    );
}

#[test]
fn build_value_change_survives_hover_envelope() {
    // The point of envelopes: when the author swaps a button's fill
    // mid-hover, n.fill must reflect the new build value
    // immediately. The envelope keeps easing independently. This is
    // what avoids the AppFill / StateFill fight of an earlier draft.
    let mut tree_a =
        column([row([button("X").key("x").fill(Color::rgb(255, 0, 0))])]).padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.set_animation_mode(AnimationMode::Settled);
    state.hovered = Some(target(&tree_a, &state, "x"));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree_a, Instant::now());
    assert_eq!(
        envelope_for(&tree_a, &state, "x", EnvelopeKind::Hover),
        Some(1.0)
    );

    // Rebuild: same button, fill swapped to blue.
    let mut tree_b =
        column([row([button("X").key("x").fill(Color::rgb(0, 0, 255))])]).padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree_b, Instant::now());

    let observed = find_fill(&tree_b, "x").expect("x fill");
    assert_eq!(
        (observed.r, observed.g, observed.b),
        (0, 0, 255),
        "build fill should pass through unchanged — envelope handles state delta separately",
    );
    assert_eq!(
        envelope_for(&tree_b, &state, "x", EnvelopeKind::Hover),
        Some(1.0)
    );
}

#[test]
fn focus_ring_alpha_eases_in_and_out() {
    let (mut tree, mut state) = lay_out_counter();
    state.set_animation_mode(AnimationMode::Settled);

    // No focus → alpha settled at 0.
    state.tick_visual_animations(&mut tree, Instant::now());
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
        Some(0.0)
    );

    // Focus on inc → alpha settles at 1.0.
    let (mut tree, _) = lay_out_counter();
    // Re-layout against the existing state so the rect map is fresh.
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.focused = Some(target(&tree, &state, "inc"));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now());
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
        Some(1.0)
    );

    // Lose focus → alpha settles back to 0.
    let (mut tree, _) = lay_out_counter();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.focused = None;
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now());
    assert_eq!(
        envelope_for(&tree, &state, "inc", EnvelopeKind::FocusRing),
        Some(0.0)
    );
}

#[test]
fn app_fill_settles_to_new_value_in_settled_mode() {
    // .animate(SPRING_STANDARD) on a node whose fill changes
    // between rebuilds. Settled mode should produce the new fill
    // on the very first tick after the change.
    use crate::anim::Timing;
    let mut tree_a = column([
        crate::text("0"),
        row([button("X")
            .key("x")
            .fill(Color::rgb(255, 0, 0))
            .animate(Timing::SPRING_STANDARD)]),
    ])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

    state.set_animation_mode(AnimationMode::Settled);
    state.tick_visual_animations(&mut tree_a, Instant::now());
    assert_eq!(
        find_fill(&tree_a, "x").map(|c| (c.r, c.g, c.b)),
        Some((255, 0, 0))
    );

    // Rebuild with a different fill; tracker eases through.
    let mut tree_b = column([
        crate::text("0"),
        row([button("X")
            .key("x")
            .fill(Color::rgb(0, 0, 255))
            .animate(Timing::SPRING_STANDARD)]),
    ])
    .padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_b, Instant::now());

    assert_eq!(
        find_fill(&tree_b, "x").map(|c| (c.r, c.g, c.b)),
        Some((0, 0, 255)),
        "Settled mode should snap to the new build value",
    );
}

#[test]
fn app_fill_eases_in_live_mode() {
    // Same setup as above but in Live mode: after a small dt the
    // colour should be partway between red and blue, not at either.
    use crate::anim::Timing;
    let mut tree_a = column([row([button("X")
        .key("x")
        .fill(Color::rgb(255, 0, 0))
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

    let t0 = Instant::now();
    state.tick_visual_animations(&mut tree_a, t0);

    let mut tree_b = column([row([button("X")
        .key("x")
        .fill(Color::rgb(0, 0, 255))
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    let needs_redraw =
        state.tick_visual_animations(&mut tree_b, t0 + std::time::Duration::from_millis(8));
    let mid = find_fill(&tree_b, "x").expect("mid fill");

    assert!(
        needs_redraw,
        "spring should still be in flight after one tick"
    );
    assert!(
        mid.r < 255 && mid.b < 255,
        "expected mid-flight, got {mid:?}",
    );
    assert!(mid.r > 0 || mid.b > 0, "should have moved off the start",);
}

#[test]
fn app_translate_eases_on_rebuild() {
    use crate::anim::Timing;
    let mut tree_a = column([row([button("slide")
        .key("s")
        .translate(0.0, 0.0)
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.set_animation_mode(AnimationMode::Settled);
    state.tick_visual_animations(&mut tree_a, Instant::now());

    // Rebuild with a different translate.
    let mut tree_b = column([row([button("slide")
        .key("s")
        .translate(100.0, 50.0)
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_b, Instant::now());

    let n = find_node(&tree_b, "s").expect("s node");
    assert!((n.translate.0 - 100.0).abs() < 0.5);
    assert!((n.translate.1 - 50.0).abs() < 0.5);
}

#[test]
fn state_envelope_composes_on_app_eased_fill() {
    // A keyed interactive node with .animate() AND being hovered.
    // After Settled tick: n.fill = (eased) build value, hover
    // envelope = 1. draw_ops in apply_state then mixes the build
    // colour toward its lightened version by the envelope amount.
    // Since the envelope is at 1, the emitted Quad's fill should
    // equal lighten(build_fill, HOVER_LIGHTEN).
    use crate::anim::Timing;
    let mut tree = column([row([button("X")
        .key("x")
        .fill(Color::rgb(100, 100, 100))
        .animate(Timing::SPRING_STANDARD)])])
    .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

    state.set_animation_mode(AnimationMode::Settled);
    state.hovered = Some(target(&tree, &state, "x"));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree, Instant::now());

    // Build fill survives untouched (envelope handles the delta).
    let n_fill = find_fill(&tree, "x").expect("x fill");
    assert_eq!((n_fill.r, n_fill.g, n_fill.b), (100, 100, 100));
    assert_eq!(
        envelope_for(&tree, &state, "x", EnvelopeKind::Hover),
        Some(1.0)
    );
}

#[test]
fn app_animation_skipped_when_animate_not_set() {
    // Without .animate(), app props are not tracked — the node's
    // fill snaps to whatever the build produces, no easing.
    let mut tree_a = column([row([button("X").key("x").fill(Color::rgb(255, 0, 0))])]) // no .animate()
        .padding(20.0);
    let mut state = UiState::new();
    layout(&mut tree_a, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_a, Instant::now());

    let mut tree_b =
        column([row([button("X").key("x").fill(Color::rgb(0, 0, 255))])]).padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_b, Instant::now());

    let observed = find_fill(&tree_b, "x").expect("x fill");
    assert_eq!(
        (observed.r, observed.g, observed.b),
        (0, 0, 255),
        "no .animate() — value should snap",
    );
}

#[test]
fn animation_entries_gc_when_node_leaves_tree() {
    // Build a tree with two buttons; hover one to seed an entry.
    // Then build a different tree with only one button. The orphan's
    // animation entries should be trimmed.
    let (mut tree_a, mut state) = lay_out_counter();
    state.hovered = Some(target(&tree_a, &state, "inc"));
    state.apply_to_state();
    state.tick_visual_animations(&mut tree_a, Instant::now());
    let inc_id_a = find_id(&tree_a, "inc").expect("inc id");
    assert!(
        state
            .animation
            .animations
            .keys()
            .any(|(id, _)| id == &inc_id_a),
        "expected at least one entry for inc"
    );

    // Rebuild with only the dec button. inc entries should be gone.
    let mut tree_b = column([crate::text("0"), row([button("-").key("dec")])]).padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.hovered = None;
    state.apply_to_state();
    state.tick_visual_animations(&mut tree_b, Instant::now());
    assert!(
        !state
            .animation
            .animations
            .keys()
            .any(|(id, _)| id == &inc_id_a),
        "stale entries for inc were not GC'd"
    );
}
