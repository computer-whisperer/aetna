//! Free constructors for common [`El`] tree shapes.
//!
//! Kept separate from the core `El` type so the central node definition
//! stays focused on fields and chainable modifiers.

use std::panic::Location;

use crate::image::Image;
use crate::layout::VirtualItems;

use super::layout_types::{Align, Axis, Size};
use super::node::El;
use super::semantics::Kind;

/// A vertical container — the layout fallback.
///
/// Reach for a named widget first: [`card`] / [`titled_card`] for boxed
/// surfaces; [`sidebar`] for nav rails; [`toolbar`] for page headers;
/// [`item`] for object rows; [`form_item`] / [`field_row`] for stacked
/// fields. `column` is the right answer when no widget shape fits.
///
/// Defaults match CSS flex's `display: flex; flex-direction: column`:
/// `axis = Column`, `align = Stretch`, `width = Hug`, `height = Hug`,
/// `gap = 0`. Children shrink to content on the main axis (height)
/// and stretch to the column's width on the cross axis.
///
/// To claim the parent's extent (the analog of `width: 100%` /
/// `flex: 1`), set `.width(Size::Fill(1.0))` /
/// `.height(Size::Fill(1.0))`. To space children apart, set
/// `.gap(tokens::SPACE_*)` — CSS-style opt-in spacing.
///
/// Switch `align` to `Center` / `Start` / `End` and children shrink
/// to their content width so the alignment can position them — the
/// same as CSS `align-items` non-stretch semantics.
///
/// **Smell:** `column([...]).fill(CARD).stroke(BORDER).radius(...)`
/// reinvents [`card`]; `column([...]).fill(CARD).stroke(BORDER).width(SIDEBAR_WIDTH)`
/// reinvents [`sidebar`]. Use the named widget — same recipe, the right
/// surface role, less to forget.
///
/// [`card`]: crate::widgets::card::card
/// [`titled_card`]: crate::widgets::card::titled_card
/// [`sidebar`]: crate::widgets::sidebar::sidebar
/// [`toolbar`]: crate::widgets::toolbar::toolbar
/// [`item`]: crate::widgets::item::item
/// [`form_item`]: crate::widgets::form::form_item
/// [`field_row`]: crate::widgets::form::field_row
#[track_caller]
pub fn column<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
}

/// A horizontal container — the layout fallback.
///
/// Reach for a named widget first: [`item`] for clickable object rows
/// (recent file, repo, project, person, asset entry — anywhere you'd
/// otherwise build a focusable row with stacked text and trailing
/// buttons); [`toolbar`] for page chrome; [`field_row`] for label +
/// control; [`tabs_list`] for segmented controls; [`breadcrumb_list`] /
/// [`pagination_content`] for navigation rows. `row` is the right
/// answer when no widget shape fits.
///
/// Defaults match CSS flex's `display: flex; flex-direction: row`:
/// `axis = Row`, `align = Stretch`, `width = Hug`, `height = Hug`,
/// `gap = 0`. Children shrink to content on the main axis (width)
/// and stretch to the row's height on the cross axis.
///
/// `Stretch` is the cross-axis default the same way `align-items:
/// stretch` is in CSS. For typical content rows (`[icon, text,
/// button]`) you almost always want `.align(Center)` to vertically
/// center the children — the CSS-Tailwind muscle memory of
/// `flex items-center`. Without it, smaller fixed-size children
/// (badges, icons) sit at the top of the row, just like CSS does.
///
/// To space children apart, set `.gap(tokens::SPACE_*)` — opt-in
/// like CSS.
///
/// **Smell:** a focusable, keyed `row([column([t1, t2]), button, button])`
/// used as a clickable resource entry — that's [`item`], not a hand-rolled
/// row. The named widget gives you hover, press, focus, the rail, and
/// the slots (`item_media`, `item_content`, `item_actions`) for free.
///
/// [`item`]: crate::widgets::item::item
/// [`toolbar`]: crate::widgets::toolbar::toolbar
/// [`field_row`]: crate::widgets::form::field_row
/// [`tabs_list`]: crate::widgets::tabs::tabs_list
/// [`breadcrumb_list`]: crate::widgets::breadcrumb::breadcrumb_list
/// [`pagination_content`]: crate::widgets::pagination::pagination_content
#[track_caller]
pub fn row<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Row)
}

/// An overlay stack; children share the parent's rect.
///
/// For modals, sheets, popovers, and tooltips reach for the named
/// widget instead — [`dialog`], [`sheet`], [`popover`], `.tooltip(...)`.
/// `stack` is the layered-visuals primitive (focus rings, custom
/// badges painted over content) that those widgets compose against.
///
/// [`dialog`]: crate::widgets::dialog::dialog
/// [`sheet`]: crate::widgets::sheet::sheet
/// [`popover`]: crate::widgets::popover::popover
#[track_caller]
pub fn stack<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Group)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Overlay)
}

