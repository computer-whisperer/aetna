//! Visual-state animation tick — drives state envelopes (hover / press /
//! focus ring) and app-driven prop tracks (`fill`, `text_color`, etc.).
//!
//! The animation map (`(computed_id, AnimProp) → Animation`) lives in
//! [`crate::state::UiState`]. So does the envelope side map. This module
//! owns the per-node walker that retargets, steps, and writes back —
//! state envelopes go to `UiState::envelopes` (read by `draw_ops`), app
//! props mutate the `El`'s author fields directly so the next build
//! reads the eased value.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::anim::{AnimProp, AnimValue, Animation, Timing};
use crate::state::{AnimationMode, EnvelopeKind};
use crate::tree::{El, InteractionState};

/// App-driven props, processed *first* on nodes with `n.animate` set.
/// They write eased build-time values back to `n.fill` etc., so the
/// state pass that follows reads the already-eased value when computing
/// hover / press deltas. State visuals therefore compose on top of
/// app-driven motion without either tracker fighting the other.
const APP_PROPS: &[AnimProp] = &[
    AnimProp::AppFill,
    AnimProp::AppStroke,
    AnimProp::AppTextColor,
    AnimProp::AppOpacity,
    AnimProp::AppScale,
    AnimProp::AppTranslateX,
    AnimProp::AppTranslateY,
];

