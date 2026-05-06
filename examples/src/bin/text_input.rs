//! Text input — smoke test for the single-line text widget.
//!
//! Two single-line `text_input` fields plus a live preview of the
//! current `(value, selection)` state for each. Run interactively:
//!
//! ```text
//! cargo run -p aetna-examples --bin text_input
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
//! - Empty fields show a muted placeholder hint until you type.
//! - The PIN field is capped at 6 characters via `max_length`.
//! - The Password field renders bullets, and Ctrl+C / Ctrl+X are
//!   suppressed (Ctrl+V still works — pasting *into* a password field
//!   is fine).

use aetna_core::prelude::*;
use aetna_core::widgets::text_input;

struct Form {
    name: String,
    name_sel: TextSelection,
    email: String,
    email_sel: TextSelection,
    pin: String,
    pin_sel: TextSelection,
    password: String,
    password_sel: TextSelection,
    clipboard: Option<arboard::Clipboard>,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            name: String::new(),
            name_sel: TextSelection::default(),
            email: String::new(),
            email_sel: TextSelection::default(),
            pin: String::new(),
            pin_sel: TextSelection::default(),
            password: String::new(),
            password_sel: TextSelection::default(),
            // arboard fails to initialize on headless / display-less
            // environments. Treat clipboard as best-effort.
            clipboard: arboard::Clipboard::new().ok(),
        }
    }
}

const PIN_OPTS: TextInputOpts<'_> = TextInputOpts {
    placeholder: Some("6 digits"),
    max_length: Some(6),
    mask: MaskMode::None,
};

const PASSWORD_OPTS: TextInputOpts<'_> = TextInputOpts {
    placeholder: Some("••••••••"),
    max_length: None,
    mask: MaskMode::Password,
};

impl Form {
    fn opts_for(&self, key: &str) -> TextInputOpts<'static> {
        match key {
            "name" => TextInputOpts::default().placeholder("Your name"),
            "email" => TextInputOpts::default().placeholder("you@example.com"),
            "pin" => PIN_OPTS,
            "password" => PASSWORD_OPTS,
            _ => TextInputOpts::default(),
        }
    }
}

impl App for Form {
    fn build(&self) -> El {
        column([
            h2("Form"),
            field_row(
                "Name",
                text_input_with(&self.name, self.name_sel, self.opts_for("name")).key("name"),
            ),
            field_row(
                "Email",
                text_input_with(&self.email, self.email_sel, self.opts_for("email")).key("email"),
            ),
            field_row(
                "PIN",
                text_input_with(&self.pin, self.pin_sel, self.opts_for("pin")).key("pin"),
            ),
            field_row(
                "Password",
                text_input_with(&self.password, self.password_sel, self.opts_for("password"))
                    .key("password"),
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
        match (event.kind, event.route()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("clear")) => {
                self.name.clear();
                self.name_sel = TextSelection::default();
                self.email.clear();
                self.email_sel = TextSelection::default();
                self.pin.clear();
                self.pin_sel = TextSelection::default();
                self.password.clear();
                self.password_sel = TextSelection::default();
                return;
            }
            (UiEventKind::Click | UiEventKind::Activate, Some("submit")) => {
                eprintln!(
                    "submit: name={:?} email={:?} pin={:?} password=<{} chars>",
                    self.name,
                    self.email,
                    self.pin,
                    self.password.chars().count()
                );
                return;
            }
            _ => {}
        }
        let key = match event.target_key() {
            Some(k) => k.to_owned(),
            None => return,
        };
        let opts = self.opts_for(&key);
        let (value, sel): (&mut String, &mut TextSelection) = match key.as_str() {
            "name" => (&mut self.name, &mut self.name_sel),
            "email" => (&mut self.email, &mut self.email_sel),
            "pin" => (&mut self.pin, &mut self.pin_sel),
            "password" => (&mut self.password, &mut self.password_sel),
            _ => return,
        };
        apply_with_clipboard(value, sel, &event, &opts, self.clipboard.as_mut());
    }
}

fn apply_with_clipboard(
    value: &mut String,
    sel: &mut TextSelection,
    event: &UiEvent,
    opts: &TextInputOpts<'_>,
    clipboard: Option<&mut arboard::Clipboard>,
) {
    match text_input::clipboard_request_for(event, opts) {
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
                text_input::replace_selection_with(value, sel, &text, opts);
            }
        }
        None => {
            text_input::apply_event_with(value, sel, event, opts);
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
    aetna_winit_wgpu::run("Aetna — text_input smoke test", viewport, Form::default())
}
