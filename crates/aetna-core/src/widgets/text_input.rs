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
///
/// # Layout
///
/// The value is rendered as **one shaped text leaf** so cosmic-text
/// applies kerning across the whole string. The caret bar and the
/// selection band sit on top of the text via overlay layout +
/// paint-time `translate`, with offsets derived from `line_width` of
/// the prefix substrings. This means moving the caret never re-shapes
/// the text — characters don't "jitter" left/right as the caret moves.
///
/// # Focus
///
/// The caret bar carries `alpha_follows_focused_ancestor()` so it only
/// paints while the input is focused (and fades in/out via the
/// library's standard focus animation).
#[track_caller]
pub fn text_input(value: &str, selection: TextSelection) -> El {
    let head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    let anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    let lo = anchor.min(head);
    let hi = anchor.max(head);
    let line_h = line_height_px();

    // Pixel offsets along the (single) shaped run. We measure substrings
    // independently here, which gives positions that are correct to
    // within sub-pixel kerning differences vs. the full-string layout.
    // Good enough for caret + selection placement at v0.8 widths.
    let head_px = prefix_width(value, head);
    let lo_px = prefix_width(value, lo);
    let hi_px = prefix_width(value, hi);

    let mut children: Vec<El> = Vec::with_capacity(3);

    // Selection band paints first (behind text, behind caret).
    if lo < hi {
        children.push(
            El::new(Kind::Custom("text_input_selection"))
                .style_profile(StyleProfile::Solid)
                .fill(tokens::SELECTION_BG)
                .radius(2.0)
                .width(Size::Fixed(hi_px - lo_px))
                .height(Size::Fixed(line_h))
                .translate(lo_px, 0.0),
        );
    }

    // The value as one shaped run. Hug width so the leaf's intrinsic
    // measure is the actual glyph extent (used for layout).
    children.push(
        text(value)
            .font_size(tokens::FONT_BASE)
            .width(Size::Hug)
            .height(Size::Fixed(line_h)),
    );

    // Caret bar — always present in the tree; the focus-fade flag
    // hides it when the input isn't focused. This keeps the widget
    // builder stateless w.r.t. focus.
    children.push(
        caret_bar()
            .translate(head_px, 0.0)
            .alpha_follows_focused_ancestor(),
    );

    El::new(Kind::Custom("text_input"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .focusable()
        .capture_keys()
        .paint_overflow(Sides::all(tokens::FOCUS_RING_WIDTH))
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_MD)
        .axis(Axis::Overlay)
        .align(Align::Start) // children pin to the left edge
        .justify(Justify::Center) // children center vertically
        .height(Size::Fixed(36.0))
        .padding(Sides::xy(tokens::SPACE_MD, 0.0))
        .children(children)
}

fn caret_bar() -> El {
    El::new(Kind::Custom("text_input_caret"))
        .style_profile(StyleProfile::Solid)
        .fill(tokens::TEXT_FOREGROUND)
        .width(Size::Fixed(2.0))
        .height(Size::Fixed(line_height_px()))
        .radius(1.0)
}

fn line_height_px() -> f32 {
    metrics::line_height(tokens::FONT_BASE)
}

fn prefix_width(value: &str, byte_index: usize) -> f32 {
    if byte_index == 0 {
        return 0.0;
    }
    metrics::line_width(
        &value[..byte_index],
        tokens::FONT_BASE,
        FontWeight::Regular,
        false,
    )
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
            // winit emits the platform's text representation alongside
            // the named-key event for several "named" keys: Backspace
            // → "\u{8}", Delete → "\u{7f}", Enter → "\r"/"\n", Escape →
            // "\u{1b}", Tab → "\t". Without filtering, the named-key
            // handler runs (correct edit) AND the text gets inserted
            // (control char appears in the value). Strip control chars
            // so only printable input ever reaches the field.
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
    fn text_input_collapsed_renders_value_as_single_text_leaf_plus_caret() {
        let el = text_input("hello", TextSelection::caret(2));
        assert!(matches!(el.kind, Kind::Custom("text_input")));
        assert!(el.focusable);
        assert!(el.capture_keys);
        // [0] = text leaf with the full value, [1] = caret bar.
        assert_eq!(el.children.len(), 2);
        assert!(matches!(el.children[0].kind, Kind::Text));
        assert_eq!(el.children[0].text.as_deref(), Some("hello"));
        assert!(matches!(
            el.children[1].kind,
            Kind::Custom("text_input_caret")
        ));
        assert!(el.children[1].alpha_follows_focused_ancestor);
    }

    #[test]
    fn text_input_with_selection_inserts_selection_band_first() {
        // anchor=2, head=4 → selection "ll", head at right edge.
        let el = text_input("hello", TextSelection::range(2, 4));
        // [0] = selection band, [1] = full-value text leaf, [2] = caret.
        assert_eq!(el.children.len(), 3);
        assert!(matches!(
            el.children[0].kind,
            Kind::Custom("text_input_selection")
        ));
        assert_eq!(el.children[1].text.as_deref(), Some("hello"));
        assert!(matches!(
            el.children[2].kind,
            Kind::Custom("text_input_caret")
        ));
    }

    #[test]
    fn text_input_caret_translate_advances_with_head() {
        // The caret's translate.x grows with the head's byte index.
        // Use line_width as ground truth; caret should be measured from
        // the start of the value to head.
        use crate::text::metrics::line_width;
        let value = "hello";
        let head = 3;
        let el = text_input(value, TextSelection::caret(head));
        let caret = el
            .children
            .iter()
            .find(|c| matches!(c.kind, Kind::Custom("text_input_caret")))
            .expect("caret child");
        let expected = line_width(&value[..head], tokens::FONT_BASE, FontWeight::Regular, false);
        assert!(
            (caret.translate.0 - expected).abs() < 0.01,
            "caret translate.x = {}, expected {}",
            caret.translate.0,
            expected
        );
    }

    #[test]
    fn text_input_clamps_off_utf8_boundary() {
        // 'é' is two bytes; head=1 sits inside the codepoint and must
        // snap back to 0. The single text leaf still renders the whole
        // value; only the caret offset reflects the snap.
        let el = text_input("é", TextSelection::caret(1));
        assert_eq!(el.children[0].text.as_deref(), Some("é"));
        let caret = el
            .children
            .iter()
            .find(|c| matches!(c.kind, Kind::Custom("text_input_caret")))
            .expect("caret child");
        // caret head clamped to 0 → translate.x = 0.
        assert!(caret.translate.0.abs() < 0.01);
    }

    #[test]
    fn caret_alpha_follows_focus_envelope() {
        // The caret bar paints with full alpha when the input is
        // focused (envelope = 1) and zero alpha when it isn't
        // (envelope = 0). This is what hides the caret in unfocused
        // inputs without any app-side focus tracking.
        use crate::draw_ops::draw_ops;
        use crate::ir::DrawOp;
        use crate::shader::UniformValue;
        use crate::state::AnimationMode;
        use web_time::Instant;

        let mut tree =
            crate::column([text_input("hi", TextSelection::caret(0)).key("ti")]).padding(20.0);
        let mut state = UiState::new();
        state.set_animation_mode(AnimationMode::Settled);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.sync_focus_order(&tree);

        // Initially unfocused: focus envelope settles to 0.
        state.apply_to_state();
        state.tick_visual_animations(&mut tree, Instant::now());
        let caret_alpha = caret_fill_alpha(&tree, &state);
        assert_eq!(caret_alpha, Some(0), "unfocused → caret invisible");

        // Focus the input: focus envelope settles to 1.
        let target = state
            .focus_order
            .iter()
            .find(|t| t.key == "ti")
            .expect("ti in focus order")
            .clone();
        state.set_focus(Some(target));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree, Instant::now());
        let caret_alpha = caret_fill_alpha(&tree, &state);
        assert_eq!(
            caret_alpha,
            Some(255),
            "focused → caret fully visible (alpha=255)"
        );

        fn caret_fill_alpha(tree: &El, state: &UiState) -> Option<u8> {
            let ops = draw_ops(tree, state);
            for op in ops {
                if let DrawOp::Quad { id, uniforms, .. } = op
                    && id.contains("text_input_caret")
                    && let Some(UniformValue::Color(c)) = uniforms.get("fill")
                {
                    return Some(c.a);
                }
            }
            None
        }
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
    fn apply_text_input_filters_control_chars() {
        // winit emits "\u{8}" alongside the named Backspace key event.
        // The TextInput branch must reject it so only the KeyDown
        // handler edits the value.
        let mut value = String::from("hi");
        let mut sel = TextSelection::caret(2);
        for ctrl in ["\u{8}", "\u{7f}", "\r", "\n", "\u{1b}", "\t"] {
            assert!(
                !apply_event(&mut value, &mut sel, &ev_text(ctrl)),
                "expected {ctrl:?} to be filtered"
            );
            assert_eq!(value, "hi");
            assert_eq!(sel, TextSelection::caret(2));
        }
        // Mixed input — printable parts come through, control parts drop.
        assert!(apply_event(&mut value, &mut sel, &ev_text("a\u{8}b")));
        assert_eq!(value, "hiab");
        assert_eq!(sel, TextSelection::caret(4));
    }

    #[test]
    fn text_input_value_emits_a_single_glyph_run() {
        // A regression test for the v0.8.2 kerning bug: splitting the
        // value into [prefix, suffix] across the caret meant cosmic-
        // text shaped each substring independently, breaking kerning
        // and causing glyphs to "jump" left/right as the caret moved.
        // The fix renders the value as one shaped run.
        use crate::draw_ops::draw_ops;
        use crate::ir::DrawOp;
        let mut tree =
            crate::column([text_input("Type", TextSelection::caret(1)).key("ti")]).padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        let ops = draw_ops(&tree, &state);
        let glyph_runs = ops
            .iter()
            .filter(|op| {
                matches!(op, DrawOp::GlyphRun { id, .. } if id.contains("text_input[ti]"))
            })
            .count();
        assert_eq!(
            glyph_runs, 1,
            "value should shape as one run; got {glyph_runs}"
        );
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
