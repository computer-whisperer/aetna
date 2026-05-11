//! Table — shadcn-shaped table anatomy.
//!
//! The boring path mirrors the common web component shape:
//! `table([table_header([table_row([...])]), table_body([...])])`.
//! Rows carry the theme-facing table metrics; `table_header` promotes
//! direct `table_row` children from body-row metrics to header metrics.

use std::panic::Location;

use super::text::text;
use crate::metrics::MetricsRole;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn table<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("table"))
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
}

#[track_caller]
pub fn table_header<I, E>(rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    let mut header = El::new(Kind::Custom("table_header"))
        .at_loc(Location::caller())
        .children(rows)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch);

    // Promote `table_row(...)` children from body-row metrics to header
    // metrics, and override the body-row default height + radius with
    // the header's recipe (shorter, no rounded corners). Explicit
    // overrides on the row itself still win.
    for row in &mut header.children {
        if row.metrics_role == Some(MetricsRole::TableRow) {
            row.metrics_role = Some(MetricsRole::TableHeader);
            if !row.explicit_height {
                row.height = Size::Fixed(36.0);
            }
            if !row.explicit_radius {
                row.radius = crate::tree::Corners::ZERO;
            }
        }
    }

    header
}

#[track_caller]
pub fn table_body<I, E>(rows: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Custom("table_body"))
        .at_loc(Location::caller())
        .children(rows)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Hug)
        .align(Align::Stretch)
}

#[track_caller]
pub fn table_row<I, E>(cells: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    row(cells)
        .at_loc(Location::caller())
        .metrics_role(MetricsRole::TableRow)
        .width(Size::Fill(1.0))
        .align(Align::Center)
        .default_height(Size::Fixed(52.0))
        .default_padding(Sides::xy(tokens::SPACE_3, 0.0))
        .default_gap(tokens::SPACE_3)
        .default_radius(tokens::RADIUS_MD)
}

#[track_caller]
pub fn table_head(label: impl Into<String>) -> El {
    table_head_el(text(label))
}

#[track_caller]
pub fn table_head_el(content: impl Into<El>) -> El {
    let mut el = content
        .into()
        .at_loc(Location::caller())
        .ellipsis()
        .width(Size::Fill(1.0));
    apply_head_style(&mut el);
    el
}

#[track_caller]
pub fn table_cell(content: impl Into<El>) -> El {
    content
        .into()
        .at_loc(Location::caller())
        .ellipsis()
        .width(Size::Fill(1.0))
}

fn apply_head_style(el: &mut El) {
    if el.kind == Kind::Text {
        el.text_role = TextRole::Caption;
        if el.font_weight == FontWeight::Regular {
            el.font_weight = FontWeight::Medium;
        }
        el.text_color = Some(tokens::MUTED_FOREGROUND);
    }
    for child in &mut el.children {
        apply_head_style(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_header_promotes_direct_table_rows() {
        let header = table_header([table_row([table_head("Name")])]);

        assert_eq!(header.children.len(), 1);
        assert_eq!(
            header.children[0].metrics_role,
            Some(MetricsRole::TableHeader)
        );
        assert_eq!(header.children[0].align, Align::Center);
    }

    #[test]
    fn table_head_el_styles_rich_text_children() {
        let head = table_head_el(text_runs([text("Rich "), text("head").bold()]));

        assert_eq!(head.kind, Kind::Inlines);
        assert_eq!(head.children[0].text_role, TextRole::Caption);
        assert_eq!(head.children[0].font_weight, FontWeight::Medium);
        assert_eq!(head.children[1].text_role, TextRole::Caption);
        assert_eq!(head.children[1].font_weight, FontWeight::Bold);
        assert_eq!(head.children[1].text.as_deref(), Some("head"));
    }
}
