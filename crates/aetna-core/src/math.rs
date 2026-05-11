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
const FRACTION_PAD_EM: f32 = 0.18;
const FRACTION_GAP_EM: f32 = 0.18;
const SQRT_GAP_EM: f32 = 0.10;

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
}

#[derive(Clone, Copy, Debug)]
struct LayoutCtx {
    size: f32,
    display: MathDisplay,
}

impl LayoutCtx {
    fn script(self) -> Self {
        Self {
            size: (self.size * SCRIPT_SCALE).max(6.0),
            display: MathDisplay::Inline,
        }
    }

    fn rule_thickness(self) -> f32 {
        (DEFAULT_RULE_THICKNESS * self.size / 16.0).max(0.75)
    }
}

pub fn layout_math(expr: &MathExpr, size: f32, display: MathDisplay) -> MathLayout {
    layout_expr(expr, LayoutCtx { size, display })
}

fn layout_expr(expr: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    match expr {
        MathExpr::Row(children) => layout_row(children, ctx),
        MathExpr::Identifier(s) => layout_glyph(s, ctx, FontWeight::Regular, true),
        MathExpr::Number(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
        MathExpr::Operator(s) => layout_operator(s, ctx),
        MathExpr::Text(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
        MathExpr::Space(em) => MathLayout {
            width: ctx.size * *em,
            ascent: ctx.size * 0.75,
            descent: ctx.size * 0.25,
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
        MathExpr::Error(s) => layout_glyph(s, ctx, FontWeight::Regular, false),
    }
}

fn layout_row(children: &[MathExpr], ctx: LayoutCtx) -> MathLayout {
    let mut width = 0.0;
    let mut ascent: f32 = ctx.size * 0.75;
    let mut descent: f32 = ctx.size * 0.25;
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
        ascent: ctx.size * 0.82,
        descent: ctx.size * 0.22,
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
    let side = operator_side_bearing(s, ctx.size);
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

fn operator_side_bearing(s: &str, size: f32) -> f32 {
    match s {
        "+" | "-" | "=" | "<" | ">" | "≤" | "≥" | "≠" | "×" | "÷" | "→" | "←" | "↔" | "∪" | "∩"
        | "⋃" | "⋂" => size * 0.18,
        "∑" | "∏" | "∫" => size * 0.12,
        "," | "." | ";" | ":" => size * 0.08,
        _ => 0.0,
    }
}

fn layout_fraction(numerator: &MathExpr, denominator: &MathExpr, ctx: LayoutCtx) -> MathLayout {
    let display_fraction = matches!(ctx.display, MathDisplay::Block);
    let child_ctx = if matches!(ctx.display, MathDisplay::Block) {
        ctx
    } else {
        ctx.script()
    };
    let num = layout_expr(numerator, child_ctx);
    let den = layout_expr(denominator, child_ctx);
    let pad = ctx.size
        * if display_fraction {
            FRACTION_PAD_EM
        } else {
            FRACTION_PAD_EM * 0.65
        };
    let gap = ctx.size
        * if display_fraction {
            FRACTION_GAP_EM
        } else {
            FRACTION_GAP_EM * 0.55
        };
    let rule = ctx.rule_thickness();
    // The math axis sits above the prose baseline. Keeping the fraction
    // rule on that axis makes inline fractions read as part of the line
    // instead of hanging mostly below it.
    let axis_shift = ctx.size * if display_fraction { 0.18 } else { 0.28 };
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
    let inner = layout_expr(child, ctx);
    let gap = ctx.size * SQRT_GAP_EM;
    let rule = ctx.rule_thickness();
    let radical_w = ctx.size * 0.72;
    let inner_x = radical_w + gap;
    let bar_y = -inner.ascent - gap - rule * 0.5;
    let tick_y = (inner.descent * 0.75).max(ctx.size * 0.13);
    let end_x = inner_x + inner.width;
    let mut atoms = Vec::new();
    atoms.push(MathAtom::Radical {
        points: [
            [0.0, -ctx.size * 0.03],
            [ctx.size * 0.12, -ctx.size * 0.1],
            [ctx.size * 0.24, tick_y],
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
    let root = layout_sqrt(base, ctx);
    let index = layout_expr(index, ctx.script());
    let root_x = index.width * 0.55;
    let index_dy = -root.ascent * 0.52;
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
    let script_gap = ctx.size * 0.06;
    let script_x = base_layout.width + script_gap;
    let sup_dy = sup_layout
        .as_ref()
        .map(|sup| -(base_layout.ascent * 0.58).max(sup.descent + ctx.size * 0.18))
        .unwrap_or(0.0);
    let sub_dy = sub_layout
        .as_ref()
        .map(|sub| (base_layout.descent + sub.ascent * 0.72).max(ctx.size * 0.28))
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
    let base_layout = layout_expr(base, ctx);
    let script_ctx = ctx.script();
    let under_layout = under.map(|expr| layout_expr(expr, script_ctx));
    let over_layout = over.map(|expr| layout_expr(expr, script_ctx));
    let gap = ctx.size * 0.12;
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
        MathExpr::Operator(s) => matches!(s.as_str(), "∑" | "∏" | "⋂" | "⋃"),
        MathExpr::Text(s) => matches!(s.as_str(), "lim" | "max" | "min" | "sup" | "inf"),
        _ => false,
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
