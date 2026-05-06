//! Tree → [`DrawOp`] resolution.
//!
//! Walks the laid-out [`El`] tree and emits a flat [`Vec<DrawOp>`] in
//! paint order. Each visual fact resolves to a `Quad` (bound to a stock
//! or custom shader, with uniforms packed) or a `GlyphRun`.
//!
//! State styling lands here on the CPU side. Hover lightens / press
//! darkens / focus-ring fade come from the eased envelopes in
//! `UiState`'s eased envelope side map, written by
//! [`UiState::tick_visual_animations`] in the prior pass. What this
//! module computes are the deltas: lerp the build-time colours toward
//! the state-modulated ones by the envelope amount, plus the non-eased
//! `Disabled` (alpha multiply) and `Loading` (text suffix) deltas.

use crate::ir::*;
use crate::shader::*;
use crate::state::{EnvelopeKind, UiState};
use crate::text::atlas::RunStyle;
use crate::text::metrics as text_metrics;
use crate::theme::Theme;
use crate::tokens;
use crate::tree::*;

/// Walk the laid-out tree and emit draw ops in paint order.
pub fn draw_ops(root: &El, ui_state: &UiState) -> Vec<DrawOp> {
    draw_ops_with_theme(root, ui_state, &Theme::default())
}

/// Walk the laid-out tree and emit draw ops using a caller-supplied theme.
pub fn draw_ops_with_theme(root: &El, ui_state: &UiState, theme: &Theme) -> Vec<DrawOp> {
    let mut out = Vec::new();
    push_node(
        root,
        ui_state,
        theme,
        &mut out,
        None,
        (0.0, 0.0),
        1.0,
        1.0,
        0.0,
        0.0,
    );
    out
}

