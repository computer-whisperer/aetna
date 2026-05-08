//! Card — shadcn-shaped content container anatomy.
//!
//! The boring path mirrors the common web component shape:
//! `card([card_header([...]), card_content([...]), card_footer([...])])`.
//! `titled_card(title, body)` is a convenience wrapper for older/simple
//! examples, built from the same anatomy rather than a separate layout.

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
