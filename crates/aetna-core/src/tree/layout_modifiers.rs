//! Layout and child-list modifiers for [`El`].

use crate::layout::{LayoutCtx, LayoutFn};
use crate::metrics::{ComponentSize, Density, MetricsRole};

use super::geometry::{Rect, Sides};
use super::layout_types::{Align, Axis, Justify, Size};
use super::node::El;

impl El {
    // ---- Sizing ----
    pub fn width(mut self, w: Size) -> Self {
        self.width = w;
        self.explicit_width = true;
        self
    }

    pub fn height(mut self, h: Size) -> Self {
        self.height = h;
        self.explicit_height = true;
        self
    }

    pub fn hug(mut self) -> Self {
        self.width = Size::Hug;
        self.height = Size::Hug;
        self.explicit_width = true;
        self.explicit_height = true;
        self
    }

    pub fn fill_size(mut self) -> Self {
        self.width = Size::Fill(1.0);
        self.height = Size::Fill(1.0);
        self.explicit_width = true;
        self.explicit_height = true;
        self
    }

    /// Set the t-shirt size for stock controls.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.component_size = Some(size);
        self
    }

    pub fn medium(self) -> Self {
        self.size(ComponentSize::Md)
    }

    pub fn large(self) -> Self {
        self.size(ComponentSize::Lg)
    }

    /// Set content density for repeated/grouped stock surfaces.
    pub fn density(mut self, density: Density) -> Self {
        self.density = Some(density);
        self
    }

    /// Set the theme-facing stock metrics role for this widget.
    pub fn metrics_role(mut self, role: MetricsRole) -> Self {
        self.metrics_role = Some(role);
        self
    }

    pub fn compact(self) -> Self {
        self.density(Density::Compact)
    }

    /// Alias for [`Self::compact`], matching MUI/List terminology.
    pub fn dense(self) -> Self {
        self.compact()
    }

    pub fn comfortable(self) -> Self {
        self.density(Density::Comfortable)
    }

    pub fn spacious(self) -> Self {
        self.density(Density::Spacious)
    }

    // ---- Layout (container) ----
    pub fn padding(mut self, p: impl Into<Sides>) -> Self {
        self.padding = p.into();
        self.explicit_padding = true;
        self
    }

    pub fn gap(mut self, g: f32) -> Self {
        self.gap = g;
        self.explicit_gap = true;
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

    // ---- Internal stock defaults ----
    pub(crate) fn default_width(mut self, w: Size) -> Self {
        self.width = w;
        self.explicit_width = false;
        self
    }

    pub(crate) fn default_height(mut self, h: Size) -> Self {
        self.height = h;
        self.explicit_height = false;
        self
    }

    pub(crate) fn default_padding(mut self, p: impl Into<Sides>) -> Self {
        self.padding = p.into();
        self.explicit_padding = false;
        self
    }

    pub(crate) fn default_gap(mut self, g: f32) -> Self {
        self.gap = g;
        self.explicit_gap = false;
        self
    }
}