// Recursion threads six "inherited from parent" paint values
// (scissor, translate, opacity, focus / hover / press envelopes) plus
// the four shared references (node, ui_state, theme, out accumulator).
// The explicit signature documents the dataflow more clearly than a
// bundling struct would.
#[allow(clippy::too_many_arguments)]
fn push_node(
    n: &El,
    ui_state: &UiState,
    theme: &Theme,
    out: &mut Vec<DrawOp>,
    inherited_scissor: Option<Rect>,
    inherited_translate: (f32, f32),
    inherited_opacity: f32,
    inherited_focus_envelope: f32,
    inherited_hover_envelope: f32,
    inherited_press_envelope: f32,
) {
    let computed = ui_state.rect(&n.computed_id);
    let state = ui_state.node_state(&n.computed_id);
    let hover_amount = ui_state.envelope(&n.computed_id, EnvelopeKind::Hover);
    let press_amount = ui_state.envelope(&n.computed_id, EnvelopeKind::Press);
    let focus_ring_alpha = ui_state.envelope(&n.computed_id, EnvelopeKind::FocusRing);

    // `state_follows_interactive_ancestor` borrows the nearest
    // focusable ancestor's hover / press envelopes for paint. The
    // hit-test only ever lands on the focusable container above, so
    // child elements (slider thumb, etc.) never receive their own
    // envelope — without this, hover / press are dead on those
    // children.
    let (effective_hover, effective_press) = if n.state_follows_interactive_ancestor {
        (inherited_hover_envelope, inherited_press_envelope)
    } else {
        (hover_amount, press_amount)
    };

    let (fill, stroke, text_color, weight, suffix) =
        apply_state(n, state, effective_hover, effective_press);

    // `translate` is subtree-inheriting: descendants paint at their
    // computed rect plus all ancestor `translate` accumulated through
    // the recursion. `scale` and `opacity` apply to this node only —
    // a parent fading to 0.5 multiplies through to descendants via
    // `inherited_opacity`, but `scale` doesn't propagate (descendants
    // keep their own paint metrics).
    let total_translate = (
        inherited_translate.0 + n.translate.0,
        inherited_translate.1 + n.translate.1,
    );
    // Nodes flagged with `alpha_follows_focused_ancestor` fade with
    // their nearest focusable ancestor's focus envelope. The flag is
    // layout-neutral; we just multiply the ancestor's envelope into
    // this node's paint opacity, and the existing alpha modulation in
    // `opaque(...)` propagates that to fill / stroke / text colors.
    let focus_alpha_mul = if n.alpha_follows_focused_ancestor {
        inherited_focus_envelope
    } else {
        1.0
    };
    let opacity = inherited_opacity * n.opacity * focus_alpha_mul;
    // Children inherit the *immediate* focusable ancestor's envelope.
    // When this node is itself focusable, its envelope replaces the
    // inherited one; otherwise the inherited value passes through.
    // Hover / press follow the same rule so opt-in descendants can
    // borrow their interactive ancestor's state envelopes (see
    // `state_follows_interactive_ancestor`).
    let child_focus_envelope = if n.focusable {
        focus_ring_alpha
    } else {
        inherited_focus_envelope
    };
    let child_hover_envelope = if n.focusable {
        hover_amount
    } else {
        inherited_hover_envelope
    };
    let child_press_envelope = if n.focusable {
        press_amount
    } else {
        inherited_press_envelope
    };

    let translated_rect = translated(computed, total_translate);
    // The layout rect, post translate + scale, is the visual boundary the
    // SDF and clip both anchor to. `painted_rect` extends it by
    // `paint_overflow` so the quad has room to draw focus rings, drop
    // shadows, and other halos *outside* the layout box without
    // affecting sibling positions. Drop shadow auto-widens the band
    // (per-side max with explicit `paint_overflow`) so `.shadow(s)`
    // works without every shadow-using widget remembering to set
    // `paint_overflow` separately. The stock-shader branch resolves the
    // *effective* shadow (post-theme) before computing `painted_rect`,
    // since surface roles can rewrite the shadow uniform.
    let inner_painted_rect = scaled_around_center(translated_rect, n.scale);
    let painted_font_size = n.font_size * n.scale;

    // Clip uses the layout rect, not the overflowed painted rect:
    // `clip()` is about constraining descendants to the layout box, not
    // about whether this element's own paint can spill into its
    // overflow band.
    let own_scissor = if n.clip {
        intersect_scissor(inherited_scissor, inner_painted_rect)
    } else {
        inherited_scissor
    };

    // Surface paint. Either a custom shader override, or the implicit
    // `stock::rounded_rect` driven by the El's fill/stroke/radius/shadow.
    if let Some(custom) = &n.shader_override {
        // Custom shaders manage their own paint extent; we only honor
        // explicit `paint_overflow` here. They may pack a shadow into
        // their own uniform name, which we can't introspect.
        let painted_rect = inner_painted_rect.outset(n.paint_overflow);
        let mut uniforms = custom.uniforms.clone();
        uniforms.insert("inner_rect", inner_rect_uniform(inner_painted_rect));
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: painted_rect,
            scissor: own_scissor,
            shader: custom.handle,
            uniforms,
        });
    } else if fill.is_some() || stroke.is_some() || focus_ring_alpha > 0.0 {
        let mut uniforms = UniformBlock::new();
        if let Some(c) = fill {
            uniforms.insert("fill", UniformValue::Color(opaque(c, opacity)));
        }
        if let Some(c) = stroke {
            uniforms.insert("stroke", UniformValue::Color(opaque(c, opacity)));
            uniforms.insert("stroke_width", UniformValue::F32(n.stroke_width));
        }
        uniforms.insert("radius", UniformValue::F32(n.radius));
        if n.shadow > 0.0 {
            uniforms.insert("shadow", UniformValue::F32(n.shadow));
        }
        uniforms.insert("inner_rect", inner_rect_uniform(inner_painted_rect));
        // Focus ring rides on the node's own quad: the library injects a
        // `focus_color` (with the eased focus alpha already multiplied
        // into its rgba) plus `focus_width`, and `stock::rounded_rect`
        // draws the ring in the `paint_overflow` band when alpha > 0.
        // Custom shaders read the same uniforms and decide for
        // themselves what to paint — the symmetry rule.
        if n.focusable && focus_ring_alpha > 0.0 {
            let base = tokens::FOCUS_RING;
            let eased_alpha = (base.a as f32 * focus_ring_alpha * opacity)
                .round()
                .clamp(0.0, 255.0) as u8;
            uniforms.insert(
                "focus_color",
                UniformValue::Color(base.with_alpha(eased_alpha)),
            );
            uniforms.insert("focus_width", UniformValue::F32(tokens::FOCUS_RING_WIDTH));
        }
        theme.apply_surface_uniforms(n.surface_role, &mut uniforms);
        // Read shadow + stroke *after* theme has had its say — surface
        // roles (Panel/Popover/Sunken/...) can override either uniform,
        // and we want the painted rect to track what actually renders.
        let effective_shadow = match uniforms.get("shadow") {
            Some(UniformValue::F32(s)) => *s,
            _ => 0.0,
        };
        let effective_stroke_width = if uniforms.contains_key("stroke") {
            match uniforms.get("stroke_width") {
                Some(UniformValue::F32(w)) => *w,
                _ => 0.0,
            }
        } else {
            0.0
        };
        let painted_rect = inner_painted_rect.outset(combined_overflow(
            n.paint_overflow,
            effective_shadow,
            effective_stroke_width,
        ));
        out.push(DrawOp::Quad {
            id: n.computed_id.clone(),
            rect: painted_rect,
            scissor: own_scissor,
            shader: theme.surface_handle(n.surface_role),
            uniforms,
        });
    }

    if let Some(text) = &n.text {
        // `padding` on a text-bearing node insets the glyph rect the
        // same way it insets the children of a container node — so
        // `text("X").padding(...)` and `column([text("X")]).padding(...)`
        // produce visually identical results. Without this, padding on
        // a text node would silently inflate intrinsic measurement only
        // and disappear once `Align::Stretch` flattened the Hug width.
        let glyph_rect = inner_painted_rect.inset(n.padding);
        let display = match suffix {
            Some(s) => format!("{text}{s}"),
            None => text.clone(),
        };
        let display = match (n.text_wrap, n.text_max_lines) {
            (TextWrap::Wrap, Some(max_lines)) => text_metrics::clamp_text_to_lines(
                &display,
                painted_font_size,
                weight,
                n.font_mono,
                glyph_rect.w,
                max_lines,
            ),
            _ => display,
        };
        let display = match (n.text_wrap, n.text_overflow) {
            (TextWrap::NoWrap, TextOverflow::Ellipsis) => text_metrics::ellipsize_text(
                &display,
                painted_font_size,
                weight,
                n.font_mono,
                glyph_rect.w,
            ),
            _ => display,
        };
        let anchor = match n.text_align {
            TextAlign::Start => TextAnchor::Start,
            TextAlign::Center => TextAnchor::Middle,
            TextAlign::End => TextAnchor::End,
        };
        let text_color = opaque(text_color.unwrap_or(tokens::TEXT_FOREGROUND), opacity);
        let layout = text_metrics::layout_text(
            &display,
            painted_font_size,
            weight,
            n.font_mono,
            n.text_wrap,
            match n.text_wrap {
                TextWrap::NoWrap => None,
                TextWrap::Wrap => Some(glyph_rect.w),
            },
        );

        // Selection band — emit behind the glyph run when this leaf is
        // selectable, keyed, and (part of) its bytes fall inside the
        // active selection range. Only single-leaf selections paint
        // here in P1a; cross-element selections need the
        // selection_order walk and ship in P1b.
        if n.selectable
            && let Some(key) = &n.key
            && let Some(view) = ui_state.current_selection.within(key)
            && !view.is_collapsed()
        {
            let (lo, hi) = view.ordered();
            let rects = text_metrics::selection_rects(
                &display,
                lo,
                hi,
                painted_font_size,
                weight,
                n.text_wrap,
                match n.text_wrap {
                    TextWrap::NoWrap => None,
                    TextWrap::Wrap => Some(glyph_rect.w),
                },
            );
            for (rx, ry, rw, rh) in rects {
                // The band must paint *behind* the glyph run; we emit
                // the Quad with `inner_rect` matching `rect` so the
                // SDF-based rounded_rect shader treats the whole band
                // as inside (no overflow halo). `inner_rect` is also
                // what the shader computes coverage against, so a
                // mismatch with `rect` would partially or fully clip
                // the fill.
                let band = Rect::new(glyph_rect.x + rx, glyph_rect.y + ry, rw, rh);
                let mut band_uniforms = UniformBlock::new();
                band_uniforms.insert(
                    "fill",
                    UniformValue::Color(opaque(tokens::SELECTION_BG, opacity)),
                );
                band_uniforms.insert("radius", UniformValue::F32(2.0));
                band_uniforms.insert("inner_rect", inner_rect_uniform(band));
                out.push(DrawOp::Quad {
                    id: format!("{}.selection-band", n.computed_id),
                    rect: band,
                    scissor: own_scissor,
                    shader: ShaderHandle::Stock(StockShader::RoundedRect),
                    uniforms: band_uniforms,
                });
            }
        }

        out.push(DrawOp::GlyphRun {
            id: n.computed_id.clone(),
            rect: glyph_rect,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::Text),
            color: text_color,
            text: display,
            size: painted_font_size,
            weight,
            mono: n.font_mono,
            wrap: n.text_wrap,
            anchor,
            layout,
        });
    }

    if let Some(source) = &n.icon {
        let color = opaque(text_color.unwrap_or(tokens::TEXT_FOREGROUND), opacity);
        let inner = inner_painted_rect.inset(n.padding);
        let icon_size = painted_font_size.min(inner.w).min(inner.h).max(1.0);
        let icon_rect = Rect::new(
            inner.center_x() - icon_size * 0.5,
            inner.center_y() - icon_size * 0.5,
            icon_size,
            icon_size,
        );
        out.push(DrawOp::Icon {
            id: n.computed_id.clone(),
            rect: icon_rect,
            scissor: own_scissor,
            source: source.clone(),
            color,
            size: icon_size,
            stroke_width: n.icon_stroke_width * n.scale,
        });
    }

    if let Some(image) = &n.image {
        let inner = inner_painted_rect.inset(n.padding);
        let dest = n.image_fit.project(image.width(), image.height(), inner);
        // Always clip image draws to the El's content rect so `Cover`
        // / `None` overflow is cropped without forcing every author to
        // call `.clip()`. The clamp respects any inherited scissor.
        let scissor = match own_scissor {
            Some(s) => s.intersect(inner),
            None => Some(inner),
        };
        let tint = n.image_tint.map(|c| opaque(c, opacity));
        out.push(DrawOp::Image {
            id: n.computed_id.clone(),
            rect: dest,
            scissor,
            image: image.clone(),
            tint,
            radius: n.radius,
            fit: n.image_fit,
        });
    }

    // Attributed paragraph: aggregate child Text/HardBreak runs into one
    // DrawOp::AttributedText so cosmic-text shapes the runs together
    // (wrapping crosses run boundaries like real prose). Skip recursion
    // into children — they're encoded in the runs and don't paint
    // independently.
    if matches!(n.kind, Kind::Inlines) {
        let glyph_rect = inner_painted_rect.inset(n.padding);
        let runs = collect_inline_runs(n, opacity);
        let concat: String = runs.iter().map(|(t, _)| t.as_str()).collect();
        let inline_size = inline_paragraph_font_size(n) * n.scale;
        let anchor = match n.text_align {
            TextAlign::Start => TextAnchor::Start,
            TextAlign::Center => TextAnchor::Middle,
            TextAlign::End => TextAnchor::End,
        };
        let layout = text_metrics::layout_text(
            &concat,
            inline_size,
            FontWeight::Regular,
            false,
            n.text_wrap,
            match n.text_wrap {
                TextWrap::NoWrap => None,
                TextWrap::Wrap => Some(glyph_rect.w),
            },
        );
        out.push(DrawOp::AttributedText {
            id: n.computed_id.clone(),
            rect: glyph_rect,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::Text),
            runs,
            size: inline_size,
            wrap: n.text_wrap,
            anchor,
            layout,
        });
        return;
    }

    for c in &n.children {
        push_node(
            c,
            ui_state,
            theme,
            out,
            own_scissor,
            total_translate,
            opacity,
            child_focus_envelope,
            child_hover_envelope,
            child_press_envelope,
        );
    }

    // Scrollbar thumb. Painted *after* children so it sits on top
    // visually, with `own_scissor` so it inherits the scrollable's
    // clip but is otherwise free of the scroll offset (the layout
    // pass shifts the children, not the thumb). `thumb_rects` is
    // populated only when the scrollable opted in and content
    // overflows, so the gating is implicit. When the pointer is
    // anywhere within the track or a drag is active, the visible
    // thumb expands to `SCROLLBAR_THUMB_WIDTH_ACTIVE` (right-anchored)
    // so the cursor sits inside the thumb instead of pinning the
    // track's right edge.
    if let Some(thumb_rect) = ui_state.thumb_rects.get(&n.computed_id) {
        let active = thumb_is_active(n, ui_state);
        let visible = if active {
            let new_w = tokens::SCROLLBAR_THUMB_WIDTH_ACTIVE.max(thumb_rect.w);
            Rect::new(
                thumb_rect.right() - new_w,
                thumb_rect.y,
                new_w,
                thumb_rect.h,
            )
        } else {
            *thumb_rect
        };
        let painted_thumb = translated(visible, total_translate);
        let base_fill = if active {
            tokens::SCROLLBAR_THUMB_FILL_ACTIVE
        } else {
            tokens::SCROLLBAR_THUMB_FILL
        };
        let mut uniforms = UniformBlock::new();
        uniforms.insert("fill", UniformValue::Color(opaque(base_fill, opacity)));
        uniforms.insert("radius", UniformValue::F32(visible.w.min(visible.h) * 0.5));
        uniforms.insert("inner_rect", inner_rect_uniform(painted_thumb));
        out.push(DrawOp::Quad {
            id: format!("{}.scrollbar-thumb", n.computed_id),
            rect: painted_thumb,
            scissor: own_scissor,
            shader: ShaderHandle::Stock(StockShader::RoundedRect),
            uniforms,
        });
    }
}

