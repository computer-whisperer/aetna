//! Single-line text input widget.
//!
//! `text_input(value, caret)` renders a focusable, key-capturing input
//! field with a visible caret bar between the prefix and suffix of the
//! current value. The application owns both the string and the caret
//! byte index; routed events are folded back via [`apply_event`] in the
//! app's `on_event` handler.
//!
//! ```ignore
//! struct App { name: String, name_caret: usize }
//! impl aetna_core::App for App {
//!     fn build(&self) -> El {
//!         text_input(&self.name, self.name_caret).key("name")
//!     }
//!     fn on_event(&mut self, e: UiEvent) {
//!         if e.target.as_ref().map(|t| t.key.as_str()) == Some("name") {
//!             text_input::apply_event(&mut self.name, &mut self.name_caret, &e);
//!         }
//!     }
//! }
//! ```
//!
//! # Dogfood note (v0.8.1)
//!
//! This widget is the v0.8.1 dogfood proof: it composes only the
//! public widget-kit surface — `Kind::Custom("text_input")`,
//! `.focusable() + .capture_keys()`, `.paint_overflow()` for the focus
//! ring band, `.axis(Row)` for the inline `[prefix, caret, suffix]`
//! layout, and the `widgets::text` constructor for the two text
//! segments. No library internals are reached. See `widget_kit.md`.

use std::panic::Location;

use crate::event::{UiEvent, UiEventKind, UiKey};
use crate::style::StyleProfile;
use crate::text::metrics::{self, hit_text};
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

