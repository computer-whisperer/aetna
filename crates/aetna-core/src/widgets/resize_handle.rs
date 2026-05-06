//! Resize handle — a thin draggable bar between siblings that adjusts
//! their sizes. Use it to build a movable divider between a sidebar and
//! a main pane, or between two weighted panes in a split view.
//!
//! The handle is a sibling primitive, not a container wrapper. Drop it
//! between two siblings inside any `row()` / `column()`; the app owns
//! the size state and folds drag events back through one of the
//! [`apply_event_*`] helpers (or a custom handler built on
//! [`delta_from_event`]).
//!
//! # Pinned sidebar (one fixed-pixel pane + one filling pane)
//!
//! ```ignore
//! use aetna_core::prelude::*;
//! use aetna_core::widgets::resize_handle::{self, ResizeDrag};
//!
//! struct Editor {
//!     sidebar_w: f32,
//!     sidebar_drag: ResizeDrag,
//! }
//!
//! impl Default for Editor {
//!     fn default() -> Self {
//!         Self {
//!             sidebar_w: tokens::SIDEBAR_WIDTH,
//!             sidebar_drag: ResizeDrag::default(),
//!         }
//!     }
//! }
//!
//! impl App for Editor {
//!     fn build(&self) -> El {
//!         row([
//!             file_tree().width(Size::Fixed(self.sidebar_w)),
//!             resize_handle(Axis::Row).key("sidebar:resize"),
//!             editor_pane().width(Size::Fill(1.0)),
//!         ])
//!         .height(Size::Fill(1.0))
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent) {
//!         resize_handle::apply_event_fixed(
//!             &mut self.sidebar_w,
//!             &mut self.sidebar_drag,
//!             &event,
//!             "sidebar:resize",
//!             Axis::Row,
//!             tokens::SIDEBAR_WIDTH_MIN,
//!             tokens::SIDEBAR_WIDTH_MAX,
//!         );
//!     }
//! }
//! ```
//!
//! # Weighted split (two `Fill` siblings sharing a parent)
//!
//! ```ignore
//! struct Diff {
//!     weights: [f32; 2],
//!     drag: ResizeWeightsDrag,
//!     row_width: f32, // captured from the previous frame's layout
//! }
//!
//! impl App for Diff {
//!     fn build(&self) -> El {
//!         row([
//!             left().width(Size::Fill(self.weights[0])),
//!             resize_handle(Axis::Row).key("diff:split"),
//!             right().width(Size::Fill(self.weights[1])),
//!         ])
//!         .key("diff:row")
//!         .height(Size::Fill(1.0))
//!     }
//!
//!     fn on_event(&mut self, event: UiEvent) {
//!         resize_handle::apply_event_weights(
//!             &mut self.weights,
//!             &mut self.drag,
//!             &event,
//!             "diff:split",
//!             Axis::Row,
//!             self.row_width,
//!             0.15, // each pane keeps at least 15% of the row
//!         );
//!     }
//! }
//! ```
//!
//! Apps that need finer control (multi-pane redistribution, snapping,
//! collapse-on-min) should fold [`delta_from_event`] into their own
//! handler.
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface (`Kind::Custom`,
//! `.focusable()`, `.paint_overflow()`). No privileged internals.

use std::panic::Location;

use crate::event::{UiEvent, UiEventKind, UiKey};
use crate::tokens;
use crate::tree::*;

/// Thickness of the handle's interactive bar in logical pixels. The
/// hit area is intentionally wider than the painted hairline so the
/// pointer doesn't have to land pixel-perfect — comparable to
/// VS Code's ~6–8px and Slack's ~10px native handles, and roughly
/// double shadcn's ~5px effective hit area (`w-px` + `after:w-1`).
pub const HANDLE_THICKNESS: f32 = 8.0;

/// Visual width of the painted hairline inside the wider hit area.
const HAIRLINE_THICKNESS: f32 = 2.0;

/// Pixels the value moves per Arrow key press when the handle is
/// keyboard-focused. Small enough for fine alignment, large enough
/// that a held-down arrow makes visible progress.
pub const KEYBOARD_STEP_PX: f32 = 8.0;

/// Pixels the value moves per PageUp / PageDown press.
pub const KEYBOARD_PAGE_STEP_PX: f32 = 40.0;

/// A thin draggable bar that lives between two siblings. `axis` is the
/// container axis the handle resizes along — `Axis::Row` for a
/// vertical bar in a row of panes (drags left/right), `Axis::Column`
/// for a horizontal bar in a column (drags up/down).
///
/// Chain `.key(...)` on the returned `El` to receive pointer / drag /
/// key events; route them through one of the [`apply_event_fixed`] or
/// [`apply_event_weights`] helpers.
#[track_caller]
pub fn resize_handle(axis: Axis) -> El {
    let (width, height) = match axis {
        Axis::Row => (Size::Fixed(HANDLE_THICKNESS), Size::Fill(1.0)),
        Axis::Column => (Size::Fill(1.0), Size::Fixed(HANDLE_THICKNESS)),
        Axis::Overlay => (Size::Fixed(HANDLE_THICKNESS), Size::Fixed(HANDLE_THICKNESS)),
    };
    let hairline = match axis {
        Axis::Row => El::new(Kind::Custom("resize-handle-hairline"))
            .width(Size::Fixed(HAIRLINE_THICKNESS))
            .height(Size::Fill(1.0))
            .fill(tokens::BORDER),
        Axis::Column | Axis::Overlay => El::new(Kind::Custom("resize-handle-hairline"))
            .width(Size::Fill(1.0))
            .height(Size::Fixed(HAIRLINE_THICKNESS))
            .fill(tokens::BORDER),
    };
    // No `capture_keys()` — Tab must keep traversing past the handle.
    // Arrow / PageUp / PageDown / Home / End still arrive as `KeyDown`
    // events on the focused handle by default, which `apply_event_*`
    // folds into the size value.
    stack([hairline])
        .at_loc(Location::caller())
        .focusable()
        .paint_overflow(Sides::all(tokens::FOCUS_RING_WIDTH))
        .width(width)
        .height(height)
}

/// Drag-anchor state for [`apply_event_fixed`]. Lives in the app
/// struct alongside the size value the handle controls; default-init
/// it (`ResizeDrag::default()`) and pass `&mut`.
///
/// `anchor` is the pointer position captured at PointerDown along the
/// handle's resize axis; `initial` is the size value at that moment.
/// Combining the two with the current pointer position gives an
/// absolute target size each frame, so drags don't accumulate float
/// rounding error across many `Drag` events.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResizeDrag {
    pub anchor: Option<f32>,
    pub initial: f32,
}

/// Drag-anchor state for [`apply_event_weights`]. Captures the pointer
/// position and the pair of weights at the moment the drag began so
/// the helper can recompute absolute target weights each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct ResizeWeightsDrag {
    pub anchor: Option<f32>,
    pub initial: [f32; 2],
}