/// Active when the user is actively dragging this scrollable's thumb
/// or the pointer is hovering anywhere inside its track (the
/// generous-hitbox column on the right). Hover is computed against
/// the *un-translated* track rect since the pointer position is
/// captured pre-translate.
fn thumb_is_active(n: &El, ui_state: &UiState) -> bool {
    if let Some(drag) = ui_state.thumb_drag.as_ref()
        && drag.scroll_id == n.computed_id
    {
        return true;
    }
    if let (Some((px, py)), Some(track)) = (
        ui_state.pointer_pos,
        ui_state.thumb_tracks.get(&n.computed_id),
    ) {
        return track.contains(px, py);
    }
    false
}

/// Walk an Inlines paragraph's children and produce source-order
/// (text, RunStyle) tuples. Each `Kind::Text` child contributes one
/// run carrying its `font_weight`, `text_italic`, `font_mono`, and
/// `text_color`. `Kind::HardBreak` contributes a `\n` run with default
/// styling — cosmic-text turns the newline into a line break during
/// shaping, so style doesn't matter (no glyph is emitted).
fn collect_inline_runs(node: &El, opacity: f32) -> Vec<(String, RunStyle)> {
    let mut runs: Vec<(String, RunStyle)> = Vec::with_capacity(node.children.len());
    for c in &node.children {
        match c.kind {
            Kind::Text => {
                if let Some(text) = &c.text {
                    let color = opaque(c.text_color.unwrap_or(tokens::TEXT_FOREGROUND), opacity);
                    let mut style = RunStyle::new(c.font_weight, color);
                    if c.text_italic {
                        style = style.italic();
                    }
                    if c.font_mono {
                        style = style.mono();
                    }
                    if let Some(bg) = c.text_bg {
                        style = style.with_bg(opaque(bg, opacity));
                    }
                    runs.push((text.clone(), style));
                }
            }
            Kind::HardBreak => {
                runs.push((
                    "\n".to_string(),
                    RunStyle::new(FontWeight::Regular, tokens::TEXT_FOREGROUND),
                ));
            }
            _ => {}
        }
    }
    runs
}

/// Pick the dominant font size for the paragraph's approximate
/// pre-shaping layout (used by SVG and lint). Mirrors the layout
/// pass's `inline_paragraph_size` heuristic — max across text
/// children, falling back to the parent's own `font_size`.
fn inline_paragraph_font_size(node: &El) -> f32 {
    let mut size: f32 = node.font_size;
    for c in &node.children {
        if matches!(c.kind, Kind::Text) {
            size = size.max(c.font_size);
        }
    }
    size
}

fn translated(r: Rect, offset: (f32, f32)) -> Rect {
    if offset.0 == 0.0 && offset.1 == 0.0 {
        return r;
    }
    Rect::new(r.x + offset.0, r.y + offset.1, r.w, r.h)
}

/// Combine an element's explicit `paint_overflow` with the implicit
/// halo a non-zero `shadow` and / or `stroke` needs around the layout
/// rect. The shadow's SDF in `stock::rounded_rect` softens over a
/// `blur`-wide band around an offset-down silhouette: alpha hits zero
/// at distance `blur` outside the (offset) box, so left/right need
/// `blur`, top needs `blur*0.5` (offset reduces upward extent), bottom
/// needs `blur*1.5`. Stroke straddles the boundary — its outside half
/// (`stroke_width*0.5`) plus the AA tail (≈1 px) lives just outside the
/// layout rect, so the painted quad needs that much room on every side
/// or the cardinal pixels of curved boundaries (the radio indicator's
/// circle, switch thumb, …) get clipped and the shape looks flattened
/// at top / bottom / left / right. Per-side max with the user's
/// `paint_overflow` so a focus-ring outset + shadow + stroke on the
/// same node all fit.
fn combined_overflow(paint_overflow: Sides, shadow: f32, stroke_width: f32) -> Sides {
    let stroke_halo = if stroke_width > 0.0 {
        stroke_width * 0.5 + 1.0
    } else {
        0.0
    };
    let stroked = if stroke_halo > 0.0 {
        Sides {
            left: paint_overflow.left.max(stroke_halo),
            right: paint_overflow.right.max(stroke_halo),
            top: paint_overflow.top.max(stroke_halo),
            bottom: paint_overflow.bottom.max(stroke_halo),
        }
    } else {
        paint_overflow
    };
    if shadow <= 0.0 {
        return stroked;
    }
    Sides {
        left: stroked.left.max(shadow),
        right: stroked.right.max(shadow),
        top: stroked.top.max(shadow * 0.5),
        bottom: stroked.bottom.max(shadow * 1.5),
    }
}

