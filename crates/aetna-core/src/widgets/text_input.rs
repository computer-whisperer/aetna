//! Single-line text input widget with selection.
//!
//! `text_input(value, selection)` renders a focusable, key-capturing
//! input field with a visible caret and (when non-empty) a tinted
//! selection rectangle behind the selected glyphs. The application
//! owns both the string and the [`TextSelection`]; routed events are
//! folded back via [`apply_event`] in the app's `on_event` handler.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! struct Form {
//!     name: String,
//!     name_sel: TextSelection,
//! }
//!
//! impl App for Form {
//!     fn build(&self) -> El {
//!         text_input(&self.name, self.name_sel).key("name")
//!     }
//!
//!     fn on_event(&mut self, e: UiEvent) {
//!         if e.target_key() == Some("name") {
//!             text_input::apply_event(&mut self.name, &mut self.name_sel, &e);
//!         }
//!     }
//! }
//! ```
//!
//! # Dogfood note
//!
//! Composes only the public widget-kit surface. The widget pairs a
//! caret + character/IME path with selection semantics layered on top
//! via [`TextSelection`] (a value type, not stored in `widget_state`),
//! covering drag-select, shift-extend, replace-on-type, and `Ctrl+A`.
//! See `widget_kit.md`.

use std::borrow::Cow;
use std::panic::Location;

use crate::cursor::Cursor;
use crate::event::{UiEvent, UiEventKind, UiKey};
use crate::selection::{Selection, SelectionPoint, SelectionRange};
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

/// How (or whether) the rendered text should be visually masked. The
/// underlying `value` is always the real string; mask only affects
/// what's painted, what widths are measured against (so caret and
/// selection band line up with the dots), and which pointer column
/// maps to which byte offset.
///
/// The library's [`clipboard_request_for`] also reads this — copy /
/// cut are suppressed for masked fields (a password manager pasted in
/// is fine, but you don't want Ctrl+C to leak the secret to the system
/// clipboard).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaskMode {
    #[default]
    None,
    Password,
}

const MASK_CHAR: char = '•';

/// Optional configuration for [`text_input_with`] / [`apply_event_with`].
/// The defaults reproduce [`text_input`] / [`apply_event`] verbatim, so
/// callers only set the fields they need.
///
/// Fields mirror the corresponding HTML `<input>` attributes:
/// `placeholder`, `maxlength`, `type=password`. The same value is
/// expected to be available both at build-time (so the placeholder
/// renders, the mask is applied) and at event-time (so `max_length`
/// can clip a paste, and Copy / Cut can be suppressed on a masked
/// field) — that joint availability is why this is a struct the app
/// holds onto rather than chained modifiers on the returned `El`.
#[derive(Clone, Copy, Debug, Default)]
pub struct TextInputOpts<'a> {
    /// Muted hint text shown only while `value` is empty. Visible even
    /// while the field is focused (matches HTML `<input placeholder>`).
    pub placeholder: Option<&'a str>,
    /// Cap on the *character* count of `value` after an edit. Inserts
    /// (typing, paste, IME commit) are truncated so the post-edit
    /// length doesn't exceed this. Existing values longer than the cap
    /// are left alone — the cap only constrains future inserts.
    pub max_length: Option<usize>,
    /// Visual masking of the rendered value. See [`MaskMode`].
    pub mask: MaskMode,
}

impl<'a> TextInputOpts<'a> {
    pub fn placeholder(mut self, p: &'a str) -> Self {
        self.placeholder = Some(p);
        self
    }

    pub fn max_length(mut self, n: usize) -> Self {
        self.max_length = Some(n);
        self
    }

    pub fn password(mut self) -> Self {
        self.mask = MaskMode::Password;
        self
    }

    fn is_masked(&self) -> bool {
        !matches!(self.mask, MaskMode::None)
    }
}

