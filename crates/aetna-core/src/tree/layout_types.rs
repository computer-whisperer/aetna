//! Layout intent enums carried by [`El`](crate::El).

/// Sizing intent along one axis.
///
/// - `Fixed(px)` -- exact size.
/// - `Fill(weight)` -- claim a share of leftover space; weights are relative.
/// - `Hug` -- intrinsic size of contents.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Size {
    Fixed(f32),
    Fill(f32),
    #[default]
    Hug,
}

/// Layout direction for a container's children.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Axis {
    /// No layout; children share the parent's rect.
    #[default]
    Overlay,
    /// Stack children top-to-bottom.
    Column,
    /// Stack children left-to-right.
    Row,
}

/// Cross-axis sizing and alignment of children, mirroring CSS `align-items`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Align {
    /// Pin to the start of the cross axis.
    Start,
    /// Center in the cross extent.
    Center,
    /// Pin to the end of the cross axis.
    End,
    /// Stretch non-`Fixed` children to the container's cross extent.
    #[default]
    Stretch,
}

/// Main-axis distribution when children do not fill the container.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}
