//! Text input — smoke test for the single-line text widget.
//!
//! Four fields plus a live preview of the global selection state.
//! Run interactively:
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
//!   the system clipboard.
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
    email: String,
    pin: String,
    password: String,
    /// Single global selection field — every input reads / writes its
    /// slice through `selection.within(key)` instead of holding its
    /// own `TextSelection`.
    selection: Selection,
    clipboard: Option<arboard::Clipboard>,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            name: String::new(),
            email: String::new(),
            pin: String::new(),
            password: String::new(),
            selection: Selection::default(),
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

    fn value_for(&self, key: &str) -> Option<&str> {
        match key {
            "name" => Some(&self.name),
            "email" => Some(&self.email),
            "pin" => Some(&self.pin),
            "password" => Some(&self.password),
            _ => None,
        }
    }
}

impl App for Form {
    fn build(&self) -> El {
        column([
            h2("Form"),
            field_row(
                "Name",
                text_input_with(&self.name, &self.selection, "name", self.opts_for("name")),
            ),
            field_row(
                "Email",
                text_input_with(&self.email, &self.selection, "email", self.opts_for("email")),
            ),
            field_row(
                "PIN",
                text_input_with(&self.pin, &self.selection, "pin", self.opts_for("pin")),
            ),
            field_row(
                "Password",
                text_input_with(
                    &self.password,
                    &self.selection,
                    "password",
                    self.opts_for("password"),
                ),
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

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    fn on_event(&mut self, event: UiEvent) {
        // Library-emitted selection updates: the runtime doesn't
        // touch text_input's own selection (text_input handles it
        // inside apply_event), but `SelectionChanged` fires when a
        // press lands somewhere non-selectable / non-focusable to
        // clear the active static-text selection. Honor that.
        if event.kind == UiEventKind::SelectionChanged
            && let Some(sel) = event.selection.as_ref()
        {
            self.selection = sel.clone();
            return;
        }
        match (event.kind, event.route()) {
            (UiEventKind::Click | UiEventKind::Activate, Some("clear")) => {
                self.name.clear();
                self.email.clear();
                self.pin.clear();
                self.password.clear();
                self.selection = Selection::default();
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
        let (value, key_str): (&mut String, &str) = match key.as_str() {
            "name" => (&mut self.name, "name"),
            "email" => (&mut self.email, "email"),
            "pin" => (&mut self.pin, "pin"),
            "password" => (&mut self.password, "password"),
            _ => return,
        };
        apply_with_clipboard(
            value,
            &mut self.selection,
            key_str,
            &event,
            &opts,
            self.clipboard.as_mut(),
        );
    }
}

fn apply_with_clipboard(
    value: &mut String,
    selection: &mut Selection,
    key: &str,
    event: &UiEvent,
    opts: &TextInputOpts<'_>,
    clipboard: Option<&mut arboard::Clipboard>,
) {
    match text_input::clipboard_request_for(event, opts) {
        Some(ClipboardKind::Copy) => {
            if let (Some(cb), Some(view)) = (clipboard, selection.within(key)) {
                let _ = cb.set_text(text_input::selected_text(value, view).to_string());
            }
        }
        Some(ClipboardKind::Cut) => {
            if let Some(view) = selection.within(key) {
                if let Some(cb) = clipboard {
                    let _ = cb.set_text(text_input::selected_text(value, view).to_string());
                }
                let mut local = view;
                text_input::replace_selection(value, &mut local, "");
                selection.set_within(key, local);
            }
        }
        Some(ClipboardKind::Paste) => {
            if let Some(cb) = clipboard
                && let Ok(text) = cb.get_text()
            {
                let mut local = selection.within(key).unwrap_or_default();
                text_input::replace_selection_with(value, &mut local, &text, opts);
                selection.set_within(key, local);
                // If selection wasn't in our key, claim it now.
                if selection.within(key).is_none() {
                    selection.range = Some(SelectionRange {
                        anchor: SelectionPoint::new(key, local.head),
                        head: SelectionPoint::new(key, local.head),
                    });
                }
            }
        }
        None => {
            text_input::apply_event_with(value, selection, key, event, opts);
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
            preview_line(form, "name"),
            preview_line(form, "email"),
            preview_line(form, "pin"),
            mono(format!("global: {:?}", form.selection)).font_size(tokens::FONT_SM),
        ],
    )
}

fn preview_line(form: &Form, key: &str) -> El {
    let value = form.value_for(key).unwrap_or("");
    let summary = match form.selection.within(key) {
        Some(view) if view.is_collapsed() => {
            format!("{key:>8} = {:?}  caret={}", value, view.head)
        }
        Some(view) => {
            let (lo, hi) = view.ordered();
            format!(
                "{key:>8} = {:?}  selection={}..{}  ({:?})",
                value,
                lo,
                hi,
                &value[lo..hi]
            )
        }
        None => format!("{key:>8} = {:?}  (not selected)", value),
    };
    mono(summary).font_size(tokens::FONT_SM)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 640.0, 420.0);
    aetna_winit_wgpu::run("Aetna — text_input smoke test", viewport, Form::default())
}
