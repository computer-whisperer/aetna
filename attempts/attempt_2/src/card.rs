//! Card component — the basic container for grouped content.
//!
//! `card(title, body)` is the canonical form. The first argument is a
//! short heading rendered in `h3` style; subsequent children are stacked
//! in a column inside the card body with comfortable padding and gap.
//!
//! Cards default to filling the parent's width and hugging their height.

use crate::text::h3;
use crate::theme::theme;
use crate::tree::*;

/// A card with a title and a column of body children.
///
/// Pass a string title and an iterable of child elements:
///
/// ```
/// use attempt_2::*;
/// let _ = card("Account", [text("user@example.com"), badge("Verified").success()]);
/// ```
pub fn card<I, E>(title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let t = theme();
    let mut children: Vec<El> = vec![h3(title)];
    children.extend(body.into_iter().map(Into::into));

    El::new(Kind::Card)
        .children(children)
        .fill(t.bg.card)
        .stroke(t.border.default)
        .radius(t.radius.lg)
        .shadow(t.shadow.md)
        .padding(Sides::all(t.space.lg))
        .gap(t.space.md)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        // Apply column layout via the same internal helper the primitives use.
        .into_column()
}

impl El {
    /// Internal helper: turn this El into a column-layout container.
    fn into_column(mut self) -> Self {
        self.axis = Axis::Column;
        self.align = Align::Stretch;
        self
    }
}
