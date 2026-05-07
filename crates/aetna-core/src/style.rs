//! Style modifier methods on [`El`] — kind-aware via [`StyleProfile`].
//!
//! Each component declares its [`StyleProfile`] in its constructor.
//! Style modifiers (`.primary`, `.success`, `.muted`, etc.) dispatch
//! on the profile, not on `Kind`. That means adding a new component
//! is a self-contained file change: declare a profile, the existing
//! modifier vocabulary just works.
//!
//! Profile semantics:
//!
//! - [`StyleProfile::Solid`] — color modifiers produce solid fills
//!   (Button, Toggle thumb, …).
//! - [`StyleProfile::Tinted`] — color modifiers produce tinted alpha
//!   fills with status-colored text (Badge, highlighted Card, …).
//! - [`StyleProfile::Surface`] — color modifiers tint a subtle bg;
//!   `.muted` swaps to a neutral surface (Card, TextField, Select, …).
//! - [`StyleProfile::TextOnly`] — color modifiers only change text color
//!   (Text, Heading, …).
//!
//! Modifier groups in this file:
//!
//! - **Color/status:** `primary`, `success`, `warning`, `destructive`, `info`
//! - **Surface variants:** `secondary`, `ghost`, `outline`, `muted`
//! - **Semantic states:** `selected`, `current`, `disabled`, `invalid`, `loading`
//! - **Typography roles:** `caption`, `label`, `body`, `title`, `heading`, `display`, `code`
//! - **Text shape:** `bold`, `semibold`, `small`, `xsmall`, `color`

use crate::metrics::ComponentSize;
use crate::tokens;
use crate::tree::*;

/// How a component reacts to style/color modifiers.
///
/// Set once in the component's constructor; the modifier methods dispatch
/// on this rather than on [`Kind`], so adding a new component never
/// requires editing this file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StyleProfile {
    Solid,
    Tinted,
    Surface,
    #[default]
    TextOnly,
}

impl El {
    // ===== Color / status (profile-aware) =====

    pub fn primary(self) -> Self {
        tint(self, tokens::PRIMARY)
    }
    pub fn success(self) -> Self {
        tint(self, tokens::SUCCESS)
    }
    pub fn warning(self) -> Self {
        tint(self, tokens::WARNING)
    }
    pub fn destructive(self) -> Self {
        tint(self, tokens::DESTRUCTIVE)
    }
    pub fn info(self) -> Self {
        tint(self, tokens::INFO)
    }

    // ===== Surface variants =====

    /// Default-styled secondary surface. This is the default look for
    /// `button(...)`; calling `.secondary()` makes intent explicit.
    pub fn secondary(mut self) -> Self {
        self.fill = Some(tokens::BG_MUTED);
        self.stroke = Some(tokens::BORDER);
        self.stroke_width = 1.0;
        set_content_color(&mut self, tokens::TEXT_FOREGROUND);
        self.font_weight = FontWeight::Medium;
        self
    }

    /// No fill, no border. Low-emphasis actions like "Cancel" alongside
    /// a primary "Save".
    pub fn ghost(mut self) -> Self {
        self.fill = None;
        self.stroke = None;
        self.stroke_width = 0.0;
        set_content_color(&mut self, tokens::TEXT_MUTED_FOREGROUND);
        self
    }

    /// Outline-only style: no fill, prominent border.
    pub fn outline(mut self) -> Self {
        self.fill = None;
        self.stroke = Some(tokens::BORDER_STRONG);
        self.stroke_width = 1.0;
        set_content_color(&mut self, tokens::TEXT_FOREGROUND);
        self
    }

    /// Muted/neutral emphasis. On surface profiles this swaps to a
    /// neutral background; on text-only profiles it switches the text
    /// color to muted-foreground.
    pub fn muted(mut self) -> Self {
        match self.style_profile {
            StyleProfile::Solid | StyleProfile::Tinted | StyleProfile::Surface => {
                self.fill = Some(tokens::BG_MUTED);
                self.stroke = Some(tokens::BORDER);
                self.stroke_width = 1.0;
                set_content_color(&mut self, tokens::TEXT_MUTED_FOREGROUND);
            }
            StyleProfile::TextOnly => {
                set_content_color(&mut self, tokens::TEXT_MUTED_FOREGROUND);
            }
        }
        self
    }

    // ===== Semantic states =====