/// Project a 2D pointer position onto the resize axis.
fn project(pos: (f32, f32), axis: Axis) -> f32 {
    match axis {
        Axis::Row | Axis::Overlay => pos.0,
        Axis::Column => pos.1,
    }
}

/// Pixel delta from drag anchor to the event's current pointer
/// position along `axis`. Returns `None` if the drag hasn't started
/// (PointerDown not seen) or the event carries no pointer.
///
/// Useful for apps that want to roll their own redistribution logic
/// (multi-pane, snapping, collapse-to-zero). Most apps should use
/// [`apply_event_fixed`] or [`apply_event_weights`] directly.
pub fn delta_from_event(drag: &ResizeDrag, event: &UiEvent, axis: Axis) -> Option<f32> {
    let anchor = drag.anchor?;
    let pos = event.pointer?;
    Some(project(pos, axis) - anchor)
}

/// Fold a routed event into a fixed-pixel size value (e.g. a sidebar
/// width). Returns `true` when the value changed.
///
/// Handles the full drag lifecycle: PointerDown captures the anchor,
/// Drag updates the value, PointerUp clears the anchor. Arrow keys on
/// the focused handle nudge by [`KEYBOARD_STEP_PX`]; PageUp /
/// PageDown by [`KEYBOARD_PAGE_STEP_PX`]; Home / End jump to `min` /
/// `max`.
///
/// `axis` must match the axis the handle was constructed with —
/// `Axis::Row` for a sidebar in a row, `Axis::Column` for a top pane
/// in a column.
pub fn apply_event_fixed(
    value: &mut f32,
    drag: &mut ResizeDrag,
    event: &UiEvent,
    key: &str,
    axis: Axis,
    min: f32,
    max: f32,
) -> bool {
    if event.route() != Some(key) {
        return false;
    }
    match event.kind {
        UiEventKind::PointerDown => {
            if let Some(pos) = event.pointer {
                drag.anchor = Some(project(pos, axis));
                drag.initial = *value;
            }
            false
        }
        UiEventKind::Drag => {
            let Some(anchor) = drag.anchor else {
                return false;
            };
            let Some(pos) = event.pointer else {
                return false;
            };
            let next = (drag.initial + project(pos, axis) - anchor).clamp(min, max);
            let changed = (next - *value).abs() > f32::EPSILON;
            *value = next;
            changed
        }
        UiEventKind::PointerUp => {
            drag.anchor = None;
            false
        }
        UiEventKind::KeyDown => apply_key(value, event, min, max),
        _ => false,
    }
}

