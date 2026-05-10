//! Multi-line text area widget with selection.
//!
//! `text_area(value, selection, key)` is the multi-line companion to
//! [`crate::widgets::text_input::text_input`]. It shares the same app
//! state shape — a `String` (with embedded `\n`s) and a
//! global [`crate::selection::Selection`] — and delegates its
//! invariants (focusable + capture_keys + paint_overflow + style
//! profile + clipboard helpers) to the same kit primitives.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! struct Notes {
//!     body: String,
//!     selection: Selection,
//! }
//!
//! impl App for Notes {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         text_area(&self.body, &self.selection, "body")
//!     }
//!
//!     fn on_event(&mut self, e: UiEvent) {
//!         if e.target_key() == Some("body") {
//!             text_area::apply_event(&mut self.body, &mut self.selection, "body", &e);
//!         } else if let Some(selection) = e.selection.clone() {
//!             self.selection = selection;
//!         }
//!     }
//!
//!     fn selection(&self) -> Selection {
//!         self.selection.clone()
//!     }
//! }
//! ```
//!
//! # Differences from `text_input`
//!
//! - The shaped text leaf has `wrap_text()` so cosmic-text wraps lines
//!   to the container's content width.
//! - The selection band is rendered as one rectangle per visual line
//!   covered by the selection (via [`crate::selection_rects`]).
//! - The caret bar is positioned in 2D using [`crate::caret_xy`].
//! - Up/Down arrows navigate by re-hitting `(current_x, current_y ±
//!   line_h)`, which preserves visual column.
//! - `Enter` inserts `"\n"` instead of being consumed by the focus
//!   activation path.
//! - `Home`/`End` operate on the current visual line (line-wise), not
//!   the whole document.
//!
//! Everything else — drag-select, Shift+arrow extension, Ctrl+A,
//! clipboard via [`crate::widgets::text_input::clipboard_request`] —
//! is shared with `text_input` (the helpers `apply_event` calls into
//! also work for multi-line values).

use std::panic::Location;

use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind, UiKey};
use crate::metrics::MetricsRole;
use crate::selection::{Selection, SelectionPoint, SelectionRange};
use crate::style::StyleProfile;
use crate::text::metrics::TextGeometry;
use crate::tokens;
use crate::tree::*;
use crate::widgets::text::text;
use crate::widgets::text_input::{TextSelection, replace_selection};

/// Build a multi-line text area that participates in the global
/// [`crate::selection::Selection`]. The widget reads its caret +
/// selection band through `selection.within(key)`; an event-time
/// edit writes back as a single-leaf range under `key` (transferring
/// selection ownership into this area).
///
/// # Layout
///
/// The value is rendered as **one wrapped shaped text leaf** so
/// cosmic-text lays out the entire buffer in one shape pass. The
/// selection bands and caret bar sit on top via overlay layout +
/// paint-time `translate`. Selection / caret pixel positions are
/// derived from [`crate::selection_rects`] and [`crate::caret_xy`].
///
/// # Sizing
///
/// Defaults to `Fill(1.0)` width and a Hug height — the field grows
/// to fit its content, starting at one-line tall when empty. Use the
/// standard `.height(Size::Fixed(...))` builder to give it a fixed
/// shape (typical for forms).
#[track_caller]
pub fn text_area(value: &str, selection: &Selection, key: &str) -> El {
    build_text_area(value, selection.within(key)).key(key)
}