impl TextSelection {
    /// Collapsed selection at byte offset `head`.
    pub const fn caret(head: usize) -> Self {
        Self { anchor: head, head }
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
/// Build a single-line text input that participates in the global
/// [`crate::selection::Selection`]. The widget reads its
/// caret + selection band through `selection.within(key)`:
///
/// - Selection is in this `key` → render caret at `head.byte` and a
///   band from `min(anchor.byte, head.byte)` to the max.
/// - Selection lives in another key (or is empty) → render no band;
///   caret falls back to byte 0 (still hidden by the focus envelope
///   when the input isn't focused).
///
/// The widget sets `.key(key)` on the returned `El` itself — callers
/// no longer chain `.key(...)` after this builder.
#[track_caller]
pub fn text_input(value: &str, selection: &Selection, key: &str) -> El {
    text_input_with(value, selection, key, TextInputOpts::default())
}

/// Like [`text_input`], but takes an optional [`TextInputOpts`] for
/// placeholder / max-length / password masking. Pass
/// `TextInputOpts::default()` for an output identical to
/// [`text_input`].
#[track_caller]
pub fn text_input_with(
    value: &str,
    selection: &Selection,
    key: &str,
    opts: TextInputOpts<'_>,
) -> El {
    let view = selection.within(key).unwrap_or_default();
    build_text_input(value, view, opts).key(key)
}

/// Render the input El given an already-extracted local view. Pure
/// rendering: doesn't touch [`Selection`], doesn't set the El's key.
/// Public callers should go through [`text_input`] /
/// [`text_input_with`] instead.
#[track_caller]
fn build_text_input(value: &str, selection: TextSelection, opts: TextInputOpts<'_>) -> El {
    let head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    let anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    let lo = anchor.min(head);
    let hi = anchor.max(head);
    let line_h = line_height_px();

    // Pick the rendered string. In password mode each scalar of `value`
    // becomes one bullet; widths and indices below all reference this
    // displayed string so the caret and selection band sit under the
    // dots, not under the (invisible) original glyphs.
    let display = display_str(value, opts.mask);

    // Pixel offsets along the (single) shaped run. We measure substrings
    // independently here, which gives positions that are correct to
    // within sub-pixel kerning differences vs. the full-string layout.
    // Good enough for caret + selection placement at typical widths.
    let to_display = |b: usize| original_to_display_byte(value, b, opts.mask);
    let head_px = prefix_width(&display, to_display(head));
    let lo_px = prefix_width(&display, to_display(lo));
    let hi_px = prefix_width(&display, to_display(hi));

    let mut children: Vec<El> = Vec::with_capacity(4);

    // Selection band paints first (behind text, behind caret). The
    // band is fill-only and inherits its parent input's focus
    // envelope, so `dim_fill` produces the macOS-style muted-when-
    // unfocused color without any per-frame state plumbing here.
    if lo < hi {
        children.push(
            El::new(Kind::Custom("text_input_selection"))
                .style_profile(StyleProfile::Solid)
                .fill(tokens::SELECTION_BG)
                .dim_fill(tokens::SELECTION_BG_UNFOCUSED)
                .radius(2.0)
                .width(Size::Fixed(hi_px - lo_px))
                .height(Size::Fixed(line_h))
                .translate(lo_px, 0.0),
        );
    }

    // Placeholder hint — shown only while the value is empty. Sits at
    // the same origin as the (empty) text leaf, so it visually fills
    // the gap. The caret still paints on top.
    if value.is_empty()
        && let Some(ph) = opts.placeholder
    {
        children.push(
            text(ph)
                .font_size(tokens::FONT_BASE)
                .muted()
                .width(Size::Hug)
                .height(Size::Fixed(line_h)),
        );
    }

    // The value (or its mask) as one shaped run. Hug width so the
    // leaf's intrinsic measure is the actual glyph extent.
    children.push(
        text(display.into_owned())
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
            .alpha_follows_focused_ancestor()
            .blink_when_focused(),
    );

    El::new(Kind::Custom("text_input"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Input)
        .focusable()
        .capture_keys()
        .paint_overflow(Sides::all(tokens::FOCUS_RING_WIDTH))
        .cursor(Cursor::Text)
        .fill(tokens::BG_MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_MD)
        .axis(Axis::Overlay)
        .align(Align::Start) // children pin to the left edge
        .justify(Justify::Center) // children center vertically
        .width(Size::Fill(1.0))
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
///
/// The function operates on the global [`Selection`] through `key`:
/// when an event mutates the input's contents, the result is written
/// back as a single-leaf range under `key`, transferring selection
/// ownership to this input. Callers route by `event.target_key()` for
/// pointer events; key events flow naturally to whatever widget is
/// focused (and the runtime targets the event accordingly).
pub fn apply_event(
    value: &mut String,
    selection: &mut Selection,
    key: &str,
    event: &UiEvent,
) -> bool {
    apply_event_with(value, selection, key, event, &TextInputOpts::default())
}

/// Like [`apply_event`], but takes a [`TextInputOpts`] so the field
/// honors `max_length` and password-masked pointer hits. Default opts
/// produce identical behavior to [`apply_event`].
pub fn apply_event_with(
    value: &mut String,
    selection: &mut Selection,
    key: &str,
    event: &UiEvent,
    opts: &TextInputOpts<'_>,
) -> bool {
    let mut local = selection.within(key).unwrap_or_default();
    let changed = fold_event_local(value, &mut local, event, opts);
    if changed {
        selection.range = Some(SelectionRange {
            anchor: SelectionPoint::new(key, local.anchor),
            head: SelectionPoint::new(key, local.head),
        });
    }
    changed
}

/// Apply the event to the input's *local* (`TextSelection`) view of
/// its slice. The internal worker behind [`apply_event_with`]; pure
/// in the sense that it doesn't touch [`Selection`].
fn fold_event_local(
    value: &mut String,
    selection: &mut TextSelection,
    event: &UiEvent,
    opts: &TextInputOpts<'_>,
) -> bool {
    selection.anchor = clamp_to_char_boundary(value, selection.anchor.min(value.len()));
    selection.head = clamp_to_char_boundary(value, selection.head.min(value.len()));
    match event.kind {
        UiEventKind::TextInput => {
            let Some(insert) = event.text.as_deref() else {
                return false;
            };
            // winit emits TextInput alongside named-key / shortcut
            // KeyDowns. Two filters protect us:
            //
            // 1. Strip control characters — winit fires "\u{8}" for
            //    Backspace, "\u{7f}" for Delete, "\r"/"\n" for Enter,
            //    "\u{1b}" for Escape, "\t" for Tab. The named-key arm
            //    handles those correctly; we don't want a duplicate
            //    insertion of the control byte.
            //
            // 2. Drop the event when Ctrl-or-Cmd is held (without Alt
            //    — AltGr on Windows is reported as Ctrl+Alt and is a
            //    legitimate text-producing modifier). Ctrl+C / Ctrl+V
            //    etc. emit TextInput("c"/"v") on some platforms; the
            //    clipboard side already handled the KeyDown, and we
            //    don't want the literal letter to land in the field.
            if (event.modifiers.ctrl && !event.modifiers.alt) || event.modifiers.logo {
                return false;
            }
            let filtered: String = insert.chars().filter(|c| !c.is_control()).collect();
            if filtered.is_empty() {
                return false;
            }
            let to_insert = clip_to_max_length(value, *selection, &filtered, opts.max_length);
            if to_insert.is_empty() {
                return false;
            }
            replace_selection(value, selection, &to_insert);
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
                    if selection.head == 0 && (mods.shift || selection.anchor == 0) {
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
                    if selection.head == end && (mods.shift || selection.anchor == end) {
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
            let pos = caret_from_x(value, local_x, opts.mask);
            // Multi-click: 2 = select word at hit; ≥3 = select all.
            // Modifier-shift extend still wins over multi-click — it
            // reads as "extend whatever I had", and that's what shift-
            // double-click does in browsers. Single-click (and
            // missing/zero count, e.g. synthetic events) keeps the
            // existing set-caret behavior.
            if !event.modifiers.shift {
                match event.click_count {
                    2 => {
                        let (lo, hi) = crate::selection::word_range_at(value, pos);
                        selection.anchor = lo;
                        selection.head = hi;
                        return true;
                    }
                    n if n >= 3 => {
                        selection.anchor = 0;
                        selection.head = value.len();
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
            let (Some((px, _py)), Some(target)) = (event.pointer, event.target.as_ref()) else {
                return false;
            };
            let local_x = px - target.rect.x - tokens::SPACE_MD;
            selection.head = caret_from_x(value, local_x, opts.mask);
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

/// [`replace_selection`] that respects [`TextInputOpts::max_length`]:
/// the replacement is truncated (by character count) so the post-edit
/// `value` doesn't exceed the cap. Use this for paste / drop / IME
/// commit flows where the field has a length cap. Returns the byte
/// length of the actually-inserted text — useful when the caller wants
/// to know whether the input was clipped.
pub fn replace_selection_with(
    value: &mut String,
    selection: &mut TextSelection,
    replacement: &str,
    opts: &TextInputOpts<'_>,
) -> usize {
    let clipped = clip_to_max_length(value, *selection, replacement, opts.max_length);
    let len = clipped.len();
    replace_selection(value, selection, &clipped);
    len
}

/// `(0, value.len())` — the selection that spans the whole field.
pub fn select_all(value: &str) -> TextSelection {
    TextSelection {
        anchor: 0,
        head: value.len(),
    }
}

/// Which clipboard operation a keypress is requesting. The library
/// itself never touches the platform clipboard; [`clipboard_request`]
/// just identifies the keystroke and the app dispatches the actual
/// `set_text` / `get_text` call against whatever clipboard backend it
/// uses (`arboard` on native, the web Clipboard API on wasm, etc.).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardKind {
    /// `Ctrl+C` / `Cmd+C` — copy the current selection.
    Copy,
    /// `Ctrl+X` / `Cmd+X` — copy the current selection, then delete it.
    Cut,
    /// `Ctrl+V` / `Cmd+V` — replace the selection with clipboard text.
    Paste,
}

/// Detect a clipboard keystroke (Ctrl/Cmd + C/X/V) in `event`.
/// Returns `None` for any other event, including `Ctrl+Shift+C`
/// (browser dev tools convention) and `Ctrl+Alt+V`.
///
/// Apps integrate clipboard by checking this before falling through
/// to [`apply_event`]:
///
/// ```ignore
/// match text_input::clipboard_request(&event) {
///     Some(ClipboardKind::Copy) => { clipboard.set_text(text_input::selected_text(&value, sel)); }
///     Some(ClipboardKind::Cut) => {
///         clipboard.set_text(text_input::selected_text(&value, sel));
///         text_input::replace_selection(&mut value, &mut sel, "");
///     }
///     Some(ClipboardKind::Paste) => {
///         if let Ok(text) = clipboard.get_text() {
///             text_input::replace_selection(&mut value, &mut sel, &text);
///         }
///     }
///     None => { text_input::apply_event(&mut value, &mut sel, &event); }
/// }
/// ```
pub fn clipboard_request(event: &UiEvent) -> Option<ClipboardKind> {
    clipboard_request_for(event, &TextInputOpts::default())
}

/// Mask-aware variant of [`clipboard_request`]: returns `None` for
/// `Copy` / `Cut` when the field is masked (password mode). Paste is
/// still recognized — pasting *into* a password field is normal.
pub fn clipboard_request_for(event: &UiEvent, opts: &TextInputOpts<'_>) -> Option<ClipboardKind> {
    if event.kind != UiEventKind::KeyDown {
        return None;
    }
    let kp = event.key_press.as_ref()?;
    let mods = kp.modifiers;
    // Reject when Alt or Shift is held — those modifiers select
    // different bindings (browser dev tools, alternative paste, etc.).
    if mods.alt || mods.shift {
        return None;
    }
    // Either Ctrl (Linux / Windows) or Logo / Cmd (macOS).
    if !(mods.ctrl || mods.logo) {
        return None;
    }
    let UiKey::Character(c) = &kp.key else {
        return None;
    };
    let kind = match c.to_ascii_lowercase().as_str() {
        "c" => ClipboardKind::Copy,
        "x" => ClipboardKind::Cut,
        "v" => ClipboardKind::Paste,
        _ => return None,
    };
    if opts.is_masked()
        && matches!(kind, ClipboardKind::Copy | ClipboardKind::Cut)
    {
        return None;
    }
    Some(kind)
}

fn caret_from_x(value: &str, local_x: f32, mask: MaskMode) -> usize {
    if value.is_empty() || local_x <= 0.0 {
        return 0;
    }
    let probe = display_str(value, mask);
    let local_y = metrics::line_height(tokens::FONT_BASE) * 0.5;
    let hit = hit_text(
        &probe,
        tokens::FONT_BASE,
        FontWeight::Regular,
        TextWrap::NoWrap,
        None,
        local_x,
        local_y,
    );
    let display_byte = match hit {
        Some(h) => h.byte_index.min(probe.len()),
        None => probe.len(),
    };
    display_to_original_byte(value, display_byte, mask)
}

/// Borrow `value` directly when [`MaskMode::None`]; otherwise build a
/// masked rendering (one [`MASK_CHAR`] per Unicode scalar). Used at
/// build-time to position the caret / selection band against the same
/// pixel widths the text leaf will eventually shape.
fn display_str(value: &str, mask: MaskMode) -> Cow<'_, str> {
    match mask {
        MaskMode::None => Cow::Borrowed(value),
        MaskMode::Password => {
            let n = value.chars().count();
            let mut s = String::with_capacity(n * MASK_CHAR.len_utf8());
            for _ in 0..n {
                s.push(MASK_CHAR);
            }
            Cow::Owned(s)
        }
    }
}

fn original_to_display_byte(value: &str, byte_index: usize, mask: MaskMode) -> usize {
    match mask {
        MaskMode::None => byte_index.min(value.len()),
        MaskMode::Password => {
            let clamped = clamp_to_char_boundary(value, byte_index.min(value.len()));
            value[..clamped].chars().count() * MASK_CHAR.len_utf8()
        }
    }
}

/// Inverse of [`original_to_display_byte`].
fn display_to_original_byte(value: &str, display_byte: usize, mask: MaskMode) -> usize {
    match mask {
        MaskMode::None => clamp_to_char_boundary(value, display_byte.min(value.len())),
        MaskMode::Password => {
            let scalar_idx = display_byte / MASK_CHAR.len_utf8();
            value
                .char_indices()
                .nth(scalar_idx)
                .map(|(i, _)| i)
                .unwrap_or(value.len())
        }
    }
}

/// Truncate `replacement` so that, after replacing the current
/// selection in `value`, the post-edit character count doesn't exceed
/// `max_length`. Returns `replacement` unchanged when no cap is set;
/// when the value already exceeds the cap, refuses any insert (we
/// don't auto-shrink an existing value just because the cap was
/// lowered — that's the caller's call). Defensive against an
/// unclamped `selection`.
fn clip_to_max_length<'a>(
    value: &str,
    selection: TextSelection,
    replacement: &'a str,
    max_length: Option<usize>,
) -> Cow<'a, str> {
    let Some(max) = max_length else {
        return Cow::Borrowed(replacement);
    };
    let lo = clamp_to_char_boundary(value, selection.anchor.min(selection.head).min(value.len()));
    let hi = clamp_to_char_boundary(value, selection.anchor.max(selection.head).min(value.len()));
    let post_other = value[..lo].chars().count() + value[hi..].chars().count();
    let allowed = max.saturating_sub(post_other);
    if replacement.chars().count() <= allowed {
        Cow::Borrowed(replacement)
    } else {
        Cow::Owned(replacement.chars().take(allowed).collect())
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

    /// Test key for the local-view shim helpers below. Matches the
    /// `.key("ti")` chain used by every fixture in this module so the
    /// `text_input` and `text_input_with` shims (which set the El's
    /// key internally) line up with the existing assertions.
    const TEST_KEY: &str = "ti";

    /// Wrap the old `text_input(value, TextSelection)` API by lifting
    /// the local view into a single-leaf [`Selection`] under
    /// [`TEST_KEY`]. Lets the existing test bodies stay readable
    /// against the post-migration API.
    #[track_caller]
    fn text_input(value: &str, sel: TextSelection) -> El {
        super::text_input(value, &as_selection(sel), TEST_KEY)
    }

    #[track_caller]
    fn text_input_with(value: &str, sel: TextSelection, opts: TextInputOpts<'_>) -> El {
        super::text_input_with(value, &as_selection(sel), TEST_KEY, opts)
    }

    fn apply_event(value: &mut String, sel: &mut TextSelection, event: &UiEvent) -> bool {
        let mut g = as_selection(*sel);
        let changed = super::apply_event(value, &mut g, TEST_KEY, event);
        sync_back(sel, &g);
        changed
    }

    fn apply_event_with(
        value: &mut String,
        sel: &mut TextSelection,
        event: &UiEvent,
        opts: &TextInputOpts<'_>,
    ) -> bool {
        let mut g = as_selection(*sel);
        let changed = super::apply_event_with(value, &mut g, TEST_KEY, event, opts);
        sync_back(sel, &g);
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

    fn sync_back(local: &mut TextSelection, global: &Selection) {
        match global.within(TEST_KEY) {
            Some(view) => *local = view,
            None => *local = TextSelection::default(),
        }
    }

    fn ev_text(s: &str) -> UiEvent {
        ev_text_with_mods(s, KeyModifiers::default())
    }

    fn ev_text_with_mods(s: &str, modifiers: KeyModifiers) -> UiEvent {
        UiEvent {
            key: None,
            target: None,
            pointer: None,
            key_press: None,
            text: Some(s.into()),
            selection: None,
            modifiers,
            click_count: 0,
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
            selection: None,
            modifiers,
            click_count: 0,
            kind: UiEventKind::KeyDown,
        }
    }

    fn ev_pointer_down(target: UiTarget, pointer: (f32, f32), modifiers: KeyModifiers) -> UiEvent {
        ev_pointer_down_with_count(target, pointer, modifiers, 1)
    }

    fn ev_pointer_down_with_count(
        target: UiTarget,
        pointer: (f32, f32),
        modifiers: KeyModifiers,
        click_count: u8,
    ) -> UiEvent {
        UiEvent {
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

    fn ev_drag(target: UiTarget, pointer: (f32, f32)) -> UiEvent {
        UiEvent {
            key: Some(target.key.clone()),
            target: Some(target),
            pointer: Some(pointer),
            key_press: None,
            text: None,
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 0,
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
    fn text_input_declares_text_cursor() {
        let el = text_input("hello", TextSelection::caret(0));
        assert_eq!(el.cursor, Some(Cursor::Text));
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
        let expected = line_width(
            &value[..head],
            tokens::FONT_BASE,
            FontWeight::Regular,
            false,
        );
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
    fn selection_band_fill_dims_when_input_unfocused() {
        // When the input lacks focus, the band paints in
        // SELECTION_BG_UNFOCUSED. As focus animates in, dim_fill lerps
        // the painted color toward SELECTION_BG.
        use crate::draw_ops::draw_ops;
        use crate::ir::DrawOp;
        use crate::shader::UniformValue;
        use crate::state::AnimationMode;
        use web_time::Instant;

        let mut tree =
            crate::column([text_input("hello", TextSelection::range(0, 5)).key("ti")])
                .padding(20.0);
        let mut state = UiState::new();
        state.set_animation_mode(AnimationMode::Settled);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.sync_focus_order(&tree);

        // Unfocused: focus envelope settles to 0 → band fill matches
        // SELECTION_BG_UNFOCUSED rgb (alpha is multiplied by `opacity`
        // so we compare rgb only).
        state.apply_to_state();
        state.tick_visual_animations(&mut tree, Instant::now());
        let unfocused = band_fill(&tree, &state).expect("band quad emitted");
        assert_eq!(
            (unfocused.r, unfocused.g, unfocused.b),
            (
                tokens::SELECTION_BG_UNFOCUSED.r,
                tokens::SELECTION_BG_UNFOCUSED.g,
                tokens::SELECTION_BG_UNFOCUSED.b
            ),
            "unfocused → band rgb is the muted token"
        );

        // Focused: focus envelope settles to 1 → band fill matches
        // SELECTION_BG.
        let target = state
            .focus_order
            .iter()
            .find(|t| t.key == "ti")
            .expect("ti in focus order")
            .clone();
        state.set_focus(Some(target));
        state.apply_to_state();
        state.tick_visual_animations(&mut tree, Instant::now());
        let focused = band_fill(&tree, &state).expect("band quad emitted");
        assert_eq!(
            (focused.r, focused.g, focused.b),
            (
                tokens::SELECTION_BG.r,
                tokens::SELECTION_BG.g,
                tokens::SELECTION_BG.b
            ),
            "focused → band rgb is the saturated token"
        );

        fn band_fill(tree: &El, state: &UiState) -> Option<crate::tree::Color> {
            let ops = draw_ops(tree, state);
            for op in ops {
                if let DrawOp::Quad { id, uniforms, .. } = op
                    && id.contains("text_input_selection")
                    && let Some(UniformValue::Color(c)) = uniforms.get("fill")
                {
                    return Some(*c);
                }
            }
            None
        }
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
    fn caret_blink_alpha_holds_solid_through_grace_then_cycles() {
        // The blink helper is deterministic on input duration; this
        // test pins the cycle shape we paint with.
        use crate::state::caret_blink_alpha_for;
        use std::time::Duration;
        // Inside the 500ms grace window → solid.
        assert_eq!(caret_blink_alpha_for(Duration::from_millis(0)), 1.0);
        assert_eq!(caret_blink_alpha_for(Duration::from_millis(499)), 1.0);
        // Past grace, first half of the 1060ms period → on.
        assert_eq!(caret_blink_alpha_for(Duration::from_millis(500)), 1.0);
        assert_eq!(caret_blink_alpha_for(Duration::from_millis(1029)), 1.0);
        // Second half → off.
        assert_eq!(caret_blink_alpha_for(Duration::from_millis(1030)), 0.0);
        assert_eq!(caret_blink_alpha_for(Duration::from_millis(1559)), 0.0);
        // Back to on for the next cycle.
        assert_eq!(caret_blink_alpha_for(Duration::from_millis(1560)), 1.0);
    }

    #[test]
    fn caret_paint_alpha_blinks_after_focus_in_live_mode() {
        // Drive the tick at staged Instants so we hit each phase of
        // the blink cycle; verifies the painter actually multiplies
        // the caret bar's alpha by ui_state.caret_blink_alpha.
        use crate::draw_ops::draw_ops;
        use crate::ir::DrawOp;
        use crate::shader::UniformValue;
        use crate::state::AnimationMode;
        use std::time::Duration;

        let mut tree =
            crate::column([text_input("hi", TextSelection::caret(0)).key("ti")]).padding(20.0);
        let mut state = UiState::new();
        state.set_animation_mode(AnimationMode::Live);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.sync_focus_order(&tree);

        // Focus the input — set_focus bumps caret_activity_at.
        let target = state
            .focus_order
            .iter()
            .find(|t| t.key == "ti")
            .unwrap()
            .clone();
        state.set_focus(Some(target));
        let activity_at = state.caret_activity_at.expect("set_focus bumps activity");
        let input_id = tree.children[0].computed_id.clone();

        // Pin focus envelope after each tick so the caret's
        // focus-fade contribution is out of the picture and we can
        // attribute alpha changes purely to the blink.
        let pin_focus = |state: &mut UiState| {
            state.envelopes.insert(
                (input_id.clone(), crate::state::EnvelopeKind::FocusRing),
                1.0,
            );
        };

        // t = 0 → grace, on.
        state.tick_visual_animations(&mut tree, activity_at);
        pin_focus(&mut state);
        assert_eq!(caret_alpha(&tree, &state), Some(255));

        // t = 1100ms → second half of cycle, off.
        state.tick_visual_animations(&mut tree, activity_at + Duration::from_millis(1100));
        pin_focus(&mut state);
        assert_eq!(caret_alpha(&tree, &state), Some(0));

        // t = 1600ms → back on.
        state.tick_visual_animations(&mut tree, activity_at + Duration::from_millis(1600));
        pin_focus(&mut state);
        assert_eq!(caret_alpha(&tree, &state), Some(255));

        fn caret_alpha(tree: &El, state: &UiState) -> Option<u8> {
            for op in draw_ops(tree, state) {
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
    fn caret_blink_resumes_solid_after_selection_change() {
        // Editing (selection change) bumps activity, which puts the
        // caret back into the grace window even mid-cycle.
        use crate::state::AnimationMode;
        use std::time::Duration;
        use web_time::Instant;

        let mut tree =
            crate::column([text_input("hi", TextSelection::caret(0)).key("ti")]).padding(20.0);
        let mut state = UiState::new();
        state.set_animation_mode(AnimationMode::Live);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.sync_focus_order(&tree);

        // Drive activity to deep into the off phase.
        let t0 = Instant::now();
        state.bump_caret_activity(t0);
        state.tick_visual_animations(&mut tree, t0 + Duration::from_millis(1100));
        assert_eq!(state.caret_blink_alpha, 0.0, "deep in off phase");

        // Re-bump (e.g. user typed) — alpha snaps back to solid.
        state.bump_caret_activity(t0 + Duration::from_millis(1100));
        assert_eq!(state.caret_blink_alpha, 1.0, "fresh activity → solid");
    }

    #[test]
    fn caret_tick_requests_redraw_while_capture_keys_node_focused() {
        // Without this, the host's animation loop wouldn't keep
        // pumping frames during idle, and the caret would freeze
        // mid-blink.
        use crate::state::AnimationMode;
        use web_time::Instant;

        let mut tree =
            crate::column([text_input("hi", TextSelection::caret(0)).key("ti")]).padding(20.0);
        let mut state = UiState::new();
        state.set_animation_mode(AnimationMode::Live);
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        state.sync_focus_order(&tree);

        // No focus → no redraw demand from blink.
        let no_focus = state.tick_visual_animations(&mut tree, Instant::now());
        assert!(!no_focus, "without focus, blink doesn't request redraws");

        // Focus the input → tick should keep requesting redraws so
        // the on/off cycle keeps animating.
        let target = state
            .focus_order
            .iter()
            .find(|t| t.key == "ti")
            .unwrap()
            .clone();
        state.set_focus(Some(target));
        let focused = state.tick_visual_animations(&mut tree, Instant::now());
        assert!(focused, "focused capture_keys node → tick demands redraws");
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
        assert!(!apply_event(
            &mut value,
            &mut sel,
            &ev_key(UiKey::Backspace)
        ));
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
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_key(UiKey::ArrowRight)
        ));
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
    fn apply_double_click_selects_word_at_caret() {
        let mut value = String::from("hello world");
        let mut sel = TextSelection::caret(0);
        // Click somewhere inside "world" with click_count = 2.
        let target = ti_target();
        let click_x = target.rect.x + tokens::SPACE_MD
            + crate::text::metrics::line_width(
                "hello w",
                tokens::FONT_BASE,
                FontWeight::Regular,
                false,
            );
        let down = ev_pointer_down_with_count(
            target.clone(),
            (click_x, target.rect.y + 18.0),
            KeyModifiers::default(),
            2,
        );
        assert!(apply_event(&mut value, &mut sel, &down));
        // "world" sits at bytes 6..11.
        assert_eq!(sel.anchor, 6);
        assert_eq!(sel.head, 11);
    }

    #[test]
    fn apply_triple_click_selects_all() {
        let mut value = String::from("hello world");
        let mut sel = TextSelection::caret(0);
        let target = ti_target();
        let down = ev_pointer_down_with_count(
            target.clone(),
            (target.rect.x + 1.0, target.rect.y + 18.0),
            KeyModifiers::default(),
            3,
        );
        assert!(apply_event(&mut value, &mut sel, &down));
        assert_eq!(sel.anchor, 0);
        assert_eq!(sel.head, value.len());
    }

    #[test]
    fn apply_shift_double_click_falls_back_to_extend_not_word_select() {
        // Shift + double-click extends the existing selection rather
        // than replacing it with the word — matching browser behavior.
        let mut value = String::from("hello world");
        let mut sel = TextSelection::caret(0);
        let target = ti_target();
        let click_x = target.rect.x + tokens::SPACE_MD
            + crate::text::metrics::line_width(
                "hello w",
                tokens::FONT_BASE,
                FontWeight::Regular,
                false,
            );
        let shift = KeyModifiers {
            shift: true,
            ..Default::default()
        };
        let down = ev_pointer_down_with_count(
            target.clone(),
            (click_x, target.rect.y + 18.0),
            shift,
            2,
        );
        assert!(apply_event(&mut value, &mut sel, &down));
        // anchor unchanged at 0; head moved to the click position.
        assert_eq!(sel.anchor, 0);
        assert!(sel.head > 0 && sel.head < value.len());
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
            selection: None,
            modifiers: KeyModifiers::default(),
            click_count: 1,
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
    fn apply_text_input_drops_when_ctrl_or_cmd_is_held() {
        // winit emits TextInput("c") alongside KeyDown(Ctrl+C) on some
        // platforms. The clipboard handler consumes the KeyDown; the
        // TextInput must be ignored, otherwise the literal 'c'
        // replaces the selection right after the copy.
        let mut value = String::from("hello");
        let mut sel = TextSelection::range(0, 5);
        let ctrl = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        let cmd = KeyModifiers {
            logo: true,
            ..Default::default()
        };
        assert!(!apply_event(
            &mut value,
            &mut sel,
            &ev_text_with_mods("c", ctrl)
        ));
        assert_eq!(value, "hello");
        assert!(!apply_event(
            &mut value,
            &mut sel,
            &ev_text_with_mods("v", cmd)
        ));
        assert_eq!(value, "hello");
        // AltGr (Ctrl+Alt) on Windows still produces text — exempt it.
        let altgr = KeyModifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        let mut value = String::from("");
        let mut sel = TextSelection::caret(0);
        assert!(apply_event(
            &mut value,
            &mut sel,
            &ev_text_with_mods("é", altgr)
        ));
        assert_eq!(value, "é");
    }

    #[test]
    fn text_input_value_emits_a_single_glyph_run() {
        // Regression test against a kerning bug: splitting the value
        // into [prefix, suffix] across the caret meant cosmic-text
        // shaped each substring independently, breaking kerning and
        // causing glyphs to "jump" left/right as the caret moved.
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
            .filter(|op| matches!(op, DrawOp::GlyphRun { id, .. } if id.contains("text_input[ti]")))
            .count();
        assert_eq!(
            glyph_runs, 1,
            "value should shape as one run; got {glyph_runs}"
        );
    }

    #[test]
    fn clipboard_request_detects_ctrl_c_x_v() {
        let ctrl = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        let cases = [
            ("c", ClipboardKind::Copy),
            ("C", ClipboardKind::Copy),
            ("x", ClipboardKind::Cut),
            ("v", ClipboardKind::Paste),
        ];
        for (ch, expected) in cases {
            let e = ev_key_with_mods(UiKey::Character(ch.into()), ctrl);
            assert_eq!(clipboard_request(&e), Some(expected), "char {ch:?}");
        }
    }

    #[test]
    fn clipboard_request_accepts_cmd_on_macos() {
        // winit reports Cmd as Logo. Apps should get the same behavior
        // on Linux/Windows (Ctrl) and macOS (Logo).
        let logo = KeyModifiers {
            logo: true,
            ..Default::default()
        };
        let e = ev_key_with_mods(UiKey::Character("c".into()), logo);
        assert_eq!(clipboard_request(&e), Some(ClipboardKind::Copy));
    }

    #[test]
    fn clipboard_request_rejects_with_shift_or_alt() {
        // Ctrl+Shift+C is browser devtools, not Copy.
        let e = ev_key_with_mods(
            UiKey::Character("c".into()),
            KeyModifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(clipboard_request(&e), None);

        let e = ev_key_with_mods(
            UiKey::Character("v".into()),
            KeyModifiers {
                ctrl: true,
                alt: true,
                ..Default::default()
            },
        );
        assert_eq!(clipboard_request(&e), None);
    }

    #[test]
    fn clipboard_request_ignores_other_keys_and_event_kinds() {
        // Plain "c" without modifiers is just text input.
        let e = ev_key(UiKey::Character("c".into()));
        assert_eq!(clipboard_request(&e), None);
        // Ctrl+A is select-all (handled by apply_event), not clipboard.
        let e = ev_key_with_mods(
            UiKey::Character("a".into()),
            KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
        );
        assert_eq!(clipboard_request(&e), None);
        // TextInput events never report a clipboard request.
        assert_eq!(clipboard_request(&ev_text("c")), None);
    }

    fn password_opts() -> TextInputOpts<'static> {
        TextInputOpts::default().password()
    }

    #[test]
    fn password_input_renders_value_as_bullets_not_plaintext() {
        // The text leaf should never expose the original characters in
        // a password field. One bullet per scalar.
        let el = text_input_with("hunter2", TextSelection::caret(0), password_opts());
        let leaf = el
            .children
            .iter()
            .find(|c| matches!(c.kind, Kind::Text))
            .expect("text leaf");
        assert_eq!(leaf.text.as_deref(), Some("•••••••"));
    }

    #[test]
    fn password_input_caret_position_uses_masked_widths() {
        // Caret offset must come from the rendered (masked) prefix
        // width, not the original-string prefix width — otherwise the
        // caret drifts away from the dots.
        use crate::text::metrics::line_width;
        let value = "abc";
        let head = 2;
        let el = text_input_with(value, TextSelection::caret(head), password_opts());
        let caret = el
            .children
            .iter()
            .find(|c| matches!(c.kind, Kind::Custom("text_input_caret")))
            .expect("caret child");
        // Two bullets of prefix.
        let expected = line_width("••", tokens::FONT_BASE, FontWeight::Regular, false);
        assert!(
            (caret.translate.0 - expected).abs() < 0.01,
            "caret translate.x = {}, expected {}",
            caret.translate.0,
            expected
        );
    }

    #[test]
    fn password_pointer_click_maps_back_to_original_byte() {
        // A pointer at the right edge of a 5-char password should
        // place the caret at byte index value.len() (=5 for ASCII).
        let mut value = String::from("abcde");
        let mut sel = TextSelection::default();
        let target = ti_target();
        let down = ev_pointer_down(
            target.clone(),
            (target.rect.x + target.rect.w - 4.0, target.rect.y + 18.0),
            KeyModifiers::default(),
        );
        assert!(apply_event_with(&mut value, &mut sel, &down, &password_opts()));
        assert_eq!(sel.head, value.len());
    }

    #[test]
    fn password_pointer_click_with_multibyte_value() {
        // Mask is one bullet per scalar; the returned byte index must
        // be a valid boundary in the (multi-byte) original value.
        // 'é' is 2 bytes; "éé" is 4 bytes total.
        let mut value = String::from("éé");
        let mut sel = TextSelection::default();
        let target = ti_target();
        // Click at a position that should land between the two bullets.
        let bullet_w = metrics::line_width("•", tokens::FONT_BASE, FontWeight::Regular, false);
        let click_x = target.rect.x + tokens::SPACE_MD + bullet_w * 1.4;
        let down = ev_pointer_down(
            target,
            (click_x, ti_target().rect.y + 18.0),
            KeyModifiers::default(),
        );
        assert!(apply_event_with(&mut value, &mut sel, &down, &password_opts()));
        // After 1 scalar in "éé" the byte offset is 2 (or 4 if the hit
        // landed past the second bullet). Either way, must be a char
        // boundary in `value`.
        assert!(
            value.is_char_boundary(sel.head),
            "head={} not on a char boundary in {value:?}",
            sel.head
        );
        assert!(sel.head == 2 || sel.head == 4, "head={}", sel.head);
    }

    #[test]
    fn password_clipboard_request_suppresses_copy_and_cut_only() {
        let ctrl = KeyModifiers {
            ctrl: true,
            ..Default::default()
        };
        let opts = password_opts();
        let copy = ev_key_with_mods(UiKey::Character("c".into()), ctrl);
        let cut = ev_key_with_mods(UiKey::Character("x".into()), ctrl);
        let paste = ev_key_with_mods(UiKey::Character("v".into()), ctrl);
        assert_eq!(clipboard_request_for(&copy, &opts), None);
        assert_eq!(clipboard_request_for(&cut, &opts), None);
        assert_eq!(
            clipboard_request_for(&paste, &opts),
            Some(ClipboardKind::Paste)
        );
        // Plain (non-masked) opts behave like the legacy entry point.
        let plain = TextInputOpts::default();
        assert_eq!(
            clipboard_request_for(&copy, &plain),
            Some(ClipboardKind::Copy)
        );
    }

    #[test]
    fn placeholder_renders_only_when_value_is_empty() {
        let opts = TextInputOpts::default().placeholder("Email");
        let empty = text_input_with("", TextSelection::default(), opts);
        let muted_leaf = empty.children.iter().find(|c| {
            matches!(c.kind, Kind::Text) && c.text.as_deref() == Some("Email")
        });
        assert!(muted_leaf.is_some(), "placeholder leaf should be present");

        let nonempty = text_input_with("hi", TextSelection::caret(2), opts);
        let muted_leaf = nonempty.children.iter().find(|c| {
            matches!(c.kind, Kind::Text) && c.text.as_deref() == Some("Email")
        });
        assert!(
            muted_leaf.is_none(),
            "placeholder should not render once the field has a value"
        );
    }

    #[test]
    fn max_length_truncates_text_input_inserts() {
        let mut value = String::from("ab");
        let mut sel = TextSelection::caret(2);
        let opts = TextInputOpts::default().max_length(4);
        // "cdef" would push to 6 chars; only "cd" fits.
        assert!(apply_event_with(
            &mut value,
            &mut sel,
            &ev_text("cdef"),
            &opts
        ));
        assert_eq!(value, "abcd");
        assert_eq!(sel, TextSelection::caret(4));
        // A further insert is refused — there's no room.
        assert!(!apply_event_with(
            &mut value,
            &mut sel,
            &ev_text("z"),
            &opts
        ));
        assert_eq!(value, "abcd");
    }

    #[test]
    fn max_length_replaces_selection_with_capacity_freed_by_removal() {
        // Replacing 3 chars with 5 chars at a 4-char cap: post_other = 0,
        // allowed = 4, replacement truncated to 4.
        let mut value = String::from("abc");
        let mut sel = TextSelection::range(0, 3); // whole value selected
        let opts = TextInputOpts::default().max_length(4);
        assert!(apply_event_with(
            &mut value,
            &mut sel,
            &ev_text("12345"),
            &opts
        ));
        assert_eq!(value, "1234");
        assert_eq!(sel, TextSelection::caret(4));
    }

    #[test]
    fn replace_selection_with_max_length_clips_a_paste() {
        let mut value = String::from("ab");
        let mut sel = TextSelection::caret(2);
        let opts = TextInputOpts::default().max_length(5);
        // Paste 10 chars into a value already at 2/5; only 3 fit.
        let inserted = replace_selection_with(&mut value, &mut sel, "0123456789", &opts);
        assert_eq!(value, "ab012");
        assert_eq!(inserted, 3);
        assert_eq!(sel, TextSelection::caret(5));
    }

    #[test]
    fn max_length_does_not_shrink_an_already_overlong_value() {
        // Caller is allowed to pass a value already longer than the cap;
        // the cap only constrains future inserts. Existing chars stay.
        let mut value = String::from("abcdef");
        let mut sel = TextSelection::caret(6);
        let opts = TextInputOpts::default().max_length(3);
        // No room for a new char.
        assert!(!apply_event_with(
            &mut value,
            &mut sel,
            &ev_text("z"),
            &opts
        ));
        assert_eq!(value, "abcdef");
        // But a delete still works — apply_event_with isn't gating
        // removals on max_length.
        assert!(apply_event_with(
            &mut value,
            &mut sel,
            &ev_key(UiKey::Backspace),
            &opts
        ));
        assert_eq!(value, "abcde");
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
            .into_iter()
            .find(|e| e.kind == UiEventKind::PointerDown)
            .expect("pointer_down emits PointerDown");
        assert!(apply_event(&mut value, &mut sel, &down));

        let drag = core
            .pointer_moved(drag_x, cy)
            .into_iter()
            .find(|e| e.kind == UiEventKind::Drag)
            .expect("Drag while pressed");
        assert!(apply_event(&mut value, &mut sel, &drag));

        let events = core.pointer_up(drag_x, cy, PointerButton::Primary);
        for e in &events {
            apply_event(&mut value, &mut sel, e);
        }
        assert!(
            !sel.is_collapsed(),
            "expected drag-select to leave a non-empty selection"
        );
        assert_eq!(
            sel.anchor, 0,
            "anchor should sit at the down position (caret 0)"
        );
        assert!(
            sel.head > 0 && sel.head <= value.len(),
            "head={} value.len={}",
            sel.head,
            value.len()
        );
    }

    // ---- Global-Selection integration ----
    //
    // The shimmed tests above exercise the local edit logic via the
    // `(value, &mut Selection, key, event)` API by routing through a
    // single fixed test key. The tests here verify the *integration*
    // semantics that only the post-migration API can express.

    #[test]
    fn apply_event_writes_back_under_the_inputs_key() {
        // Type a character: the resulting range lives under "name".
        let mut value = String::new();
        let mut sel = Selection::default();
        let event = ev_text("h");
        assert!(super::apply_event(&mut value, &mut sel, "name", &event));
        assert_eq!(value, "h");
        let r = sel.range.as_ref().expect("selection set");
        assert_eq!(r.anchor.key, "name");
        assert_eq!(r.head.key, "name");
        assert_eq!(r.head.byte, 1);
    }

    #[test]
    fn apply_event_claims_selection_when_event_routed_from_elsewhere() {
        // Selection is currently in another key (e.g. a static text
        // paragraph). The user is focused on the "email" input and
        // types — the event arrives because the runtime routes
        // capture_keys events to the focused element. apply_event
        // claims the selection by writing back into the input's key.
        let mut value = String::new();
        let mut sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("para-a", 0),
                head: SelectionPoint::new("para-a", 5),
            }),
        };
        let event = ev_text("x");
        assert!(super::apply_event(&mut value, &mut sel, "email", &event));
        assert_eq!(value, "x");
        let r = sel.range.as_ref().unwrap();
        assert_eq!(r.anchor.key, "email", "selection ownership migrated");
        assert_eq!(r.head.byte, 1);
    }

    #[test]
    fn apply_event_leaves_selection_alone_when_event_is_unhandled() {
        // A KeyDown the input doesn't recognize (e.g. F-key) should
        // not perturb the global selection — even if it lives in
        // another key. apply_event returns false; we don't write back.
        let mut value = String::from("hi");
        let mut sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("para-a", 0),
                head: SelectionPoint::new("para-a", 3),
            }),
        };
        let event = ev_key(UiKey::Other("F1".into()));
        assert!(!super::apply_event(&mut value, &mut sel, "name", &event));
        // Selection unchanged.
        let r = sel.range.as_ref().unwrap();
        assert_eq!(r.anchor.key, "para-a");
        assert_eq!(r.head.byte, 3);
    }

    #[test]
    fn text_input_renders_caret_at_local_byte_when_selection_is_within_key() {
        let sel = Selection::caret("name", 2);
        let el = super::text_input("hello", &sel, "name");
        // Builder set the El's key.
        assert_eq!(el.key.as_deref(), Some("name"));
        // Caret child translates to the prefix width of "he".
        let caret = el
            .children
            .iter()
            .find(|c| matches!(c.kind, Kind::Custom("text_input_caret")))
            .expect("caret child");
        let expected = metrics::line_width("he", tokens::FONT_BASE, FontWeight::Regular, false);
        assert!(
            (caret.translate.0 - expected).abs() < 0.01,
            "caret.x={} expected {}",
            caret.translate.0,
            expected
        );
    }

    #[test]
    fn text_input_falls_back_to_caret_zero_when_selection_lives_elsewhere() {
        // Selection is in another paragraph — this input renders no
        // band, and the caret falls back to byte 0 (still hidden by
        // the focus envelope when the input isn't focused).
        let sel = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("other", 0),
                head: SelectionPoint::new("other", 5),
            }),
        };
        let el = super::text_input("hello", &sel, "name");
        let band = el
            .children
            .iter()
            .find(|c| matches!(c.kind, Kind::Custom("text_input_selection")));
        assert!(band.is_none(), "no band when selection lives elsewhere");
        let caret = el
            .children
            .iter()
            .find(|c| matches!(c.kind, Kind::Custom("text_input_caret")))
            .expect("caret child");
        assert!(
            caret.translate.0.abs() < 0.01,
            "caret pinned at x=0; got {}",
            caret.translate.0
        );
    }
}