/// Fold a routed event into a `[left_weight, right_weight]` pair that
/// share a parent's main-axis extent. Returns `true` when the
/// weights changed.
///
/// `parent_main_extent` is the parent's width (Row) or height
/// (Column) in logical pixels — the helper needs it to convert the
/// pointer's pixel delta into a weight delta. Capture it after each
/// frame's layout via `UiState::rect_of_key("parent:key")` and feed
/// it back here.
///
/// `min_weight` clamps each side so a pane can't be dragged below it.
/// Pass a small fraction (e.g. `0.15`) to leave room for the pane's
/// content.
pub fn apply_event_weights(
    weights: &mut [f32; 2],
    drag: &mut ResizeWeightsDrag,
    event: &UiEvent,
    key: &str,
    axis: Axis,
    parent_main_extent: f32,
    min_weight: f32,
) -> bool {
    if event.route() != Some(key) {
        return false;
    }
    match event.kind {
        UiEventKind::PointerDown => {
            if let Some(pos) = event.pointer {
                drag.anchor = Some(project(pos, axis));
                drag.initial = *weights;
            }
            false
        }
        UiEventKind::Drag => {
            let Some(anchor) = drag.anchor else {
                return false;
            };
            let Some(pos) = event.pointer else {
                return false;
            };
            if parent_main_extent <= 0.0 {
                return false;
            }
            let total = drag.initial[0] + drag.initial[1];
            if total <= 0.0 {
                return false;
            }
            // Pixel delta → weight delta, scaled by the parent's
            // weight density (total weight per pixel of shared extent).
            let pixel_delta = project(pos, axis) - anchor;
            let weight_delta = pixel_delta * (total / parent_main_extent);
            let lo = min_weight.max(0.0);
            let hi = (total - lo).max(lo);
            let next_left = (drag.initial[0] + weight_delta).clamp(lo, hi);
            let next_right = total - next_left;
            let changed = (next_left - weights[0]).abs() > f32::EPSILON
                || (next_right - weights[1]).abs() > f32::EPSILON;
            weights[0] = next_left;
            weights[1] = next_right;
            changed
        }
        UiEventKind::PointerUp => {
            drag.anchor = None;
            false
        }
        _ => false,
    }
}