/// A vertical scroll viewport. Children stack as in [`column()`]; the
/// container clips overflow and translates content by the current scroll
/// offset. Wheel events over the viewport update the offset.
///
/// Give it a `.key("...")` so the offset persists by name across
/// rebuilds — without a key, the offset is keyed by sibling index and
/// resets if structure shifts.
#[track_caller]
pub fn scroll<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Scroll)
        .at_loc(Location::caller())
        .children(children)
        .axis(Axis::Column)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .clip()
        .scrollable()
        .scrollbar()
}

/// Block whose direct children flow inline (text leaves + embeds +
/// hard breaks). Models HTML's `<p>` shape: heterogeneous children,
/// attributed runs, optional inline embeds. Children are styled via
/// the existing modifier chain (`.bold()`, `.italic()`, `.color(c)`,
/// `.code()`, `.link(url)`, etc.) — there is no parallel
/// `RichText`/`TextRun` type.
///
/// ```ignore
/// text_runs([
///     text("Aetna — "),
///     text("rich text").bold(),
///     text(" composition."),
///     hard_break(),
///     text("Custom shaders, custom layouts, "),
///     text("virtual_list").code(),
///     text(" — and inline runs."),
/// ])
/// ```
#[track_caller]
pub fn text_runs<I, E>(children: I) -> El
where
    I: IntoIterator<Item = E>,
    E: Into<El>,
{
    El::new(Kind::Inlines)
        .at_loc(Location::caller())
        .axis(Axis::Column)
        .align(Align::Start)
        .width(Size::Fill(1.0))
        .children(children)
}

/// Forced line break inside a [`text_runs`] block. Mirrors HTML's
/// `<br>`. Outside an `Inlines` parent, lays out as a zero-size leaf.
#[track_caller]
pub fn hard_break() -> El {
    El::new(Kind::HardBreak)
        .at_loc(Location::caller())
        .width(Size::Hug)
        .height(Size::Hug)
}

/// Virtualized vertical list of `count` rows of fixed height
/// `row_height`. The library calls `build_row(i)` only for indices
/// whose rect intersects the visible viewport, then lays them out at
/// the scroll-shifted Y. Authors typically key rows with a stable
/// identifier (`button("foo").key("msg-abc")`) so hover/press/focus
/// state survives scrolling.
///
/// The returned El defaults to `Size::Fill(1.0)` on both axes (it's a
/// viewport — its size is decided by the parent). `Size::Hug` would
/// defeat virtualization and panics at layout time.
#[track_caller]
pub fn virtual_list<F>(count: usize, row_height: f32, build_row: F) -> El
where
    F: Fn(usize) -> El + Send + Sync + 'static,
{
    let mut el = El::new(Kind::VirtualList)
        .at_loc(Location::caller())
        .axis(Axis::Column)
        .align(Align::Stretch)
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .clip()
        .scrollable()
        .scrollbar();
    el.virtual_items = Some(VirtualItems::new(count, row_height, build_row));
    el
}

/// A `Fill(1)` filler. Inside a `row` it pushes siblings to the right;
/// inside a `column` it pushes siblings to the bottom.
#[track_caller]
pub fn spacer() -> El {
    El::new(Kind::Spacer)
        .at_loc(Location::caller())
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
}

/// A raster image element. The El hugs the image's natural pixel
/// size by default; set [`El::width`] / [`El::height`] for an
/// explicit box, and [`El::image_fit`] to control projection.
///
/// ```
/// use aetna_core::prelude::*;
/// let pixels = vec![0u8; 4 * 4 * 4];
/// let img = Image::from_rgba8(4, 4, pixels);
/// let _ = image(img).image_fit(ImageFit::Cover).radius(8.0);
/// ```
#[track_caller]
pub fn image(img: impl Into<Image>) -> El {
    El::new(Kind::Image).at_loc(Location::caller()).image(img)
}

/// A 1-pixel separator line.
#[track_caller]
pub fn divider() -> El {
    El::new(Kind::Divider)
        .at_loc(Location::caller())
        .height(Size::Fixed(1.0))
        .width(Size::Fill(1.0))
        .fill(crate::tokens::BORDER)
}

// ---------- &str → El convenience ----------
//
// Lets `titled_card("Title", ["a body line"])` work without `text(...)`.

impl From<&str> for El {
    fn from(s: &str) -> Self {
        crate::widgets::text::text(s)
    }
}

impl From<String> for El {
    fn from(s: String) -> Self {
        crate::widgets::text::text(s)
    }
}

impl From<&String> for El {
    fn from(s: &String) -> Self {
        crate::widgets::text::text(s.as_str())
    }
}
