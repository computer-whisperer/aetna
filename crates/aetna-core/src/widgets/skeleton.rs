//! Skeleton — loading placeholder block.
//!
//! Mirrors shadcn's intentionally small primitive: create a muted rounded
//! rectangle, then size it with normal Aetna modifiers.
//!
//! ```ignore
//! use aetna_core::prelude::*;
//!
//! column([
//!     skeleton().width(Size::Fixed(220.0)),
//!     skeleton().height(Size::Fixed(80.0)),
//! ])
//! ```

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

#[track_caller]
pub fn skeleton() -> El {
    El::new(Kind::Custom("skeleton"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::Surface)
        .surface_role(SurfaceRole::Sunken)
        .fill(tokens::BG_MUTED)
        .default_radius(tokens::RADIUS_MD)
        .default_width(Size::Fill(1.0))
        .default_height(Size::Fixed(16.0))
        .opacity(0.72)
}

#[track_caller]
pub fn skeleton_circle(size: f32) -> El {
    skeleton()
        .at_loc(Location::caller())
        .width(Size::Fixed(size))
        .height(Size::Fixed(size))
        .radius(tokens::RADIUS_PILL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_is_muted_rounded_block() {
        let s = skeleton();

        assert_eq!(s.kind, Kind::Custom("skeleton"));
        assert_eq!(s.style_profile, StyleProfile::Surface);
        assert_eq!(s.surface_role, SurfaceRole::Sunken);
        assert_eq!(s.fill, Some(tokens::BG_MUTED));
        assert_eq!(s.width, Size::Fill(1.0));
        assert_eq!(s.height, Size::Fixed(16.0));
        assert_eq!(s.radius, tokens::RADIUS_MD);
        assert!(s.opacity < 1.0);
    }

    #[test]
    fn skeleton_circle_sets_equal_axes_and_pill_radius() {
        let s = skeleton_circle(32.0);

        assert_eq!(s.width, Size::Fixed(32.0));
        assert_eq!(s.height, Size::Fixed(32.0));
        assert_eq!(s.radius, tokens::RADIUS_PILL);
    }
}