#[track_caller]
fn build_text_area(value: &str, view: Option<TextSelection>) -> El {
    let selection = view.unwrap_or_default();
    let head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    let anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    let lo = anchor.min(head);
    let hi = anchor.max(head);

    let mut children: Vec<El> = Vec::with_capacity(8);

    // Selection bands (one per visual line covered by the selection).
    // We use `None` for the wrap width here so the layout matches the
    // text leaf's eventual wrap. In practice the discrepancy is fine
    // because `text_area` always wraps to the same `available_width`
    // the layout pass passes to the text leaf — which is the container
    // content width. The builder doesn't have access to that width, so
    // the visual-line splits use NoWrap for the builder-time
    // approximation. This means visible selection bands mirror
    // BufferLine breaks (`\n`) but ignore soft wraps. Soft-wrap
    // selection painting is a future improvement.
    let geometry = text_area_geometry(value);
    let rects = geometry.selection_rects(lo, hi);
    for (rx, ry, rw, rh) in rects {
        children.push(
            El::new(Kind::Custom("text_area_selection"))
                .style_profile(StyleProfile::Solid)
                .fill(tokens::SELECTION_BG)
                .dim_fill(tokens::SELECTION_BG_UNFOCUSED)
                .radius(2.0)
                .width(Size::Fixed(rw))
                .height(Size::Fixed(rh))
                .translate(rx, ry),
        );
    }

    // The value rendered as one wrapped, shaped run. Hug height so the
    // container can grow to fit; Fill width so cosmic-text wraps to
    // the available width.
    children.push(
        text(value)
            .wrap_text()
            .width(Size::Fill(1.0))
            .height(Size::Hug),
    );

    // Caret bar — emitted only when the active selection actually
    // lives in this area. See the matching gate in
    // `text_input::build_text_input` for the rationale.
    if view.is_some() {
        let (caret_x, caret_y) = geometry.caret_xy(head);
        children.push(
            caret_bar()
                .translate(caret_x, caret_y)
                .alpha_follows_focused_ancestor()
                .blink_when_focused(),
        );
    }

    El::new(Kind::Custom("text_area"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::TextArea)
        .surface_role(SurfaceRole::Input)
        .focusable()
        // Same as text_input: ring stays on click too.
        .always_show_focus_ring()
        .capture_keys()
        .paint_overflow(Sides::all(tokens::RING_WIDTH))
        .cursor(Cursor::Text)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_MD)
        .axis(Axis::Overlay)
        .align(Align::Start)
        .justify(Justify::Start)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .default_padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .children(children)
}

fn caret_bar() -> El {
    El::new(Kind::Custom("text_area_caret"))
        .style_profile(StyleProfile::Solid)
        .fill(tokens::FOREGROUND)
        .width(Size::Fixed(2.0))
        .height(Size::Fixed(line_height_px()))
        .radius(1.0)
}

fn line_height_px() -> f32 {
    tokens::TEXT_SM.line_height
}

fn text_area_geometry(value: &str) -> TextGeometry<'_> {
    TextGeometry::new(
        value,
        tokens::TEXT_SM.size,
        FontWeight::Regular,
        false,
        TextWrap::NoWrap,
        None,
    )
}

/// Fold a routed [`UiEvent`] into `value` and the global
/// [`Selection`]. Returns `true` when either was mutated.
///
/// Same contract as [`crate::widgets::text_input::apply_event`] plus
/// these multi-line additions:
///
/// - [`UiKey::ArrowUp`] / [`UiKey::ArrowDown`] move the caret one
///   visual line up / down, preserving visual column. With Shift the
///   selection extends; without Shift it collapses to the new caret.
/// - [`UiKey::Enter`] inserts `"\n"` (replaces the selection if
///   non-empty, otherwise inserts at the caret).
/// - `Home` / `End` go to the start / end of the current visual line.
/// - Pointer events use 2D coordinates: clicking inside any line
///   positions the caret at the hit-tested glyph column.
///
/// On any mutation the selection is written back as a single-leaf
/// range under `key`, transferring ownership of the global selection
/// into this area.
pub fn apply_event(
    value: &mut String,
    selection: &mut Selection,
    key: &str,
    event: &UiEvent,
) -> bool {
    let mut local = selection.within(key).unwrap_or_default();
    let changed = fold_event_local(value, &mut local, event);
    if changed {
        selection.range = Some(SelectionRange {
            anchor: SelectionPoint::new(key, local.anchor),
            head: SelectionPoint::new(key, local.head),
        });
    }
    changed
}