/// State-driven envelopes, processed *after* app props. Always tracked
/// on keyed interactive nodes — no author opt-in. Each is a 0..1 amount
/// written to `UiState::envelopes`; `apply_state` in `draw_ops` mixes
/// the build-time visual toward the state-modulated visual based on it.
const STATE_PROPS: &[AnimProp] = &[
    AnimProp::HoverAmount,
    AnimProp::PressAmount,
    AnimProp::FocusRingAlpha,
];

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_node(
    node: &mut El,
    anims: &mut HashMap<(String, AnimProp), Animation>,
    envelopes: &mut HashMap<(String, EnvelopeKind), f32>,
    node_states: &HashMap<String, InteractionState>,
    visited: &mut HashSet<(String, AnimProp)>,
    now: Instant,
    mode: AnimationMode,
    needs_redraw: &mut bool,
) {
    if !node.computed_id.is_empty() {
        // App-driven props: only on nodes that opted in via .animate().
        if let Some(timing) = node.animate {
            for &prop in APP_PROPS {
                process_prop(
                    node, prop, timing, anims, envelopes, node_states, visited, now, mode,
                    needs_redraw,
                );
            }
        }
        // State-driven props: only on keyed interactive nodes; the
        // library always tracks these, no author opt-in.
        if node.key.is_some() {
            for &prop in STATE_PROPS {
                let timing = state_timing_for(prop);
                process_prop(
                    node, prop, timing, anims, envelopes, node_states, visited, now, mode,
                    needs_redraw,
                );
            }
        }
    }
    for child in &mut node.children {
        tick_node(
            child, anims, envelopes, node_states, visited, now, mode, needs_redraw,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn process_prop(
    node: &mut El,
    prop: AnimProp,
    timing: Timing,
    anims: &mut HashMap<(String, AnimProp), Animation>,
    envelopes: &mut HashMap<(String, EnvelopeKind), f32>,
    node_states: &HashMap<String, InteractionState>,
    visited: &mut HashSet<(String, AnimProp)>,
    now: Instant,
    mode: AnimationMode,
    needs_redraw: &mut bool,
) {
    let state = node_states
        .get(&node.computed_id)
        .copied()
        .unwrap_or_default();
    let Some(target) = compute_target(node, prop, state) else {
        return;
    };
    let key = (node.computed_id.clone(), prop);
    visited.insert(key.clone());
    let anim = anims
        .entry(key)
        .or_insert_with(|| Animation::new(target, target, timing, now));
    anim.retarget(target, now);
    let settled = match mode {
        AnimationMode::Live => anim.step(now),
        AnimationMode::Settled => {
            anim.settle();
            true
        }
    };
    write_prop(node, prop, anim.current, envelopes);
    if !settled {
        *needs_redraw = true;
    }
}

/// Compute the visual target for `prop` based on the node's current
/// interaction state and its build-closure-supplied original value.
/// Returns `None` if the prop doesn't apply (e.g., a node with no fill
/// has no `AppFill` to animate).
fn compute_target(n: &El, prop: AnimProp, state: InteractionState) -> Option<AnimValue> {
    match prop {
        AnimProp::HoverAmount => Some(AnimValue::Float(
            if matches!(state, InteractionState::Hover) { 1.0 } else { 0.0 },
        )),
        AnimProp::PressAmount => Some(AnimValue::Float(
            if matches!(state, InteractionState::Press) { 1.0 } else { 0.0 },
        )),
        AnimProp::FocusRingAlpha => Some(AnimValue::Float(
            if matches!(state, InteractionState::Focus) { 1.0 } else { 0.0 },
        )),
        AnimProp::AppFill => n.fill.map(AnimValue::Color),
        AnimProp::AppStroke => n.stroke.map(AnimValue::Color),
        AnimProp::AppTextColor => n.text_color.map(AnimValue::Color),
        AnimProp::AppOpacity => Some(AnimValue::Float(n.opacity)),
        AnimProp::AppScale => Some(AnimValue::Float(n.scale)),
        AnimProp::AppTranslateX => Some(AnimValue::Float(n.translate.0)),
        AnimProp::AppTranslateY => Some(AnimValue::Float(n.translate.1)),
    }
}

/// Library-default timing for state-driven envelopes. Hover, press,
/// focus transitions are uniformly snappy — overshoot on a 0..1
/// envelope reads as jitter, so we stick to a near-critical preset.
fn state_timing_for(prop: AnimProp) -> Timing {
    match prop {
        AnimProp::HoverAmount
        | AnimProp::PressAmount
        | AnimProp::FocusRingAlpha => Timing::SPRING_QUICK,
        // App props don't reach this function — they pull timing from
        // the per-node `animate` setting in `tick_node`.
        _ => Timing::SPRING_QUICK,
    }
}

fn write_prop(
    n: &mut El,
    prop: AnimProp,
    value: AnimValue,
    envelopes: &mut HashMap<(String, EnvelopeKind), f32>,
) {
    match (prop, value) {
        (AnimProp::AppFill, AnimValue::Color(c)) => n.fill = Some(c),
        (AnimProp::AppStroke, AnimValue::Color(c)) => n.stroke = Some(c),
        (AnimProp::AppTextColor, AnimValue::Color(c)) => n.text_color = Some(c),
        (AnimProp::HoverAmount, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::Hover),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::PressAmount, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::Press),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::FocusRingAlpha, AnimValue::Float(v)) => {
            envelopes.insert(
                (n.computed_id.clone(), EnvelopeKind::FocusRing),
                v.clamp(0.0, 1.0),
            );
        }
        (AnimProp::AppOpacity, AnimValue::Float(v)) => n.opacity = v.clamp(0.0, 1.0),
        (AnimProp::AppScale, AnimValue::Float(v)) => n.scale = v.max(0.0),
        (AnimProp::AppTranslateX, AnimValue::Float(v)) => n.translate.0 = v,
        (AnimProp::AppTranslateY, AnimValue::Float(v)) => n.translate.1 = v,
        _ => {}
    }
}

pub(crate) fn is_in_flight(anim: &Animation) -> bool {
    let cur = anim.current.channels();
    let tgt = anim.target.channels();
    if cur.n != tgt.n {
        return true;
    }
    for i in 0..cur.n {
        if (cur.v[i] - tgt.v[i]).abs() > f32::EPSILON {
            return true;
        }
        if anim.velocity.n == cur.n && anim.velocity.v[i].abs() > f32::EPSILON {
            return true;
        }
    }
    false
}
