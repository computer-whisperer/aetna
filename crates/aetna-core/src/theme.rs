//! Theme-level shader routing.
//!
//! Aetna widgets expose familiar style knobs (`fill`, `stroke`, `radius`,
//! `shadow`) while the renderer resolves those facts into shader bindings.
//! `Theme` is the indirection layer between those two worlds: an app can
//! keep using stock widgets and globally swap the shader recipe that paints
//! implicit surfaces.
//!
//! This is intentionally shader-first. Token colors are still authored as
//! constants today, but surface appearance can already move from
//! `stock::rounded_rect` to a custom material without rewriting every
//! `button`, `card`, or `text_input`.

use std::collections::BTreeMap;

use crate::shader::{ShaderHandle, StockShader, UniformBlock, UniformValue};
use crate::tokens;
use crate::tree::{Color, SurfaceRole};
use crate::vector::IconMaterial;

/// Runtime paint theme for implicit widget visuals.
#[derive(Clone, Debug)]
pub struct Theme {
    surface: SurfaceTheme,
    roles: BTreeMap<SurfaceRole, SurfaceTheme>,
    icon_material: IconMaterial,
}

impl Theme {
    /// Current default: stock rounded-rect surfaces.
    pub fn aetna_dark() -> Self {
        Self::default()
    }

    /// Route all implicit surfaces through a custom shader.
    ///
    /// The draw-op pass still emits the familiar rounded-rect uniforms
    /// (`fill`, `stroke`, `radius`, `shadow`, `focus_color`, …). When
    /// `rounded_rect_slots` is enabled, those values are also copied into
    /// `vec_a`..`vec_d`, matching the cross-backend [`crate::paint::QuadInstance`]
    /// ABI so custom shaders can be drop-in material replacements.
    pub fn with_surface_shader(mut self, shader: &'static str) -> Self {
        self.surface.handle = ShaderHandle::Custom(shader);
        self.surface.rounded_rect_slots = true;
        self
    }

    /// Add a uniform to every implicit surface draw. Existing node
    /// uniforms win, so a local widget override can still specialize a
    /// shader parameter.
    pub fn with_surface_uniform(mut self, key: &'static str, value: UniformValue) -> Self {
        self.surface.uniforms.insert(key, value);
        self
    }

    /// Route a specific semantic surface role through a custom shader.
    /// Roles without an override use the global surface recipe.
    pub fn with_role_shader(mut self, role: SurfaceRole, shader: &'static str) -> Self {
        self.role_mut(role).handle = ShaderHandle::Custom(shader);
        self.role_mut(role).rounded_rect_slots = true;
        self
    }

    /// Add a uniform to a specific semantic surface role.
    pub fn with_role_uniform(
        mut self,
        role: SurfaceRole,
        key: &'static str,
        value: UniformValue,
    ) -> Self {
        self.role_mut(role).uniforms.insert(key, value);
        self
    }

    /// Select the stock material used by native vector icon painters.
    /// Backends without vector icon support may ignore this while still
    /// preserving the theme value for API parity.
    pub fn with_icon_material(mut self, material: IconMaterial) -> Self {
        self.icon_material = material;
        self
    }

    pub fn icon_material(&self) -> IconMaterial {
        self.icon_material
    }

    pub(crate) fn surface_handle(&self, role: SurfaceRole) -> ShaderHandle {
        self.role_theme(role).handle
    }

    pub(crate) fn apply_surface_uniforms(&self, role: SurfaceRole, uniforms: &mut UniformBlock) {
        let surface = self.role_theme(role);
        uniforms
            .entry("surface_role")
            .or_insert(UniformValue::F32(role.uniform_id()));
        apply_role_material(role, uniforms);
        if surface.rounded_rect_slots {
            add_rounded_rect_slots(uniforms);
        }
        for (key, value) in &surface.uniforms {
            uniforms.entry(*key).or_insert(*value);
        }
    }

    fn role_mut(&mut self, role: SurfaceRole) -> &mut SurfaceTheme {
        self.roles
            .entry(role)
            .or_insert_with(|| self.surface.clone())
    }