/// Apply the event to the area's local view. Internal worker behind
/// [`apply_event`]; pure in the sense that it doesn't touch
/// [`Selection`].
fn fold_event_local(value: &mut String, selection: &mut TextSelection, event: &UiEvent) -> bool {
    selection.anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    selection.head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    match event.kind {
        UiEventKind::TextInput => {
            let Some(insert) = event.text.as_deref() else {
                return false;
            };
            // See text_input::apply_event for the rationale: drop
            // shortcut-side TextInput emissions (Ctrl/Cmd held) so the
            // 'c' from Ctrl+C doesn't replace the selection after the
            // clipboard handler has already consumed the keystroke.
            if (event.modifiers.ctrl && !event.modifiers.alt) || event.modifiers.logo {
                return false;
            }
            let filtered: String = insert.chars().filter(|c| !c.is_control()).collect();
            if filtered.is_empty() {
                return false;
            }
            replace_selection(value, selection, &filtered);
            true
        }
        UiEventKind::KeyDown => {
            let Some(kp) = event.key_press.as_ref() else {
                return false;
            };
            let mods = kp.modifiers;
            // Ctrl+A: select all.
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
                UiKey::Enter => {
                    replace_selection(value, selection, "\n");
                    true
                }
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
                        selection.ordered().1
                    };
                    selection.head = target;
                    if !mods.shift {
                        selection.anchor = target;
                    }
                    true
                }
                UiKey::ArrowUp => {
                    let new = move_caret_vertically(value, selection.head, -1);
                    if new == selection.head {
                        return false;
                    }
                    selection.head = new;
                    if !mods.shift {
                        selection.anchor = new;
                    }
                    true
                }
                UiKey::ArrowDown => {
                    let new = move_caret_vertically(value, selection.head, 1);
                    if new == selection.head {
                        return false;
                    }
                    selection.head = new;
                    if !mods.shift {
                        selection.anchor = new;
                    }
                    true
                }
                UiKey::Home => {
                    let line_start = current_line_start(value, selection.head);
                    if selection.head == line_start
                        && (mods.shift || selection.anchor == line_start)
                    {
                        return false;
                    }
                    selection.head = line_start;
                    if !mods.shift {
                        selection.anchor = line_start;
                    }
                    true
                }
                UiKey::End => {
                    let line_end = current_line_end(value, selection.head);
                    if selection.head == line_end && (mods.shift || selection.anchor == line_end) {
                        return false;
                    }
                    selection.head = line_end;
                    if !mods.shift {
                        selection.anchor = line_end;
                    }
                    true
                }
                _ => false,
            }
        }
        UiEventKind::PointerDown => {
            let (Some((px, py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_3;
            let local_y = py - target.rect.y - tokens::SPACE_2;
            let pos = caret_from_xy(value, local_x, local_y);
            // Multi-click: 2 = word at caret, ≥3 = line containing
            // caret (delimited by '\n'). Shift+multi-click falls
            // through to the extend behavior, same as text_input.
            if !event.modifiers.shift {
                match event.click_count {
                    2 => {
                        let (lo, hi) = crate::selection::word_range_at(value, pos);
                        selection.anchor = lo;
                        selection.head = hi;
                        return true;
                    }
                    n if n >= 3 => {
                        let (lo, hi) = crate::selection::line_range_at(value, pos);
                        selection.anchor = lo;
                        selection.head = hi;
                        return true;
                    }
                    _ => {}
                }
            }
            selection.head = pos;
            if !event.modifiers.shift {
                selection.anchor = pos;
            }
            true
        }
        UiEventKind::Drag => {
            let (Some((px, py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_3;
            let local_y = py - target.rect.y - tokens::SPACE_2;
            selection.head = caret_from_xy(value, local_x, local_y);
            true
        }
        UiEventKind::Click => false,
        _ => false,
    }
}

/// Return the byte offset of the caret position one visual line in
/// `direction` (-1 = up, +1 = down) from `byte_index`. Returns the
/// input unchanged when there is no line in that direction (already at
/// the first / last visual line).
fn move_caret_vertically(value: &str, byte_index: usize, direction: i32) -> usize {
    let geometry = text_area_geometry(value);
    let (x, y) = geometry.caret_xy(byte_index);
    let line_h = geometry.line_height();
    let target_y = y + direction as f32 * line_h;
    if target_y < -0.5 {
        // Above the first line — clamp to start of value.
        return 0;
    }
    // Probe slightly inside the target line to make the geometry hit-test find it.
    let probe_y = target_y + line_h * 0.5;
    let Some(byte) = geometry.hit_byte(x, probe_y) else {
        // No line at probe_y — past the bottom of the text. Clamp to end.
        return value.len();
    };
    byte
}

/// Resolve the byte offset a pointer event maps to inside a text
/// area's `value`. Mirrors [`crate::widgets::text_input::caret_byte_at`]
/// but accounts for vertical position as well, so the caller lands on
/// the line under the pointer. Returns `None` for events without a
/// pointer or target rect. Used by Linux middle-click paste flows.
#[track_caller]
pub fn caret_byte_at(value: &str, event: &UiEvent) -> Option<usize> {
    let (px, py) = event.pointer?;
    let target = event.target.as_ref()?;
    let local_x = px - target.rect.x - tokens::SPACE_3;
    let local_y = py - target.rect.y - tokens::SPACE_2;
    Some(caret_from_xy(value, local_x, local_y))
}

fn caret_from_xy(value: &str, x: f32, y: f32) -> usize {
    let geometry = text_area_geometry(value);
    let line_h = geometry.line_height();
    let probe_y = y.max(line_h * 0.5);
    let Some(byte) = geometry.hit_byte(x.max(0.0), probe_y) else {
        return value.len();
    };
    byte
}

fn current_line_start(value: &str, byte_index: usize) -> usize {
    value[..byte_index.min(value.len())]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0)
}

fn current_line_end(value: &str, byte_index: usize) -> usize {
    let from = byte_index.min(value.len());
    value[from..]
        .find('\n')
        .map(|i| from + i)
        .unwrap_or(value.len())
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
    use crate::event::{KeyModifiers, KeyPress};

    /// Test key for the local-view shim. Mirrors the one in
    /// `text_input::tests`; lets the existing test bodies keep using
    /// `apply_event(&mut value, &mut sel, &event)` against the new
    /// `(value, &mut Selection, key, event)` API.
    const TEST_KEY: &str = "ta";

    fn text_area(value: &str, sel: TextSelection) -> El {
        super::text_area(value, &as_selection(sel), TEST_KEY)
    }

    fn apply_event(value: &mut String, sel: &mut TextSelection, event: &UiEvent) -> bool {
        let mut g = as_selection(*sel);
        let changed = super::apply_event(value, &mut g, TEST_KEY, event);
        match g.within(TEST_KEY) {
            Some(view) => *sel = view,
            None => *sel = TextSelection::default(),
        }
        changed
    }

    fn as_selection(sel: TextSelection) -> Selection {
        Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new(TEST_KEY, sel.anchor),
                head: SelectionPoint::new(TEST_KEY, sel.head),
            }),
        }
    }

    fn ev_key(key: UiKey) -> UiEvent {
        ev_key_with_mods(key, KeyModifiers::default())
    }

    fn ev_key_with_mods(key: UiKey, modifiers: KeyModifiers) -> UiEvent {
        UiEvent {
            path: None,
            key: None,
            target: None,
            pointer: None,
            key_press: Some(KeyPress {
                key,
                modifiers,
                repeat: false,
            }),
            text: None,
            selection: None,
            modifiers,
            click_count: 0,
            kind: UiEventKind::KeyDown,
        }
    }

    fn ta_target() -> crate::event::UiTarget {
        crate::event::UiTarget {
            key: "ta".to_string(),
            node_id: "/ta".to_string(),
            rect: crate::tree::Rect::new(0.0, 0.0, 200.0, 100.0),
            tooltip: None,
        }
    }

    fn ev_pointer_down_with_count(
        local: (f32, f32),
        modifiers: KeyModifiers,
        click_count: u8,
    ) -> UiEvent {
        let target = ta_target();
        let pointer = (
            target.rect.x + tokens::SPACE_3 + local.0,
            target.rect.y + tokens::SPACE_2 + local.1,
        );
        UiEvent {
            path: None,
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: None,
            selection: None,
            modifiers,
            click_count,
            kind: UiEventKind::PointerDown,
        }
    }

    #[test]
    fn text_area_declares_text_cursor() {
        let el = text_area("hello", TextSelection::caret(0));
        assert_eq!(el.cursor, Some(Cursor::Text));
    }

    #[test]
    fn enter_inserts_newline_and_advances_caret() {
        let mut value = String::from("hello");
        let mut sel = TextSelection::caret(2);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::Enter)));
        assert_eq!(value, "he\nllo");
        assert_eq!(sel, TextSelection::caret(3));
    }

    #[test]
    fn arrow_down_moves_to_next_line_at_similar_column() {
        let mut value = String::from("alpha\nbravo");
        // Caret at 'p' in alpha (index 2). Down should land near 'a'
        // in bravo (index 8 = 6 + 2).
        let mut sel = TextSelection::caret(2);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowDown)));
        assert!(
            (8..=10).contains(&sel.head),
            "head={} not near column 2 of line 2",
            sel.head
        );
        assert_eq!(sel.anchor, sel.head);
    }

    #[test]
    fn arrow_up_at_top_clamps_to_start() {
        let mut value = String::from("alpha\nbravo");
        let mut sel = TextSelection::caret(2);
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::ArrowUp)));
        assert_eq!(sel, TextSelection::caret(0));
    }

    #[test]
    fn home_goes_to_current_line_start() {
        let mut value = String::from("alpha\nbravo");
        let mut sel = TextSelection::caret(8); // 'a' in bravo
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::Home)));
        assert_eq!(sel, TextSelection::caret(6));
    }

    #[test]
    fn end_goes_to_current_line_end() {
        let mut value = String::from("alpha\nbravo");
        let mut sel = TextSelection::caret(7); // 'r' in bravo
        assert!(apply_event(&mut value, &mut sel, &ev_key(UiKey::End)));
        assert_eq!(sel, TextSelection::caret(11));
    }

    #[test]
    fn shift_arrow_down_extends_selection_anchor_stays() {
        let mut value = String::from("alpha\nbravo");
        let mut sel = TextSelection::caret(2);
        let mods = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key_with_mods(UiKey::ArrowDown, mods)
        ));
        assert_eq!(sel.anchor, 2);
        assert!(sel.head > 2);
    }

    #[test]
    fn double_click_selects_word_at_caret() {
        let mut value = String::from("first second\nthird");
        let mut sel = TextSelection::caret(0);
        // Click at top-left → caret_from_xy returns ~0; word_range_at
        // returns "first" → bytes 0..5.
        let down = ev_pointer_down_with_count((1.0, 1.0), KeyModifiers::default(), 2);
        assert!(apply_event(&mut value, &mut sel, &down));
        assert_eq!(sel.anchor, 0);
        assert_eq!(sel.head, 5);
    }

    #[test]
    fn triple_click_selects_line_around_caret_not_whole_value() {
        // text_area's triple-click selects the *line* (delimited by
        // '\n'), not the whole value — the user might have a long
        // multi-line note and want to grab a single paragraph.
        let mut value = String::from("first line\nsecond line\nthird");
        let mut sel = TextSelection::caret(0);
        // Click at top-left → first line.
        let down = ev_pointer_down_with_count((1.0, 1.0), KeyModifiers::default(), 3);
        assert!(apply_event(&mut value, &mut sel, &down));
        assert_eq!(sel.anchor, 0);
        assert_eq!(sel.head, 10, "selects 'first line' (excludes the \\n)");
    }

    #[test]
    fn ctrl_or_cmd_text_input_is_dropped() {
        // Mirror of the text_input regression: winit can emit
        // TextInput("c") alongside KeyDown(Ctrl+C) on some platforms,
        // and the clipboard wrapper consumes the KeyDown. Without the
        // ctrl/cmd guard, 'c' would replace the selection after the
        // copy.
        let mut value = String::from("first\nsecond");
        let mut sel = TextSelection::range(0, value.len());
        let ctrl = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        let ev = UiEvent {
            path: None,
            key: None,
            target: None,
            pointer: None,
            key_press: None,
            text: Some("c".into()),
            selection: None,
            modifiers: ctrl,
            click_count: 0,
            kind: UiEventKind::TextInput,
        };
        assert!(!apply_event(&mut value, &mut sel, &ev));
        assert_eq!(value, "first\nsecond");
    }

    #[test]
    fn renders_as_overlay_with_capture_keys_and_focus_ring() {
        let el = text_area("foo\nbar", TextSelection::caret(0));
        assert!(matches!(el.kind, Kind::Custom("text_area")));
        assert!(el.focusable);
        assert!(el.capture_keys);
        assert!(matches!(el.axis, Axis::Overlay));
    }
}
