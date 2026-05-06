//! Slider — track + fill + thumb, value normalized to `0.0..=1.0`.
//!
//! Apps own the underlying value (and any range conversion). The
//! widget is a pure visual + identity carrier:
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! // App holds `volume_pct: u32` (0..=150).
//! let normalized = volume_pct as f32 / 150.0;
//! slider(normalized, tokens::PRIMARY).key(format!("volume:{node_id}"))
//! ```
//!
//! Pointer routing is delivered to `App::on_event` as `Click`,
//! `PointerDown`, and `Drag` events whose `key` matches the slider's
//! key. Use [`normalized_from_event`] to convert the pointer-x within
//! the slider's `target.rect` to a normalized value:
//!
//! ```ignore
//! if matches!(event.kind, UiEventKind::PointerDown | UiEventKind::Drag)
//!     && event.route() == Some(my_key)
//! {
//!     let normalized = slider::normalized_from_event(
//!         event.target_rect().unwrap(),
//!         event.pointer_x().unwrap(),
//!     );
//!     self.volume_pct = (normalized * 150.0).round() as u32;
//! }
//! ```
//!
//! Caller passes the fill color so the slider can reflect state
//! (`tokens::PRIMARY` for normal, `tokens::TEXT_MUTED_FOREGROUND` for
//! a disabled/muted look, etc.). Default height is 18 px; override
//! with `.height(...)` to grow the hit area without distorting the
//! visuals.
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface
//! (`Kind::Custom`, `.focusable()`, `.layout()`, stack of three
//! sub-rects). An app crate can fork this file and produce an
//! equivalent widget against the same API.

use std::panic::Location;

use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind, UiKey};
use crate::layout::LayoutCtx;
use crate::tokens;
use crate::tree::*;

/// Track height in pixels. Public so apps can compute matching layouts
/// (e.g. an inline value label aligned to the slider center).
pub const TRACK_HEIGHT: f32 = 10.0;

/// Thumb diameter in pixels.
pub const THUMB_SIZE: f32 = 14.0;

/// Default vertical extent — pads the track to give the thumb room and
/// makes the hit area comfortable for pointer dragging.
pub const DEFAULT_HEIGHT: f32 = 18.0;

/// A horizontal slider rendering `value` (normalized to `0.0..=1.0`)
/// as a fill from the track's left edge plus a thumb at the value's
/// position. `fill_color` styles the active portion of the track
/// (typically `tokens::PRIMARY`; pass `tokens::TEXT_MUTED_FOREGROUND`
/// to render a disabled/muted state). Chain `.key(...)` to receive
/// pointer events.
#[track_caller]
pub fn slider(value: f32, fill_color: Color) -> El {
    let value = value.clamp(0.0, 1.0);
    let layout = move |ctx: LayoutCtx| {
        let rect = ctx.container;
        let usable = (rect.w - THUMB_SIZE).max(1.0);
        let track_x = rect.x + THUMB_SIZE * 0.5;
        let track_y = rect.y + (rect.h - TRACK_HEIGHT) * 0.5;
        let thumb_x = rect.x + value * usable;
        let thumb_y = rect.y + (rect.h - THUMB_SIZE) * 0.5;
        vec![
            Rect::new(track_x, track_y, usable, TRACK_HEIGHT),
            Rect::new(track_x, track_y, value * usable, TRACK_HEIGHT),
            Rect::new(thumb_x, thumb_y, THUMB_SIZE, THUMB_SIZE),
        ]
    };

    stack([
        El::new(Kind::Custom("slider-track"))
            .height(Size::Fixed(TRACK_HEIGHT))
            .width(Size::Fill(1.0))
            .fill(tokens::BG_MUTED)
            .radius(tokens::RADIUS_PILL),
        El::new(Kind::Custom("slider-fill"))
            .height(Size::Fixed(TRACK_HEIGHT))
            .width(Size::Fill(1.0))
            .fill(fill_color)
            .radius(tokens::RADIUS_PILL),
        El::new(Kind::Custom("slider-thumb"))
            .width(Size::Fixed(THUMB_SIZE))
            .height(Size::Fixed(THUMB_SIZE))
            .fill(tokens::TEXT_FOREGROUND)
            .stroke(tokens::BORDER)
            .radius(tokens::RADIUS_PILL)
            // The hit-test resolves to the focusable container above,
            // so the thumb never receives hover / press envelopes of
            // its own. Borrow the ancestor's so grabbing the slider
            // visibly reacts on the thumb itself — mirrors shadcn's
            // `hover:ring-4 hover:ring-ring/50`.
            .state_follows_interactive_ancestor(),
    ])
    .at_loc(Location::caller())
    .focusable()
    // Grab at rest, Grabbing while the press is anchored here — the
    // resolver picks `cursor_pressed` only on the literal press target,
    // so an ancestor's `cursor_pressed` won't leak into descendants.
    .cursor(Cursor::Grab)
    .cursor_pressed(Cursor::Grabbing)
    .layout(layout)
    .height(Size::Fixed(DEFAULT_HEIGHT))
    .width(Size::Fill(1.0))
}

