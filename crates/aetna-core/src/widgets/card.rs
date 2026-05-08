//! Card — shadcn-shaped content container anatomy.
//!
//! The boring path mirrors the common web component shape:
//! `card([card_header([...]), card_content([...]), card_footer([...])])`.
//! `titled_card(title, body)` is a convenience wrapper for older/simple
//! examples, built from the same anatomy rather than a separate layout.
//!
//! `card()` is also the canonical "panel surface" — the bundle of
//! [`SurfaceRole::Panel`] + `tokens::CARD` fill + `tokens::BORDER`
//! stroke + radius + shadow that an LLM might otherwise hand-roll. If
//! the helpers (`card_header`, `card_content`, `card_footer`) don't fit
//! your data shape, **wrap your custom composition in `card([...])`
//! instead of replacing it** — that keeps the surface recipe correct
//! everywhere. Same applies to right-rail inspector panes and other
//! "boxed" wrappers that aren't navigation (use `sidebar()` for that).
//!
//! `card_header` / `card_content` / `card_footer` do not supply default
//! padding (unlike `sidebar()`, which bundles `default_padding(SPACE_4)`).
//! Apply `.padding(SPACE_4)` on `card_header` and `card_footer`, and
//! `Sides { left: SPACE_4, right: SPACE_4, top: 0.0, bottom: SPACE_4 }`
//! on `card_content` (or `0.0` when the slot's only child is a
//! `scroll(...)` that should reach the card edges). A header bar with
//! a tinted strip — common for inspector panes and diff/hunk frames —
//! is `card_header([...]).fill(tokens::MUTED).padding(...)`; do not
//! hand-roll the strip as a `row(...).fill(MUTED).stroke(BORDER)`
//! sibling of the body.

use std::panic::Location;

use super::text::{h3, text};
use crate::metrics::MetricsRole;
use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn card<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Card)
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .metrics_role(MetricsRole::Card)
        .surface_role(SurfaceRole::Panel)
        .children(children)
        .fill(tokens::CARD)
        .stroke(tokens::BORDER)
        .default_radius(tokens::RADIUS_MD)
        .shadow(tokens::SHADOW_MD)
        .width(Size::Fill(1.0))
        .default_height(Size::Hug)
        .axis(Axis::Column)
        .align(Align::Stretch)
}

#[track_caller]
pub fn titled_card<I, E>(title: impl Into<String>, body: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    card([
        card_header([card_title(title)]),
        card_content(body.into_iter().map(Into::into).collect::<Vec<_>>()),
    ])
    .at_loc(Location::caller())
}

#[track_caller]
pub fn card_header<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::CardHeader)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

#[track_caller]
pub fn card_title(title: impl Into<String>) -> El {
    h3(title)
        .at_loc(Location::caller())
        .line_height(tokens::TEXT_BASE.size)
}

#[track_caller]
pub fn card_description(description: impl Into<String>) -> El {
    text(description)
        .at_loc(Location::caller())
        .muted()
        .wrap_text()
        .width(Size::Fill(1.0))
}

#[track_caller]
pub fn card_content<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    column(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::CardContent)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
}

#[track_caller]
pub fn card_footer<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(children)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::CardFooter)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Center)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_title_uses_tight_heading_rhythm() {
        let title = card_title("Latency");

        assert_eq!(title.text_role, TextRole::Title);
        assert_eq!(title.font_size, tokens::TEXT_BASE.size);
        assert_eq!(title.line_height, tokens::TEXT_BASE.size);
        assert_eq!(title.font_weight, FontWeight::Semibold);
    }
}
