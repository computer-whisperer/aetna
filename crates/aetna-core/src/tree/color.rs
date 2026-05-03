//! Color values + the small bit of arithmetic that goes with them.
//!
//! Token name has no effect on rendering — it's metadata for inspection,
//! lint, and shader-manifest output. Future render-time theme substitution
//! would key off this name.

/// A color (RGBA8) optionally tagged with the theme token it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
    pub token: Option<&'static str>,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r,
            g,
            b,
            a,
            token: None,
        }
    }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }
    pub const fn token(name: &'static str, r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r,
            g,
            b,
            a,
            token: Some(name),
        }
    }
    pub fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    /// Lighten by a 0..1 factor (mix toward white).
    pub fn lighten(self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: lerp_u8(self.r, 255, t),
            g: lerp_u8(self.g, 255, t),
            b: lerp_u8(self.b, 255, t),
            ..self
        }
    }
    /// Darken by a 0..1 factor (mix toward black).
    pub fn darken(self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: lerp_u8(self.r, 0, t),
            g: lerp_u8(self.g, 0, t),
            b: lerp_u8(self.b, 0, t),
            ..self
        }
    }

    /// Linearly interpolate between two colours by `t` in `[0, 1]`.
    /// `t = 0` returns `self`, `t = 1` returns `other`. Token metadata
    /// is preserved from `self` so an interpolated token stays named.
    pub fn mix(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: lerp_u8(self.r, other.r, t),
            g: lerp_u8(self.g, other.g, t),
            b: lerp_u8(self.b, other.b, t),
            a: lerp_u8(self.a, other.a, t),
            token: self.token,
        }
    }
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}