    /// Selected row/item treatment. Use for the item that is selected
    /// inside a collection, not for transient keyboard focus.
    pub fn selected(mut self) -> Self {
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::PRIMARY);
        } else {
            match self.style_profile {
                StyleProfile::TextOnly => {}
                StyleProfile::Solid | StyleProfile::Tinted | StyleProfile::Surface => {}
            }
            {
                self.style_profile = StyleProfile::Surface;
                self.surface_role = SurfaceRole::Selected;
                self.fill = Some(tokens::PRIMARY.with_alpha(28));
                self.stroke = Some(tokens::PRIMARY.with_alpha(90));
                self.stroke_width = 1.0;
                set_content_color(&mut self, tokens::TEXT_FOREGROUND);
            }
        }
        self
    }

    /// Current navigation/page treatment. Slightly quieter than
    /// [`Self::selected`] so nav chrome does not compete with content.
    pub fn current(mut self) -> Self {
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::TEXT_FOREGROUND);
            self.font_weight = FontWeight::Semibold;
        } else {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Current;
            self.fill = Some(tokens::BG_RAISED);
            self.stroke = Some(tokens::BORDER);
            self.stroke_width = 1.0;
            set_content_color(&mut self, tokens::TEXT_FOREGROUND);
            self.font_weight = FontWeight::Semibold;
        }
        self
    }

    /// Disabled treatment for controls and rows. Also removes the node
    /// from focus order and blocks pointer hits on this element.
    pub fn disabled(mut self) -> Self {
        self.opacity = tokens::DISABLED_ALPHA;
        self.focusable = false;
        self.block_pointer = true;
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::TEXT_MUTED_FOREGROUND);
        }
        self
    }

    /// Invalid/error treatment for inputs, rows, and validation badges.
    pub fn invalid(mut self) -> Self {
        if !text_only_leaf(&self) {
            self.style_profile = StyleProfile::Surface;
            self.surface_role = SurfaceRole::Danger;
        }
        self.stroke = Some(tokens::DESTRUCTIVE);
        self.stroke_width = 1.0;
        if text_only_leaf(&self) {
            self.text_color = Some(tokens::DESTRUCTIVE);
        }
        self
    }

    /// Loading treatment for a direct text-bearing node. Container
    /// widgets can still use this for opacity even when they do not
    /// have their own label text.
    pub fn loading(mut self) -> Self {
        self.opacity = self.opacity.min(0.78);
        if let Some(label) = &mut self.text {
            label.push_str("...");
        }
        self
    }

    // ===== Typography roles =====

    pub fn text_role(mut self, role: TextRole) -> Self {
        self.text_role = role;
        apply_text_role(&mut self);
        self
    }

    pub fn caption(self) -> Self {
        self.text_role(TextRole::Caption)
    }

    pub fn label(self) -> Self {
        self.text_role(TextRole::Label)
    }

    pub fn body(self) -> Self {
        self.text_role(TextRole::Body)
    }

    pub fn title(self) -> Self {
        self.text_role(TextRole::Title)
    }

    pub fn heading(self) -> Self {
        self.text_role(TextRole::Heading)
    }

    pub fn display(self) -> Self {
        self.text_role(TextRole::Display)
    }

    // ===== Text shape =====

    pub fn bold(mut self) -> Self {
        self.font_weight = FontWeight::Bold;
        self
    }
    pub fn semibold(mut self) -> Self {
        self.font_weight = FontWeight::Semibold;
        self
    }
    pub fn small(mut self) -> Self {
        if text_only_leaf(&self) {
            apply_type_token(&mut self, tokens::TEXT_SM);
        } else {
            self.component_size = Some(ComponentSize::Sm);
        }
        self
    }
    pub fn xsmall(mut self) -> Self {
        if text_only_leaf(&self) {
            apply_type_token(&mut self, tokens::TEXT_XS);
        } else {
            self.component_size = Some(ComponentSize::Xs);
        }
        self
    }
    /// Set an explicit text color.
    pub fn color(mut self, c: Color) -> Self {
        self.text_color = Some(c);
        self
    }
}

fn text_only_leaf(el: &El) -> bool {
    matches!(el.style_profile, StyleProfile::TextOnly) && el.text.is_some()
}

fn apply_type_token(el: &mut El, token: tokens::TypeToken) {
    el.font_size = token.size;
    el.line_height = token.line_height;
}

fn apply_text_role(el: &mut El) {
    match el.text_role {
        TextRole::Body => {
            apply_type_token(el, tokens::TEXT_SM);
            el.font_weight = FontWeight::Regular;
            el.font_mono = false;
            el.text_color = Some(tokens::TEXT_FOREGROUND);
        }
        TextRole::Caption => {
            apply_type_token(el, tokens::TEXT_XS);
            el.font_weight = FontWeight::Regular;
            el.font_mono = false;
            el.text_color = Some(tokens::TEXT_MUTED_FOREGROUND);
        }
        TextRole::Label => {
            apply_type_token(el, tokens::TEXT_SM);
            el.font_weight = FontWeight::Medium;
            el.font_mono = false;
            el.text_color = Some(tokens::TEXT_FOREGROUND);
        }
        TextRole::Title => {
            apply_type_token(el, tokens::TEXT_BASE);
            el.font_weight = FontWeight::Semibold;
            el.font_mono = false;
            el.text_color = Some(tokens::TEXT_FOREGROUND);
        }
        TextRole::Heading => {
            apply_type_token(el, tokens::TEXT_2XL);
            el.font_weight = FontWeight::Semibold;
            el.font_mono = false;
            el.text_color = Some(tokens::TEXT_FOREGROUND);
        }
        TextRole::Display => {
            apply_type_token(el, tokens::TEXT_3XL);
            el.font_weight = FontWeight::Bold;
            el.font_mono = false;
            el.text_color = Some(tokens::TEXT_FOREGROUND);
        }
        TextRole::Code => {
            apply_type_token(el, tokens::TEXT_XS);
            el.font_weight = FontWeight::Regular;
            el.font_mono = true;
            el.text_color = Some(tokens::TEXT_FOREGROUND);
        }
    }
}

