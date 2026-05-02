//! Layout pass — flex-style layout over the [`El`] tree.
//!
//! Walks the tree top-down, computing each node's `computed` rect.
//! Sizing rules are deliberately simple:
//!
//! - `Fixed(px)` — exact size on its axis.
//! - `Hug` — intrinsic size (text width, sum of children, etc.).
//! - `Fill(weight)` — share leftover space with other `Fill` siblings.
//!
//! Cross-axis behavior is governed by the parent's [`Align`]; main-axis
//! distribution by [`Justify`] (or just insert a [`spacer`] which is the
//! shadcn-flavored convention).
//!
//! Text intrinsic measurement is approximate: `chars * font_size * 0.58`.
//! Good enough for SVG fixtures; replaced by real shaping when we move to
//! a GPU backend.
//!
//! [`spacer`]: crate::tree::spacer

use crate::tree::*;

/// Lay out the whole tree into the given viewport rect.
///
/// Sets `computed` on every node. Run this once before `render_*`.
pub fn layout(root: &mut El, viewport: Rect) {
    root.computed = viewport;
    layout_children(root);
}

fn layout_children(node: &mut El) {
    match node.axis {
        Axis::Overlay => {
            for c in &mut node.children {
                let inner = node.computed.inset(node.padding);
                c.computed = clamp(c, inner);
                layout_children(c);
            }
        }
        Axis::Column => layout_axis(node, true),
        Axis::Row => layout_axis(node, false),
    }
}

fn layout_axis(node: &mut El, vertical: bool) {
    let inner = node.computed.inset(node.padding);
    let n = node.children.len();
    if n == 0 {
        return;
    }

    let total_gap = node.gap * n.saturating_sub(1) as f32;
    let main_extent = if vertical { inner.h } else { inner.w };
    let cross_extent = if vertical { inner.w } else { inner.h };

    // First pass: figure out how much main-axis space is consumed by
    // Fixed/Hug children; Fill children share the remainder weighted.
    let intrinsics: Vec<(f32, f32)> = node.children.iter().map(intrinsic).collect();

    let mut consumed = 0.0;
    let mut fill_weight_total = 0.0;
    for (c, (iw, ih)) in node.children.iter().zip(intrinsics.iter()) {
        let main_size = main_size_of(c, *iw, *ih, vertical);
        match main_size {
            MainSize::Resolved(v) => consumed += v,
            MainSize::Fill(w) => fill_weight_total += w.max(0.001),
        }
    }
    let remaining = (main_extent - consumed - total_gap).max(0.0);

    // Where on the main axis the first child starts, given justify.
    let used = consumed + total_gap;
    let mut cursor = match node.justify {
        Justify::Start => 0.0,
        Justify::Center => ((main_extent - used - remaining_if_no_fill(fill_weight_total, remaining)) * 0.5).max(0.0),
        Justify::End => (main_extent - used - remaining_if_no_fill(fill_weight_total, remaining)).max(0.0),
        // SpaceBetween is approximate: only meaningful with no Fill children.
        Justify::SpaceBetween => 0.0,
    };
    let between_extra = if matches!(node.justify, Justify::SpaceBetween) && n > 1 && fill_weight_total == 0.0 {
        remaining / (n - 1) as f32
    } else {
        0.0
    };

    for (i, (c, (iw, ih))) in node.children.iter_mut().zip(intrinsics).enumerate() {
        // Main-axis size
        let main_size = match main_size_of(c, iw, ih, vertical) {
            MainSize::Resolved(v) => v,
            MainSize::Fill(w) => remaining * w.max(0.001) / fill_weight_total.max(0.001),
        };

        // Cross-axis size: respect Fixed/Hug, otherwise stretch (or hug if
        // align != Stretch and child has Fill cross size with no constraint).
        let cross_intent = if vertical { c.width } else { c.height };
        let cross_intrinsic = if vertical { iw } else { ih };
        let cross_size = match cross_intent {
            Size::Fixed(v) => v,
            Size::Hug => cross_intrinsic,
            Size::Fill(_) => match node.align {
                Align::Stretch => cross_extent,
                _ => cross_extrinsic_or_extent(cross_intrinsic, cross_extent),
            },
        };

        // Cross-axis offset
        let cross_off = match node.align {
            Align::Start | Align::Stretch => 0.0,
            Align::Center => (cross_extent - cross_size) * 0.5,
            Align::End => cross_extent - cross_size,
        };

        if vertical {
            c.computed = Rect::new(inner.x + cross_off, inner.y + cursor, cross_size, main_size);
        } else {
            c.computed = Rect::new(inner.x + cursor, inner.y + cross_off, main_size, cross_size);
        }

        cursor += main_size + node.gap + if i + 1 < n { between_extra } else { 0.0 };
        layout_children(c);
    }
}

