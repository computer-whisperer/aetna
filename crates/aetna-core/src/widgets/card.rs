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
//! Default padding and gap for the slots are density-driven and
//! supplied by the metrics pass — see [`crate::metrics::card_header_metrics`],
//! [`card_content_metrics`](crate::metrics::card_content_metrics), and
//! [`card_footer_metrics`](crate::metrics::card_footer_metrics). The
//! values mirror shadcn's anatomy at three densities (Compact:
//! `16/16/16/8` header + `16/16/4/16` content; Comfortable: `20/20/20/12` +
//! `20/20/8/20`; Spacious: `24/24/24/16` + `24/24/8/24`), so the visual
//! rhythm comes from `card_header`'s heavier bottom padding rather than
//! from doubling header + content top paddings. Naive
//! `card([card_header([...]), card_content([...])])` produces correct
//! visuals on first try — *do not* add explicit `.padding(...)` to
//! match shadcn's `p-6` literal, since that takes you off the
//! density-aware path. Override only when the design intentionally
//! deviates: pass `.padding(0.0)` when the slot's only child is a
//! `scroll(...)` that should reach the card edges, or pass
//! `Sides { ... }` to set a custom recipe (and accept that explicit
//! padding will not adapt across densities). A header bar with a
//! tinted strip — common for inspector panes and diff/hunk frames —
//! is `card_header([...]).fill(tokens::MUTED)`; do not hand-roll the
//! strip as a `row(...).fill(MUTED).stroke(BORDER)` sibling of the
//! body.

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
