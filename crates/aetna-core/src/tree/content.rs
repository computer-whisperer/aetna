//! Content-related [`El`] modifiers: text runs, icon source, and raster image source.

use crate::image::{Image, ImageFit};

use super::color::Color;
use super::layout_types::Size;
use super::node::El;
use super::text_types::{FontWeight, TextAlign, TextOverflow, TextRole, TextWrap};

impl El {
    // ---- Text-bearing ----
    pub fn text(mut self, t: impl Into<String>) -> Self {
        self.text = Some(t.into());
        self
    }

    pub fn text_color(mut self, c: Color) -> Self {
        self.text_color = Some(c);
        self
    }

    pub fn text_align(mut self, align: TextAlign) -> Self {
        self.text_align = align;
        self
    }

    pub fn center_text(self) -> Self {
        self.text_align(TextAlign::Center)
    }

    pub fn end_text(self) -> Self {
        self.text_align(TextAlign::End)
    }

    pub fn text_wrap(mut self, wrap: TextWrap) -> Self {
        self.text_wrap = wrap;
        self
    }

    pub fn wrap_text(self) -> Self {
        self.text_wrap(TextWrap::Wrap)
    }

    pub fn nowrap_text(self) -> Self {
        self.text_wrap(TextWrap::NoWrap)
    }

    pub fn text_overflow(mut self, overflow: TextOverflow) -> Self {
        self.text_overflow = overflow;
        self
    }

    pub fn ellipsis(self) -> Self {
        self.text_overflow(TextOverflow::Ellipsis)
    }

    pub fn max_lines(mut self, lines: usize) -> Self {
        self.text_max_lines = Some(lines.max(1));
        self
    }

    pub fn font_size(mut self, s: f32) -> Self {
        self.font_size = s;
        self.line_height = crate::tokens::line_height_for_size(s);
        self
    }

    pub fn line_height(mut self, h: f32) -> Self {
        self.line_height = h.max(1.0);
        self
    }

    pub fn font_weight(mut self, w: FontWeight) -> Self {
        self.font_weight = w;
        self
    }

    /// Set the icon for this element to either a built-in [`crate::IconName`],
    /// an app-supplied [`crate::SvgIcon`], or a string-typed name from
    /// the built-in vocabulary.
    pub fn icon_source(mut self, source: impl crate::svg_icon::IntoIconSource) -> Self {
        self.icon = Some(source.into_icon_source());
        self
    }

    /// Convenience alias for [`Self::icon_source`] preserved for call
    /// sites that want the historical name.
    pub fn icon_name(self, source: impl crate::svg_icon::IntoIconSource) -> Self {
        self.icon_source(source)
    }

    pub fn icon_stroke_width(mut self, width: f32) -> Self {
        self.icon_stroke_width = width.max(0.25);
        self
    }

    pub fn icon_size(mut self, size: f32) -> Self {
        let size = size.max(1.0);
        self.font_size = size;
        self.line_height = size;
        self.width = Size::Fixed(size);
        self.height = Size::Fixed(size);
        self.explicit_width = true;
        self.explicit_height = true;
        self
    }

    /// Attach a raster image. Usually you'll want the [`crate::image`]
    /// free builder instead, which sets [`crate::Kind::Image`] for you; this
    /// method exists for cases where you've already constructed an El
    /// (e.g. through a stock widget) and want to swap in pixel art.
    pub fn image(mut self, image: impl Into<Image>) -> Self {
        self.image = Some(image.into());
        self
    }

    pub fn image_fit(mut self, fit: ImageFit) -> Self {
        self.image_fit = fit;
        self
    }

    pub fn image_tint(mut self, c: Color) -> Self {
        self.image_tint = Some(c);
        self
    }

    pub fn mono(mut self) -> Self {
        self.font_mono = true;
        self
    }

    /// Italic styling for a text run. Honoured by the
    /// [`crate::Kind::Inlines`] layout pass and (best-effort) on
    /// standalone text Els.
    pub fn italic(mut self) -> Self {
        self.text_italic = true;
        self
    }

    /// Inline-run background. Honoured when this El is a styled text
    /// leaf inside an [`crate::Kind::Inlines`] parent: the shaped span
    /// paints a solid quad behind its glyphs (per-line if the span
    /// wraps). Mirrors HTML's `<mark>` / inline `background`; the rect
    /// tracks the glyph extent rather than the El's layout box, so a
    /// wrapped highlight follows the prose. No effect on standalone
    /// text Els.
    pub fn background(mut self, color: Color) -> Self {
        self.text_bg = Some(color);
        self
    }

    /// Underline styling for a text run.
    pub fn underline(mut self) -> Self {
        self.text_underline = true;
        self
    }

    /// Strikethrough styling for a text run.
    pub fn strikethrough(mut self) -> Self {
        self.text_strikethrough = true;
        self
    }

    /// Markdown-flavoured inline-code styling. Currently `mono`-styled;
    /// a tinted background per the theme is a future addition. Authors
    /// who want raw mono without code chrome should use [`Self::mono`]
    /// instead.
    pub fn code(self) -> Self {
        self.text_role(TextRole::Code)
    }

    /// Mark this run as a link to `url`. Inside an
    /// [`crate::Kind::Inlines`] parent the run paints with a
    /// link-themed color; runs sharing the same URL group together for
    /// hit-test.
    pub fn link(mut self, url: impl Into<String>) -> Self {
        self.text_link = Some(url.into());
        self
    }
}
