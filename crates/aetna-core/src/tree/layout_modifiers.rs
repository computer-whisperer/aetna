//! Layout and child-list modifiers for [`El`].

use crate::layout::{LayoutCtx, LayoutFn};

use super::geometry::{Rect, Sides};
use super::layout_types::{Align, Axis, Justify, Size};
use super::node::El;

impl El {
    // ---- Sizing ----
    pub fn width(mut self, w: Size) -> Self {
        self.width = w;
        self
    }

    pub fn height(mut self, h: Size) -> Self {
        self.height = h;
        self
    }

    pub fn hug(mut self) -> Self {
        self.width = Size::Hug;
        self.height = Size::Hug;
        self
    }

    pub fn fill_size(mut self) -> Self {
        self.width = Size::Fill(1.0);
        self.height = Size::Fill(1.0);
        self
    }

    // ---- Layout (container) ----
    pub fn padding(mut self, p: impl Into<Sides>) -> Self {
        self.padding = p.into();
        self
    }

    pub fn gap(mut self, g: f32) -> Self {
        self.gap = g;
        self
    }

    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }

    pub fn justify(mut self, j: Justify) -> Self {
        self.justify = j;
        self
    }

    pub fn clip(mut self) -> Self {
        self.clip = true;
        self
    }

    pub fn scrollable(mut self) -> Self {
        self.scrollable = true;
        self
    }

    /// Show a draggable vertical scrollbar thumb when this scrollable
    /// node's content overflows.
    pub fn scrollbar(mut self) -> Self {
        self.scrollbar = true;
        self
    }

    /// Suppress the default scrollbar thumb on this scrollable node.
    pub fn no_scrollbar(mut self) -> Self {
        self.scrollbar = false;
        self
    }

    /// Treat this element's focusable children as a single
    /// arrow-navigable group.
    pub fn arrow_nav_siblings(mut self) -> Self {
        self.arrow_nav_siblings = true;
        self
    }

    /// Replace the column/row/overlay distribution for this node with
    /// a custom child layout function.
    pub fn layout<F>(mut self, f: F) -> Self
    where
        F: Fn(LayoutCtx) -> Vec<Rect> + Send + Sync + 'static,
    {
        self.layout_override = Some(LayoutFn::new(f));
        self
    }

    // ---- Children ----
    pub fn child(mut self, c: impl Into<El>) -> Self {
        self.children.push(c.into());
        self
    }

    pub fn children<I, E>(mut self, cs: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: Into<El>,
    {
        self.children.extend(cs.into_iter().map(Into::into));
        self
    }

    /// Set the layout axis directly.
    pub fn axis(mut self, a: Axis) -> Self {
        self.axis = a;
        self
    }
}
