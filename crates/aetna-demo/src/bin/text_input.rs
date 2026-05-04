//! Text input — smoke test for the v0.8.1/v0.8.2 widget.
//!
//! Two single-line `text_input` fields plus a live preview of the
//! current `(value, selection)` state for each. Run interactively:
//!
//! ```text
//! cargo run -p aetna-demo --bin text_input
//! ```
//!
//! Things to try in the window:
//!
//! - Click a field to focus it (focus ring fades in around it).
//! - Type to insert characters at the caret.
//! - Drag across text to select; the selection band paints behind the
//!   selected glyphs.
//! - Shift+ArrowLeft / ArrowRight / Home / End extend the selection.
//! - Plain ArrowLeft / ArrowRight / Home / End collapse + move.
//! - Backspace / Delete remove the selection if non-empty, otherwise
//!   one grapheme.
//! - Type while a selection is active — the selection is replaced.
//! - Ctrl+A selects all in the focused field.
//! - Ctrl+C / Ctrl+X / Ctrl+V (Cmd on macOS) — copy / cut / paste via
//!   the system clipboard. Try copying from one field and pasting
//!   into the other, or pasting text from another application.
//! - Tab / Shift+Tab moves focus between fields.

use aetna_core::widgets::text_input;
use aetna_core::*;

struct Form {
    name: String,
    name_sel: TextSelection,
    email: String,
    email_sel: TextSelection,
    clipboard: Option<arboard::Clipboard>,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            name: String::new(),
            name_sel: TextSelection::default(),
            email: String::new(),
            email_sel: TextSelection::default(),
            // arboard fails to initialize on headless / display-less
            // environments. Treat clipboard as best-effort.
            clipboard: arboard::Clipboard::new().ok(),
        }
    }
}

impl App for Form {
    fn build(&self) -> El {
        column([
            h2("Form"),
            field_row("Name", text_input(&self.name, self.name_sel).key("name")),
            field_row(
                "Email",
                text_input(&self.email, self.email_sel).key("email"),
            ),
            spacer().height(Size::Fixed(tokens::SPACE_LG)),
            preview_block(self),
            spacer().height(Size::Fixed(tokens::SPACE_LG)),
            row([
                button("Clear").key("clear").ghost(),
                spacer(),
                button("Submit").key("submit").primary(),
            ]),
        ])
        .padding(tokens::SPACE_XL)
        .gap(tokens::SPACE_MD)
    }

    fn on_event(&mut self, event: UiEvent) {
        // Click on a regular button.
        match (event.kind.clone(), event.key.as_deref()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("clear")) => {
                self.name.clear();
                self.name_sel = TextSelection::default();
                self.email.clear();
                self.email_sel = TextSelection::default();
                return;
            }
            (UiEventKind::Click | UiEventKind::Activate, Some("submit")) => {
                eprintln!("submit: name={:?} email={:?}", self.name, self.email);
                return;
            }
            _ => {}
        }
        // Route input events to the focused field by key.
        match event.target.as_ref().map(|t| t.key.as_str()) {
            Some("name") => apply_with_clipboard(
                &mut self.name,
                &mut self.name_sel,
                &event,
                self.clipboard.as_mut(),
            ),
            Some("email") => apply_with_clipboard(
                &mut self.email,
                &mut self.email_sel,
                &event,
                self.clipboard.as_mut(),
            ),
            _ => {}
        }
    }
}

fn apply_with_clipboard(
    value: &mut String,
    sel: &mut TextSelection,
    event: &UiEvent,
    clipboard: Option<&mut arboard::Clipboard>,
) {
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
            text_input::apply_event(value, sel, event);
        }
    }
}

fn field_row(label: &str, input: El) -> El {
    row([
        text(label).width(Size::Fixed(72.0)).muted(),
        input.width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_SM)
}

fn preview_block(form: &Form) -> El {
    card(
        "Live state",
        [
            preview_line("name", &form.name, form.name_sel),
            preview_line("email", &form.email, form.email_sel),
        ],
    )
}

fn preview_line(field: &str, value: &str, sel: TextSelection) -> El {
    let (lo, hi) = sel.ordered();
    let summary = if sel.is_collapsed() {
        format!("{field:>5} = {:?}  caret={}", value, sel.head)
    } else {
        format!(
            "{field:>5} = {:?}  selection={}..{}  ({:?})",
            value,
            lo,
            hi,
            &value[lo..hi]
        )
    };
    mono(summary).font_size(tokens::FONT_SM)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 640.0, 420.0);
    aetna_demo::run("Aetna — text_input smoke test", viewport, Form::default())
}