/// Scale `r` uniformly by `s` around its centre. `s == 1.0` short-circuits
/// to identity so the common case is allocation-free of float drift.
fn scaled_around_center(r: Rect, s: f32) -> Rect {
    if (s - 1.0).abs() < f32::EPSILON {
        return r;
    }
    let cx = r.center_x();
    let cy = r.center_y();
    let w = r.w * s;
    let h = r.h * s;
    Rect::new(cx - w * 0.5, cy - h * 0.5, w, h)
}

fn opaque(c: Color, opacity: f32) -> Color {
    if (opacity - 1.0).abs() < f32::EPSILON {
        return c;
    }
    let a = (c.a as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
    c.with_alpha(a)
}

/// Resolve the effective `(fill, stroke, text_color, font_weight,
/// optional text suffix)` for paint.
///
/// Hover and press are applied as **envelope mixes**: the eased amounts
/// `hover` / `press` (both 0..1, written by the animation tracker into
/// [`UiState::envelopes`]) lerp the build-time colour toward its
/// state-modulated form. This composition keeps state easing
/// independent of mid-flight changes to `n.fill` — the author can swap
/// a button's colour during a hover and the new colour appears with
/// the same eased lighten amount, no fighting between trackers.
///
/// Surfaces with no resting fill (`.ghost()`, `.outline()`, inactive tab
/// triggers) get a **synthesized state-only fill** instead — a faint
/// `BG_RAISED` whose alpha rises with hover and press. Mirrors the
/// shadcn idiom `hover:bg-accent active:bg-accent/80`: transparent at
/// rest, a soft surface fades in on interaction. Without this, the
/// envelope mix above has nothing to land on (`None.map(...)` is
/// `None`) and ghost surfaces show no feedback at all.
///
/// The synthesis only fires when the node already declares some
/// surface affordance — a non-zero radius or an explicit stroke. That
/// excludes layout-only focusable containers (the `stack(...)` outers
/// of `slider`, `switch`, `resize_handle`) where a translucent
/// rectangle behind the actual visual would compete with the widget's
/// own thumb / track / hairline.
///
/// Disabled (alpha multiply) and Loading (text suffix) aren't eased
/// and are still applied here, branching on the resolved `state`.
fn apply_state(
    n: &El,
    state: InteractionState,
    hover: f32,
    press: f32,
) -> (
    Option<Color>,
    Option<Color>,
    Option<Color>,
    FontWeight,
    Option<&'static str>,
) {
    let mut fill = n.fill;
    let mut stroke = n.stroke;
    let mut text_color = n.text_color;
    let weight = n.font_weight;
    let mut suffix = None;

    if hover > 0.0 {
        fill = fill.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN), hover));
        stroke = stroke.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN), hover));
        text_color = text_color.map(|c| c.mix(c.lighten(tokens::HOVER_LIGHTEN * 0.5), hover));
    }
    if press > 0.0 {
        fill = fill.map(|c| c.mix(c.darken(tokens::PRESS_DARKEN), press));
        stroke = stroke.map(|c| c.mix(c.darken(tokens::PRESS_DARKEN), press));
        text_color = text_color.map(|c| c.mix(c.darken(tokens::PRESS_DARKEN * 0.5), press));
    }
    if n.fill.is_none() && (hover > 0.0 || press > 0.0) && (n.radius > 0.0 || n.stroke.is_some()) {
        let alpha = (hover * tokens::STATE_FILL_HOVER_ALPHA
            + press * tokens::STATE_FILL_PRESS_ALPHA)
            .clamp(0.0, 1.0);
        fill = Some(tokens::BG_RAISED.with_alpha((alpha * 255.0).round() as u8));
    }

    match state {
        InteractionState::Default
        | InteractionState::Focus
        | InteractionState::Hover
        | InteractionState::Press => {}
        InteractionState::Disabled => {
            let alpha = (255.0 * tokens::DISABLED_ALPHA) as u8;
            fill = fill.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
            stroke = stroke.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
            text_color =
                text_color.map(|c| c.with_alpha(((c.a as u32 * alpha as u32) / 255) as u8));
        }
        InteractionState::Loading => {
            text_color = text_color.map(|c| c.with_alpha(((c.a as u32 * 200) / 255) as u8));
            suffix = Some(" ⋯");
        }
    }
    (fill, stroke, text_color, weight, suffix)
}

/// Pack a rect as the `inner_rect` uniform value (vec4 of x, y, w, h).
fn inner_rect_uniform(r: Rect) -> UniformValue {
    UniformValue::Vec4([r.x, r.y, r.w, r.h])
}

