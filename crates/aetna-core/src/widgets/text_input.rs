//! Single-line text input widget with selection.
//!
//! `text_input(value, selection)` renders a focusable, key-capturing
//! input field with a visible caret and (when non-empty) a tinted
//! selection rectangle behind the selected glyphs. The application
//! owns both the string and the [`TextSelection`]; routed events are
//! folded back via [`apply_event`] in the app's `on_event` handler.
//!
//! ```ignore
//! struct App { name: String, name_sel: TextSelection }
//! impl aetna_core::App for App {
//!     fn build(&self) -> El {
//!         text_input(&self.name, self.name_sel).key("name")
//!     }
//!     fn on_event(&mut self, e: UiEvent) {
//!         if e.target.as_ref().map(|t| t.key.as_str()) == Some("name") {
//!             text_input::apply_event(&mut self.name, &mut self.name_sel, &e);
//!         }
//!     }
//! }
//! ```
//!
//! # Dogfood note (v0.8.1 + v0.8.2)
//!
//! Composes only the public widget-kit surface. v0.8.1 introduced the
//! caret + character/IME path; v0.8.2 layers selection semantics on top
//! of the same builder via [`TextSelection`] (a value type, not stored
//! in `widget_state`), gaining drag-select, shift-extend, replace-on-
//! type, and `Ctrl+A`. See `widget_kit.md`.

use std::panic::Location;

use crate::event::{UiEvent, UiEventKind, UiKey};
use crate::style::StyleProfile;
use crate::text::metrics::{self, hit_text};
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;

/// A `(anchor, head)` byte-index pair representing the selection in a
/// text field. `head` is the caret position; the selection covers
/// `min(anchor, head)..max(anchor, head)`. When `anchor == head` the
/// selection is collapsed and the field shows just a caret.
///
/// Both indices are byte offsets into the source string and are
/// clamped to a UTF-8 grapheme boundary by every method that reads or
/// writes them — callers can safely poke them directly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSelection {
    pub anchor: usize,
    pub head: usize,
}

impl TextSelection {
    /// Collapsed selection at byte offset `head`.
    pub const fn caret(head: usize) -> Self {
        Self {
            anchor: head,
            head,
        }
    }

