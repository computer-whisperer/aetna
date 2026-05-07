//! Form-row primitives for label + control layouts.
//!
//! Settings dialogs, preference panes, and audio-config modals are
//! mostly the same row repeated: a label on the left, a control on
//! the right, vertical-center aligned, full panel width. [`field_row`]
//! is that pattern as one call.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! struct Prefs { auto_save: bool, volume: f32 }
//!
//! impl App for Prefs {
//!     fn build(&self, _cx: &BuildCx) -> El {
//!         titled_card("Audio", [
//!             field_row("Auto-save", switch(self.auto_save).key("auto_save")),
//!             field_row(
//!                 format!("Volume ({:.0}%)", self.volume * 100.0),
//!                 slider(self.volume, tokens::PRIMARY).key("volume"),
//!             ),
//!         ])
//!     }
//! }
//! ```
//!
//! # Dogfood note
//!
//! Pure composition over the public widget-kit surface — `row`,
//! `spacer`, [`crate::widgets::text::text`] with the `.label()` role
//! modifier. No internal machinery; an app crate can fork this file
//! and produce an equivalent helper.

use std::panic::Location;

use super::text::text;
use crate::tokens;
use crate::tree::*;

/// A labelled form row: label on the left, control on the right,
/// vertical-center aligned, full panel width.
///
/// The label is styled with the `.label()` text role
/// ([`TextRole::Label`]) so it picks up the same size, weight, and
/// theme color as standalone form labels (next to checkboxes,
/// switches, etc.). The control is any `El` — a switch, a slider,
/// a button, a row of controls, anything that fits on the right.
///
/// For multi-control rows (e.g. a value readout next to a slider),
/// wrap them in a `row([...])` and pass that as `control`.
#[track_caller]
pub fn field_row(label: impl Into<String>, control: impl Into<El>) -> El {
    crate::row([text(label).label(), crate::spacer(), control.into()])
        .at_loc(Location::caller())
        .gap(tokens::SPACE_MD)
        .align(Align::Center)
        .width(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::switch::switch;

    #[test]
    fn field_row_lays_out_label_spacer_control() {
        // The fixed shape — label first, spacer second, control last
        // — is what gives every form row the same visual rhythm.
        // Apps reading routed events through `target_key` rely on the
        // control keeping its own key, so the wrapper must not
        // interpose its own.
        let r = field_row("Auto-save", switch(false).key("auto_save"));
        assert_eq!(r.children.len(), 3);
        assert_eq!(r.axis, Axis::Row);
        assert!(r.key.is_none(), "field_row carries no key of its own");

        let label = &r.children[0];
        assert_eq!(label.text.as_deref(), Some("Auto-save"));
        assert_eq!(label.text_role, TextRole::Label);

        let spacer = &r.children[1];
        assert_eq!(spacer.kind, Kind::Spacer);

        let control = &r.children[2];
        assert_eq!(control.key.as_deref(), Some("auto_save"));
    }

    #[test]
    fn field_row_fills_width_and_centers_vertically() {
        // The row hugs its parent's width so a column of field rows
        // forms a clean stack; centered alignment lets a tall control
        // (a paragraph of helper text inside the control slot, say)
        // sit beside a single-line label without baseline drift.
        let r = field_row("Theme", switch(true).key("theme"));
        assert!(matches!(r.width, Size::Fill(_)));
        assert_eq!(r.align, Align::Center);
    }

    #[test]
    fn field_row_accepts_dynamic_label() {
        // Apps frequently format the label with the current value
        // (e.g. "Volume (52%)"). String types must satisfy
        // `Into<String>` the same way `card` titles do.
        let r = field_row(format!("Volume ({}%)", 52), switch(false).key("k"));
        let label = &r.children[0];
        assert_eq!(label.text.as_deref(), Some("Volume (52%)"));
    }
}
