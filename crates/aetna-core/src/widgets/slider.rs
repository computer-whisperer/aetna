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
            .radius(tokens::RADIUS_PILL),
    ])
    .at_loc(Location::caller())
    .focusable()
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

#[cfg(test)]
mod tests {
    use super::*;

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
