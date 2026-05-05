//! Text area — smoke test for the v0.8.4 multi-line widget.
//!
//! Single multi-line `text_area` plus a live preview of `(value,
//! selection)`. Run interactively:
//!
//! ```text
//! cargo run -p aetna-examples --bin text_area
//! ```
//!
//! Things to try in the window:
//!
//! - Click anywhere in the field to focus it. The focus ring fades in.
//! - Type to insert characters; `Enter` inserts a newline.
//! - Backspace / Delete remove the selection (or one grapheme when
//!   collapsed).
//! - Arrow keys navigate; Up/Down move between lines preserving the
//!   visual column.
//! - Shift+arrows extend the selection (including across line breaks).
//! - Drag across text to select. The selection band paints behind the
//!   selected glyphs, one rectangle per visual line.
//! - Home / End go to the start / end of the current line; Shift
//!   variants extend the selection.
//! - Ctrl+A selects all.
//! - Ctrl+C / Ctrl+X / Ctrl+V (Cmd on macOS) — copy / cut / paste via
//!   the system clipboard, including multi-line text from any other
//!   application.

use aetna_core::widgets::{text_area, text_input};
use aetna_core::*;

const PRESET: &str = "Multi-line text area.\n\
Try Enter for new lines, Up/Down to move between them.\n\
Shift+Arrow extends the selection across line breaks.";

struct Notes {
    body: String,
    body_sel: TextSelection,
    clipboard: Option<arboard::Clipboard>,
}

impl Default for Notes {
    fn default() -> Self {
        Self {
            body: PRESET.to_string(),
            body_sel: TextSelection::caret(0),
            // arboard fails to initialize on headless / display-less
            // environments. Treat clipboard as best-effort.
            clipboard: arboard::Clipboard::new().ok(),
        }
    }
}

impl App for Notes {
    fn build(&self) -> El {
        column([
            h2("Notes"),
            text_area(&self.body, self.body_sel)
                .key("body")
                .height(Size::Fixed(180.0)),
            spacer().height(Size::Fixed(tokens::SPACE_LG)),
            preview_block(self),
            spacer().height(Size::Fixed(tokens::SPACE_LG)),
            row([
                button("Clear").key("clear").ghost(),
                spacer(),
                button("Reset").key("reset").secondary(),
            ]),
        ])
        .padding(tokens::SPACE_XL)
        .gap(tokens::SPACE_MD)
    }

    fn on_event(&mut self, event: UiEvent) {
        match (event.kind.clone(), event.key.as_deref()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("clear")) => {
                self.body.clear();
                self.body_sel = TextSelection::default();
                return;
            }
            (UiEventKind::Click | UiEventKind::Activate, Some("reset")) => {
                self.body = PRESET.to_string();
                self.body_sel = TextSelection::caret(0);
                return;
            }
            _ => {}
        }
        if event.target.as_ref().map(|t| t.key.as_str()) == Some("body") {
            apply_with_clipboard(
                &mut self.body,
                &mut self.body_sel,
                &event,
                self.clipboard.as_mut(),
            );
        }
    }
}

fn apply_with_clipboard(
    value: &mut String,
    sel: &mut TextSelection,
    event: &UiEvent,
    clipboard: Option<&mut arboard::Clipboard>,
) {
    // The clipboard keystroke detector is shared with text_input — it
    // identifies Ctrl/Cmd+C/X/V independent of which widget handles the
    // body of the event.
    match text_input::clipboard_request(event) {
        Some(ClipboardKind::Copy) => {
            if let Some(cb) = clipboard {
                let _ = cb.set_text(text_input::selected_text(value, *sel).to_string());
            }
        }
        Some(ClipboardKind::Cut) => {
            if let Some(cb) = clipboard {
                let _ = cb.set_text(text_input::selected_text(value, *sel).to_string());
            }
            text_input::replace_selection(value, sel, "");
        }
        Some(ClipboardKind::Paste) => {
            if let Some(cb) = clipboard
                && let Ok(text) = cb.get_text()
            {
                text_input::replace_selection(value, sel, &text);
            }
        }
        None => {
            text_area::apply_event(value, sel, event);
        }
    }
}

fn preview_block(form: &Notes) -> El {
    let (lo, hi) = form.body_sel.ordered();
    let summary = if form.body_sel.is_collapsed() {
        format!(
            "len={}  caret={}  lines={}",
            form.body.len(),
            form.body_sel.head,
            form.body.lines().count().max(1)
        )
    } else {
        format!(
            "len={}  selection={}..{}  selected={:?}",
            form.body.len(),
            lo,
            hi,
            &form.body[lo..hi]
        )
    };
    card("Live state", [mono(summary).font_size(tokens::FONT_SM)])
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 520.0);
    aetna_winit_wgpu::run("Aetna — text_area smoke test", viewport, Notes::default())
}
