//! Backend-agnostic SVG/vector asset IR.
//!
//! `usvg` owns SVG normalization: XML, inherited style, transforms,
//! arcs, relative commands, and basic shapes are resolved before Aetna
//! stores anything. The renderer-facing IR below is deliberately small:
//! paths plus fill/stroke style. Backends can tessellate it with lyon or
//! feed it into more specialized vector shaders later.

use std::error::Error;
use std::fmt;

use crate::paint::rgba_f32;
use crate::tree::Color;

use bytemuck::{Pod, Zeroable};
use lyon_tessellation::geometry_builder::{BuffersBuilder, VertexBuffers};
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path as LyonPath;
use lyon_tessellation::{
    FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions, StrokeTessellator,
    StrokeVertex,
};
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconMaterial {
    /// Direct premultiplied color. This is the baseline material and
    /// should match ordinary flat SVG rendering.
    #[default]
    Flat,
    /// A proof material that uses local vector coordinates to add a
    /// subtle top-left highlight and lower shadow. This exists to prove
    /// the shared mesh carries enough data for shader-controlled icon
    /// treatments.
    Relief,
    /// A glossy icon material with local-coordinate glints and a soft
    /// inner shade. Pairs with translucent/glass surfaces.
    Glass,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct VectorMeshVertex {
    /// Logical-pixel position after fitting the vector asset into its
    /// destination rect.
    pub pos: [f32; 2],
    /// SVG/viewBox-space coordinate. Theme shaders can use this for
    /// gradients, highlights, bevels, and other icon-local effects.
    pub local: [f32; 2],
    pub color: [f32; 4],
    /// Reserved for material shaders: x = path index, y = primitive
    /// kind (0 fill, 1 stroke), z/w reserved.
    pub meta: [f32; 4],
    /// Analytic-AA extrusion: a unit normal in logical px (zero for
    /// solid interior verts). The vertex shader extrudes the position
    /// by `aa * (1 / scale_factor)` so the fringe stays one **physical**
    /// pixel wide regardless of icon render size, and emits a
    /// per-vertex coverage of 1 for `aa == 0` and 0 for nonzero `aa` so
    /// the fragment interpolates a smooth 1-px alpha ramp at the edge.
    pub aa: [f32; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorMesh {
    pub vertices: Vec<VectorMeshVertex>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorMeshRun {
    pub first: u32,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VectorMeshOptions {
    pub rect: crate::tree::Rect,
    pub current_color: Color,
    pub stroke_width: f32,
    pub tolerance: f32,
}

impl VectorMeshOptions {
    pub fn icon(rect: crate::tree::Rect, current_color: Color, stroke_width: f32) -> Self {
        Self {
            rect,
            current_color,
            stroke_width,
            tolerance: 0.05,
        }
    }
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

pub fn tessellate_vector_asset(asset: &VectorAsset, options: VectorMeshOptions) -> VectorMesh {
    let mut mesh = VectorMesh::default();
    append_vector_asset_mesh(asset, options, &mut mesh.vertices);
    mesh
}

pub fn append_vector_asset_mesh(
    asset: &VectorAsset,
    options: VectorMeshOptions,
    out: &mut Vec<VectorMeshVertex>,
) -> VectorMeshRun {
    let first = out.len() as u32;
    if options.rect.w <= 0.0 || options.rect.h <= 0.0 {
        return VectorMeshRun { first, count: 0 };
    }

    let [vx, vy, vw, vh] = asset.view_box;
    let sx = options.rect.w / vw.max(1.0);
    let sy = options.rect.h / vh.max(1.0);
    let stroke_scale = (sx + sy) * 0.5;

    for (path_index, vector_path) in asset.paths.iter().enumerate() {
        let path = build_lyon_path(vector_path, options.rect, [vx, vy], [sx, sy]);
        if let Some(fill) = vector_path.fill {
            let color = resolve_color(fill.color, options.current_color, fill.opacity);
            let mut geometry: VertexBuffers<VectorMeshVertex, u16> = VertexBuffers::new();
            let fill_options =
                FillOptions::tolerance(options.tolerance).with_fill_rule(match fill.rule {
                    VectorFillRule::NonZero => lyon_tessellation::FillRule::NonZero,
                    VectorFillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
                });
            let _ = FillTessellator::new().tessellate_path(
                &path,
                &fill_options,
                &mut BuffersBuilder::new(&mut geometry, |v: FillVertex<'_>| {
                    make_mesh_vertex(
                        v.position(),
                        options.rect,
                        [vx, vy],
                        [sx, sy],
                        color,
                        path_index,
                        VectorPrimitiveKind::Fill,
                    )
                }),
            );
            append_indexed(&geometry, out);

            // Analytic-AA fringe: a thin band centred on the fill
            // boundary. Inner verts (path side) carry `aa = 0` so they
            // sit exactly on the fill edge with full coverage; outer
            // verts carry the unit normal so the vertex shader extrudes
            // them by 1 physical pixel and they fade to zero coverage.
            // The fragment alpha-interpolates between the two. Inside
            // the fill the band overlaps existing fill triangles, which
            // are already fully covered — alpha-blending leaves them at
            // 1, so the only visible effect is the outward fade.
            let mut fringe: VertexBuffers<VectorMeshVertex, u16> = VertexBuffers::new();
            // Width=1 logical unit puts the stroke verts ±0.5 px from
            // the path; we rebase them onto the path inside the
            // constructor so the geometry is anchored at the fill edge,
            // and reuse the unit normal for shader-side extrusion.
            let fringe_options = StrokeOptions::tolerance(options.tolerance)
                .with_line_width(1.0)
                .with_line_cap(LineCap::Butt)
                .with_line_join(LineJoin::Miter)
                .with_miter_limit(4.0);
            let _ = StrokeTessellator::new().tessellate_path(
                &path,
                &fringe_options,
                &mut BuffersBuilder::new(&mut fringe, |v: StrokeVertex<'_, '_>| {
                    let position = v.position();
                    let normal = v.normal();
                    let side_sign = match v.side() {
                        lyon_tessellation::Side::Negative => -1.0_f32,
                        lyon_tessellation::Side::Positive => 1.0_f32,
                    };
                    // Move the stroke vert back to the fill boundary.
                    let path_pos = lyon_tessellation::math::point(
                        position.x - side_sign * normal.x * 0.5,
                        position.y - side_sign * normal.y * 0.5,
                    );
                    let aa = match v.side() {
                        lyon_tessellation::Side::Negative => [0.0, 0.0],
                        lyon_tessellation::Side::Positive => [normal.x, normal.y],
                    };
                    make_mesh_vertex_with_aa(
                        path_pos,
                        options.rect,
                        [vx, vy],
                        [sx, sy],
                        color,
                        path_index,
                        VectorPrimitiveKind::Fill,
                        aa,
                    )
                }),
            );
            append_indexed(&fringe, out);
        }

        if let Some(stroke) = vector_path.stroke {
            let color = resolve_color(stroke.color, options.current_color, stroke.opacity);
            let width = if matches!(stroke.color, VectorColor::CurrentColor) {
                options.stroke_width * stroke_scale
            } else {
                stroke.width * stroke_scale
            }
            .max(0.5);
            let mut geometry: VertexBuffers<VectorMeshVertex, u16> = VertexBuffers::new();
            let stroke_options = StrokeOptions::tolerance(options.tolerance)
                .with_line_width(width)
                .with_line_cap(match stroke.line_cap {
                    VectorLineCap::Butt => LineCap::Butt,
                    VectorLineCap::Round => LineCap::Round,
                    VectorLineCap::Square => LineCap::Square,
                })
                .with_line_join(match stroke.line_join {
                    VectorLineJoin::Miter => LineJoin::Miter,
                    VectorLineJoin::MiterClip => LineJoin::MiterClip,
                    VectorLineJoin::Round => LineJoin::Round,
                    VectorLineJoin::Bevel => LineJoin::Bevel,
                })
                .with_miter_limit(stroke.miter_limit.max(1.0));
            let _ = StrokeTessellator::new().tessellate_path(
                &path,
                &stroke_options,
                &mut BuffersBuilder::new(&mut geometry, |v: StrokeVertex<'_, '_>| {
                    make_mesh_vertex(
                        v.position(),
                        options.rect,
                        [vx, vy],
                        [sx, sy],
                        color,
                        path_index,
                        VectorPrimitiveKind::Stroke,
                    )
                }),
            );
            append_indexed(&geometry, out);
        }
    }

    VectorMeshRun {
        first,
        count: out.len() as u32 - first,
    }
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

#[derive(Clone, Copy)]
enum VectorPrimitiveKind {
    Fill,
    Stroke,
}

fn build_lyon_path(
    path: &VectorPath,
    rect: crate::tree::Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
) -> LyonPath {
    let mut builder = LyonPath::builder();
    let mut open = false;
    for segment in &path.segments {
        match *segment {
            VectorSegment::MoveTo(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(map_mesh_point(rect, view_origin, scale, p));
                open = true;
            }
            VectorSegment::LineTo(p) => {
                builder.line_to(map_mesh_point(rect, view_origin, scale, p));
            }
            VectorSegment::QuadTo(c, p) => {
                builder.quadratic_bezier_to(
                    map_mesh_point(rect, view_origin, scale, c),
                    map_mesh_point(rect, view_origin, scale, p),
                );
            }
            VectorSegment::CubicTo(c0, c1, p) => {
                builder.cubic_bezier_to(
                    map_mesh_point(rect, view_origin, scale, c0),
                    map_mesh_point(rect, view_origin, scale, c1),
                    map_mesh_point(rect, view_origin, scale, p),
                );
            }
            VectorSegment::Close => {
                if open {
                    builder.close();
                    open = false;
                }
            }
        }
    }
    if open {
        builder.end(false);
    }
    builder.build()
}

fn map_mesh_point(
    rect: crate::tree::Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
    p: [f32; 2],
) -> lyon_tessellation::math::Point {
    point(
        rect.x + (p[0] - view_origin[0]) * scale[0],
        rect.y + (p[1] - view_origin[1]) * scale[1],
    )
}

fn make_mesh_vertex(
    p: lyon_tessellation::math::Point,
    rect: crate::tree::Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
    color: [f32; 4],
    path_index: usize,
    kind: VectorPrimitiveKind,
) -> VectorMeshVertex {
    make_mesh_vertex_with_aa(
        p,
        rect,
        view_origin,
        scale,
        color,
        path_index,
        kind,
        [0.0, 0.0],
    )
}

#[allow(clippy::too_many_arguments)]
fn make_mesh_vertex_with_aa(
    p: lyon_tessellation::math::Point,
    rect: crate::tree::Rect,
    view_origin: [f32; 2],
    scale: [f32; 2],
    color: [f32; 4],
    path_index: usize,
    kind: VectorPrimitiveKind,
    aa: [f32; 2],
) -> VectorMeshVertex {
    let local = [
        view_origin[0] + (p.x - rect.x) / scale[0].max(f32::EPSILON),
        view_origin[1] + (p.y - rect.y) / scale[1].max(f32::EPSILON),
    ];
    VectorMeshVertex {
        pos: [p.x, p.y],
        local,
        color,
        meta: [
            path_index as f32,
            match kind {
                VectorPrimitiveKind::Fill => 0.0,
                VectorPrimitiveKind::Stroke => 1.0,
            },
            0.0,
            0.0,
        ],
        aa,
    }
}

fn resolve_color(color: VectorColor, current_color: Color, opacity: f32) -> [f32; 4] {
    let mut rgba = match color {
        VectorColor::CurrentColor => rgba_f32(current_color),
        VectorColor::Solid(color) => rgba_f32(color),
    };
    rgba[3] *= opacity.clamp(0.0, 1.0);
    rgba
}

fn append_indexed(
    geometry: &VertexBuffers<VectorMeshVertex, u16>,
    out: &mut Vec<VectorMeshVertex>,
) {
    for index in &geometry.indices {
        if let Some(vertex) = geometry.vertices.get(*index as usize) {
            out.push(*vertex);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::{all_icon_names, icon_vector_asset};

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

    #[test]
    fn tessellates_every_builtin_icon() {
        for name in all_icon_names() {
            let mesh = tessellate_vector_asset(
                icon_vector_asset(*name),
                VectorMeshOptions::icon(
                    crate::tree::Rect::new(0.0, 0.0, 16.0, 16.0),
                    Color::rgb(15, 23, 42),
                    2.0,
                ),
            );
            assert!(
                !mesh.vertices.is_empty(),
                "{} produced no tessellated vertices",
                name.name()
            );
        }
    }
}