/// Build a single-line text input. `value` is the string to render and
/// `caret` is the byte offset where the visible caret sits (clamped
/// to a UTF-8 grapheme boundary at or before the end of `value`). Use
/// [`apply_event`] in your event handler to fold routed events back
/// into your app state.
#[track_caller]
pub fn text_input(value: &str, caret: usize) -> El {
    let caret = clamp_to_char_boundary(value, caret.min(value.len()));
    let prefix = &value[..caret];
    let suffix = &value[caret..];

    let caret_bar = El::new(Kind::Custom("text_input_caret"))
        .style_profile(StyleProfile::Solid)
        .fill(tokens::TEXT_FOREGROUND)
        .width(Size::Fixed(2.0))
        .height(Size::Fixed(tokens::FONT_BASE))
        .radius(1.0);

    El::new(Kind::Custom("text_input"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .focusable()
        .capture_keys()
        .paint_overflow(Sides::all(tokens::FOCUS_RING_WIDTH))
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_MD)
        .axis(Axis::Row)
        .align(Align::Center)
        .gap(0.0)
        .height(Size::Fixed(36.0))
        .padding(Sides::xy(tokens::SPACE_MD, 0.0))
        .child(text(prefix).font_size(tokens::FONT_BASE))
        .child(caret_bar)
        .child(text(suffix).font_size(tokens::FONT_BASE))
}

/// Fold a routed [`UiEvent`] into `value` and `caret`. Returns `true`
/// when the event mutated either of them.
///
/// Handles:
/// - [`UiEventKind::TextInput`] — insert `event.text` at the caret.
/// - [`UiEventKind::KeyDown`] for Backspace, Delete, ArrowLeft,
///   ArrowRight, Home, End.
/// - [`UiEventKind::Click`] — set the caret to the byte index that the
///   click resolves to via [`metrics::hit_text`].
///
/// All caret arithmetic respects UTF-8 grapheme boundaries — arrowing
/// across a multi-byte codepoint advances by the full encoded width.
pub fn apply_event(value: &mut String, caret: &mut usize, event: &UiEvent) -> bool {
    *caret = clamp_to_char_boundary(value, (*caret).min(value.len()));
    match event.kind {
        UiEventKind::TextInput => {
            let Some(insert) = event.text.as_deref() else {
                return false;
            };
            if insert.is_empty() {
                return false;
            }
            value.insert_str(*caret, insert);
            *caret += insert.len();
            true
        }
        UiEventKind::KeyDown => {
            let Some(kp) = event.key_press.as_ref() else {
                return false;
            };
            match kp.key {
                UiKey::Backspace => {
                    if *caret == 0 {
                        return false;
                    }
                    let prev = prev_char_boundary(value, *caret);
                    value.replace_range(prev..*caret, "");
                    *caret = prev;
                    true
                }
                UiKey::Delete => {
                    if *caret >= value.len() {
                        return false;
                    }
                    let next = next_char_boundary(value, *caret);
                    value.replace_range(*caret..next, "");
                    true
                }
                UiKey::ArrowLeft => {
                    if *caret == 0 {
                        return false;
                    }
                    *caret = prev_char_boundary(value, *caret);
                    true
                }
                UiKey::ArrowRight => {
                    if *caret >= value.len() {
                        return false;
                    }
                    *caret = next_char_boundary(value, *caret);
                    true
                }
                UiKey::Home => {
                    if *caret == 0 {
                        return false;
                    }
                    *caret = 0;
                    true
                }
                UiKey::End => {
                    if *caret >= value.len() {
                        return false;
                    }
                    *caret = value.len();
                    true
                }
                _ => false,
            }
        }
        UiEventKind::Click => {
            let (Some((px, _py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_MD;
            *caret = caret_from_x(value, local_x);
            true
        }
        _ => false,
    }
}

fn caret_from_x(value: &str, local_x: f32) -> usize {
    if value.is_empty() || local_x <= 0.0 {
        return 0;
    }
    // hit_text expects y inside the first line's vertical extent.
    let local_y = metrics::line_height(tokens::FONT_BASE) * 0.5;
    match hit_text(
        value,
        tokens::FONT_BASE,
        FontWeight::Regular,
        TextWrap::NoWrap,
        None,
        local_x,
        local_y,
    ) {
        Some(hit) => hit.byte_index.min(value.len()),
        None => value.len(),
    }
}

fn clamp_to_char_boundary(s: &str, idx: usize) -> usize {
    let mut idx = idx.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn prev_char_boundary(s: &str, from: usize) -> usize {
    let mut i = from.saturating_sub(1);
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(s: &str, from: usize) -> usize {
    let mut i = (from + 1).min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{KeyModifiers, KeyPress, PointerButton, UiTarget};
    use crate::layout::layout;
    use crate::runtime::RunnerCore;
    use crate::state::UiState;

    fn ev_text(s: &str) -> UiEvent {
        UiEvent {
            key: None,
            target: None,
            pointer: None,
            key_press: None,
            text: Some(s.into()),
            kind: UiEventKind::TextInput,
        }
    }

    fn ev_key(key: UiKey) -> UiEvent {
        UiEvent {
            key: None,
            target: None,
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers: KeyModifiers::default(),
                repeat: false,
            }),
            text: None,
            kind: UiEventKind::KeyDown,
        }
    }

    fn ev_click(target: UiTarget, pointer: (f32, f32)) -> UiEvent {
        UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: None,
            kind: UiEventKind::Click,
        }
    }

    #[test]
    fn text_input_renders_three_children_with_caret_between_segments() {
        let el = text_input("hello", 2);
        assert!(matches!(el.kind, Kind::Custom("text_input")));
        assert!(el.focusable);
        assert!(el.capture_keys);
        assert_eq!(el.children.len(), 3);
        assert_eq!(el.children[0].text.as_deref(), Some("he"));
        assert!(matches!(
            el.children[1].kind,
            Kind::Custom("text_input_caret")
        ));
        assert_eq!(el.children[2].text.as_deref(), Some("llo"));
    }

    #[test]
    fn text_input_clamps_caret_to_string_length() {
        let el = text_input("hi", 99);
        assert_eq!(el.children[0].text.as_deref(), Some("hi"));
        assert_eq!(el.children[2].text.as_deref(), Some(""));
    }

    #[test]
    fn text_input_clamps_caret_off_utf8_boundary() {
        // 'é' is two bytes; caret=1 sits inside the codepoint and must
        // snap back to 0 so we never slice across a UTF-8 boundary.
        let el = text_input("é", 1);
        assert_eq!(el.children[0].text.as_deref(), Some(""));
        assert_eq!(el.children[2].text.as_deref(), Some("é"));
    }

    #[test]
    fn apply_text_input_inserts_at_caret() {
        let mut value = String::from("ho");
        let mut caret = 1;
        assert!(apply_event(&mut value, &mut caret, &ev_text("i, t")));
        assert_eq!(value, "hi, to");
        assert_eq!(caret, 5);
    }

    #[test]
    fn apply_text_input_empty_string_is_noop() {
        let mut value = String::from("hi");
        let mut caret = 1;
        assert!(!apply_event(&mut value, &mut caret, &ev_text("")));
        assert_eq!(value, "hi");
        assert_eq!(caret, 1);
    }

    #[test]
    fn apply_backspace_removes_preceding_grapheme() {
        let mut value = String::from("café");
        let mut caret = value.len();
        assert!(apply_event(&mut value, &mut caret, &ev_key(UiKey::Backspace)));
        assert_eq!(value, "caf");
        assert_eq!(caret, 3);
    }

    #[test]
    fn apply_backspace_at_start_is_noop() {
        let mut value = String::from("hi");
        let mut caret = 0;
        assert!(!apply_event(&mut value, &mut caret, &ev_key(UiKey::Backspace)));
        assert_eq!(value, "hi");
        assert_eq!(caret, 0);
    }

    #[test]
    fn apply_delete_removes_following_grapheme() {
        let mut value = String::from("hello");
        let mut caret = 1;
        assert!(apply_event(&mut value, &mut caret, &ev_key(UiKey::Delete)));
        assert_eq!(value, "hllo");
        assert_eq!(caret, 1);
    }

    #[test]
    fn apply_arrow_keys_walk_utf8_boundaries() {
        let mut value = String::from("aé");
        let mut caret = 0;
        // Right past 'a'
        apply_event(&mut value, &mut caret, &ev_key(UiKey::ArrowRight));
        assert_eq!(caret, 1);
        // Right past 'é' (2 bytes) — caret jumps by full codepoint
        apply_event(&mut value, &mut caret, &ev_key(UiKey::ArrowRight));
        assert_eq!(caret, 3);
        // ArrowRight past end is a no-op
        assert!(!apply_event(
            &mut value,
            &mut caret,
            &ev_key(UiKey::ArrowRight)
        ));
        // Walk back
        apply_event(&mut value, &mut caret, &ev_key(UiKey::ArrowLeft));
        assert_eq!(caret, 1);
        apply_event(&mut value, &mut caret, &ev_key(UiKey::ArrowLeft));
        assert_eq!(caret, 0);
        assert!(!apply_event(
            &mut value,
            &mut caret,
            &ev_key(UiKey::ArrowLeft)
        ));
    }

    #[test]
    fn apply_home_and_end_jump_to_extremes() {
        let mut value = String::from("hello");
        let mut caret = 2;
        assert!(apply_event(&mut value, &mut caret, &ev_key(UiKey::End)));
        assert_eq!(caret, 5);
        assert!(apply_event(&mut value, &mut caret, &ev_key(UiKey::Home)));
        assert_eq!(caret, 0);
    }

    #[test]
    fn apply_unrelated_key_falls_through() {
        let mut value = String::from("hi");
        let mut caret = 1;
        assert!(!apply_event(
            &mut value,
            &mut caret,
            &ev_key(UiKey::Escape)
        ));
        assert_eq!(value, "hi");
        assert_eq!(caret, 1);
    }

    #[test]
    fn apply_click_far_left_lands_at_start() {
        // A click at the input's left edge should put the caret at 0.
        let target = UiTarget {
            key: "ti".into(),
            node_id: "root.text_input[ti]".into(),
            rect: Rect::new(20.0, 20.0, 200.0, 36.0),
        };
        let mut value = String::from("hello");
        let mut caret = 5;
        let click = ev_click(target.clone(), (target.rect.x + 1.0, target.rect.y + 18.0));
        assert!(apply_event(&mut value, &mut caret, &click));
        assert_eq!(caret, 0);
    }

    #[test]
    fn apply_click_far_right_lands_at_end() {
        // A click well past the rendered text should put the caret at
        // value.len() (cosmic-text clamps to end-of-line).
        let target = UiTarget {
            key: "ti".into(),
            node_id: "root.text_input[ti]".into(),
            rect: Rect::new(20.0, 20.0, 400.0, 36.0),
        };
        let mut value = String::from("hi");
        let mut caret = 0;
        // Click near the right edge of the input — well past the
        // 2-character text.
        let click = ev_click(
            target.clone(),
            (target.rect.x + target.rect.w - 4.0, target.rect.y + 18.0),
        );
        assert!(apply_event(&mut value, &mut caret, &click));
        assert_eq!(caret, value.len());
    }

    #[test]
    fn end_to_end_click_in_runner_drives_caret() {
        // Lay out a tree with one text_input keyed "ti", drive a click
        // through RunnerCore, and verify the resulting Click event's
        // pointer + target.rect feed apply_event correctly.
        let mut value = String::from("hello world");
        let caret_initial = 0;
        let mut tree = crate::column([text_input(&value, caret_initial).key("ti")]).padding(20.0);
        let mut core = RunnerCore::new();
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        core.ui_state = state;
        core.snapshot(&tree, &mut Default::default());

        let rect = core.rect_of_key("ti").expect("ti rect");
        let cx = rect.x + 60.0;
        let cy = rect.y + rect.h * 0.5;
        core.pointer_moved(cx, cy);
        core.pointer_down(cx, cy, PointerButton::Primary);
        let events = core.pointer_up(cx, cy, PointerButton::Primary);
        let click = events
            .into_iter()
            .find(|e| e.kind == UiEventKind::Click)
            .expect("click event");

        let mut caret = caret_initial;
        assert!(apply_event(&mut value, &mut caret, &click));
        // Clicking 60px into "hello world" should land somewhere
        // inside the string, well past the start.
        assert!(caret > 0 && caret < value.len(), "caret={caret}");
    }
}
