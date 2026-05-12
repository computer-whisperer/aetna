//! Backend-neutral clipboard helpers for host integrations.
//!
//! Platform clipboard IO stays in hosts (`aetna-winit-wgpu`, `aetna-web`,
//! custom shells). This module only contains the event/tree glue those
//! hosts need to turn clipboard operations into normal Aetna app events.

use crate::{
    event::{App, BuildCx, KeyModifiers, UiEvent, UiEventKind, UiKey},
    selection,
    state::UiState,
};

/// Resolve the currently selected text for an app from the host's last
/// [`UiState`].
///
/// Hosts call this after the runtime has updated selection state and
/// before performing platform clipboard IO. The helper rebuilds the
/// app tree with the same UI state the renderer uses, then asks the
/// selection system to extract text from the app's current
/// [`crate::event::App::selection`].
pub fn selected_text_for_app<A: App>(app: &A, ui_state: &UiState) -> Option<String> {
    let theme = app.theme();
    let cx = BuildCx::new(&theme).with_ui_state(ui_state);
    let tree = app.build(&cx);
    selection::selected_text(&tree, &app.selection())
}

/// Rewrite a key event into a text-paste event with `text`.
///
/// Use this after the host has obtained text from a platform clipboard
/// backend. The target route and hit metadata stay attached to the
/// original focused key event; keyboard modifiers are cleared so text
/// widgets do not treat the paste as a literal Ctrl/Cmd text input.
pub fn paste_text_event(mut event: UiEvent, text: impl Into<String>) -> UiEvent {
    event.kind = UiEventKind::TextInput;
    event.key_press = None;
    event.text = Some(text.into());
    event.modifiers = KeyModifiers::default();
    event.click_count = 0;
    event
}

/// Rewrite a key event into a forward-delete event.
///
/// Hosts use this for `Cut`: copy the selected text through the
/// platform backend, then dispatch this event so the focused text
/// widget deletes the selection using the same path as a normal Delete
/// keypress.
pub fn delete_selection_event(mut event: UiEvent) -> UiEvent {
    event.modifiers = KeyModifiers::default();
    if let Some(key_press) = event.key_press.as_mut() {
        key_press.key = UiKey::Delete;
        key_press.modifiers = KeyModifiers::default();
        key_press.repeat = false;
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        KeyPress, Selection, SelectionPoint, SelectionRange, widgets::text_input::text_input,
    };

    fn key_event() -> UiEvent {
        UiEvent {
            key: Some("field".into()),
            target: None,
            pointer: None,
            key_press: Some(KeyPress {
                key: UiKey::Character("v".into()),
                modifiers: KeyModifiers {
                    ctrl: true,
                    ..Default::default()
                },
                repeat: true,
            }),
            text: None,
            selection: None,
            modifiers: KeyModifiers {
                ctrl: true,
                ..Default::default()
            },
            click_count: 0,
            path: None,
            kind: UiEventKind::KeyDown,
        }
    }

    #[test]
    fn paste_text_event_rewrites_key_event_to_plain_text_input() {
        let event = paste_text_event(key_event(), "hello");
        assert_eq!(event.kind, UiEventKind::TextInput);
        assert_eq!(event.text.as_deref(), Some("hello"));
        assert!(event.key_press.is_none());
        assert_eq!(event.modifiers, KeyModifiers::default());
        assert_eq!(event.route(), Some("field"));
    }

    #[test]
    fn delete_selection_event_rewrites_key_to_forward_delete() {
        let event = delete_selection_event(key_event());
        let key_press = event.key_press.expect("key press");
        assert_eq!(key_press.key, UiKey::Delete);
        assert_eq!(key_press.modifiers, KeyModifiers::default());
        assert!(!key_press.repeat);
        assert_eq!(event.modifiers, KeyModifiers::default());
    }

    #[test]
    fn selected_text_for_app_extracts_text_from_rebuilt_tree() {
        struct AppWithSelectableText;
        impl App for AppWithSelectableText {
            fn build(&self, _cx: &BuildCx) -> crate::El {
                text_input("hello clipboard", &self.selection(), "copy-source")
            }

            fn selection(&self) -> Selection {
                Selection {
                    range: Some(SelectionRange {
                        anchor: SelectionPoint::new("copy-source", 6),
                        head: SelectionPoint::new("copy-source", 15),
                    }),
                }
            }
        }

        let ui_state = UiState::new();
        assert_eq!(
            selected_text_for_app(&AppWithSelectableText, &ui_state).as_deref(),
            Some("clipboard")
        );
    }
}