    fn role_theme(&self, role: SurfaceRole) -> &SurfaceTheme {
        self.roles.get(&role).unwrap_or(&self.surface)
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            surface: SurfaceTheme {
                handle: ShaderHandle::Stock(StockShader::RoundedRect),
                uniforms: UniformBlock::new(),
                rounded_rect_slots: false,
            },
            roles: BTreeMap::new(),
            icon_material: IconMaterial::Flat,
        }
    }
}

#[derive(Clone, Debug)]
struct SurfaceTheme {
    handle: ShaderHandle,
    uniforms: UniformBlock,
    rounded_rect_slots: bool,
}

fn add_rounded_rect_slots(uniforms: &mut UniformBlock) {
    if let Some(fill) = uniforms.get("fill").copied() {
        uniforms.entry("vec_a").or_insert(fill);
    }
    if let Some(stroke) = uniforms.get("stroke").copied() {
        uniforms.entry("vec_b").or_insert(stroke);
    }

    let stroke_width = as_f32(uniforms.get("stroke_width")).unwrap_or(0.0);
    let radius = as_f32(uniforms.get("radius")).unwrap_or(0.0);
    let shadow = as_f32(uniforms.get("shadow")).unwrap_or(0.0);
    let focus_width = as_f32(uniforms.get("focus_width")).unwrap_or(0.0);
    uniforms.entry("vec_c").or_insert(UniformValue::Vec4([
        stroke_width,
        radius,
        shadow,
        focus_width,
    ]));

    if let Some(focus_color) = uniforms.get("focus_color").copied() {
        uniforms.entry("vec_d").or_insert(focus_color);
    }
}

fn apply_role_material(role: SurfaceRole, uniforms: &mut UniformBlock) {
    match role {
        SurfaceRole::None => {}
        SurfaceRole::Panel => {
            set_color(uniforms, "stroke", tokens::BORDER.with_alpha(210));
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", tokens::SHADOW_SM);
        }
        SurfaceRole::Raised => {
            default_color(uniforms, "stroke", tokens::BORDER);
            default_f32(uniforms, "stroke_width", 1.0);
            default_f32(uniforms, "shadow", tokens::SHADOW_SM * 0.5);
        }
        SurfaceRole::Sunken | SurfaceRole::Input => {
            set_color(uniforms, "fill", tokens::BG_MUTED.darken(0.08));
            set_color(uniforms, "stroke", tokens::BORDER_STRONG.with_alpha(190));
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", 0.0);
        }
        SurfaceRole::Popover => {
            set_color(uniforms, "stroke", tokens::BORDER_STRONG);
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", tokens::SHADOW_LG);
        }
        SurfaceRole::Selected => {
            default_color(uniforms, "fill", tokens::PRIMARY.with_alpha(28));
            set_color(uniforms, "stroke", tokens::PRIMARY.with_alpha(110));
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", 0.0);
        }
        SurfaceRole::Current => {
            default_color(uniforms, "fill", tokens::BG_RAISED);
            set_color(uniforms, "stroke", tokens::BORDER.with_alpha(180));
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", 0.0);
        }
        SurfaceRole::Danger => {
            set_color(uniforms, "stroke", tokens::DESTRUCTIVE);
            set_f32(uniforms, "stroke_width", 1.0);
            set_f32(uniforms, "shadow", 0.0);
        }
    }
}

fn default_color(uniforms: &mut UniformBlock, key: &'static str, color: Color) {
    uniforms.entry(key).or_insert(UniformValue::Color(color));
}

fn set_color(uniforms: &mut UniformBlock, key: &'static str, color: Color) {
    uniforms.insert(key, UniformValue::Color(color));
}

fn default_f32(uniforms: &mut UniformBlock, key: &'static str, value: f32) {
    uniforms.entry(key).or_insert(UniformValue::F32(value));
}

fn set_f32(uniforms: &mut UniformBlock, key: &'static str, value: f32) {
    uniforms.insert(key, UniformValue::F32(value));
}

fn as_f32(value: Option<&UniformValue>) -> Option<f32> {
    match value {
        Some(UniformValue::F32(v)) => Some(*v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_can_route_icon_material() {
        let theme = Theme::default().with_icon_material(IconMaterial::Relief);
        assert_eq!(theme.icon_material(), IconMaterial::Relief);
    }
}