fn tint(mut el: El, c: Color) -> El {
    match el.style_profile {
        StyleProfile::Solid => {
            el.fill = Some(c);
            el.stroke = Some(c);
            el.stroke_width = 1.0;
            set_content_color(&mut el, text_on_solid(c));
            el.font_weight = FontWeight::Semibold;
        }
        StyleProfile::Tinted => {
            el.fill = Some(c.with_alpha(38));
            el.stroke = Some(c.with_alpha(120));
            el.stroke_width = 1.0;
            set_content_color(&mut el, c);
        }
        StyleProfile::Surface => {
            el.fill = Some(c.with_alpha(38));
            el.stroke = Some(c.with_alpha(120));
            el.stroke_width = 1.0;
            set_content_color(&mut el, c);
        }
        StyleProfile::TextOnly => {
            set_content_color(&mut el, c);
        }
    }
    el
}

fn set_content_color(el: &mut El, color: Color) {
    el.text_color = Some(color);
    for child in &mut el.children {
        if child.text.is_some() || child.icon.is_some() {
            child.text_color = Some(color);
        }
    }
}

/// Pick a contrasting text color for a solid background fill.
///
/// Rec. 601 luminance threshold tuned so light/saturated fills (accent
/// blue, success green, warning yellow) get dark text, and darker
/// saturated fills (destructive red) get light text.
fn text_on_solid(c: Color) -> Color {
    let lum = 0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32;
    if lum > 150.0 {
        tokens::TEXT_ON_SOLID_DARK
    } else {
        tokens::TEXT_ON_SOLID_LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{button, button_with_icon, icon_button, row, text};

    #[test]
    fn selected_marks_surface_with_accent_treatment() {
        let el = row([text("Selected")]).selected();
        assert_eq!(el.fill, Some(tokens::PRIMARY.with_alpha(28)));
        assert_eq!(el.stroke, Some(tokens::PRIMARY.with_alpha(90)));
        assert_eq!(el.stroke_width, 1.0);
        assert_eq!(el.surface_role, SurfaceRole::Selected);
    }

    #[test]
    fn current_marks_container_as_selected_surface_role() {
        let el = row([text("Current")]).current();
        assert_eq!(el.fill, Some(tokens::BG_RAISED));
        assert_eq!(el.stroke, Some(tokens::BORDER));
        assert_eq!(el.surface_role, SurfaceRole::Current);
        assert_eq!(el.style_profile, StyleProfile::Surface);
    }

    #[test]
    fn disabled_removes_focus_and_dims_control() {
        let el = button("Disabled").disabled();
        assert!(!el.focusable);
        assert!(el.block_pointer);
        assert_eq!(el.opacity, tokens::DISABLED_ALPHA);
    }

    #[test]
    fn icon_button_uses_same_solid_style_surface_as_button() {
        let el = icon_button("menu").primary();
        assert_eq!(el.icon, Some(crate::IconSource::Builtin(IconName::Menu)));
        assert_eq!(el.fill, Some(tokens::PRIMARY));
        assert_eq!(el.text_color, Some(text_on_solid(tokens::PRIMARY)));
        assert_eq!(el.surface_role, SurfaceRole::Raised);
    }

    #[test]
    fn button_with_icon_propagates_variant_content_color() {
        let el = button_with_icon("upload", "Publish").primary();
        assert_eq!(el.fill, Some(tokens::PRIMARY));
        assert_eq!(
            el.children[0].icon,
            Some(crate::IconSource::Builtin(IconName::Upload))
        );
        let expected = text_on_solid(tokens::PRIMARY);
        assert_eq!(el.children[0].text_color, Some(expected));
        assert_eq!(el.children[1].text.as_deref(), Some("Publish"));
        assert_eq!(el.children[1].text_color, Some(expected));
    }

    #[test]
    fn loading_appends_direct_label_text() {
        let el = button("Save").loading();
        assert_eq!(el.text.as_deref(), Some("Save..."));
        assert_eq!(el.opacity, 0.78);
    }

    #[test]
    fn text_roles_apply_inspectable_typographic_defaults() {
        let caption = text("Caption").caption();
        assert_eq!(caption.text_role, TextRole::Caption);
        assert_eq!(caption.font_size, tokens::TEXT_XS.size);
        assert_eq!(caption.line_height, tokens::TEXT_XS.line_height);
        assert_eq!(caption.text_color, Some(tokens::TEXT_MUTED_FOREGROUND));

        let label = text("Label").label();
        assert_eq!(label.text_role, TextRole::Label);
        assert_eq!(label.font_size, tokens::TEXT_SM.size);
        assert_eq!(label.line_height, tokens::TEXT_SM.line_height);
        assert_eq!(label.font_weight, FontWeight::Medium);

        let code = text("Code").code();
        assert_eq!(code.text_role, TextRole::Code);
        assert!(code.font_mono);
    }
}