fn intersect_scissor(current: Option<Rect>, next: Rect) -> Option<Rect> {
    match current {
        Some(r) => Some(r.intersect(next).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))),
        None => Some(next),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::UiState;
    use crate::{button, column, row};

    #[test]
    fn ghost_surface_synthesizes_state_fill_for_hover_and_press() {
        // Surfaces with no resting fill (`.ghost()`, inactive tab
        // triggers, `.outline()`) must still show interaction feedback.
        // The hover/press envelope mix is `fill.map(...)` which
        // collapses to `None` when there's nothing to lerp from, so
        // `apply_state` synthesizes a translucent BG_RAISED fill whose
        // alpha rises with hover and press.
        // `.ghost()` clears fill / stroke; a real tab trigger or
        // ghost button also carries a radius (the visual affordance
        // the synthesis gates on).
        let ghost = El::new(Kind::Custom("tab_trigger"))
            .ghost()
            .radius(tokens::RADIUS_SM);
        assert!(ghost.fill.is_none(), "ghost has no resting fill");

        let (rest_fill, ..) = apply_state(&ghost, InteractionState::Default, 0.0, 0.0);
        assert_eq!(rest_fill, None, "no envelope, no synthesized fill");

        let (hover_fill, ..) = apply_state(&ghost, InteractionState::Hover, 1.0, 0.0);
        let hover_alpha = (tokens::STATE_FILL_HOVER_ALPHA * 255.0).round() as u8;
        assert_eq!(
            hover_fill,
            Some(tokens::BG_RAISED.with_alpha(hover_alpha)),
            "hover at peak fades a faint BG_RAISED in",
        );

        let (press_fill, ..) = apply_state(&ghost, InteractionState::Press, 1.0, 1.0);
        let press_alpha = ((tokens::STATE_FILL_HOVER_ALPHA + tokens::STATE_FILL_PRESS_ALPHA)
            * 255.0)
            .round() as u8;
        assert_eq!(
            press_fill,
            Some(tokens::BG_RAISED.with_alpha(press_alpha)),
            "press while hovered sums the two envelope contributions",
        );
    }

    #[test]
    fn state_follows_interactive_ancestor_borrows_envelopes() {
        // A child flagged with `state_follows_interactive_ancestor` —
        // the slider thumb pattern — borrows hover and press
        // envelopes from its focusable container, since hit-test
        // never resolves to it directly.
        use crate::layout::layout;

        let mut tree = column([row([crate::stack([El::new(Kind::Custom("thumb"))
            .key("thumb")
            .width(Size::Fixed(14.0))
            .height(Size::Fixed(14.0))
            .fill(tokens::TEXT_FOREGROUND)
            .radius(tokens::RADIUS_PILL)
            .state_follows_interactive_ancestor()])
        .key("container")
        .focusable()
        .width(Size::Fixed(120.0))
        .height(Size::Fixed(18.0))])])
        .padding(20.0);
        let mut state = UiState::new();
        layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));

        // Drive the container into Press by setting both `pressed` and
        // `hovered` (post-fix gating requires hover==pressed for press
        // to fire) and snap envelopes via Settled mode.
        let container_target = state
            .target_of_key(&tree, "container")
            .expect("container target");
        state.hovered = Some(container_target.clone());
        state.pressed = Some(container_target);
        state.apply_to_state();
        state.set_animation_mode(crate::state::AnimationMode::Settled);
        state.tick_visual_animations(&mut tree, web_time::Instant::now());

        // The thumb's *own* envelopes stay zero — only the container
        // got the press. But via the cascade flag, the thumb's paint
        // sees the container's press envelope.
        let ops = draw_ops(&tree, &state);
        let thumb_op = ops
            .iter()
            .find(|op| op.id().contains("thumb"))
            .expect("thumb quad");
        let DrawOp::Quad { uniforms, .. } = thumb_op else {
            panic!("expected thumb quad");
        };
        let UniformValue::Color(thumb_fill) = uniforms.get("fill").expect("thumb fill") else {
            panic!("expected color uniform");
        };
        // Press darkens TEXT_FOREGROUND by PRESS_DARKEN. Without the
        // cascade, the thumb would paint at TEXT_FOREGROUND unchanged.
        let expected =
            tokens::TEXT_FOREGROUND.mix(tokens::TEXT_FOREGROUND.darken(tokens::PRESS_DARKEN), 1.0);
        assert_eq!(
            (thumb_fill.r, thumb_fill.g, thumb_fill.b),
            (expected.r, expected.g, expected.b),
            "flagged thumb borrows the container's press envelope",
        );
    }

    #[test]
    fn drag_select_through_runtime_paints_band_in_next_frame() {
        // End-to-end: simulate pointer_down + pointer_moved on a
        // selectable paragraph, then drive a fresh `prepare_layout`
        // and verify the band is in the resulting DrawOps. Catches
        // regressions where the runtime's per-frame updates would
        // overwrite the live selection or where the painter doesn't
        // see the manager's writes.
        use crate::event::PointerButton;
        use crate::runtime::{PrepareTimings, RunnerCore};

        let mut core = RunnerCore::new();
        let mut tree = column([crate::widgets::text::paragraph("Hello, world!")
            .key("p")
            .selectable()])
        .padding(20.0);
        let viewport = Rect::new(0.0, 0.0, 400.0, 200.0);
        // First prepare_layout populates the selection_order, etc.
        let mut t = PrepareTimings::default();
        let _ = core.prepare_layout(&mut tree, viewport, 1.0, &mut t);
        // Snapshot so pointer events can hit-test against this frame.
        core.snapshot(&tree, &mut t);

        let p_rect = core.rect_of_key("p").expect("p rect");
        let cy = p_rect.y + p_rect.h * 0.5;
        let _ = core.pointer_down(p_rect.x + 4.0, cy, PointerButton::Primary);
        // Drag to extend.
        let _ = core.pointer_moved(p_rect.x + p_rect.w - 8.0, cy);

        // Selection in UiState must be a non-collapsed range now.
        let sel = &core.ui_state.current_selection;
        let r = sel.range.as_ref().expect("selection set");
        assert!(
            r.anchor.byte != r.head.byte,
            "drag should extend head past anchor (anchor={}, head={})",
            r.anchor.byte,
            r.head.byte
        );

        // Re-run prepare_layout (the per-frame loop). The painter
        // should emit a selection band Quad on this frame.
        let mut t2 = PrepareTimings::default();
        let (ops, _) = core.prepare_layout(&mut tree, viewport, 1.0, &mut t2);
        let bands: Vec<&DrawOp> = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::Quad { id, .. } if id.contains("selection-band")))
            .collect();
        assert!(
            !bands.is_empty(),
            "after drag-select, prepare_layout should emit a selection band Quad"
        );
        // Verify the band's painted rect overlaps the leaf's painted
        // rect — otherwise the highlight is rendered, but off-screen.
        if let DrawOp::Quad { rect, .. } = bands[0] {
            assert!(
                rect.intersect(p_rect).is_some(),
                "band rect = {rect:?} doesn't overlap leaf rect = {p_rect:?}"
            );
        }
    }

    #[test]
    fn selectable_leaf_paints_selection_band_when_key_matches_active_selection() {
        use crate::selection::{Selection, SelectionPoint, SelectionRange};

        let mut tree = column([crate::widgets::text::paragraph("Hello, world!")
            .key("p")
            .selectable()])
        .padding(20.0);
        let mut state = UiState::new();
        crate::layout::layout(&mut tree, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        // Pre-painter sanity: no current selection → no band.
        let ops_pre = draw_ops(&tree, &state);
        let bands_pre = ops_pre
            .iter()
            .filter(|op| matches!(op, DrawOp::Quad { id, .. } if id.contains("selection-band")))
            .count();
        assert_eq!(bands_pre, 0, "no band should paint when selection is empty");

        state.current_selection = Selection {
            range: Some(SelectionRange {
                anchor: SelectionPoint::new("p", 0),
                head: SelectionPoint::new("p", 5),
            }),
        };
        let ops = draw_ops(&tree, &state);
        let bands: Vec<&DrawOp> = ops
            .iter()
            .filter(|op| matches!(op, DrawOp::Quad { id, .. } if id.contains("selection-band")))
            .collect();
        assert!(
            !bands.is_empty(),
            "selection range over keyed selectable leaf should emit at least one band Quad"
        );
        if let DrawOp::Quad { rect, .. } = bands[0] {
            // Band must overlap the leaf's painted rect (positive area).
            assert!(rect.w > 0.0 && rect.h > 0.0, "band rect = {rect:?}");
        }
    }

    #[test]
    fn layout_only_focusable_container_does_not_synthesize_fill() {
        // The outer wrappers of `slider`, `switch`, and `resize_handle`
        // are `focusable` `stack(...)`s with no fill, no radius, and no
        // stroke — they exist purely to capture pointer/keyboard events
        // for the visible children below. Synthesizing a state fill
        // here would paint a translucent rectangle across the widget's
        // hit area on hover / press, competing with the actual thumb /
        // track / hairline. Gate the synthesis on the node having some
        // surface affordance of its own.
        let layout_only = El::new(Kind::Custom("slider")).focusable();
        assert!(layout_only.fill.is_none());
        assert_eq!(layout_only.radius, 0.0);
        assert!(layout_only.stroke.is_none());

        let (rest_fill, ..) = apply_state(&layout_only, InteractionState::Default, 0.0, 0.0);
        let (hover_fill, ..) = apply_state(&layout_only, InteractionState::Hover, 1.0, 0.0);
        let (press_fill, ..) = apply_state(&layout_only, InteractionState::Press, 1.0, 1.0);
        assert_eq!(rest_fill, None);
        assert_eq!(hover_fill, None);
        assert_eq!(press_fill, None);
    }

    #[test]
    fn solid_surface_keeps_envelope_mix_unchanged() {
        // Surfaces with a resting fill still go through the existing
        // lighten/darken envelope mix — the synthesized state fill only
        // kicks in when the resting fill is None.
        let solid = El::new(Kind::Custom("button")).fill(tokens::BG_MUTED);
        let (rest_fill, ..) = apply_state(&solid, InteractionState::Default, 0.0, 0.0);
        assert_eq!(rest_fill, Some(tokens::BG_MUTED));

        let (hover_fill, ..) = apply_state(&solid, InteractionState::Hover, 1.0, 0.0);
        assert_eq!(
            hover_fill,
            Some(tokens::BG_MUTED.mix(tokens::BG_MUTED.lighten(tokens::HOVER_LIGHTEN), 1.0)),
            "solid surfaces lighten existing fill, not synthesize a new one",
        );
    }

    #[test]
    fn clip_sets_scissor_on_descendant_ops() {
        let mut root = column([row([
            button("Inside").key("inside"),
            button("Too wide").key("outside").width(Size::Fixed(300.0)),
        ])
        .clip()
        .width(Size::Fixed(120.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 100.0));

        let ops = draw_ops(&root, &state);
        let clipped = ops
            .iter()
            .find(|op| op.id().contains("outside"))
            .expect("outside button op");
        let DrawOp::Quad { scissor, .. } = clipped else {
            panic!("expected button surface quad");
        };
        assert_eq!(*scissor, Some(Rect::new(0.0, 0.0, 120.0, 36.0)));
    }

    #[test]
    fn inline_text_bg_propagates_to_run_style() {
        // text_runs([..text("hit").background(...)..]) flows into the
        // Inlines collector and lands on the per-run RunStyle.bg of
        // the AttributedText draw op. Other runs keep `bg: None`.
        let highlight = Color::rgb(220, 200, 60);
        let mut root = crate::text_runs([
            crate::text("plain "),
            crate::text("marked").background(highlight),
            crate::text(" rest"),
        ]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 320.0, 80.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::AttributedText { runs, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::AttributedText { .. }))
            .expect("attr op")
        else {
            unreachable!()
        };
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].1.bg, None);
        assert_eq!(runs[1].1.bg, Some(highlight));
        assert_eq!(runs[2].1.bg, None);
    }

    #[test]
    fn text_align_center_emits_middle_anchor() {
        let mut root = crate::text("Centered").center_text();
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 80.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::GlyphRun { anchor, .. } = &ops[0] else {
            panic!("expected glyph run");
        };
        assert_eq!(*anchor, TextAnchor::Middle);
    }

    #[test]
    fn paragraph_emits_wrapped_glyph_run() {
        let mut root = crate::paragraph("This sentence should wrap in a narrow box.")
            .width(Size::Fixed(120.0));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 120.0, 120.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::GlyphRun { wrap, .. } = &ops[0] else {
            panic!("expected glyph run");
        };
        assert_eq!(*wrap, TextWrap::Wrap);
    }

    #[test]
    fn padding_on_text_node_insets_glyph_rect() {
        // Regression: `text("X").padding(...)` used to inflate the
        // node's intrinsic size but emit the GlyphRun against the full
        // (uninset) layout rect, so glyphs anchored at the parent's
        // edge whenever Stretch flattened the Hug width. The fix is
        // for draw_ops to inset the glyph rect by `node.padding`,
        // making text padding behave the same as container padding.
        let mut root = column([crate::text("Chat").padding(Sides::xy(12.0, 8.0))])
            .width(Size::Fixed(320.0))
            .height(Size::Fill(1.0));
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 320.0, 600.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::GlyphRun { rect, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::GlyphRun { .. }))
            .expect("text node emits a glyph run")
        else {
            unreachable!()
        };
        // Column stretched the text element to 320×(text_height + 16);
        // the glyph rect should be inset by the padding on each side.
        assert!(
            (rect.x - 12.0).abs() < 1e-3,
            "glyph rect.x = {}, expected 12 (left padding)",
            rect.x,
        );
        assert!(
            (rect.w - (320.0 - 24.0)).abs() < 1e-3,
            "glyph rect.w = {}, expected 296 (320 minus 12+12)",
            rect.w,
        );
        assert!(
            (rect.y - 8.0).abs() < 1e-3,
            "glyph rect.y = {}, expected 8 (top padding)",
            rect.y,
        );
    }

    #[test]
    fn padding_on_icon_node_insets_icon_rect() {
        // Same fix applies to icon nodes: the centered icon should
        // center in the inset rect, not the full layout rect. Override
        // the Fixed width that `icon_size(...)` sets so the padding
        // has room — without the override, padding(20) on a 16-wide
        // element would produce a negative inset.
        let mut root = column([crate::icon(IconName::Folder)
            .icon_size(16.0)
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(40.0))
            .padding(Sides::xy(20.0, 0.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));

        let ops = draw_ops(&root, &state);
        let DrawOp::Icon { rect, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Icon { .. }))
            .expect("icon node emits an icon op")
        else {
            unreachable!()
        };
        // Element 80×40, inner after Sides::xy(20, 0) → (20, 0, 40, 40),
        // inner.center_x() = 40, 16px icon → x = 32.
        assert!(
            (rect.x - 32.0).abs() < 1e-3,
            "icon rect.x = {}, expected 32 (centered in inset rect)",
            rect.x,
        );
    }

    #[test]
    fn image_intrinsic_is_natural_pixel_size() {
        let pixels = vec![0u8; 80 * 40 * 4];
        let img = crate::image::Image::from_rgba8(80, 40, pixels);
        let el = crate::tree::image(img);
        let (w, h) = crate::layout::intrinsic(&el);
        assert!((w - 80.0).abs() < 1e-3, "intrinsic w = {w}");
        assert!((h - 40.0).abs() < 1e-3, "intrinsic h = {h}");
    }

    #[test]
    fn image_emits_draw_op_with_fit_projection() {
        // 100×50 image into a 400×400 box with Cover: dest = 800×400.
        let pixels = vec![0u8; 100 * 50 * 4];
        let img = crate::image::Image::from_rgba8(100, 50, pixels);
        let mut root = crate::row([crate::tree::image(img)
            .image_fit(crate::image::ImageFit::Cover)
            .width(Size::Fixed(400.0))
            .height(Size::Fixed(400.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 600.0, 600.0));
        let ops = draw_ops(&root, &state);
        let img_op = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Image { .. }))
            .expect("image El emits a DrawOp::Image");
        let DrawOp::Image {
            rect, scissor, fit, ..
        } = img_op
        else {
            unreachable!()
        };
        assert_eq!(*fit, crate::image::ImageFit::Cover);
        // Cover scale = max(400/100, 400/50) = 8 → 800×400 dest.
        assert!((rect.w - 800.0).abs() < 1e-3, "rect.w = {}", rect.w);
        assert!((rect.h - 400.0).abs() < 1e-3, "rect.h = {}", rect.h);
        // Scissor clamps to content (400×400 box) so the horizontal
        // overflow is cropped without an explicit `.clip()`.
        let s = scissor.expect("image draw op carries a scissor");
        assert!((s.w - 400.0).abs() < 1e-3, "scissor.w = {}", s.w);
        assert!((s.h - 400.0).abs() < 1e-3, "scissor.h = {}", s.h);
    }

    #[test]
    fn image_tint_propagates_with_opacity() {
        let pixels = vec![0u8; 4 * 4 * 4];
        let img = crate::image::Image::from_rgba8(4, 4, pixels);
        let mut root = crate::tree::image(img)
            .image_tint(Color::rgb(200, 100, 50))
            .opacity(0.5);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::Image { tint, .. } = ops
            .iter()
            .find(|op| matches!(op, DrawOp::Image { .. }))
            .expect("image emits draw op")
        else {
            unreachable!()
        };
        let tint = tint.expect("image_tint set, draw op carries tint");
        // Opacity halves the alpha channel of the tint (255 → 128).
        assert_eq!(tint.a, 128, "tint.a after 0.5 opacity = {}", tint.a);
        assert_eq!((tint.r, tint.g, tint.b), (200, 100, 50));
    }

    #[test]
    fn opacity_multiplies_alpha_on_quad_uniforms() {
        let mut root = button("X")
            .fill(Color::rgba(200, 100, 50, 200))
            .opacity(0.5);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let ops = draw_ops(&root, &state);
        let DrawOp::Quad { uniforms, .. } = &ops[0] else {
            panic!("expected quad op");
        };
        let UniformValue::Color(c) = uniforms.get("fill").expect("fill") else {
            panic!("fill should be a colour");
        };
        // 200 * 0.5 = 100
        assert_eq!(c.a, 100, "alpha should be halved by opacity 0.5");
    }

    #[test]
    fn theme_can_route_implicit_surfaces_to_custom_shader() {
        let mut root = button("X").primary();
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));

        let theme = Theme::default()
            .with_surface_shader("xp_surface")
            .with_surface_uniform("theme_strength", UniformValue::F32(0.75));
        let ops = draw_ops_with_theme(&root, &state, &theme);
        let DrawOp::Quad {
            shader, uniforms, ..
        } = &ops[0]
        else {
            panic!("expected themed surface quad");
        };

        assert_eq!(*shader, ShaderHandle::Custom("xp_surface"));
        assert_eq!(
            uniforms.get("theme_strength"),
            Some(&UniformValue::F32(0.75))
        );
        assert!(
            matches!(uniforms.get("fill"), Some(UniformValue::Color(_))),
            "familiar rounded-rect uniforms should stay available for manifests"
        );
        assert!(
            matches!(uniforms.get("vec_a"), Some(UniformValue::Color(_))),
            "custom surface shaders should also receive packed instance slots"
        );
        assert_eq!(
            uniforms.get("vec_c"),
            Some(&UniformValue::Vec4([
                1.0,
                tokens::RADIUS_MD,
                tokens::SHADOW_SM * 0.5,
                0.0
            ]))
        );
    }

    #[test]
    fn theme_can_route_surface_role_to_custom_shader() {
        let mut root = crate::card("Panel", [crate::text("Body")])
            .surface_role(SurfaceRole::Popover)
            .key("panel");
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 240.0, 120.0));

        let theme = Theme::default()
            .with_role_shader(SurfaceRole::Popover, "popover_surface")
            .with_role_uniform(SurfaceRole::Popover, "elevation", UniformValue::F32(2.0));
        let ops = draw_ops_with_theme(&root, &state, &theme);
        let DrawOp::Quad {
            shader, uniforms, ..
        } = &ops[0]
        else {
            panic!("expected themed surface quad");
        };

        assert_eq!(*shader, ShaderHandle::Custom("popover_surface"));
        assert_eq!(uniforms.get("elevation"), Some(&UniformValue::F32(2.0)));
        assert_eq!(
            uniforms.get("surface_role"),
            Some(&UniformValue::F32(SurfaceRole::Popover.uniform_id()))
        );
        assert!(
            matches!(uniforms.get("vec_a"), Some(UniformValue::Color(_))),
            "role-routed custom shaders should receive packed rect slots"
        );
        assert_eq!(
            uniforms.get("vec_c"),
            Some(&UniformValue::Vec4([
                1.0,
                tokens::RADIUS_LG,
                tokens::SHADOW_LG,
                0.0
            ]))
        );
    }

    #[test]
    fn translate_offsets_paint_rect_and_inherits_to_children() {
        // Parent translate of (50, 30) should land child rects at
        // child.computed + (50, 30). The button widget uses
        // `paint_overflow` for its focus ring, which grows the painted
        // rect outward — so we compare against the `inner_rect` uniform
        // (the post-translate layout rect) rather than the raw quad rect.
        let mut root = column([button("X").key("x")]).translate(50.0, 30.0);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
        let inner = inner_rect_quad_for(&root, &state, "x").expect("x quad inner_rect");
        let untranslated = find_computed(&root, &state, "x").expect("x computed");

        assert!((inner.x - (untranslated.x + 50.0)).abs() < 0.5);
        assert!((inner.y - (untranslated.y + 30.0)).abs() < 0.5);
    }

    #[test]
    fn scale_scales_rect_around_center() {
        let mut root = column([button("X").key("x").scale(2.0).width(Size::Fixed(40.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 100.0));
        let pre = find_computed(&root, &state, "x").expect("computed");
        let post = inner_rect_quad_for(&root, &state, "x").expect("painted inner_rect");

        // 2x scale around centre: w doubles, x shifts left by w/2.
        assert!((post.w - pre.w * 2.0).abs() < 0.5);
        assert!((post.h - pre.h * 2.0).abs() < 0.5);
        let pre_cx = pre.center_x();
        let post_cx = post.center_x();
        assert!(
            (pre_cx - post_cx).abs() < 0.5,
            "centre should be preserved by scale-around-centre",
        );
    }

    #[test]
    fn shadow_auto_expands_painted_rect_around_inner_rect() {
        // `.shadow(s)` should auto-widen the painted quad without the
        // widget needing to set `paint_overflow` — the shader needs the
        // halo room to draw the soft band outside the layout rect.
        // No surface_role here so the El's shadow value reaches the
        // shader unchanged and we can assert the exact halo geometry.
        let mut root = column([El::new(Kind::Group)
            .key("c")
            .fill(tokens::BG_CARD)
            .radius(tokens::RADIUS_LG)
            .shadow(tokens::SHADOW_MD)
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(40.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let ops = draw_ops(&root, &state);
        let (painted, inner) = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Quad {
                    id, rect, uniforms, ..
                } if id.contains("c") => {
                    let UniformValue::Vec4(v) = uniforms.get("inner_rect")? else {
                        return None;
                    };
                    Some((*rect, Rect::new(v[0], v[1], v[2], v[3])))
                }
                _ => None,
            })
            .expect("shadowed quad with inner_rect");

        // SHADOW_MD (== 12) → l=12, r=12, t=6, b=18.
        let blur = tokens::SHADOW_MD;
        assert!(
            (inner.x - painted.x - blur).abs() < 0.5,
            "left halo == blur, painted.x={}, inner.x={}",
            painted.x,
            inner.x,
        );
        assert!(
            (painted.right() - inner.right() - blur).abs() < 0.5,
            "right halo == blur",
        );
        assert!(
            (inner.y - painted.y - blur * 0.5).abs() < 0.5,
            "top halo == blur * 0.5",
        );
        assert!(
            (painted.bottom() - inner.bottom() - blur * 1.5).abs() < 0.5,
            "bottom halo == blur * 1.5",
        );
    }

    #[test]
    fn shadow_overflow_takes_per_side_max_with_explicit_paint_overflow() {
        // A focus-style outset of 8 on every side combined with
        // SHADOW_MD (12) should resolve to: l=12, r=12, t=8, b=18 —
        // shadow wins on left/right/bottom, paint_overflow wins on top.
        let combined =
            super::combined_overflow(crate::tree::Sides::all(8.0), tokens::SHADOW_MD, 0.0);
        assert!((combined.left - 12.0).abs() < f32::EPSILON);
        assert!((combined.right - 12.0).abs() < f32::EPSILON);
        assert!((combined.top - 8.0).abs() < f32::EPSILON);
        assert!((combined.bottom - 18.0).abs() < f32::EPSILON);
    }

    #[test]
    fn shadow_overflow_is_zero_when_shadow_is_zero() {
        let combined = super::combined_overflow(crate::tree::Sides::zero(), 0.0, 0.0);
        assert_eq!(combined, crate::tree::Sides::zero());
    }

    #[test]
    fn stroke_overflow_outsets_painted_rect_by_half_width_plus_aa_tail() {
        // Stroke straddles the boundary; without any outset, cardinal
        // pixels of curved boundaries (radio indicator, switch thumb)
        // get clipped because the outside half of the band falls
        // outside the layout rect. Auto-widen by stroke_width/2 + 1px
        // (AA tail) so the full band rasterises symmetrically.
        let combined = super::combined_overflow(crate::tree::Sides::zero(), 0.0, 1.0);
        let halo = 1.0 * 0.5 + 1.0;
        assert!((combined.left - halo).abs() < f32::EPSILON);
        assert!((combined.right - halo).abs() < f32::EPSILON);
        assert!((combined.top - halo).abs() < f32::EPSILON);
        assert!((combined.bottom - halo).abs() < f32::EPSILON);
    }

    #[test]
    fn stroke_and_shadow_take_per_side_max() {
        // Shadow's bottom halo (blur*1.5 = 18) beats stroke (1.5) on
        // the bottom; stroke beats shadow on the top (blur*0.5 = 6 vs
        // 1.5? no — shadow wins there too). Use a small shadow so the
        // stroke halo wins on the top.
        let combined = super::combined_overflow(crate::tree::Sides::zero(), 1.0, 4.0);
        // Stroke halo = 4*0.5 + 1 = 3. Shadow blur = 1 → top = 0.5,
        // bottom = 1.5, l/r = 1. Stroke wins on every side.
        assert!(
            (combined.top - 3.0).abs() < f32::EPSILON,
            "top = {}",
            combined.top
        );
        assert!((combined.left - 3.0).abs() < f32::EPSILON);
        assert!((combined.right - 3.0).abs() < f32::EPSILON);
        // Bottom: max(stroke=3, shadow*1.5=1.5) → stroke wins.
        assert!((combined.bottom - 3.0).abs() < f32::EPSILON);
    }

    #[test]
    fn stroked_indicator_painted_rect_outsets_layout_rect() {
        // Regression for the radio indicator: a small stroked circle
        // looked flattened at the cardinal directions because its
        // painted quad equalled the 16×16 layout rect, clipping the
        // outside half of the stroke band on the top/bottom/left/right.
        // After the fix, the quad outsets by stroke_width/2 + 1 on each
        // side so the AA tail rasterises cleanly.
        let mut root = column([El::new(Kind::Custom("radio-indicator"))
            .key("indicator")
            .width(Size::Fixed(16.0))
            .height(Size::Fixed(16.0))
            .radius(tokens::RADIUS_PILL)
            .fill(tokens::BG_CARD)
            .stroke(tokens::BORDER_STRONG)]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 100.0, 100.0));

        let ops = draw_ops(&root, &state);
        let (painted, inner) = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Quad {
                    id, rect, uniforms, ..
                } if id.contains("indicator") => {
                    let UniformValue::Vec4(v) = uniforms.get("inner_rect")? else {
                        return None;
                    };
                    Some((*rect, Rect::new(v[0], v[1], v[2], v[3])))
                }
                _ => None,
            })
            .expect("stroked indicator quad with inner_rect");

        // stroke_width default = 1 → halo = 0.5 + 1 = 1.5 on each side.
        let halo = 1.5;
        assert!(
            (inner.x - painted.x - halo).abs() < 1e-3,
            "left halo, painted.x={}, inner.x={}",
            painted.x,
            inner.x,
        );
        assert!(
            (painted.right() - inner.right() - halo).abs() < 1e-3,
            "right halo",
        );
        assert!((inner.y - painted.y - halo).abs() < 1e-3, "top halo",);
        assert!(
            (painted.bottom() - inner.bottom() - halo).abs() < 1e-3,
            "bottom halo",
        );
        // Layout rect itself is unchanged — only the painted quad
        // grows; the SDF still anchors to the original 16×16 box.
        assert!((inner.w - 16.0).abs() < 1e-3);
        assert!((inner.h - 16.0).abs() < 1e-3);
    }

    #[test]
    fn shadow_uniform_is_set_when_n_shadow_is_nonzero() {
        let mut root = column([El::new(Kind::Group)
            .key("c")
            .fill(tokens::BG_CARD)
            .radius(tokens::RADIUS_LG)
            .shadow(tokens::SHADOW_MD)
            .width(Size::Fixed(80.0))
            .height(Size::Fixed(40.0))]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let ops = draw_ops(&root, &state);
        let uniforms = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Quad { id, uniforms, .. } if id.contains("c") => Some(uniforms.clone()),
                _ => None,
            })
            .expect("shadowed quad");
        assert_eq!(
            uniforms.get("shadow"),
            Some(&UniformValue::F32(tokens::SHADOW_MD)),
            ".shadow(SHADOW_MD) on a node without surface_role must reach the shader unchanged",
        );
    }

    #[test]
    fn theme_role_override_propagates_to_painted_rect() {
        // The card widget binds SurfaceRole::Panel, which forces the
        // shadow uniform to SHADOW_SM regardless of the El's own
        // `.shadow(SHADOW_MD)` setting. The painted rect should track
        // the *effective* shadow (SM = 4), not the larger MD the
        // builder requested — over-expanding wastes overdraw budget.
        let mut root = column([crate::card("Card", [crate::text("Body")]).key("c")]);
        let mut state = UiState::new();
        crate::layout::layout(&mut root, &mut state, Rect::new(0.0, 0.0, 200.0, 200.0));
        let ops = draw_ops(&root, &state);
        let (painted, inner) = ops
            .iter()
            .find_map(|op| match op {
                DrawOp::Quad {
                    id, rect, uniforms, ..
                } if id.contains("c") => {
                    let UniformValue::Vec4(v) = uniforms.get("inner_rect")? else {
                        return None;
                    };
                    Some((*rect, Rect::new(v[0], v[1], v[2], v[3])))
                }
                _ => None,
            })
            .expect("card quad with inner_rect");

        let blur = tokens::SHADOW_SM;
        assert!(
            (inner.x - painted.x - blur).abs() < 0.5,
            "left halo == effective (theme-resolved) shadow, painted.x={}, inner.x={}",
            painted.x,
            inner.x,
        );
        assert!(
            (painted.bottom() - inner.bottom() - blur * 1.5).abs() < 0.5,
            "bottom halo == effective shadow * 1.5",
        );
    }

    /// Read the painted layout rect (== quad's `inner_rect` uniform) for
    /// the first quad whose id contains `key`. Falls back to the quad's
    /// `rect` for shaders that don't carry an `inner_rect` uniform.
    fn inner_rect_quad_for(root: &El, ui_state: &UiState, key: &str) -> Option<Rect> {
        use crate::shader::UniformValue;
        let ops = draw_ops(root, ui_state);
        for op in ops {
            if let DrawOp::Quad {
                id, rect, uniforms, ..
            } = op
                && id.contains(key)
            {
                if let Some(UniformValue::Vec4(v)) = uniforms.get("inner_rect") {
                    return Some(Rect::new(v[0], v[1], v[2], v[3]));
                }
                return Some(rect);
            }
        }
        None
    }

    fn find_computed(node: &El, ui_state: &UiState, key: &str) -> Option<Rect> {
        if node.key.as_deref() == Some(key) {
            return Some(ui_state.rect(&node.computed_id));
        }
        node.children
            .iter()
            .find_map(|c| find_computed(c, ui_state, key))
    }
}
