//! Backend-agnostic SVG/vector asset IR.
//!
//! `usvg` owns SVG normalization: XML, inherited style, transforms,
//! arcs, relative commands, and basic shapes are resolved before Aetna
//! stores anything. The renderer-facing IR below is deliberately small:
//! paths plus fill/stroke style. Backends can tessellate it with lyon or
//! feed it into more specialized vector shaders later.

use std::error::Error;
use std::fmt;

use crate::tree::Color;

use usvg::tiny_skia_path;

#[derive(Clone, Debug, PartialEq)]
pub struct VectorAsset {
    pub view_box: [f32; 4],
    pub paths: Vec<VectorPath>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorPath {
    pub segments: Vec<VectorSegment>,
    pub fill: Option<VectorFill>,
    pub stroke: Option<VectorStroke>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VectorSegment {
    MoveTo([f32; 2]),
    LineTo([f32; 2]),
    QuadTo([f32; 2], [f32; 2]),
    CubicTo([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorFill {
    pub color: VectorColor,
    pub opacity: f32,
    pub rule: VectorFillRule,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorStroke {
    pub color: VectorColor,
    pub opacity: f32,
    pub width: f32,
    pub line_cap: VectorLineCap,
    pub line_join: VectorLineJoin,
    pub miter_limit: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum VectorColor {
    CurrentColor,
    Solid(Color),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorFillRule {
    NonZero,
    EvenOdd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorLineCap {
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorLineJoin {
    Miter,
    MiterClip,
    Round,
    Bevel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorParseError {
    message: String,
}

impl VectorParseError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for VectorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for VectorParseError {}

pub fn parse_svg_asset(svg: &str) -> Result<VectorAsset, VectorParseError> {
    parse_svg_asset_with_color_mode(svg, false)
}

pub(crate) fn parse_current_color_svg_asset(svg: &str) -> Result<VectorAsset, VectorParseError> {
    parse_svg_asset_with_color_mode(svg, true)
}

fn parse_svg_asset_with_color_mode(
    svg: &str,
    force_current_color: bool,
) -> Result<VectorAsset, VectorParseError> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default())
        .map_err(|e| VectorParseError::new(format!("invalid SVG: {e}")))?;
    let size = tree.size();
    let mut asset = VectorAsset {
        view_box: [0.0, 0.0, size.width(), size.height()],
        paths: Vec::new(),
    };
    collect_group(tree.root(), force_current_color, &mut asset.paths);
    if asset.paths.is_empty() {
        return Err(VectorParseError::new("SVG produced no renderable paths"));
    }
    Ok(asset)
}

fn collect_group(group: &usvg::Group, force_current_color: bool, out: &mut Vec<VectorPath>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => collect_group(group, force_current_color, out),
            usvg::Node::Path(path) if path.is_visible() => {
                if let Some(vector_path) = convert_path(path, force_current_color) {
                    out.push(vector_path);
                }
            }
            _ => {}
        }
    }
}

fn convert_path(path: &usvg::Path, force_current_color: bool) -> Option<VectorPath> {
    let transform = path.abs_transform();
    let mut segments = Vec::new();
    for segment in path.data().segments() {
        match segment {
            tiny_skia_path::PathSegment::MoveTo(p) => {
                segments.push(VectorSegment::MoveTo(map_point(transform, p)));
            }
            tiny_skia_path::PathSegment::LineTo(p) => {
                segments.push(VectorSegment::LineTo(map_point(transform, p)));
            }
            tiny_skia_path::PathSegment::QuadTo(p0, p1) => {
                segments.push(VectorSegment::QuadTo(
                    map_point(transform, p0),
                    map_point(transform, p1),
                ));
            }
            tiny_skia_path::PathSegment::CubicTo(p0, p1, p2) => {
                segments.push(VectorSegment::CubicTo(
                    map_point(transform, p0),
                    map_point(transform, p1),
                    map_point(transform, p2),
                ));
            }
            tiny_skia_path::PathSegment::Close => segments.push(VectorSegment::Close),
        }
    }
    if segments.is_empty() {
        return None;
    }

    Some(VectorPath {
        segments,
        fill: path
            .fill()
            .and_then(|fill| convert_fill(fill, force_current_color)),
        stroke: path
            .stroke()
            .and_then(|stroke| convert_stroke(stroke, force_current_color)),
    })
}

fn convert_fill(fill: &usvg::Fill, force_current_color: bool) -> Option<VectorFill> {
    Some(VectorFill {
        color: convert_paint(fill.paint(), force_current_color)?,
        opacity: fill.opacity().get(),
        rule: match fill.rule() {
            usvg::FillRule::NonZero => VectorFillRule::NonZero,
            usvg::FillRule::EvenOdd => VectorFillRule::EvenOdd,
        },
    })
}

fn convert_stroke(stroke: &usvg::Stroke, force_current_color: bool) -> Option<VectorStroke> {
    Some(VectorStroke {
        color: convert_paint(stroke.paint(), force_current_color)?,
        opacity: stroke.opacity().get(),
        width: stroke.width().get(),
        line_cap: match stroke.linecap() {
            usvg::LineCap::Butt => VectorLineCap::Butt,
            usvg::LineCap::Round => VectorLineCap::Round,
            usvg::LineCap::Square => VectorLineCap::Square,
        },
        line_join: match stroke.linejoin() {
            usvg::LineJoin::Miter => VectorLineJoin::Miter,
            usvg::LineJoin::MiterClip => VectorLineJoin::MiterClip,
            usvg::LineJoin::Round => VectorLineJoin::Round,
            usvg::LineJoin::Bevel => VectorLineJoin::Bevel,
        },
        miter_limit: stroke.miterlimit().get(),
    })
}

fn convert_paint(paint: &usvg::Paint, force_current_color: bool) -> Option<VectorColor> {
    if force_current_color {
        return Some(VectorColor::CurrentColor);
    }
    match paint {
        usvg::Paint::Color(c) => Some(VectorColor::Solid(Color::rgba(c.red, c.green, c.blue, 255))),
        usvg::Paint::LinearGradient(_)
        | usvg::Paint::RadialGradient(_)
        | usvg::Paint::Pattern(_) => None,
    }
}

fn map_point(transform: tiny_skia_path::Transform, mut point: tiny_skia_path::Point) -> [f32; 2] {
    transform.map_point(&mut point);
    [point.x, point.y]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_svg_shapes_into_paths() {
        let asset = parse_svg_asset(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4" fill="none" stroke="#000" stroke-width="2"/></svg>"##,
        )
        .unwrap();
        assert_eq!(asset.view_box, [0.0, 0.0, 24.0, 24.0]);
        assert_eq!(asset.paths.len(), 1);
        assert!(asset.paths[0].stroke.is_some());
        assert!(asset.paths[0].segments.len() > 4);
    }
}