fn apply_key(value: &mut f32, event: &UiEvent, min: f32, max: f32) -> bool {
    let Some(press) = event.key_press.as_ref() else {
        return false;
    };
    let prev = *value;
    let next = match press.key {
        UiKey::ArrowRight | UiKey::ArrowDown => *value + KEYBOARD_STEP_PX,
        UiKey::ArrowLeft | UiKey::ArrowUp => *value - KEYBOARD_STEP_PX,
        UiKey::PageUp => *value + KEYBOARD_PAGE_STEP_PX,
        UiKey::PageDown => *value - KEYBOARD_PAGE_STEP_PX,
        UiKey::Home => min,
        UiKey::End => max,
        _ => return false,
    };
    *value = next.clamp(min, max);
    (*value - prev).abs() > f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, KeyPress, UiTarget};

    fn pointer_event(kind: UiEventKind, key: &str, x: f32) -> UiEvent {
        UiEvent {
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}"),
                rect: Rect::new(0.0, 0.0, 6.0, 400.0),
            }),
            pointer: Some((x, 100.0)),
            key_press: None,
            text: None,
            modifiers: KeyModifiers::default(),
            kind,
        }
    }

    fn key_event(key: &str, ui_key: UiKey) -> UiEvent {
        UiEvent {
            key: Some(key.to_string()),
            target: Some(UiTarget {
                key: key.to_string(),
                node_id: format!("/{key}"),
                rect: Rect::new(0.0, 0.0, 6.0, 400.0),
            }),
            pointer: None,
            key_press: Some(KeyPress {
                key: ui_key,
                modifiers: KeyModifiers::default(),
                repeat: false,
            }),
            text: None,
            modifiers: KeyModifiers::default(),
            kind: UiEventKind::KeyDown,
        }
    }

    #[test]
    fn handle_is_focusable_and_thin_in_its_resize_axis() {
        let row_handle = resize_handle(Axis::Row);
        assert!(row_handle.focusable);
        assert_eq!(row_handle.width, Size::Fixed(HANDLE_THICKNESS));
        assert_eq!(row_handle.height, Size::Fill(1.0));

        let col_handle = resize_handle(Axis::Column);
        assert_eq!(col_handle.width, Size::Fill(1.0));
        assert_eq!(col_handle.height, Size::Fixed(HANDLE_THICKNESS));
    }

    #[test]
    fn handle_does_not_capture_keys() {
        // Regression guard: an earlier sketch added `capture_keys()`
        // which silently swallowed Tab and trapped focus on the
        // handle. The handle must let the runtime's default Tab
        // traversal move focus past it; arrow keys still arrive as
        // `KeyDown` without opting in.
        assert!(!resize_handle(Axis::Row).capture_keys);
        assert!(!resize_handle(Axis::Column).capture_keys);
    }

    #[test]
    fn fixed_drag_uses_absolute_anchor_so_no_drift() {
        // Down at x=300, drag to x=350 → +50px. Drag to x=380 → +80px
        // (absolute from anchor, not accumulated from previous Drag).
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "h", 300.0),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert_eq!(drag.anchor, Some(300.0));
        assert_eq!(drag.initial, 256.0);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 350.0),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert!((value - 306.0).abs() < 1e-3);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 380.0),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert!((value - 336.0).abs() < 1e-3);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerUp, "h", 380.0),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert_eq!(drag.anchor, None, "anchor cleared on PointerUp");
    }

    #[test]
    fn fixed_drag_clamps_to_min_max() {
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();
        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "h", 300.0),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        // Way beyond max.
        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 1000.0),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert_eq!(value, 480.0);
        // Way below min.
        apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "h", 0.0),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert_eq!(value, 180.0);
    }

    #[test]
    fn fixed_ignores_unrouted_events() {
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();
        let changed = apply_event_fixed(
            &mut value,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "other", 300.0),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert!(!changed);
        assert_eq!(drag.anchor, None);
        assert_eq!(value, 256.0);
    }

    #[test]
    fn fixed_arrow_keys_nudge_within_bounds() {
        let mut value = 256.0;
        let mut drag = ResizeDrag::default();
        apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", UiKey::ArrowRight),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert!((value - (256.0 + KEYBOARD_STEP_PX)).abs() < 1e-3);

        apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", UiKey::Home),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert_eq!(value, 180.0);

        // ArrowLeft at min is a no-op.
        let unchanged = apply_event_fixed(
            &mut value,
            &mut drag,
            &key_event("h", UiKey::ArrowLeft),
            "h",
            Axis::Row,
            180.0,
            480.0,
        );
        assert!(!unchanged);
        assert_eq!(value, 180.0);
    }

    #[test]
    fn weights_drag_redistributes_proportionally_to_parent_extent() {
        // Parent row 800px wide, weights [1.0, 1.0] → each pane is
        // 400px. Drag the handle 100px right → +0.25 weight to left,
        // -0.25 from right. (100 px / 800 px * 2.0 total = 0.25.)
        let mut weights = [1.0, 1.0];
        let mut drag = ResizeWeightsDrag::default();
        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "split", 400.0),
            "split",
            Axis::Row,
            800.0,
            0.15,
        );
        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "split", 500.0),
            "split",
            Axis::Row,
            800.0,
            0.15,
        );
        assert!((weights[0] - 1.25).abs() < 1e-3, "left = {}", weights[0]);
        assert!((weights[1] - 0.75).abs() < 1e-3, "right = {}", weights[1]);
        assert!(
            (weights[0] + weights[1] - 2.0).abs() < 1e-3,
            "total weight is conserved"
        );
    }

    #[test]
    fn weights_drag_clamps_each_side_to_min_weight() {
        let mut weights = [1.0, 1.0];
        let mut drag = ResizeWeightsDrag::default();
        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::PointerDown, "split", 400.0),
            "split",
            Axis::Row,
            800.0,
            0.5, // each side floors at 0.5 of weight
        );
        // Way past either end — should clamp.
        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "split", 10_000.0),
            "split",
            Axis::Row,
            800.0,
            0.5,
        );
        assert!((weights[0] - 1.5).abs() < 1e-3);
        assert!((weights[1] - 0.5).abs() < 1e-3);

        apply_event_weights(
            &mut weights,
            &mut drag,
            &pointer_event(UiEventKind::Drag, "split", -10_000.0),
            "split",
            Axis::Row,
            800.0,
            0.5,
        );
        assert!((weights[0] - 0.5).abs() < 1e-3);
        assert!((weights[1] - 1.5).abs() < 1e-3);
    }

    #[test]
    fn delta_from_event_returns_none_until_pointerdown() {
        let drag = ResizeDrag::default();
        let drag_event = pointer_event(UiEventKind::Drag, "h", 350.0);
        assert!(delta_from_event(&drag, &drag_event, Axis::Row).is_none());
    }
}
