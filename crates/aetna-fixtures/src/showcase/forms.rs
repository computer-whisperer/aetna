//! Forms — composition pattern.
//!
//! Demonstrates the `form` / `form_section` / `form_item` /
//! `form_label` / `form_control` / `form_description` / `form_message`
//! anatomy. Each anatomy is what you reach for when a single label needs
//! to live above its control with helper text below and an
//! validation/error message slot ready.

use aetna_core::prelude::*;

pub struct State {
    pub display_name: String,
    pub email: String,
    pub bio: String,
    pub selection: Selection,
    pub show_errors: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            display_name: "Christian".into(),
            email: "user@".into(),
            bio: "Building Aetna — a renderer-agnostic UI kit for Rust apps and AI agents.".into(),
            selection: Selection::default(),
            show_errors: true,
        }
    }
}

pub fn view(state: &State) -> El {
    let email_error = state
        .show_errors
        .then(|| email_validation_message(&state.email))
        .flatten();

    scroll([form([
        h1("Forms"),
        paragraph(
            "Composition pattern: each `form_item` pairs a `form_label`, \
             `form_control`, optional `form_description`, and an \
             error/validation `form_message` slot. The widget wraps a \
             plain text/select/etc. control — apps remain in charge of \
             value capture.",
        )
        .muted(),
        section_with_heading(
            "Profile",
            "Public-facing identity",
            form_section([
                form_item([
                    form_label("Display name"),
                    form_control(text_input(
                        &state.display_name,
                        &state.selection,
                        "forms-display-name",
                    )),
                    form_description("Shown next to comments and pull requests."),
                ]),
                form_item({
                    let mut parts = vec![
                        form_label("Email"),
                        form_control(text_input(&state.email, &state.selection, "forms-email")),
                        form_description("We'll send password resets here."),
                    ];
                    if let Some(msg) = email_error.as_deref() {
                        parts.push(form_message(msg));
                    }
                    parts
                }),
            ]),
        ),
        section_with_heading(
            "About",
            "Free-form bio shown on your profile",
            form_section([form_item([
                form_label("Bio"),
                form_control(
                    text_area(&state.bio, &state.selection, "forms-bio").height(Size::Fixed(96.0)),
                ),
                form_description("Markdown-style bold and italic render in your profile card."),
            ])]),
        ),
        row([
            spacer(),
            button("Cancel").ghost().key("forms-cancel"),
            button("Save").primary().key("forms-save"),
        ])
        .gap(tokens::SPACE_2),
    ])
    .gap(tokens::SPACE_4)
    .padding(Sides {
        left: tokens::RING_WIDTH,
        right: tokens::SCROLLBAR_HITBOX_WIDTH,
        top: 0.0,
        bottom: 0.0,
    })])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    match e.target_key() {
        Some("forms-display-name") => {
            text_input::apply_event(
                &mut state.display_name,
                &mut state.selection,
                "forms-display-name",
                &e,
            );
        }
        Some("forms-email") => {
            text_input::apply_event(&mut state.email, &mut state.selection, "forms-email", &e);
        }
        Some("forms-bio") => {
            text_area::apply_event(&mut state.bio, &mut state.selection, "forms-bio", &e);
        }
        _ => {}
    }
}

fn section_with_heading(title: &str, subtitle: &str, body: El) -> El {
    column([
        h3(title.to_string()),
        text(subtitle.to_string()).muted().small(),
        body,
    ])
    .gap(tokens::SPACE_2)
    .width(Size::Fill(1.0))
}

/// Toy email validation. Returns `Some(message)` when the value is
/// not yet acceptable; `None` when it parses cleanly.
fn email_validation_message(s: &str) -> Option<String> {
    if s.is_empty() {
        return Some("Email is required.".into());
    }
    if !s.contains('@') {
        return Some("Email must contain `@`.".into());
    }
    let parts: Vec<&str> = s.splitn(2, '@').collect();
    if parts[1].is_empty() {
        return Some("Domain is required after `@`.".into());
    }
    if !parts[1].contains('.') {
        return Some("Domain looks incomplete.".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_validation_flags_missing_at() {
        assert!(email_validation_message("hello").is_some());
        assert!(email_validation_message("hello@example.com").is_none());
        assert!(email_validation_message("").is_some());
        assert!(email_validation_message("hello@").is_some());
        assert!(email_validation_message("hello@nodot").is_some());
    }
}