enum MainSize { Resolved(f32), Fill(f32) }

fn main_size_of(c: &El, iw: f32, ih: f32, vertical: bool) -> MainSize {
    let s = if vertical { c.height } else { c.width };
    let intr = if vertical { ih } else { iw };
    match s {
        Size::Fixed(v) => MainSize::Resolved(v),
        Size::Hug => MainSize::Resolved(intr),
        Size::Fill(w) => MainSize::Fill(w),
    }
}

fn cross_extrinsic_or_extent(intrinsic: f32, extent: f32) -> f32 {
    // Non-stretch alignment with Fill cross intent: behave like Hug-ish to
    // avoid stretching to extent unintentionally. Cap at extent.
    intrinsic.min(extent)
}

fn remaining_if_no_fill(fill_weight_total: f32, remaining: f32) -> f32 {
    if fill_weight_total == 0.0 { remaining } else { 0.0 }
}

/// Apply min/max constraints (none yet — placeholder for when we add them).
fn clamp(c: &El, parent: Rect) -> Rect {
    // For overlay layout, default each child to fill the parent unless it has
    // explicit fixed dimensions.
    let w = match c.width {
        Size::Fixed(v) => v,
        _ => parent.w,
    };
    let h = match c.height {
        Size::Fixed(v) => v,
        _ => parent.h,
    };
    Rect::new(parent.x, parent.y, w, h)
}

/// Approximate intrinsic (width, height) for hugging layouts.
///
/// Roughly: text width = `chars * 0.58 * font_size`; container size = sum
/// of children + gaps + padding.
pub fn intrinsic(c: &El) -> (f32, f32) {
    if let Some(text) = &c.text {
        let chars = text.chars().count() as f32;
        let w = chars * c.font_size * char_width_factor(c.font_mono) + c.padding.left + c.padding.right;
        let h = c.font_size * 1.4 + c.padding.top + c.padding.bottom;
        return apply_min(c, w, h);
    }
    match c.axis {
        Axis::Overlay => {
            let mut w: f32 = 0.0;
            let mut h: f32 = 0.0;
            for ch in &c.children {
                let (cw, ch_) = intrinsic(ch);
                w = w.max(cw);
                h = h.max(ch_);
            }
            apply_min(c, w + c.padding.left + c.padding.right, h + c.padding.top + c.padding.bottom)
        }
        Axis::Column => {
            let mut w: f32 = 0.0;
            let mut h: f32 = c.padding.top + c.padding.bottom;
            let n = c.children.len();
            for (i, ch) in c.children.iter().enumerate() {
                let (cw, chh) = intrinsic(ch);
                w = w.max(cw);
                h += chh;
                if i + 1 < n { h += c.gap; }
            }
            apply_min(c, w + c.padding.left + c.padding.right, h)
        }
        Axis::Row => {
            let mut w: f32 = c.padding.left + c.padding.right;
            let mut h: f32 = 0.0;
            let n = c.children.len();
            for (i, ch) in c.children.iter().enumerate() {
                let (cw, chh) = intrinsic(ch);
                w += cw;
                if i + 1 < n { w += c.gap; }
                h = h.max(chh);
            }
            apply_min(c, w, h + c.padding.top + c.padding.bottom)
        }
    }
}

fn apply_min(c: &El, mut w: f32, mut h: f32) -> (f32, f32) {
    if let Size::Fixed(v) = c.width { w = v; }
    if let Size::Fixed(v) = c.height { h = v; }
    (w, h)
}

fn char_width_factor(mono: bool) -> f32 {
    if mono { 0.62 } else { 0.56 }
}
