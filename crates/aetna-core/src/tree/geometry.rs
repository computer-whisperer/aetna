//! Geometry primitives used by layout, hit-testing, and painting.

/// A rectangle in **logical pixels**. The host's `scale_factor` is
/// applied at paint time, so layout, hit-testing, and `Rect`-shaped
/// API arguments all speak the same un-scaled coordinate space.
///
/// Origin top-left, +y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    pub fn center_x(self) -> f32 {
        self.x + self.w * 0.5
    }

    pub fn center_y(self) -> f32 {
        self.y + self.h * 0.5
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersect(self, other: Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x2 <= x1 {
            return None;
        }
        if y2 <= y1 {
            return None;
        }
        Some(Rect::new(x1, y1, x2 - x1, y2 - y1))
    }

    pub fn inset(self, p: Sides) -> Self {
        Self::new(
            self.x + p.left,
            self.y + p.top,
            (self.w - p.left - p.right).max(0.0),
            (self.h - p.top - p.bottom).max(0.0),
        )
    }

    /// Inverse of [`Self::inset`]: extend the rect outward by `p` on each side.
    pub fn outset(self, p: Sides) -> Self {
        Self::new(
            self.x - p.left,
            self.y - p.top,
            self.w + p.left + p.right,
            self.h + p.top + p.bottom,
        )
    }
}

/// Per-side padding/inset values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sides {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl Sides {
    pub const fn all(v: f32) -> Self {
        Self {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }

    pub const fn xy(x: f32, y: f32) -> Self {
        Self {
            left: x,
            right: x,
            top: y,
            bottom: y,
        }
    }

    pub const fn zero() -> Self {
        Self::all(0.0)
    }
}

impl From<f32> for Sides {
    fn from(v: f32) -> Self {
        Sides::all(v)
    }
}