    /// Selection from `anchor` to `head`. Either order is valid; the
    /// widget renders `min..max` as the highlighted band.
    pub const fn range(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    /// `(min, max)` byte offsets, ordered.
    pub fn ordered(self) -> (usize, usize) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    /// True when the selection is collapsed (anchor == head).
    pub fn is_collapsed(self) -> bool {
        self.anchor == self.head
    }
}

/// Build a single-line text input. `value` is the string to render
/// and `selection` carries the caret + selection state. Both are
/// owned by the application — pass them in from your state and update
/// them via [`apply_event`] in your event handler.
#[track_caller]
pub fn text_input(value: &str, selection: TextSelection) -> El {
    let head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    let anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    let lo = anchor.min(head);
    let hi = anchor.max(head);

    let caret_bar = caret_bar();
    let mut children: Vec<El> = Vec::with_capacity(5);

    if lo == hi {
        // Collapsed: [text(prefix), caret, text(suffix)]
        children.push(text(&value[..head]).font_size(tokens::FONT_BASE));
        children.push(caret_bar);
        children.push(text(&value[head..]).font_size(tokens::FONT_BASE));
    } else {
        let prefix = &value[..lo];
        let selected = &value[lo..hi];
        let suffix = &value[hi..];
        children.push(text(prefix).font_size(tokens::FONT_BASE));
        // Caret renders on the side of the selection where `head` sits
        // — at `lo` if head was the smaller end, otherwise at `hi`. The
        // selection band always renders the same way regardless.
        if head == lo {
            children.push(caret_bar);
            children.push(selection_segment(selected));
        } else {
            children.push(selection_segment(selected));
            children.push(caret_bar);
        }
        children.push(text(suffix).font_size(tokens::FONT_BASE));
    }

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
        .children(children)
}

fn caret_bar() -> El {
    El::new(Kind::Custom("text_input_caret"))
        .style_profile(StyleProfile::Solid)
        .fill(tokens::TEXT_FOREGROUND)
        .width(Size::Fixed(2.0))
        .height(Size::Fixed(tokens::FONT_BASE))
        .radius(1.0)
}

fn selection_segment(selected: &str) -> El {
    let bg = El::new(Kind::Custom("text_input_selection"))
        .style_profile(StyleProfile::Solid)
        .fill(tokens::SELECTION_BG)
        .radius(2.0);
    crate::tree::stack([bg, text(selected).font_size(tokens::FONT_BASE)])
        .width(Size::Hug)
        .height(Size::Hug)
}

/// Fold a routed [`UiEvent`] into `value` and `selection`. Returns
/// `true` when either was mutated.
///
/// Handles:
/// - [`UiEventKind::TextInput`] — replace the selection with the
///   composed text (or insert at the caret when collapsed).
/// - [`UiEventKind::KeyDown`] for Backspace, Delete, ArrowLeft,
///   ArrowRight, Home, End. Without Shift the selection collapses and
///   moves; with Shift the head extends and the anchor stays.
/// - [`UiEventKind::KeyDown`] for Ctrl+A — select all.
/// - [`UiEventKind::PointerDown`] — set the caret to the click position
///   and the anchor to the same position. With Shift held, only the
///   head moves (extend selection from the existing anchor).
/// - [`UiEventKind::Drag`] — extend the head to the dragged position;
///   the anchor stays where pointer-down placed it.
/// - [`UiEventKind::Click`] — no-op. The selection was already
///   established by the prior PointerDown / Drag sequence.
///
/// All caret arithmetic respects UTF-8 grapheme boundaries.
pub fn apply_event(value: &mut String, selection: &mut TextSelection, event: &UiEvent) -> bool {
    selection.anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    selection.head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    match event.kind {
        UiEventKind::TextInput => {
            let Some(insert) = event.text.as_deref() else {
                return false;
            };
            if insert.is_empty() {
                return false;
            }
            replace_selection(value, selection, insert);
            true
        }
        UiEventKind::KeyDown => {
            let Some(kp) = event.key_press.as_ref() else {
                return false;
            };
            let mods = kp.modifiers;
            // Ctrl+A: select all. We test for this before modifier-less
            // key arms so the "Character('a')" path doesn't reach
            // KeyDown's no-op fallthrough.
            if mods.ctrl
                && !mods.alt
                && !mods.logo
                && let UiKey::Character(c) = &kp.key
                && c.eq_ignore_ascii_case("a")
            {
                let len = value.len();
                if selection.anchor == 0 && selection.head == len {
                    return false;
                }
                *selection = TextSelection {
                    anchor: 0,
                    head: len,
                };
                return true;
            }
            match kp.key {
                UiKey::Backspace => {
                    if !selection.is_collapsed() {
                        replace_selection(value, selection, "");
                        return true;
                    }
                    if selection.head == 0 {
                        return false;
                    }
                    let prev = prev_char_boundary(value, selection.head);
                    value.replace_range(prev..selection.head, "");
                    selection.head = prev;
                    selection.anchor = prev;
                    true
                }
                UiKey::Delete => {
                    if !selection.is_collapsed() {
                        replace_selection(value, selection, "");
                        return true;
                    }
                    if selection.head >= value.len() {
                        return false;
                    }
                    let next = next_char_boundary(value, selection.head);
                    value.replace_range(selection.head..next, "");
                    true
                }
                UiKey::ArrowLeft => {
                    let target = if selection.is_collapsed() || mods.shift {
                        if selection.head == 0 {
                            return false;
                        }
                        prev_char_boundary(value, selection.head)
                    } else {
                        // Collapse a non-empty selection to its left edge.
                        selection.ordered().0
                    };
                    selection.head = target;
                    if !mods.shift {
                        selection.anchor = target;
                    }
                    true
                }
                UiKey::ArrowRight => {
                    let target = if selection.is_collapsed() || mods.shift {
                        if selection.head >= value.len() {
                            return false;
                        }
                        next_char_boundary(value, selection.head)
                    } else {
                        // Collapse a non-empty selection to its right edge.
                        selection.ordered().1
                    };
                    selection.head = target;
                    if !mods.shift {
                        selection.anchor = target;
                    }
                    true
                }
                UiKey::Home => {
                    if selection.head == 0
                        && (mods.shift || selection.anchor == 0)
                    {
                        return false;
                    }
                    selection.head = 0;
                    if !mods.shift {
                        selection.anchor = 0;
                    }
                    true
                }
                UiKey::End => {
                    let end = value.len();
                    if selection.head == end
                        && (mods.shift || selection.anchor == end)
                    {
                        return false;
                    }
                    selection.head = end;
                    if !mods.shift {
                        selection.anchor = end;
                    }
                    true
                }
                _ => false,
            }
        }
        UiEventKind::PointerDown => {
            let (Some((px, _py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_MD;
            let pos = caret_from_x(value, local_x);
            selection.head = pos;
            if !event.modifiers.shift {
                selection.anchor = pos;
            }
            true
        }
        UiEventKind::Drag => {
            let (Some((px, _py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_MD;
            selection.head = caret_from_x(value, local_x);
            true
        }
        UiEventKind::Click => false,
        _ => false,
    }
}

/// The currently-selected substring of `value`. Returns `""` when the
/// selection is collapsed.
pub fn selected_text(value: &str, selection: TextSelection) -> &str {
    let head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    let anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    &value[anchor.min(head)..anchor.max(head)]
}

/// Replace the selected substring (or insert at the caret when the
/// selection is collapsed) with `replacement`. Updates `selection` to
/// a collapsed caret immediately after the inserted text.
pub fn replace_selection(value: &mut String, selection: &mut TextSelection, replacement: &str) {
    selection.anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    selection.head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    let (lo, hi) = selection.ordered();
    value.replace_range(lo..hi, replacement);
    let new_caret = lo + replacement.len();
    selection.anchor = new_caret;
    selection.head = new_caret;
}

/// `(0, value.len())` — the selection that spans the whole field.
pub fn select_all(value: &str) -> TextSelection {
    TextSelection {
        anchor: 0,
        head: value.len(),
    }
}

fn caret_from_x(value: &str, local_x: f32) -> usize {
    if value.is_empty() || local_x <= 0.0 {
        return 0;
    }
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
            modifiers: KeyModifiers::default(),
            kind: UiEventKind::TextInput,
        }
    }

    fn ev_key(key: UiKey) -> UiEvent {
        ev_key_with_mods(key, KeyModifiers::default())
    }

    fn ev_key_with_mods(key: UiKey, modifiers: KeyModifiers) -> UiEvent {
        UiEvent {
            key: None,
            target: None,
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers,
                repeat: false,
            }),
            text: None,
            modifiers,
            kind: UiEventKind::KeyDown,
        }
    }

    fn ev_pointer_down(target: UiTarget, pointer: (f32, f32), modifiers: KeyModifiers) -> UiEvent {
        UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: None,
            modifiers,
            kind: UiEventKind::PointerDown,
        }
    }

    fn ev_drag(target: UiTarget, pointer: (f32, f32)) -> UiEvent {
        UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: None,
            modifiers: KeyModifiers::default(),
            kind: UiEventKind::Drag,
        }
    }

    fn ti_target() -> UiTarget {
        UiTarget {
            key: "ti".into(),
            node_id: "root.text_input[ti]".into(),
            rect: Rect::new(20.0, 20.0, 400.0, 36.0),
        }
    }

    #[test]
    fn text_input_collapsed_renders_three_children_with_caret() {
        let el = text_input("hello", TextSelection::caret(2));
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
    fn text_input_with_selection_emits_four_children_caret_at_head_side() {
        // anchor=2, head=4 → selection "ll", head at right edge.
        let el = text_input("hello", TextSelection::range(2, 4));
        assert_eq!(el.children.len(), 4);
        assert_eq!(el.children[0].text.as_deref(), Some("he"));
        // [1] is the selection stack, [2] is the caret bar (head=4=hi).
        assert!(matches!(el.children[1].kind, Kind::Group)); // stack returns a Group
        assert!(matches!(
            el.children[2].kind,
            Kind::Custom("text_input_caret")
        ));
        assert_eq!(el.children[3].text.as_deref(), Some("o"));
    }

    #[test]
    fn text_input_with_selection_caret_left_when_head_is_min() {
        // anchor=4, head=2 → selection "ll", head at left edge.
        let el = text_input("hello", TextSelection::range(4, 2));
        assert_eq!(el.children.len(), 4);
        assert_eq!(el.children[0].text.as_deref(), Some("he"));
        // Caret precedes the selection segment now.
        assert!(matches!(
            el.children[1].kind,
            Kind::Custom("text_input_caret")
        ));
        assert!(matches!(el.children[2].kind, Kind::Group));
        assert_eq!(el.children[3].text.as_deref(), Some("o"));
    }

    #[test]
    fn text_input_clamps_off_utf8_boundary() {
        // 'é' is two bytes; head=1 sits inside the codepoint and must
        // snap back to 0.
        let el = text_input("é", TextSelection::caret(1));
        assert_eq!(el.children[0].text.as_deref(), Some(""));
        assert_eq!(el.children[2].text.as_deref(), Some("é"));
    }

    #[test]
    fn apply_text_input_inserts_at_caret_when_collapsed() {
        let mut value = String::from("ho");
        let mut sel = TextSelection::caret(1);
        assert!(apply_event(&mut value, &mut sel, &ev_text("i, t")));
        assert_eq!(value, "hi, to");
        assert_eq!(sel, TextSelection::caret(5));
    }

    #[test]
    fn apply_text_input_replaces_selection() {
        let mut value = String::from("hello world");
        let mut sel = TextSelection::range(6, 11); // "world"
        assert!(apply_event(&mut value, &mut sel, &ev_text("kit")));
        assert_eq!(value, "hello kit");
        assert_eq!(sel, TextSelection::caret(9));
    }

    #[test]
    fn apply_backspace_removes_selection_when_non_empty() {
        let mut value = String::from("hello world");
        let mut sel = TextSelection::range(6, 11);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::Backspace)));
        assert_eq!(value, "hello ");
        assert_eq!(sel, TextSelection::caret(6));
    }

    #[test]
    fn apply_delete_removes_selection_when_non_empty() {
        let mut value = String::from("hello world");
        let mut sel = TextSelection::range(0, 6); // "hello "
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::Delete)));
        assert_eq!(value, "world");
        assert_eq!(sel, TextSelection::caret(0));
    }

    #[test]
    fn apply_backspace_collapsed_at_start_is_noop() {
        let mut value = String::from("hi");
        let mut sel = TextSelection::caret(0);
        assert!(!apply_event(&mut value, &mut sel, &ev_key(UiKey::Backspace)));
    }

    #[test]
    fn apply_arrow_walks_utf8_boundaries() {
        let mut value = String::from("aé");
        let mut sel = TextSelection::caret(0);
        apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowRight));
        assert_eq!(sel.head, 1);
        apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowRight));
        assert_eq!(sel.head, 3);
        assert!(!apply_event(
            &mut value,
            &mut sel,
            &ev_key(UiKey::ArrowRight)
        ));
        apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowLeft));
        assert_eq!(sel.head, 1);
    }

    #[test]
    fn apply_arrow_collapses_selection_without_shift() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::range(1, 4); // "ell"
        // ArrowLeft (no shift) collapses to the LEFT edge of the
        // selection (the smaller of anchor/head).
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowLeft)));
        assert_eq!(sel, TextSelection::caret(1));

        let mut sel = TextSelection::range(1, 4);
        // ArrowRight (no shift) collapses to the RIGHT edge.
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowRight)));
        assert_eq!(sel, TextSelection::caret(4));
    }

    #[test]
    fn apply_shift_arrow_extends_selection() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::caret(2);
        let shift = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods(UiKey::ArrowRight, shift)
        ));
        assert_eq!(sel, TextSelection::range(2, 3));
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods(UiKey::ArrowRight, shift)
        ));
        assert_eq!(sel, TextSelection::range(2, 4));
        // Shift+ArrowLeft retreats the head, anchor stays.
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods(UiKey::ArrowLeft, shift)
        ));
        assert_eq!(sel, TextSelection::range(2, 3));
    }

    #[test]
    fn apply_home_end_collapse_or_extend() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::caret(2);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::End)));
        assert_eq!(sel, TextSelection::caret(5));
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::Home)));
        assert_eq!(sel, TextSelection::caret(0));

        // Shift+End extends.
        let shift = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        let mut sel = TextSelection::caret(2);
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods(UiKey::End, shift)
        ));
        assert_eq!(sel, TextSelection::range(2, 5));
    }

    #[test]
    fn apply_ctrl_a_selects_all() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::caret(2);
        let ctrl = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods(UiKey::Character("a".into()), ctrl)
        ));
        assert_eq!(sel, TextSelection::range(0, 5));
        // A second Ctrl+A is a no-op.
        assert!(!apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods(UiKey::Character("a".into()), ctrl)
        ));
    }

    #[test]
    fn apply_pointer_down_sets_anchor_and_head() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::range(0, 5);
        // Click far-left should collapse to caret=0.
        let down = ev_pointer_down(
            ti_target(),
            (ti_target().rect.x + 1.0, ti_target().rect.y + 18.0),
            KeyModifiers::default(),
        );
        assert!(apply_event(&mut value, &mut sel, &down));
        assert_eq!(sel, TextSelection::caret(0));
    }

    #[test]
    fn apply_shift_pointer_down_only_moves_head() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::caret(2);
        let shift = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        // Click far-right with shift: head goes to end, anchor stays.
        let down = ev_pointer_down(
            ti_target(),
            (
                ti_target().rect.x + ti_target().rect.w - 4.0,
                ti_target().rect.y + 18.0,
            ),
            shift,
        );
        assert!(apply_event(&mut value, &mut sel, &down));
        assert_eq!(sel.anchor, 2);
        assert_eq!(sel.head, value.len());
    }

    #[test]
    fn apply_drag_extends_head_only() {
        let mut value = String::from("hello world");
        let mut sel = TextSelection::caret(0);
        // First, pointer-down at the start.
        let down = ev_pointer_down(
            ti_target(),
            (ti_target().rect.x + 1.0, ti_target().rect.y + 18.0),
            KeyModifiers::default(),
        );
        apply_event(&mut value, &mut sel, &down);
        assert_eq!(sel, TextSelection::caret(0));
        // Drag to the right edge — head extends, anchor stays at 0.
        let drag = ev_drag(
            ti_target(),
            (
                ti_target().rect.x + ti_target().rect.w - 4.0,
                ti_target().rect.y + 18.0,
            ),
        );
        assert!(apply_event(&mut value, &mut sel, &drag));
        assert_eq!(sel.anchor, 0);
        assert_eq!(sel.head, value.len());
    }

    #[test]
    fn apply_click_is_noop_for_selection() {
        // Click fires after a drag — handling it would clobber the
        // selection drag established. We deliberately ignore Click in
        // text_input.
        let mut value = String::from("hello");
        let mut sel = TextSelection::range(0, 5);
        let click = UiEvent {
            key: Some("ti".into()),
            target: Some(ti_target()),
            pointer: Some((ti_target().rect.x + 1.0, ti_target().rect.y + 18.0)),
            key_press: None,
            text: None,
            modifiers: KeyModifiers::default(),
            kind: UiEventKind::Click,
        };
        assert!(!apply_event(&mut value, &mut sel, &click));
        assert_eq!(sel, TextSelection::range(0, 5));
    }

    #[test]
    fn helpers_selected_text_and_replace_selection() {
        let value = String::from("hello world");
        let sel = TextSelection::range(6, 11);
        assert_eq!(selected_text(&value, sel), "world");

        let mut value = value;
        let mut sel = sel;
        replace_selection(&mut value, &mut sel, "kit");
        assert_eq!(value, "hello kit");
        assert_eq!(sel, TextSelection::caret(9));

        assert_eq!(select_all(&value), TextSelection::range(0, value.len()));
    }

    #[test]
    fn end_to_end_drag_select_through_runner_core() {
        // Lay out a tree with one text_input keyed "ti". Drive a
        // pointer_down + drag + pointer_up sequence through RunnerCore;
        // verify the resulting events fold into a non-empty selection.
        let mut value = String::from("hello world");
        let mut sel = TextSelection::default();
        let mut tree = crate::column([text_input(&value, sel).key("ti")]).padding(20.0);
        let mut core = RunnerCore::new();
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        core.ui_state = state;
        core.snapshot(&tree, &mut Default::default());

        let rect = core.rect_of_key("ti").expect("ti rect");
        let down_x = rect.x + 8.0;
        let drag_x = rect.x + 80.0;
        let cy = rect.y + rect.h * 0.5;

        core.pointer_moved(down_x, cy);
        let down = core
            .pointer_down(down_x, cy, PointerButton::Primary)
            .expect("pointer_down emits PointerDown");
        assert!(apply_event(&mut value, &mut sel, &down));

        let drag = core
            .pointer_moved(drag_x, cy)
            .expect("Drag while pressed");
        assert!(apply_event(&mut value, &mut sel, &drag));

        let events = core.pointer_up(drag_x, cy, PointerButton::Primary);
        for e in &events {
            apply_event(&mut value, &mut sel, e);
        }
        assert!(!sel.is_collapsed(), "expected drag-select to leave a non-empty selection");
        assert_eq!(sel.anchor, 0, "anchor should sit at the down position (caret 0)");
        assert!(sel.head > 0 && sel.head <= value.len(), "head={} value.len={}", sel.head, value.len());
    }
}