/// Convert a pointer-x within the slider's container rect to a
/// normalized value in `0.0..=1.0`. Inverse of the layout's
/// thumb-position math: `0.0` at thumb-leftmost, `1.0` at
/// thumb-rightmost. Clamps to the range when the pointer drifts
/// outside the slider.
pub fn normalized_from_event(rect: Rect, x: f32) -> f32 {
    let usable = (rect.w - THUMB_SIZE).max(1.0);
    let local = x - rect.x - THUMB_SIZE * 0.5;
    (local / usable).clamp(0.0, 1.0)
}

/// Action implied by a key event routed to a focused slider.
///
/// [`classify_event`] returns one of these so apps that drive their
/// own typed value (e.g. `volume_pct: u32`) can take the abstract
/// action without going through `f32`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum SliderAction {
    /// Move the value by `delta` (in the same `0.0..=1.0` space the
    /// widget paints in). Negative steps decrement.
    Step(f32),
    /// Set the value to a specific normalized position. Used for the
    /// `Home` / `End` jumps; pointer-driven absolute sets stay in
    /// [`normalized_from_event`].
    Set(f32),
}

/// Classify a `KeyDown` event routed to the slider's `key` against
/// the standard range pattern: `ArrowUp` / `ArrowRight` increment by
/// `step`, `ArrowDown` / `ArrowLeft` decrement by `step`, `PageUp` /
/// `PageDown` adjust by `page_step`, `Home` / `End` jump to the ends.
///
/// Returns `None` when the event isn't a key event for this slider
/// or the key doesn't match a slider action — apps fall through to
/// other handling.
pub fn classify_event(
    event: &UiEvent,
    key: &str,
    step: f32,
    page_step: f32,
) -> Option<SliderAction> {
    if event.kind != UiEventKind::KeyDown || event.route() != Some(key) {
        return None;
    }
    let press = event.key_press.as_ref()?;
    Some(match press.key {
        UiKey::ArrowUp | UiKey::ArrowRight => SliderAction::Step(step),
        UiKey::ArrowDown | UiKey::ArrowLeft => SliderAction::Step(-step),
        UiKey::PageUp => SliderAction::Step(page_step),
        UiKey::PageDown => SliderAction::Step(-page_step),
        UiKey::Home => SliderAction::Set(0.0),
        UiKey::End => SliderAction::Set(1.0),
        _ => return None,
    })
}

