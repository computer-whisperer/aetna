//! Visual, cursor, and paint-transform modifiers for [`El`].

use crate::anim::Timing;
use crate::shader::ShaderBinding;
use crate::style::StyleProfile;

use super::color::Color;
use super::geometry::Sides;
use super::node::El;
use super::semantics::SurfaceRole;

impl El {
    // ---- Visual ----
    pub fn fill(mut self, c: Color) -> Self {
        self.fill = Some(c);
        self
    }

    /// Fill applied when the nearest focusable ancestor isn't focused;
    /// the painter lerps from `dim_fill` toward `fill` as the focus
    /// envelope rises from 0 to 1. See [`Self::dim_fill`] field doc.
    pub fn dim_fill(mut self, c: Color) -> Self {
        self.dim_fill = Some(c);
        self
    }

    pub fn stroke(mut self, c: Color) -> Self {
        self.stroke = Some(c);
        if self.stroke_width == 0.0 {
            self.stroke_width = 1.0;
        }
        self
    }

    pub fn stroke_width(mut self, w: f32) -> Self {
        self.stroke_width = w;
        self
    }

    pub fn radius(mut self, r: f32) -> Self {
        self.radius = r;
        self.explicit_radius = true;
        self
    }

    pub fn shadow(mut self, s: f32) -> Self {
        self.shadow = s;
        self
    }

    pub fn surface_role(mut self, role: SurfaceRole) -> Self {
        self.surface_role = role;
        self
    }

    /// Permit paint to extend beyond this element's layout bounds by
    /// `outset` on each side. Layout-neutral; siblings don't move and
    /// hit-testing still uses the layout rect.
    pub fn paint_overflow(mut self, outset: impl Into<Sides>) -> Self {
        self.paint_overflow = outset.into();
        self
    }

    /// Attach a hover tooltip to this element. The runtime synthesizes
    /// a floating tooltip layer when the pointer rests on the node for
    /// the configured delay.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// Declare the pointer cursor when the pointer is over this
    /// element.
    pub fn cursor(mut self, cursor: crate::cursor::Cursor) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Declare the cursor shown only while a press is captured at this
    /// exact node.
    pub fn cursor_pressed(mut self, cursor: crate::cursor::Cursor) -> Self {
        self.cursor_pressed = Some(cursor);
        self
    }

    // ---- Paint-time transforms (animatable via `.animate()`) ----
    /// Multiply this element's paint alpha by `v` (clamped to `[0, 1]`).
    pub fn opacity(mut self, v: f32) -> Self {
        self.opacity = v.clamp(0.0, 1.0);
        self
    }

    /// Offset this element's paint and its descendants by `(x, y)` in
    /// logical pixels.
    pub fn translate(mut self, x: f32, y: f32) -> Self {
        self.translate = (x, y);
        self
    }

    /// Uniformly scale this element's paint around its rect centre.
    pub fn scale(mut self, v: f32) -> Self {
        self.scale = v.max(0.0);
        self
    }

    /// Opt this element into app-driven prop interpolation.
    pub fn animate(mut self, timing: Timing) -> Self {
        self.animate = Some(timing);
        self
    }

    /// Bind a shader for the surface paint, replacing the implicit
    /// `stock::rounded_rect`.
    pub fn shader(mut self, binding: ShaderBinding) -> Self {
        self.shader_override = Some(binding);
        self
    }

    // ---- Internal: style profile ----
    pub fn style_profile(mut self, p: StyleProfile) -> Self {
        self.style_profile = p;
        self
    }

    pub(crate) fn default_radius(mut self, r: f32) -> Self {
        self.radius = r;
        self.explicit_radius = false;
        self
    }
}
