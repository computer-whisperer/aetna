//! Native math expression IR and box layout.
//!
//! This module is intentionally presentation-oriented. It is shaped like
//! MathML Core because that is the interchange target Aetna wants to accept,
//! but layout lowers into TeX-style boxes: width, ascent, descent, and a flat
//! list of positioned glyph/rule atoms.

use std::sync::Arc;

use crate::text::metrics as text_metrics;
use crate::tree::{Color, FontFamily, FontWeight, Rect, TextWrap};

const DEFAULT_RULE_THICKNESS: f32 = 1.1;
const SCRIPT_SCALE: f32 = 0.72;
const LARGE_OPERATOR_SCALE: f32 = 1.35;
const FRACTION_PAD_EM: f32 = 0.18;
const FRACTION_GAP_EM: f32 = 0.18;
const SQRT_GAP_EM: f32 = 0.10;
const TABLE_COL_GAP_EM: f32 = 0.8;
const TABLE_ROW_GAP_EM: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MathDisplay {
    #[default]
    Inline,
    Block,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum MathExpr {
    Row(Vec<MathExpr>),
    Identifier(String),
    Number(String),
    Operator(String),
    Text(String),
    Space(f32),
    Fraction {
        numerator: Arc<MathExpr>,
        denominator: Arc<MathExpr>,
    },
    Sqrt(Arc<MathExpr>),
    Root {
        base: Arc<MathExpr>,
        index: Arc<MathExpr>,
    },
    Scripts {
        base: Arc<MathExpr>,
        sub: Option<Arc<MathExpr>>,
        sup: Option<Arc<MathExpr>>,
    },
    UnderOver {
        base: Arc<MathExpr>,
        under: Option<Arc<MathExpr>>,
        over: Option<Arc<MathExpr>>,
    },
    Fenced {
        open: Option<String>,
        close: Option<String>,
        body: Arc<MathExpr>,
    },
    Table {
        rows: Vec<Vec<MathExpr>>,
    },
    Error(String),
}

impl MathExpr {
    pub fn row(children: impl IntoIterator<Item = MathExpr>) -> Self {
        let mut children: Vec<MathExpr> = children.into_iter().collect();
        match children.len() {
            0 => MathExpr::Row(Vec::new()),
            1 => children.pop().unwrap(),
            _ => MathExpr::Row(children),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MathLayout {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub atoms: Vec<MathAtom>,
}

impl MathLayout {
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MathAtom {
    Glyph {
        text: String,
        x: f32,
        y_baseline: f32,
        size: f32,
        weight: FontWeight,
        italic: bool,
    },
    Rule {
        rect: Rect,
    },
    Radical {
        points: [[f32; 2]; 5],
        thickness: f32,
    },
    Delimiter {
        delimiter: String,
        rect: Rect,
        thickness: f32,
    },
}

#[derive(Clone, Copy, Debug)]
struct LayoutCtx {
    size: f32,
    display: MathDisplay,
}

impl LayoutCtx {
    fn script(self) -> Self {
        Self {
            size: self.metrics().script_size(),
            display: MathDisplay::Inline,
        }
    }

    fn large_operator(self) -> Self {
        Self {
            size: self.metrics().large_operator_size(),
            display: self.display,
        }
    }

    fn metrics(self) -> MathMetrics {
        MathMetrics {
            size: self.size,
            display: self.display,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MathMetrics {
    size: f32,
    display: MathDisplay,
}

impl MathMetrics {
    fn font_constants(self) -> Option<OpenTypeMathConstants> {
        open_type_math_constants()
    }

    fn script_size(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.script_scale(self.size))
            .unwrap_or(self.size * SCRIPT_SCALE)
            .max(6.0)
    }

    fn large_operator_size(self) -> f32 {
        self.size * LARGE_OPERATOR_SCALE
    }

    fn rule_thickness(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.fraction_rule_thickness(self.size))
            .unwrap_or(DEFAULT_RULE_THICKNESS * self.size / 16.0)
            .max(0.75)
    }

    fn radical_rule_thickness(self) -> f32 {
        self.font_constants()
            .and_then(|constants| constants.radical_rule_thickness(self.size))
            .unwrap_or_else(|| self.rule_thickness())
            .max(0.75)
    }

    fn default_ascent(self) -> f32 {
        self.size * 0.75
    }

    fn default_descent(self) -> f32 {
        self.size * 0.25
    }

    fn glyph_ascent(self) -> f32 {
        self.size * 0.82
    }

    fn glyph_descent(self) -> f32 {
        self.size * 0.22
    }

    fn space_width(self, em: f32) -> f32 {
        self.size * em
    }

    fn operator_side_bearing(self, operator: &str) -> f32 {
        match operator {
            "+" | "-" | "=" | "<" | ">" | "≤" | "≥" | "≠" | "×" | "÷" | "→" | "←" | "↔" | "∪"
            | "∩" | "⋃" | "⋂" => self.size * 0.18,
            "∑" | "∏" | "∫" => self.size * 0.12,
            "," | "." | ";" | ":" => self.size * 0.08,
            _ => 0.0,
        }
    }

    fn fraction_pad(self) -> f32 {
        self.size
            * if matches!(self.display, MathDisplay::Block) {
                FRACTION_PAD_EM
            } else {
                FRACTION_PAD_EM * 0.65
            }
    }

    fn fraction_gap(self) -> f32 {
        self.size
            * if matches!(self.display, MathDisplay::Block) {
                FRACTION_GAP_EM
            } else {
                FRACTION_GAP_EM * 0.55
            }
    }

    fn fraction_axis_shift(self) -> f32 {
        self.size
            * if matches!(self.display, MathDisplay::Block) {
                0.18
            } else {
                0.28
            }
    }

    fn sqrt_gap(self) -> f32 {
        self.size * SQRT_GAP_EM
    }

    fn radical_width(self) -> f32 {
        self.size * 0.72
    }

    fn radical_left_flair_y(self) -> f32 {
        -self.size * 0.03
    }

    fn radical_hook_x(self) -> f32 {
        self.size * 0.12
    }

    fn radical_hook_y(self) -> f32 {
        -self.size * 0.1
    }

    fn radical_tick_x(self) -> f32 {
        self.size * 0.24
    }

    fn radical_tick_y(self, inner_descent: f32) -> f32 {
        (inner_descent * 0.75).max(self.size * 0.13)
    }

    fn root_offset_x(self, index_width: f32) -> f32 {
        index_width * 0.55
    }

    fn root_index_shift(self, root_ascent: f32) -> f32 {
        -root_ascent * 0.52
    }

    fn script_gap(self) -> f32 {
        self.size * 0.06
    }

    fn superscript_shift(self, base_ascent: f32, sup_descent: f32) -> f32 {
        -(base_ascent * 0.58).max(sup_descent + self.size * 0.18)
    }

    fn subscript_shift(self, base_descent: f32, sub_ascent: f32) -> f32 {
        (base_descent + sub_ascent * 0.72).max(self.size * 0.28)
    }

    fn under_over_gap(self) -> f32 {
        self.size * 0.12
    }

    fn table_col_gap(self) -> f32 {
        self.size * TABLE_COL_GAP_EM
    }

    fn table_row_gap(self) -> f32 {
        self.size * TABLE_ROW_GAP_EM
    }

    fn delimiter_gap(self) -> f32 {
        self.size * 0.08
    }

    fn delimiter_overshoot(self) -> f32 {
        (self.size * 0.08).max(self.rule_thickness())
    }

    fn delimiter_width(self) -> f32 {
        self.size * 0.42
    }
}

#[derive(Clone, Copy, Debug)]
struct OpenTypeMathConstants {
    units_per_em: f32,
    script_percent_scale_down: i16,
    fraction_rule_thickness: i16,
    radical_rule_thickness: i16,
}

impl OpenTypeMathConstants {
    fn font_units(self, value: i16, size: f32) -> Option<f32> {
        (value > 0 && self.units_per_em > 0.0).then(|| value as f32 / self.units_per_em * size)
    }

    fn script_scale(self, size: f32) -> Option<f32> {
        (self.script_percent_scale_down > 0)
            .then(|| size * self.script_percent_scale_down as f32 / 100.0)
    }

    fn fraction_rule_thickness(self, size: f32) -> Option<f32> {
        self.font_units(self.fraction_rule_thickness, size)
    }

    fn radical_rule_thickness(self, size: f32) -> Option<f32> {
        self.font_units(self.radical_rule_thickness, size)
    }
}

fn open_type_math_constants() -> Option<OpenTypeMathConstants> {
    #[cfg(feature = "symbols")]
    {
        static CONSTANTS: std::sync::OnceLock<Option<OpenTypeMathConstants>> =
            std::sync::OnceLock::new();
        *CONSTANTS
            .get_or_init(|| parse_open_type_math_constants(aetna_fonts::NOTO_SANS_MATH_REGULAR))
    }
    #[cfg(not(feature = "symbols"))]
    {
        None
    }
}

#[cfg(feature = "symbols")]
fn parse_open_type_math_constants(font: &[u8]) -> Option<OpenTypeMathConstants> {
    let face = ttf_parser::Face::parse(font, 0).ok()?;
    let constants = face.tables().math?.constants?;
    Some(OpenTypeMathConstants {
        units_per_em: face.units_per_em() as f32,
        script_percent_scale_down: constants.script_percent_scale_down(),
        fraction_rule_thickness: constants.fraction_rule_thickness().value,
        radical_rule_thickness: constants.radical_rule_thickness().value,
    })
}

pub fn layout_math(expr: &MathExpr, size: f32, display: MathDisplay) -> MathLayout {
    layout_expr(expr, LayoutCtx { size, display })
}

fn layout_expr(expr: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    match expr {
        MathExpr::Row(children) => layout_row(children, ctx),
        MathExpr::Identifier(s) => layout_glyph(s, ctx, FontWeight::Regular, true),
        MathExpr::Number(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
        MathExpr::Operator(s) => layout_operator(s, ctx),
        MathExpr::Text(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
        MathExpr::Space(em) => MathLayout {
            width: metrics.space_width(*em),
            ascent: metrics.default_ascent(),
            descent: metrics.default_descent(),
            atoms: Vec::new(),
        },
        MathExpr::Fraction {
            numerator,
            denominator,
        } => layout_fraction(numerator, denominator, ctx),
        MathExpr::Sqrt(child) => layout_sqrt(child, ctx),
        MathExpr::Root { base, index } => layout_root(base, index, ctx),
        MathExpr::Scripts { base, sub, sup } => {
            layout_scripts(base, sub.as_deref(), sup.as_deref(), ctx)
        }
        MathExpr::UnderOver { base, under, over } => {
            layout_under_over(base, under.as_deref(), over.as_deref(), ctx)
        }
        MathExpr::Fenced { open, close, body } => layout_fenced(open, close, body, ctx),
        MathExpr::Table { rows } => layout_table(rows, ctx),
        MathExpr::Error(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
    }
}

fn layout_row(children: &[MathExpr], ctx: LayoutCtx) -> MathLayout {
    let mut width = 0.0;
    let metrics = ctx.metrics();
    let mut ascent: f32 = metrics.default_ascent();
    let mut descent: f32 = metrics.default_descent();
    let mut atoms = Vec::new();
    for child in children {
        let child_layout = layout_expr(child, ctx);
        translate_atoms(&mut atoms, child_layout.atoms, width, 0.0);
        width += child_layout.width;
        ascent = ascent.max(child_layout.ascent);
        descent = descent.max(child_layout.descent);
    }
    MathLayout {
        width,
        ascent,
        descent,
        atoms,
    }
}

fn layout_glyph(s: &str, ctx: LayoutCtx, weight: FontWeight, italic: bool) -> MathLayout {
    if s.is_empty() {
        return MathLayout {
            width: 0.0,
            ascent: 0.0,
            descent: 0.0,
            atoms: Vec::new(),
        };
    }
    let measured = text_metrics::measure_text(s, ctx.size, weight, false, TextWrap::NoWrap, None);
    MathLayout {
        width: measured.width,
        ascent: ctx.metrics().glyph_ascent(),
        descent: ctx.metrics().glyph_descent(),
        atoms: vec![MathAtom::Glyph {
            text: s.to_string(),
            x: 0.0,
            y_baseline: 0.0,
            size: ctx.size,
            weight,
            italic,
        }],
    }
}

fn layout_operator(s: &str, ctx: LayoutCtx) -> MathLayout {
    let mut layout = layout_glyph(s, ctx, FontWeight::Regular, false);
    let side = ctx.metrics().operator_side_bearing(s);
    if side > 0.0 {
        for atom in &mut layout.atoms {
            if let MathAtom::Glyph { x, .. } = atom {
                *x += side;
            }
        }
        layout.width += side * 2.0;
    }
    layout
}

fn layout_fraction(numerator: &MathExpr, denominator: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    let child_ctx = if matches!(ctx.display, MathDisplay::Block) {
        ctx
    } else {
        ctx.script()
    };
    let num = layout_expr(numerator, child_ctx);
    let den = layout_expr(denominator, child_ctx);
    let pad = metrics.fraction_pad();
    let gap = metrics.fraction_gap();
    let rule = metrics.rule_thickness();
    // The math axis sits above the prose baseline. Keeping the fraction
    // rule on that axis makes inline fractions read as part of the line
    // instead of hanging mostly below it.
    let axis_shift = metrics.fraction_axis_shift();
    let rule_center_y = -axis_shift;
    let width = num.width.max(den.width) + pad * 2.0;
    let num_x = (width - num.width) * 0.5;
    let den_x = (width - den.width) * 0.5;
    let num_dy = rule_center_y - gap - rule * 0.5 - num.descent;
    let den_dy = rule_center_y + gap + rule * 0.5 + den.ascent;
    let ascent = -num_dy + num.ascent;
    let descent = den_dy + den.descent;
    let mut atoms = Vec::new();
    translate_atoms(&mut atoms, num.atoms, num_x, num_dy);
    atoms.push(MathAtom::Rule {
        rect: Rect::new(0.0, rule_center_y - rule * 0.5, width, rule),
    });
    translate_atoms(&mut atoms, den.atoms, den_x, den_dy);
    MathLayout {
        width,
        ascent,
        descent,
        atoms,
    }
}

fn layout_sqrt(child: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    let inner = layout_expr(child, ctx);
    let gap = metrics.sqrt_gap();
    let rule = metrics.radical_rule_thickness();
    let radical_w = metrics.radical_width();
    let inner_x = radical_w + gap;
    let bar_y = -inner.ascent - gap - rule * 0.5;
    let tick_y = metrics.radical_tick_y(inner.descent);
    let end_x = inner_x + inner.width;
    let mut atoms = Vec::new();
    atoms.push(MathAtom::Radical {
        points: [
            [0.0, metrics.radical_left_flair_y()],
            [metrics.radical_hook_x(), metrics.radical_hook_y()],
            [metrics.radical_tick_x(), tick_y],
            [radical_w, bar_y],
            [end_x, bar_y],
        ],
        thickness: rule,
    });
    translate_atoms(&mut atoms, inner.atoms, inner_x, 0.0);
    MathLayout {
        width: end_x,
        ascent: -bar_y + rule * 0.5,
        descent: tick_y + rule * 0.5,
        atoms,
    }
}

fn layout_root(base: &MathExpr, index: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let metrics = ctx.metrics();
    let root = layout_sqrt(base, ctx);
    let index = layout_expr(index, ctx.script());
    let root_x = metrics.root_offset_x(index.width);
    let index_dy = metrics.root_index_shift(root.ascent);
    let mut atoms = Vec::new();
    translate_atoms(&mut atoms, index.atoms, 0.0, index_dy);
    translate_atoms(&mut atoms, root.atoms, root_x, 0.0);
    MathLayout {
        width: root_x + root.width,
        ascent: root.ascent.max(-index_dy + index.ascent),
        descent: root.descent.max(index_dy + index.descent),
        atoms,
    }
}

fn layout_scripts(
    base: &MathExpr,
    sub: Option<&MathExpr>,
    sup: Option<&MathExpr>,
    ctx: LayoutCtx,
) -> MathLayout {
    if matches!(ctx.display, MathDisplay::Block) && is_display_limits_base(base) {
        return layout_under_over(base, sub, sup, ctx);
    }
    let base_layout = layout_expr(base, ctx);
    let script_ctx = ctx.script();
    let sub_layout = sub.map(|expr| layout_expr(expr, script_ctx));
    let sup_layout = sup.map(|expr| layout_expr(expr, script_ctx));
    let metrics = ctx.metrics();
    let script_gap = metrics.script_gap();
    let script_x = base_layout.width + script_gap;
    let sup_dy = sup_layout
        .as_ref()
        .map(|sup| metrics.superscript_shift(base_layout.ascent, sup.descent))
        .unwrap_or(0.0);
    let sub_dy = sub_layout
        .as_ref()
        .map(|sub| metrics.subscript_shift(base_layout.descent, sub.ascent))
        .unwrap_or(0.0);
    let mut atoms = Vec::new();
    translate_atoms(&mut atoms, base_layout.atoms, 0.0, 0.0);
    let mut script_width: f32 = 0.0;
    let mut ascent = base_layout.ascent;
    let mut descent = base_layout.descent;
    if let Some(sup) = sup_layout {
        script_width = script_width.max(sup.width);
        ascent = ascent.max(-sup_dy + sup.ascent);
        translate_atoms(&mut atoms, sup.atoms, script_x, sup_dy);
    }
    if let Some(sub) = sub_layout {
        script_width = script_width.max(sub.width);
        descent = descent.max(sub_dy + sub.descent);
        translate_atoms(&mut atoms, sub.atoms, script_x, sub_dy);
    }
    MathLayout {
        width: base_layout.width + script_gap + script_width,
        ascent,
        descent,
        atoms,
    }
}

fn layout_under_over(
    base: &MathExpr,
    under: Option<&MathExpr>,
    over: Option<&MathExpr>,
    ctx: LayoutCtx,
) -> MathLayout {
    let base_ctx = if matches!(ctx.display, MathDisplay::Block) && is_large_operator_base(base) {
        ctx.large_operator()
    } else {
        ctx
    };
    let base_layout = layout_expr(base, base_ctx);
    let script_ctx = ctx.script();
    let under_layout = under.map(|expr| layout_expr(expr, script_ctx));
    let over_layout = over.map(|expr| layout_expr(expr, script_ctx));
    let gap = ctx.metrics().under_over_gap();
    let width = base_layout
        .width
        .max(under_layout.as_ref().map(|l| l.width).unwrap_or(0.0))
        .max(over_layout.as_ref().map(|l| l.width).unwrap_or(0.0));
    let base_x = (width - base_layout.width) * 0.5;
    let mut atoms = Vec::new();
    let mut ascent = base_layout.ascent;
    let mut descent = base_layout.descent;
    translate_atoms(&mut atoms, base_layout.atoms, base_x, 0.0);
    if let Some(over) = over_layout {
        let over_x = (width - over.width) * 0.5;
        let over_dy = -base_layout.ascent - gap - over.descent;
        ascent = ascent.max(-over_dy + over.ascent);
        translate_atoms(&mut atoms, over.atoms, over_x, over_dy);
    }
    if let Some(under) = under_layout {
        let under_x = (width - under.width) * 0.5;
        let under_dy = base_layout.descent + gap + under.ascent;
        descent = descent.max(under_dy + under.descent);
        translate_atoms(&mut atoms, under.atoms, under_x, under_dy);
    }
    MathLayout {
        width,
        ascent,
        descent,
        atoms,
    }
}

fn is_display_limits_base(expr: &MathExpr) -> bool {
    match expr {
        MathExpr::Operator(_) => is_large_operator_base(expr),
        MathExpr::Text(s) => matches!(s.as_str(), "lim" | "max" | "min" | "sup" | "inf"),
        _ => false,
    }
}

fn is_large_operator_base(expr: &MathExpr) -> bool {
    matches!(expr, MathExpr::Operator(s) if matches!(s.as_str(), "∑" | "∏" | "⋂" | "⋃"))
}

fn layout_fenced(
    open: &Option<String>,
    close: &Option<String>,
    body: &MathExpr,
    ctx: LayoutCtx,
) -> MathLayout {
    let body_layout = layout_expr(body, ctx);
    let delimiter_rect = delimiter_rect(&body_layout, ctx);
    let metrics = ctx.metrics();
    let gap = metrics.delimiter_gap();
    let open_layout = open
        .as_deref()
        .map(|delimiter| layout_delimiter(delimiter, delimiter_rect, ctx));
    let close_layout = close
        .as_deref()
        .map(|delimiter| layout_delimiter(delimiter, delimiter_rect, ctx));
    let open_width = open_layout
        .as_ref()
        .map(|layout| layout.width + gap)
        .unwrap_or(0.0);
    let close_width = close_layout
        .as_ref()
        .map(|layout| layout.width + gap)
        .unwrap_or(0.0);
    let delimiter_ascent = open_layout
        .as_ref()
        .into_iter()
        .chain(close_layout.as_ref())
        .map(|layout| layout.ascent)
        .fold(0.0, f32::max);
    let delimiter_descent = open_layout
        .as_ref()
        .into_iter()
        .chain(close_layout.as_ref())
        .map(|layout| layout.descent)
        .fold(0.0, f32::max);
    let mut atoms = Vec::new();
    if let Some(open) = open_layout {
        translate_atoms(&mut atoms, open.atoms, 0.0, 0.0);
    }
    translate_atoms(&mut atoms, body_layout.atoms, open_width, 0.0);
    if let Some(close) = close_layout {
        translate_atoms(
            &mut atoms,
            close.atoms,
            open_width + body_layout.width + gap,
            0.0,
        );
    }
    MathLayout {
        width: open_width + body_layout.width + close_width,
        ascent: body_layout.ascent.max(delimiter_ascent),
        descent: body_layout.descent.max(delimiter_descent),
        atoms,
    }
}

fn delimiter_rect(body: &MathLayout, ctx: LayoutCtx) -> Rect {
    let metrics = ctx.metrics();
    let overshoot = metrics.delimiter_overshoot();
    let top = -body.ascent - overshoot;
    let bottom = body.descent + overshoot;
    Rect::new(0.0, top, metrics.delimiter_width(), bottom - top)
}

fn layout_delimiter(delimiter: &str, rect: Rect, ctx: LayoutCtx) -> MathLayout {
    if !is_vector_delimiter(delimiter) {
        return layout_glyph(delimiter, ctx, FontWeight::Regular, false);
    }
    MathLayout {
        width: rect.w,
        ascent: -rect.y,
        descent: rect.y + rect.h,
        atoms: vec![MathAtom::Delimiter {
            delimiter: delimiter.to_string(),
            rect,
            thickness: ctx.metrics().rule_thickness(),
        }],
    }
}

fn is_vector_delimiter(delimiter: &str) -> bool {
    matches!(
        delimiter,
        "(" | ")" | "[" | "]" | "{" | "}" | "|" | "‖" | "⟨" | "⟩" | "⌊" | "⌋" | "⌈" | "⌉"
    )
}

fn layout_table(rows: &[Vec<MathExpr>], ctx: LayoutCtx) -> MathLayout {
    if rows.is_empty() {
        return MathLayout {
            width: 0.0,
            ascent: 0.0,
            descent: 0.0,
            atoms: Vec::new(),
        };
    }
    let cell_layouts: Vec<Vec<MathLayout>> = rows
        .iter()
        .map(|row| row.iter().map(|cell| layout_expr(cell, ctx)).collect())
        .collect();
    let metrics = ctx.metrics();
    let col_count = cell_layouts.iter().map(Vec::len).max().unwrap_or(0);
    let mut col_widths = vec![0.0_f32; col_count];
    let mut row_ascents = vec![metrics.default_ascent(); rows.len()];
    let mut row_descents = vec![metrics.default_descent(); rows.len()];
    for (row_index, row) in cell_layouts.iter().enumerate() {
        for (col_index, cell) in row.iter().enumerate() {
            col_widths[col_index] = col_widths[col_index].max(cell.width);
            row_ascents[row_index] = row_ascents[row_index].max(cell.ascent);
            row_descents[row_index] = row_descents[row_index].max(cell.descent);
        }
    }
    let col_gap = metrics.table_col_gap();
    let row_gap = metrics.table_row_gap();
    let width = col_widths.iter().sum::<f32>() + col_gap * col_count.saturating_sub(1) as f32;
    let row_heights: Vec<f32> = row_ascents
        .iter()
        .zip(row_descents.iter())
        .map(|(ascent, descent)| ascent + descent)
        .collect();
    let height = row_heights.iter().sum::<f32>() + row_gap * rows.len().saturating_sub(1) as f32;
    let baseline_origin = height * 0.5;
    let mut atoms = Vec::new();
    let mut row_top = 0.0;
    for (row_index, row) in cell_layouts.into_iter().enumerate() {
        let row_baseline = row_top + row_ascents[row_index];
        let mut col_left = 0.0;
        for (col_index, cell) in row.into_iter().enumerate() {
            let cell_x = col_left + (col_widths[col_index] - cell.width) * 0.5;
            translate_atoms(
                &mut atoms,
                cell.atoms,
                cell_x,
                row_baseline - baseline_origin,
            );
            col_left += col_widths[col_index] + col_gap;
        }
        row_top += row_heights[row_index] + row_gap;
    }
    MathLayout {
        width,
        ascent: baseline_origin,
        descent: height - baseline_origin,
        atoms,
    }
}

fn translate_atoms(out: &mut Vec<MathAtom>, atoms: Vec<MathAtom>, dx: f32, dy: f32) {
    out.extend(atoms.into_iter().map(|atom| match atom {
        MathAtom::Glyph {
            text,
            x,
            y_baseline,
            size,
            weight,
            italic,
        } => MathAtom::Glyph {
            text,
            x: x + dx,
            y_baseline: y_baseline + dy,
            size,
            weight,
            italic,
        },
        MathAtom::Rule { rect } => MathAtom::Rule {
            rect: Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h),
        },
        MathAtom::Radical { points, thickness } => MathAtom::Radical {
            points: points.map(|[x, y]| [x + dx, y + dy]),
            thickness,
        },
        MathAtom::Delimiter {
            delimiter,
            rect,
            thickness,
        } => MathAtom::Delimiter {
            delimiter,
            rect: Rect::new(rect.x + dx, rect.y + dy, rect.w, rect.h),
            thickness,
        },
    }));
}

pub fn parse_tex(input: &str) -> Result<MathExpr, MathParseError> {
    let mut parser = TexParser::new(input);
    let expr = parser.parse_row(None)?;
    parser.skip_ws();
    if parser.peek().is_some() {
        return Err(parser.error("unexpected trailing input"));
    }
    Ok(expr)
}

pub fn parse_mathml(input: &str) -> Result<MathExpr, MathParseError> {
    Ok(parse_mathml_with_display(input)?.0)
}

pub fn parse_mathml_with_display(input: &str) -> Result<(MathExpr, MathDisplay), MathParseError> {
    let doc = roxmltree::Document::parse(input).map_err(|err| {
        let pos = err.pos();
        MathParseError {
            message: err.to_string(),
            byte: text_pos_to_byte(input, pos.row, pos.col),
        }
    })?;
    let root = doc.root_element();
    let display = match root.attribute("display") {
        Some("block") => MathDisplay::Block,
        _ => MathDisplay::Inline,
    };
    let expr = parse_mathml_node(root)?;
    Ok((expr, display))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathParseError {
    pub message: String,
    pub byte: usize,
}

fn parse_mathml_node(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let name = node.tag_name().name();
    match name {
        "math" | "mrow" => Ok(MathExpr::row(parse_mathml_children(node)?)),
        "mi" => Ok(MathExpr::Identifier(normalized_node_text(node))),
        "mn" => Ok(MathExpr::Number(normalized_node_text(node))),
        "mo" => Ok(MathExpr::Operator(normalized_node_text(node))),
        "mtext" => Ok(MathExpr::Text(normalized_node_text(node))),
        "mspace" => Ok(MathExpr::Space(parse_mathml_space(node))),
        "mfrac" => {
            let children = mathml_element_children(node);
            require_mathml_arity(node, &children, 2)?;
            Ok(MathExpr::Fraction {
                numerator: Arc::new(parse_mathml_node(children[0])?),
                denominator: Arc::new(parse_mathml_node(children[1])?),
            })
        }
        "msqrt" => Ok(MathExpr::Sqrt(Arc::new(MathExpr::row(
            parse_mathml_children(node)?,
        )))),
        "mroot" => {
            let children = mathml_element_children(node);
            require_mathml_arity(node, &children, 2)?;
            Ok(MathExpr::Root {
                base: Arc::new(parse_mathml_node(children[0])?),
                index: Arc::new(parse_mathml_node(children[1])?),
            })
        }
        "msub" => parse_mathml_scripts(node, true, false),
        "msup" => parse_mathml_scripts(node, false, true),
        "msubsup" => parse_mathml_scripts(node, true, true),
        "munder" => parse_mathml_under_over(node, true, false),
        "mover" => parse_mathml_under_over(node, false, true),
        "munderover" => parse_mathml_under_over(node, true, true),
        "mfenced" => parse_mathml_fenced(node),
        "mtable" => parse_mathml_table(node),
        "mtr" => Ok(MathExpr::row(
            mathml_element_children(node)
                .into_iter()
                .map(parse_mathml_node)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        "mtd" => Ok(MathExpr::row(parse_mathml_children(node)?)),
        unsupported => Ok(MathExpr::Error(format!(
            "unsupported MathML element <{unsupported}>"
        ))),
    }
}

fn parse_mathml_children(node: roxmltree::Node<'_, '_>) -> Result<Vec<MathExpr>, MathParseError> {
    mathml_element_children(node)
        .into_iter()
        .map(parse_mathml_node)
        .collect()
}

fn mathml_element_children<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
) -> Vec<roxmltree::Node<'a, 'input>> {
    node.children()
        .filter(roxmltree::Node::is_element)
        .collect()
}

fn require_mathml_arity(
    node: roxmltree::Node<'_, '_>,
    children: &[roxmltree::Node<'_, '_>],
    expected: usize,
) -> Result<(), MathParseError> {
    if children.len() == expected {
        Ok(())
    } else {
        Err(mathml_error_at(
            node,
            format!(
                "<{}> expected {expected} element children, got {}",
                node.tag_name().name(),
                children.len()
            ),
        ))
    }
}

fn parse_mathml_scripts(
    node: roxmltree::Node<'_, '_>,
    has_sub: bool,
    has_sup: bool,
) -> Result<MathExpr, MathParseError> {
    let children = mathml_element_children(node);
    let expected = 1 + usize::from(has_sub) + usize::from(has_sup);
    require_mathml_arity(node, &children, expected)?;
    let base = Arc::new(parse_mathml_node(children[0])?);
    let sub = has_sub.then(|| {
        let index = 1;
        parse_mathml_node(children[index]).map(Arc::new)
    });
    let sup = has_sup.then(|| {
        let index = if has_sub { 2 } else { 1 };
        parse_mathml_node(children[index]).map(Arc::new)
    });
    Ok(MathExpr::Scripts {
        base,
        sub: sub.transpose()?,
        sup: sup.transpose()?,
    })
}

fn parse_mathml_under_over(
    node: roxmltree::Node<'_, '_>,
    has_under: bool,
    has_over: bool,
) -> Result<MathExpr, MathParseError> {
    let children = mathml_element_children(node);
    let expected = 1 + usize::from(has_under) + usize::from(has_over);
    require_mathml_arity(node, &children, expected)?;
    let base = Arc::new(parse_mathml_node(children[0])?);
    let under = has_under.then(|| {
        let index = 1;
        parse_mathml_node(children[index]).map(Arc::new)
    });
    let over = has_over.then(|| {
        let index = if has_under { 2 } else { 1 };
        parse_mathml_node(children[index]).map(Arc::new)
    });
    Ok(MathExpr::UnderOver {
        base,
        under: under.transpose()?,
        over: over.transpose()?,
    })
}

fn parse_mathml_table(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let mut rows = Vec::new();
    for row_node in mathml_element_children(node) {
        if !matches!(row_node.tag_name().name(), "mtr" | "mlabeledtr") {
            return Err(mathml_error_at(
                row_node,
                format!(
                    "<mtable> expected row element children, got <{}>",
                    row_node.tag_name().name()
                ),
            ));
        }
        let mut row = Vec::new();
        for cell_node in mathml_element_children(row_node) {
            require_mathml_tag(cell_node, "mtd")?;
            row.push(MathExpr::row(parse_mathml_children(cell_node)?));
        }
        rows.push(row);
    }
    Ok(MathExpr::Table { rows })
}

fn parse_mathml_fenced(node: roxmltree::Node<'_, '_>) -> Result<MathExpr, MathParseError> {
    let open = parse_fence_attr(node.attribute("open").unwrap_or("("));
    let close = parse_fence_attr(node.attribute("close").unwrap_or(")"));
    let separator = match node.attribute("separators") {
        Some(value) => value
            .chars()
            .find(|ch| !ch.is_whitespace())
            .map(|ch| ch.to_string()),
        None => Some(",".to_string()),
    };
    let children = parse_mathml_children(node)?;
    let mut body = Vec::new();
    for (index, child) in children.into_iter().enumerate() {
        if index > 0
            && let Some(separator) = &separator
        {
            body.push(MathExpr::Operator(separator.clone()));
        }
        body.push(child);
    }
    Ok(MathExpr::Fenced {
        open,
        close,
        body: Arc::new(MathExpr::row(body)),
    })
}

fn parse_fence_attr(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "." {
        None
    } else {
        Some(value.to_string())
    }
}

fn require_mathml_tag(node: roxmltree::Node<'_, '_>, expected: &str) -> Result<(), MathParseError> {
    if node.tag_name().name() == expected {
        Ok(())
    } else {
        Err(mathml_error_at(
            node,
            format!(
                "expected <{expected}> element, got <{}>",
                node.tag_name().name()
            ),
        ))
    }
}

fn normalized_node_text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter(roxmltree::Node::is_text)
        .filter_map(|n| n.text())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_mathml_space(node: roxmltree::Node<'_, '_>) -> f32 {
    node.attribute("width")
        .and_then(parse_em_length)
        .unwrap_or(0.3)
}

fn parse_em_length(s: &str) -> Option<f32> {
    let trimmed = s.trim();
    if let Some(number) = trimmed.strip_suffix("em") {
        return number.trim().parse().ok();
    }
    if let Some(number) = trimmed.strip_suffix("px") {
        return number.trim().parse::<f32>().ok().map(|px| px / 16.0);
    }
    trimmed.parse().ok()
}

fn mathml_error_at(node: roxmltree::Node<'_, '_>, message: String) -> MathParseError {
    MathParseError {
        message,
        byte: node.range().start,
    }
}

fn text_pos_to_byte(input: &str, row: u32, col: u32) -> usize {
    let mut current_row = 1;
    let mut current_col = 1;
    for (byte, ch) in input.char_indices() {
        if current_row == row && current_col == col {
            return byte;
        }
        if ch == '\n' {
            current_row += 1;
            current_col = 1;
        } else {
            current_col += 1;
        }
    }
    input.len()
}

struct TexParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> TexParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn parse_row(&mut self, until: Option<char>) -> Result<MathExpr, MathParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.starts_with_command("right") {
                return Err(self.error("unexpected \\right"));
            }
            match self.peek() {
                None => {
                    if until.is_some() {
                        return Err(self.error("unclosed group"));
                    }
                    break;
                }
                Some(ch) if Some(ch) == until => {
                    self.bump();
                    break;
                }
                Some('}') => return Err(self.error("unexpected closing brace")),
                _ => {
                    let atom = self.parse_atom_with_scripts()?;
                    items.push(atom);
                }
            }
        }
        Ok(MathExpr::row(items))
    }

    fn parse_row_until_right(&mut self) -> Result<MathExpr, MathParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws();
            if self.peek().is_none() {
                return Err(self.error("unclosed \\left"));
            }
            if self.starts_with_command("right") {
                break;
            }
            if self.peek() == Some('}') {
                return Err(self.error("unexpected closing brace"));
            }
            let atom = self.parse_atom_with_scripts()?;
            items.push(atom);
        }
        Ok(MathExpr::row(items))
    }

    fn parse_atom_with_scripts(&mut self) -> Result<MathExpr, MathParseError> {
        let mut base = self.parse_atom()?;
        let mut sub = None;
        let mut sup = None;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('_') => {
                    self.bump();
                    sub = Some(Arc::new(self.parse_script_arg()?));
                }
                Some('^') => {
                    self.bump();
                    sup = Some(Arc::new(self.parse_script_arg()?));
                }
                _ => break,
            }
        }
        if sub.is_some() || sup.is_some() {
            base = MathExpr::Scripts {
                base: Arc::new(base),
                sub,
                sup,
            };
        }
        Ok(base)
    }

    fn parse_script_arg(&mut self) -> Result<MathExpr, MathParseError> {
        self.skip_ws();
        if self.peek() == Some('{') {
            self.bump();
            self.parse_row(Some('}'))
        } else {
            self.parse_atom()
        }
    }

    fn parse_atom(&mut self) -> Result<MathExpr, MathParseError> {
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.bump();
                self.parse_row(Some('}'))
            }
            Some('\\') => self.parse_command(),
            Some(ch) if ch.is_ascii_digit() => Ok(MathExpr::Number(
                self.take_while(|c| c.is_ascii_digit() || c == '.'),
            )),
            Some(ch) if ch.is_alphabetic() => Ok(MathExpr::Identifier(ch.to_string()).tap(|_| {
                self.bump();
            })),
            Some(ch) => {
                self.bump();
                Ok(if ch.is_whitespace() {
                    MathExpr::Space(0.3)
                } else {
                    MathExpr::Operator(ch.to_string())
                })
            }
            None => Err(self.error("expected math atom")),
        }
    }

    fn parse_command(&mut self) -> Result<MathExpr, MathParseError> {
        self.expect('\\')?;
        let name = self.take_while(|c| c.is_ascii_alphabetic());
        if name.is_empty() {
            let escaped = self
                .bump()
                .ok_or_else(|| self.error("expected escaped character"))?;
            return Ok(MathExpr::Operator(escaped.to_string()));
        }
        match name.as_str() {
            "frac" => {
                let numerator = Arc::new(self.parse_required_group()?);
                let denominator = Arc::new(self.parse_required_group()?);
                Ok(MathExpr::Fraction {
                    numerator,
                    denominator,
                })
            }
            "sqrt" => {
                let index = self.parse_optional_bracket_group()?;
                let base = Arc::new(self.parse_required_group()?);
                Ok(match index {
                    Some(index) => MathExpr::Root {
                        base,
                        index: Arc::new(index),
                    },
                    None => MathExpr::Sqrt(base),
                })
            }
            "left" => {
                let open = self.parse_delimiter()?;
                let body = Arc::new(self.parse_row_until_right()?);
                self.consume_command("right")?;
                let close = self.parse_delimiter()?;
                Ok(MathExpr::Fenced { open, close, body })
            }
            "right" => Err(self.error("unexpected \\right")),
            "cdot" => Ok(MathExpr::Operator("·".into())),
            "times" => Ok(MathExpr::Operator("×".into())),
            "div" => Ok(MathExpr::Operator("÷".into())),
            "pm" => Ok(MathExpr::Operator("±".into())),
            "le" | "leq" => Ok(MathExpr::Operator("≤".into())),
            "ge" | "geq" => Ok(MathExpr::Operator("≥".into())),
            "ne" | "neq" => Ok(MathExpr::Operator("≠".into())),
            "to" | "rightarrow" => Ok(MathExpr::Operator("→".into())),
            "leftarrow" => Ok(MathExpr::Operator("←".into())),
            "sum" => Ok(MathExpr::Operator("∑".into())),
            "prod" => Ok(MathExpr::Operator("∏".into())),
            "int" => Ok(MathExpr::Operator("∫".into())),
            "cup" => Ok(MathExpr::Operator("∪".into())),
            "cap" => Ok(MathExpr::Operator("∩".into())),
            "bigcup" => Ok(MathExpr::Operator("⋃".into())),
            "bigcap" => Ok(MathExpr::Operator("⋂".into())),
            "infty" => Ok(MathExpr::Identifier("∞".into())),
            "pi" => Ok(MathExpr::Identifier("π".into())),
            "theta" => Ok(MathExpr::Identifier("θ".into())),
            "lambda" => Ok(MathExpr::Identifier("λ".into())),
            "mu" => Ok(MathExpr::Identifier("μ".into())),
            "sigma" => Ok(MathExpr::Identifier("σ".into())),
            "alpha" => Ok(MathExpr::Identifier("α".into())),
            "beta" => Ok(MathExpr::Identifier("β".into())),
            "gamma" => Ok(MathExpr::Identifier("γ".into())),
            "Delta" => Ok(MathExpr::Identifier("Δ".into())),
            "Omega" => Ok(MathExpr::Identifier("Ω".into())),
            "sin" | "cos" | "tan" | "log" | "ln" | "lim" | "max" | "min" | "sup" | "inf" => {
                Ok(MathExpr::Text(name))
            }
            _ => Ok(MathExpr::Identifier(format!("\\{name}"))),
        }
    }

    fn parse_required_group(&mut self) -> Result<MathExpr, MathParseError> {
        self.skip_ws();
        self.expect('{')?;
        self.parse_row(Some('}'))
    }

    fn parse_optional_bracket_group(&mut self) -> Result<Option<MathExpr>, MathParseError> {
        self.skip_ws();
        if self.peek() != Some('[') {
            return Ok(None);
        }
        self.bump();
        self.parse_row(Some(']')).map(Some)
    }

    fn parse_delimiter(&mut self) -> Result<Option<String>, MathParseError> {
        self.skip_ws();
        let delimiter = match self.bump() {
            Some('.') => return Ok(None),
            Some('\\') => {
                let name = self.take_while(|c| c.is_ascii_alphabetic());
                if name.is_empty() {
                    self.bump()
                        .ok_or_else(|| self.error("expected delimiter after escape"))?
                        .to_string()
                } else {
                    delimiter_command(&name).unwrap_or_else(|| format!("\\{name}"))
                }
            }
            Some(ch) => ch.to_string(),
            None => return Err(self.error("expected delimiter")),
        };
        Ok(Some(delimiter))
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
            self.bump();
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), MathParseError> {
        match self.bump() {
            Some(ch) if ch == expected => Ok(()),
            _ => Err(self.error(&format!("expected '{expected}'"))),
        }
    }

    fn take_while(&mut self, mut f: impl FnMut(char) -> bool) -> String {
        let start = self.pos;
        while matches!(self.peek(), Some(ch) if f(ch)) {
            self.bump();
        }
        self.input[start..self.pos].to_string()
    }

    fn starts_with_command(&self, command: &str) -> bool {
        let rest = &self.input[self.pos..];
        let Some(after_slash) = rest.strip_prefix('\\') else {
            return false;
        };
        let Some(after_command) = after_slash.strip_prefix(command) else {
            return false;
        };
        !matches!(after_command.chars().next(), Some(ch) if ch.is_ascii_alphabetic())
    }

    fn consume_command(&mut self, command: &str) -> Result<(), MathParseError> {
        if !self.starts_with_command(command) {
            return Err(self.error(&format!("expected \\{command}")));
        }
        self.expect('\\')?;
        let found = self.take_while(|c| c.is_ascii_alphabetic());
        if found == command {
            Ok(())
        } else {
            Err(self.error(&format!("expected \\{command}")))
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn error(&self, message: &str) -> MathParseError {
        MathParseError {
            message: message.to_string(),
            byte: self.pos,
        }
    }
}

fn delimiter_command(command: &str) -> Option<String> {
    let delimiter = match command {
        "lbrace" => "{",
        "rbrace" => "}",
        "lparen" => "(",
        "rparen" => ")",
        "lbrack" => "[",
        "rbrack" => "]",
        "langle" => "⟨",
        "rangle" => "⟩",
        "vert" => "|",
        "Vert" => "‖",
        "lfloor" => "⌊",
        "rfloor" => "⌋",
        "lceil" => "⌈",
        "rceil" => "⌉",
        _ => return None,
    };
    Some(delimiter.to_string())
}

trait Tap: Sized {
    fn tap(self, f: impl FnOnce(&Self)) -> Self {
        f(&self);
        self
    }
}

impl<T> Tap for T {}

pub(crate) fn math_glyph_layout(
    text: &str,
    size: f32,
    weight: FontWeight,
) -> text_metrics::TextLayout {
    text_metrics::layout_text_with_line_height_and_family(
        text,
        size,
        text_metrics::line_height(size),
        FontFamily::Inter,
        weight,
        false,
        TextWrap::NoWrap,
        None,
    )
}

pub(crate) fn resolved_math_color(color: Option<Color>) -> Color {
    color.unwrap_or(crate::tokens::FOREGROUND)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "symbols")]
    #[test]
    fn loads_bundled_open_type_math_constants() {
        let constants = open_type_math_constants().expect("bundled math font has a MATH table");
        assert!(
            constants
                .script_scale(16.0)
                .is_some_and(|size| size > 6.0 && size < 16.0),
            "script scale should come from Noto Sans Math"
        );
        assert!(
            constants
                .fraction_rule_thickness(16.0)
                .is_some_and(|thickness| thickness > 0.75 && thickness < 2.0),
            "fraction rule thickness should come from Noto Sans Math"
        );
        assert!(
            constants
                .radical_rule_thickness(16.0)
                .is_some_and(|thickness| thickness > 0.75 && thickness < 2.0),
            "radical rule thickness should come from Noto Sans Math"
        );
    }

    #[test]
    fn parses_fraction_with_scripts() {
        let expr = parse_tex(r"\frac{a^2+b^2}{\sqrt{x_1+x_2}}").expect("valid tex");
        let layout = layout_math(&expr, 16.0, MathDisplay::Block);
        assert!(layout.width > 20.0, "width = {}", layout.width);
        assert!(layout.ascent > 10.0, "ascent = {}", layout.ascent);
        assert!(layout.descent > 10.0, "descent = {}", layout.descent);
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Rule { .. })),
            "fraction should emit rule atoms"
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Radical { .. })),
            "sqrt should emit a radical atom"
        );
    }

    #[test]
    fn parses_indexed_tex_root() {
        let expr = parse_tex(r"\sqrt[3]{x+1}").expect("valid tex");
        match expr {
            MathExpr::Root { base, index } => {
                assert_eq!(*index, MathExpr::Number("3".into()));
                assert!(matches!(*base, MathExpr::Row(_)));
            }
            other => panic!("expected indexed root, got {other:?}"),
        }
        let layout = layout_math(
            &parse_tex(r"\sqrt[3]{x+1}").unwrap(),
            16.0,
            MathDisplay::Inline,
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Radical { .. })),
            "indexed root should emit a radical atom"
        );
    }

    #[test]
    fn display_sum_scripts_layout_as_limits() {
        let expr = parse_tex(r"\sum_{i=1}^{n} x_i").expect("valid tex");
        let layout = layout_math(&expr, 16.0, MathDisplay::Block);
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Glyph { text, y_baseline, .. } if text == "n" && *y_baseline < 0.0)),
            "sum upper limit should sit above the operator"
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Glyph { text, y_baseline, .. } if text == "i" && *y_baseline > 0.0)),
            "sum lower limit should sit below the operator"
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Glyph { text, size, .. } if text == "∑" && *size > 16.0)),
            "display sum should use a larger operator glyph"
        );
    }

    #[test]
    fn parses_tex_left_right_fences() {
        let expr = parse_tex(r"\left(\frac{a}{b}\right)").expect("valid fenced tex");
        match expr {
            MathExpr::Fenced { open, close, body } => {
                assert_eq!(open.as_deref(), Some("("));
                assert_eq!(close.as_deref(), Some(")"));
                assert!(matches!(*body, MathExpr::Fraction { .. }));
            }
            other => panic!("expected fenced expression, got {other:?}"),
        }
        let layout = layout_math(
            &parse_tex(r"\left(\frac{a}{b}\right)").unwrap(),
            16.0,
            MathDisplay::Inline,
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Delimiter { delimiter, rect, thickness } if delimiter == "(" && rect.h > 16.0 && *thickness < 2.0)),
            "fence should emit a stretched vector delimiter with normal stroke weight"
        );
    }

    #[test]
    fn rejects_unmatched_tex_right_fence() {
        let err = parse_tex(r"x \right)").expect_err("invalid unmatched fence");
        assert!(err.message.contains("unexpected \\right"));
    }

    #[test]
    fn reports_unclosed_group() {
        let err = parse_tex(r"\frac{1}{x").expect_err("invalid tex");
        assert!(err.message.contains("unclosed group"));
    }

    #[test]
    fn parses_mathml_fraction_with_scripts() {
        let expr = parse_mathml(
            r#"
            <math>
              <mfrac>
                <mrow>
                  <msup><mi>a</mi><mn>2</mn></msup>
                  <mo>+</mo>
                  <msup><mi>b</mi><mn>2</mn></msup>
                </mrow>
                <msqrt>
                  <msub><mi>x</mi><mn>1</mn></msub>
                  <mo>+</mo>
                  <msub><mi>x</mi><mn>2</mn></msub>
                </msqrt>
              </mfrac>
            </math>
            "#,
        )
        .expect("valid mathml");
        let layout = layout_math(&expr, 16.0, MathDisplay::Block);
        assert!(layout.width > 20.0, "width = {}", layout.width);
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Rule { .. })),
            "fraction should emit rule atoms"
        );
        assert!(
            layout
                .atoms
                .iter()
                .any(|atom| matches!(atom, MathAtom::Radical { .. })),
            "sqrt should emit a radical atom"
        );
    }

    #[test]
    fn parses_mathml_indexed_root() {
        let expr = parse_mathml(
            r#"
            <math>
              <mroot>
                <mrow><mi>x</mi><mo>+</mo><mn>1</mn></mrow>
                <mn>3</mn>
              </mroot>
            </math>
            "#,
        )
        .expect("valid mathml");
        match expr {
            MathExpr::Root { base, index } => {
                assert_eq!(*index, MathExpr::Number("3".into()));
                assert!(matches!(*base, MathExpr::Row(_)));
            }
            other => panic!("expected indexed root, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_under_over() {
        let expr = parse_mathml(
            r#"
            <math>
              <munderover>
                <mo>∑</mo>
                <mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow>
                <mi>n</mi>
              </munderover>
            </math>
            "#,
        )
        .expect("valid mathml");
        match expr {
            MathExpr::UnderOver { base, under, over } => {
                assert_eq!(*base, MathExpr::Operator("∑".into()));
                assert!(matches!(*under.unwrap(), MathExpr::Row(_)));
                assert_eq!(*over.unwrap(), MathExpr::Identifier("n".into()));
            }
            other => panic!("expected under/over expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_fenced_expression() {
        let expr = parse_mathml(
            r#"
            <math>
              <mfenced open="[" close="]" separators=",">
                <mi>a</mi>
                <mi>b</mi>
              </mfenced>
            </math>
            "#,
        )
        .expect("valid mathml fenced expression");
        match expr {
            MathExpr::Fenced { open, close, body } => {
                assert_eq!(open.as_deref(), Some("["));
                assert_eq!(close.as_deref(), Some("]"));
                match body.as_ref() {
                    MathExpr::Row(children) => {
                        assert_eq!(children.len(), 3);
                        assert_eq!(children[1], MathExpr::Operator(",".into()));
                    }
                    other => panic!("expected row body, got {other:?}"),
                }
            }
            other => panic!("expected fenced expression, got {other:?}"),
        }
    }

    #[test]
    fn parses_mathml_table() {
        let expr = parse_mathml(
            r#"
            <math>
              <mtable>
                <mtr>
                  <mtd><mi>a</mi></mtd>
                  <mtd><mi>b</mi></mtd>
                </mtr>
                <mtr>
                  <mtd><mi>c</mi></mtd>
                  <mtd><mi>d</mi></mtd>
                </mtr>
              </mtable>
            </math>
            "#,
        )
        .expect("valid mathml");
        match expr {
            MathExpr::Table { rows } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0].len(), 2);
                assert_eq!(rows[1].len(), 2);
            }
            other => panic!("expected table expression, got {other:?}"),
        }
        let layout = layout_math(
            &parse_mathml(
                r#"<math><mtable><mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr><mtr><mtd><mi>c</mi></mtd><mtd><mi>d</mi></mtd></mtr></mtable></math>"#,
            )
            .unwrap(),
            16.0,
            MathDisplay::Block,
        );
        assert!(layout.width > 20.0, "width = {}", layout.width);
        assert!(layout.ascent > 10.0, "ascent = {}", layout.ascent);
        assert!(layout.descent > 10.0, "descent = {}", layout.descent);
    }

    #[test]
    fn parses_mathml_display_attribute() {
        let (expr, display) = parse_mathml_with_display(
            r#"<math display="block"><msubsup><mi>x</mi><mn>1</mn><mn>2</mn></msubsup></math>"#,
        )
        .expect("valid mathml");
        assert_eq!(display, MathDisplay::Block);
        match expr {
            MathExpr::Scripts { base, sub, sup } => {
                assert_eq!(*base, MathExpr::Identifier("x".into()));
                assert_eq!(*sub.unwrap(), MathExpr::Number("1".into()));
                assert_eq!(*sup.unwrap(), MathExpr::Number("2".into()));
            }
            other => panic!("expected scripts expression, got {other:?}"),
        }
    }

    #[test]
    fn rejects_wrong_mathml_arity() {
        let err =
            parse_mathml(r#"<math><mfrac><mi>a</mi></mfrac></math>"#).expect_err("invalid arity");
        assert!(err.message.contains("expected 2 element children"));
    }
}