/// Apply a key event to a normalized slider value, clamping the
/// result to `0.0..=1.0`. Returns `true` when the value changed —
/// apps use that to decide whether to write back into their typed
/// state and request a redraw.
pub fn apply_event(value: &mut f32, event: &UiEvent, key: &str, step: f32, page_step: f32) -> bool {
    let Some(action) = classify_event(event, key, step, page_step) else {
        return false;
    };
    let prev = *value;
    let next = match action {
        SliderAction::Step(d) => *value + d,
        SliderAction::Set(v) => v,
    };
    *value = next.clamp(0.0, 1.0);
    *value != prev
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, KeyPress, UiTarget};

    fn key_event(key: &str, ui_key: UiKey) -> UiEvent {
        UiEvent {
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}"),
                rect: Rect::new(0.0, 0.0, 100.0, 20.0),
            }),
            pointer: None,
            key_press: Some(KeyPress {
                key: ui_key,
                modifiers: KeyModifiers::default(),
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            kind: UiEventKind::KeyDown,
        }
    }

    #[test]
    fn apply_event_steps_and_clamps() {
        let mut value = 0.5;
        assert!(apply_event(
            &mut value,
            &key_event("vol", UiKey::ArrowUp),
            "vol",
            0.1,
            0.25
        ));
        assert!((value - 0.6).abs() < 1e-6);

        assert!(apply_event(
            &mut value,
            &key_event("vol", UiKey::ArrowDown),
            "vol",
            0.1,
            0.25
        ));
        assert!((value - 0.5).abs() < 1e-6);

        // PageUp uses the larger step.
        assert!(apply_event(
            &mut value,
            &key_event("vol", UiKey::PageUp),
            "vol",
            0.1,
            0.25
        ));
        assert!((value - 0.75).abs() < 1e-6);

        // Home / End jump.
        assert!(apply_event(
            &mut value,
            &key_event("vol", UiKey::Home),
            "vol",
            0.1,
            0.25
        ));
        assert_eq!(value, 0.0);
        assert!(apply_event(
            &mut value,
            &key_event("vol", UiKey::End),
            "vol",
            0.1,
            0.25
        ));
        assert_eq!(value, 1.0);

        // Saturating: ArrowUp at 1.0 is a no-op (returns false).
        assert!(!apply_event(
            &mut value,
            &key_event("vol", UiKey::ArrowUp),
            "vol",
            0.1,
            0.25
        ));
        assert_eq!(value, 1.0);
    }

    #[test]
    fn apply_event_ignores_unrouted_or_unrelated_keys() {
        let mut value = 0.5;
        // Wrong route → no change.
        assert!(!apply_event(
            &mut value,
            &key_event("other", UiKey::ArrowUp),
            "vol",
            0.1,
            0.25
        ));
        assert_eq!(value, 0.5);

        // Routed but unrelated key → no change.
        assert!(!apply_event(
            &mut value,
            &key_event("vol", UiKey::Tab),
            "vol",
            0.1,
            0.25
        ));
        assert_eq!(value, 0.5);
    }

    #[test]
    fn classify_left_right_mirrors_up_down() {
        assert_eq!(
            classify_event(&key_event("k", UiKey::ArrowRight), "k", 0.1, 0.25),
            Some(SliderAction::Step(0.1)),
        );
        assert_eq!(
            classify_event(&key_event("k", UiKey::ArrowLeft), "k", 0.1, 0.25),
            Some(SliderAction::Step(-0.1)),
        );
    }

    #[test]
    fn thumb_borrows_state_envelopes_from_focusable_container() {
        // The hit-test resolves to the focusable container above the
        // thumb, so the thumb never receives its own hover / press
        // envelope. Without the cascade flag, grabbing the slider
        // would produce zero feedback on the thumb (the most visible
        // surface).
        let s = slider(0.5, tokens::PRIMARY);
        assert!(s.focusable, "container is the focusable / hit target");
        let thumb = s
            .children
            .iter()
            .find(|c| matches!(&c.kind, Kind::Custom(name) if *name == "slider-thumb"))
            .expect("thumb child");
        assert!(
            thumb.state_follows_interactive_ancestor,
            "thumb borrows hover / press from the slider container",
        );
        // Track and fill paint behind the thumb and have their own
        // resting visuals; they don't need the cascade.
        for c in &s.children {
            if let Kind::Custom(name) = &c.kind
                && (*name == "slider-track" || *name == "slider-fill")
            {
                assert!(!c.state_follows_interactive_ancestor);
            }
        }
    }

    #[test]
    fn slider_declares_grab_at_rest_and_grabbing_while_pressed() {
        // The resolver picks `cursor_pressed` while a press is
        // captured on the slider container, falling back to `cursor`
        // otherwise. Hover shows Grab; press anywhere on the track
        // shows Grabbing.
        let s = slider(0.5, tokens::PRIMARY);
        assert_eq!(s.cursor, Some(Cursor::Grab));
        assert_eq!(s.cursor_pressed, Some(Cursor::Grabbing));
    }

    #[test]
    fn normalized_tracks_thumb_center() {
        let rect = Rect::new(10.0, 20.0, 220.0, DEFAULT_HEIGHT);
        let left = rect.x + THUMB_SIZE * 0.5;
        let usable = rect.w - THUMB_SIZE;
        assert_eq!(normalized_from_event(rect, left), 0.0);
        assert!((normalized_from_event(rect, left + usable * 0.5) - 0.5).abs() < 1e-6);
        assert_eq!(normalized_from_event(rect, left + usable), 1.0);
        // Drifts off the ends clamp.
        assert_eq!(normalized_from_event(rect, rect.x - 30.0), 0.0);
        assert_eq!(normalized_from_event(rect, rect.x + rect.w + 30.0), 1.0);
    }
}
