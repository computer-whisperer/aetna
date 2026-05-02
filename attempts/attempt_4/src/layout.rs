//! Flex-style layout pass over the [`El`] tree.
//!
//! - `Fixed(px)` — exact size on its axis.
//! - `Hug` — intrinsic size (text width, sum of children, etc.).
//! - `Fill(weight)` — share leftover space proportionally.
//!
//! Cross-axis behavior is governed by the parent's [`Align`]; main-axis
//! distribution by [`Justify`] (or insert a [`spacer`]).
//!
//! The layout pass also assigns each node a stable path-based
//! [`El::computed_id`]: `root.0.card[account].2.button` — a node's ID is
//! parent-id + dot + role-or-key + sibling-index. IDs survive minor
//! refactors and are usable as patch / lint / draw-op targets.
//!
//! Text intrinsic measurement is approximate (`chars × font_size × 0.56`).
//! Good enough for SVG fixtures; will be replaced when glyphon-based
//! shaping lands.
//!
//! # Bug fixes vs. attempt_3
//!
//! - `Justify::Center` / `Justify::End` now actually center / right-align
//!   when there are no `Fill` children. Previous code subtracted
//!   `free_after_used` twice and degenerated to `Justify::Start`.

use crate::tree::*;

/// Lay out the whole tree into the given viewport rect.
pub fn layout(root: &mut El, viewport: Rect) {
    root.computed = viewport;
    assign_id(root, "root");
    layout_children(root);
}

fn assign_id(node: &mut El, path: &str) {
    node.computed_id = path.to_string();
    for (i, c) in node.children.iter_mut().enumerate() {
        let role = role_token(&c.kind);
        let suffix = match (&c.key, role) {
            (Some(k), r) => format!("{r}[{k}]"),
            (None, r) => format!("{r}.{i}"),
        };
        let child_path = format!("{path}.{suffix}");
        assign_id(c, &child_path);
    }
}

fn role_token(k: &Kind) -> &'static str {
    match k {
        Kind::Group => "group",
        Kind::Card => "card",
        Kind::Button => "button",
        Kind::Badge => "badge",
        Kind::Text => "text",
        Kind::Heading => "heading",
        Kind::Spacer => "spacer",
        Kind::Divider => "divider",
        Kind::Custom(name) => name,
    }
}

fn layout_children(node: &mut El) {
    match node.axis {
        Axis::Overlay => {
            let inner = node.computed.inset(node.padding);
            for c in &mut node.children {
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

    let intrinsics: Vec<(f32, f32)> = node.children.iter().map(intrinsic).collect();

    let mut consumed = 0.0;
    let mut fill_weight_total = 0.0;
    for (c, (iw, ih)) in node.children.iter().zip(intrinsics.iter()) {
        match main_size_of(c, *iw, *ih, vertical) {
            MainSize::Resolved(v) => consumed += v,
            MainSize::Fill(w) => fill_weight_total += w.max(0.001),
        }
    }
    let remaining = (main_extent - consumed - total_gap).max(0.0);

    // Free space after children + gaps. When there are Fill children they
    // claim it all, so justify is moot; otherwise this is what center/end
    // distribute around.
    let free_after_used = if fill_weight_total == 0.0 { remaining } else { 0.0 };
    let mut cursor = match node.justify {
        Justify::Start => 0.0,
        Justify::Center => free_after_used * 0.5,
        Justify::End => free_after_used,
        Justify::SpaceBetween => 0.0,
    };
    let between_extra = if matches!(node.justify, Justify::SpaceBetween) && n > 1 && fill_weight_total == 0.0 {
        remaining / (n - 1) as f32
    } else {
        0.0
    };

    for (i, (c, (iw, ih))) in node.children.iter_mut().zip(intrinsics).enumerate() {
        let main_size = match main_size_of(c, iw, ih, vertical) {
            MainSize::Resolved(v) => v,
            MainSize::Fill(w) => remaining * w.max(0.001) / fill_weight_total.max(0.001),
        };

        let cross_intent = if vertical { c.width } else { c.height };
        let cross_intrinsic = if vertical { iw } else { ih };
        let cross_size = match cross_intent {
            Size::Fixed(v) => v,
            Size::Hug => cross_intrinsic,
            Size::Fill(_) => match node.align {
                Align::Stretch => cross_extent,
                _ => cross_intrinsic.min(cross_extent),
            },
        };

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

fn clamp(c: &El, parent: Rect) -> Rect {
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
                let (cw, chh) = intrinsic(ch);
                w = w.max(cw);
                h = h.max(chh);
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
    // Conservative-leaning estimate. cosmic-text's actual run width for
    // typical sans-serif sits around 0.56–0.62 of size depending on the
    // glyph mix; pick the upper end so layout reserves enough width for
    // text that the wgpu glyph run won't overflow visibly. The SVG
    // fixture path also benefits — fewer false-positive overflow lints.
    if mono { 0.62 } else { 0.60 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for attempt_3's broken `Justify::Center` (tripped
    /// during the cold-session login fixture). When all children are
    /// Hug-sized, Justify::Center should split the leftover space.
    #[test]
    fn justify_center_centers_hug_children() {
        let mut root = column([crate::text::text("hi").width(Size::Fixed(40.0)).height(Size::Fixed(20.0))])
            .justify(Justify::Center)
            .height(Size::Fill(1.0));
        layout(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0));
        let child = &root.children[0];
        // Expected: 100 - 20 = 80 leftover; centered → starts at y=40.
        assert!((child.computed.y - 40.0).abs() < 0.5,
            "expected y≈40, got {}", child.computed.y);
    }

    #[test]
    fn justify_end_pushes_to_bottom() {
        let mut root = column([crate::text::text("hi").width(Size::Fixed(40.0)).height(Size::Fixed(20.0))])
            .justify(Justify::End)
            .height(Size::Fill(1.0));
        layout(&mut root, Rect::new(0.0, 0.0, 100.0, 100.0));
        let child = &root.children[0];
        assert!((child.computed.y - 80.0).abs() < 0.5,
            "expected y≈80, got {}", child.computed.y);
    }
}
