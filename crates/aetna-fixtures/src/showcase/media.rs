//! Media — images, icons, avatars.
//!
//! Apps construct `Image`s once (typically via `LazyLock` over a
//! decoded byte slice; here we synthesize test patterns in code so the
//! fixture is self-contained — no PNG dep). Equal pixel buffers share
//! a backend texture-cache slot, so the four `image(SOLID.clone())`
//! calls in the avatar row map to one GPU upload.

use std::sync::LazyLock;

use aetna_core::prelude::*;

#[derive(Default)]
pub struct State;

const PIPEWIRE_VOLUME_SVG: &str = include_str!("../../icons/pipewire-volume.svg");

const LINEAR_HORIZONTAL_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="0" y1="32" x2="64" y2="32" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#ff5577"/>
      <stop offset="1" stop-color="#5577ff"/>
    </linearGradient>
  </defs>
  <rect x="4" y="4" width="56" height="56" rx="12" fill="url(#g)"/>
</svg>"##;

const LINEAR_DIAGONAL_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="8" y1="8" x2="56" y2="56" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#22d3ee"/>
      <stop offset="0.5" stop-color="#3b82f6"/>
      <stop offset="1" stop-color="#8b5cf6"/>
    </linearGradient>
  </defs>
  <rect x="4" y="4" width="56" height="56" rx="12" fill="url(#g)"/>
</svg>"##;

const RADIAL_BBOX_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <radialGradient id="g" cx="35%" cy="30%" r="70%">
      <stop offset="0" stop-color="#fef3c7"/>
      <stop offset="0.5" stop-color="#f59e0b"/>
      <stop offset="1" stop-color="#7c2d12"/>
    </radialGradient>
  </defs>
  <circle cx="32" cy="32" r="28" fill="url(#g)"/>
</svg>"##;

const STROKED_GRADIENT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">
  <defs>
    <linearGradient id="g" x1="8" y1="8" x2="56" y2="56" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#10b981"/>
      <stop offset="1" stop-color="#06b6d4"/>
    </linearGradient>
  </defs>
  <path d="M 12 32 A 20 20 0 1 1 52 32" fill="none" stroke="url(#g)" stroke-width="6" stroke-linecap="round"/>
  <line x1="12" y1="48" x2="52" y2="48" stroke="url(#g)" stroke-width="6" stroke-linecap="round"/>
</svg>"##;

static PIPEWIRE_VOLUME: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(PIPEWIRE_VOLUME_SVG).expect("pipewire icon parses"));
static LINEAR_HORIZONTAL: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(LINEAR_HORIZONTAL_SVG).expect("linear-horizontal parses"));
static LINEAR_DIAGONAL: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(LINEAR_DIAGONAL_SVG).expect("linear-diagonal parses"));
static RADIAL_BBOX: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(RADIAL_BBOX_SVG).expect("radial-bbox parses"));
static STROKED_GRADIENT: LazyLock<SvgIcon> =
    LazyLock::new(|| SvgIcon::parse(STROKED_GRADIENT_SVG).expect("stroked-gradient parses"));

static GRID_RG: LazyLock<Image> =
    LazyLock::new(|| make_gradient(64, 64, [255, 64, 64], [64, 96, 255]));
static GRID_GB: LazyLock<Image> =
    LazyLock::new(|| make_gradient(64, 64, [64, 200, 100], [40, 40, 60]));
static GRID_CHECKER: LazyLock<Image> = LazyLock::new(|| make_checker(64, 64, 8));
static GRID_RING: LazyLock<Image> = LazyLock::new(|| make_ring(64, 64));
static AVATAR_SOLID: LazyLock<Image> =
    LazyLock::new(|| Image::from_rgba8(32, 32, vec![255; 32 * 32 * 4]));

fn make_gradient(w: u32, h: u32, top_left: [u8; 3], bottom_right: [u8; 3]) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let t = (x + y) as f32 / (w + h - 2) as f32;
            let r = (top_left[0] as f32 * (1.0 - t) + bottom_right[0] as f32 * t) as u8;
            let g = (top_left[1] as f32 * (1.0 - t) + bottom_right[1] as f32 * t) as u8;
            let b = (top_left[2] as f32 * (1.0 - t) + bottom_right[2] as f32 * t) as u8;
            let i = ((y * w + x) * 4) as usize;
            pixels[i] = r;
            pixels[i + 1] = g;
            pixels[i + 2] = b;
            pixels[i + 3] = 255;
        }
    }
    Image::from_rgba8(w, h, pixels)
}

fn make_checker(w: u32, h: u32, cell: u32) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let on = (((x / cell) + (y / cell)) & 1) == 0;
            let v = if on { 240 } else { 32 };
            let i = ((y * w + x) * 4) as usize;
            pixels[i] = v;
            pixels[i + 1] = v;
            pixels[i + 2] = v;
            pixels[i + 3] = 255;
        }
    }
    Image::from_rgba8(w, h, pixels)
}

fn make_ring(w: u32, h: u32) -> Image {
    let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    let r_outer = w.min(h) as f32 * 0.45;
    let r_inner = r_outer - 6.0;
    for y in 0..h {
        for x in 0..w {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let on = d <= r_outer && d >= r_inner;
            let i = ((y * w + x) * 4) as usize;
            if on {
                pixels[i] = 255;
                pixels[i + 1] = 255;
                pixels[i + 2] = 255;
                pixels[i + 3] = 255;
            } else {
                pixels[i + 3] = 0;
            }
        }
    }
    Image::from_rgba8(w, h, pixels)
}

pub fn view(animated_surface: Option<&AppTexture>) -> El {
    scroll([column([
        h1("Media"),
        paragraph(
            "Three families: raster `image`s, monochrome built-in icons, \
             and gradient-laden custom SVGs through `SvgIcon`. Avatars \
             stack the same primitives — image, fallback initials, or a \
             tinted shape.",
        )
        .muted(),
        section_card(
            "Animated surface (app-owned GPU texture)",
            "`surface(AppTexture)` composites pixels the app writes each frame — \
             3D viewports, video, animated images. The host writes a procedural \
             frame to a 96×96 RGBA8 source texture in `WinitWgpuApp::before_paint`; \
             Aetna samples it across each tile's resolved rect with bilinear \
             filtering, so the small source stretches to fill the cell — source \
             pixel dimensions and rendered size are independent. Three tiles, \
             three `SurfaceAlpha` modes, one shared texture.",
            [animated_surface_demo(animated_surface)],
        ),
        section_card(
            "Avatars",
            "Image, fallback initials, or a colored shape — same anatomy.",
            [row([
                avatar_image(GRID_RG.clone()),
                avatar_image(GRID_GB.clone()),
                avatar_initials("CB"),
                avatar_initials("AK"),
                avatar_fallback("Max Leiter"),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center)],
        ),
        section_card(
            "Raster images",
            "Test patterns generated in code so the fixture is self-contained.",
            [row([
                tile(&GRID_RG, "gradient"),
                tile(&GRID_GB, "moss"),
                tile(&GRID_CHECKER, "checker"),
                tile(&GRID_RING, "ring"),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center)],
        ),
        section_card(
            "Tints share one texture",
            "Four references to the same Image with different `image_tint(...)` colors — content-hashed into one GPU upload.",
            [row([
                tinted_avatar(Color::rgb(96, 165, 250)),
                tinted_avatar(Color::rgb(244, 114, 182)),
                tinted_avatar(Color::rgb(248, 113, 113)),
                tinted_avatar(Color::rgb(132, 204, 22)),
            ])
            .gap(tokens::SPACE_2)],
        ),
        section_card(
            "ImageFit modes",
            "Same image, four projections into identically-sized boxes.",
            [row([
                fit_demo("Contain", ImageFit::Contain),
                fit_demo("Cover", ImageFit::Cover),
                fit_demo("Fill", ImageFit::Fill),
                fit_demo("None", ImageFit::None),
            ])
            .gap(tokens::SPACE_3)],
        ),
        section_card(
            "Built-in lucide icons (monochrome / MSDF)",
            "`icon(IconName::*)` paints through the monochrome MSDF atlas.",
            [row([
                builtin_icon_tile(IconName::Activity, tokens::WARNING),
                builtin_icon_tile(IconName::Bell, tokens::PRIMARY),
                builtin_icon_tile(IconName::Check, tokens::SUCCESS),
                builtin_icon_tile(IconName::AlertCircle, tokens::DESTRUCTIVE),
                builtin_icon_tile(IconName::Settings, tokens::FOREGROUND),
            ])
            .gap(tokens::SPACE_3)],
        ),
        section_card(
            "Gradient SVGs (custom / tessellated)",
            "App-supplied SvgIcon — gradients route through the tessellated path with per-vertex colour.",
            [row([
                custom_icon_tile(&PIPEWIRE_VOLUME, "pipewire", 72.0),
                custom_icon_tile(&LINEAR_HORIZONTAL, "linear h", 56.0),
                custom_icon_tile(&LINEAR_DIAGONAL, "diagonal", 56.0),
                custom_icon_tile(&RADIAL_BBOX, "radial", 56.0),
                custom_icon_tile(&STROKED_GRADIENT, "stroked", 56.0),
            ])
            .gap(tokens::SPACE_3)
            .align(Align::Center)],
        ),
        section_card(
            "Programmatic vectors (vector() + PathBuilder)",
            "`vector(asset)` paints a programmatically-built `VectorAsset` through \
             the icon MSDF atlas. Single-solid-colour assets route to MSDF for crisp \
             scaling; multi-colour or gradient assets fall back to lyon tessellation. \
             Identical geometry hashes into one atlas slot, so a list of merge curves \
             with recurring (lane_delta, row_span) pairs shares cached rasterisations.",
            [vector_demo_row()],
        ),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)])
    .height(Size::Fill(1.0))
}

fn vector_demo_row() -> El {
    row([
        vector_tile("merge curve", curve_asset(0, 3, 4), 80.0, 100.0),
        vector_tile("steeper curve", curve_asset(0, 4, 3), 100.0, 80.0),
        vector_tile("filled diamond", diamond_asset(Color::rgb(244, 114, 182)), 48.0, 48.0),
        vector_tile(
            "rounded path",
            squiggle_asset(Color::rgb(96, 165, 250)),
            120.0,
            48.0,
        ),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Center)
}

fn vector_tile(label: &str, asset: VectorAsset, w: f32, h: f32) -> El {
    column([
        El::new(Kind::Group)
            .padding(tokens::SPACE_2)
            .child(vector(asset).width(Size::Fixed(w)).height(Size::Fixed(h)))
            .surface_role(SurfaceRole::Sunken)
            .radius(tokens::RADIUS_MD),
        text(label.to_string()).small().muted(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
}

fn curve_asset(start_lane: i32, end_lane: i32, row_span: u32) -> VectorAsset {
    let lane_w = 24.0;
    let row_h = 24.0;
    let dx = (end_lane - start_lane) as f32 * lane_w;
    let dy = row_span as f32 * row_h;
    let path = PathBuilder::new()
        .move_to(0.0, 0.0)
        .cubic_to(0.0, dy * 0.5, dx, dy * 0.5, dx, dy)
        .stroke_solid(Color::rgb(132, 204, 22), 2.0)
        .stroke_line_cap(VectorLineCap::Round)
        .build();
    VectorAsset::from_paths([0.0, 0.0, dx.abs().max(0.001), dy], vec![path])
}

fn diamond_asset(color: Color) -> VectorAsset {
    let r = 12.0;
    let path = PathBuilder::new()
        .move_to(r, 0.0)
        .line_to(2.0 * r, r)
        .line_to(r, 2.0 * r)
        .line_to(0.0, r)
        .close()
        .fill_solid(color)
        .build();
    VectorAsset::from_paths([0.0, 0.0, 2.0 * r, 2.0 * r], vec![path])
}

fn squiggle_asset(color: Color) -> VectorAsset {
    let path = PathBuilder::new()
        .move_to(0.0, 12.0)
        .quad_to(15.0, 0.0, 30.0, 12.0)
        .quad_to(45.0, 24.0, 60.0, 12.0)
        .stroke_solid(color, 2.0)
        .stroke_line_cap(VectorLineCap::Round)
        .build();
    VectorAsset::from_paths([0.0, 0.0, 60.0, 24.0], vec![path])
}

fn section_card<I: IntoIterator<Item = El>>(title: &str, blurb: &str, body: I) -> El {
    titled_card(
        title,
        std::iter::once(paragraph(blurb).muted().small())
            .chain(body)
            .collect::<Vec<_>>(),
    )
}

fn tile(img: &LazyLock<Image>, label: &str) -> El {
    column([
        image((*img).clone())
            .width(Size::Fixed(96.0))
            .height(Size::Fixed(96.0))
            .image_fit(ImageFit::Contain)
            .radius(tokens::RADIUS_MD),
        text(label.to_string()).small().muted(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
}

fn tinted_avatar(tint: Color) -> El {
    image(AVATAR_SOLID.clone())
        .width(Size::Fixed(48.0))
        .height(Size::Fixed(48.0))
        .image_fit(ImageFit::Fill)
        .image_tint(tint)
        .radius(24.0)
}

fn fit_demo(label: &str, fit: ImageFit) -> El {
    column([
        image(GRID_RG.clone())
            .width(Size::Fixed(96.0))
            .height(Size::Fixed(48.0))
            .image_fit(fit)
            .radius(tokens::RADIUS_SM)
            .stroke(tokens::BORDER),
        text(label.to_string()).small().muted(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
}

fn builtin_icon_tile(name: IconName, tint: Color) -> El {
    card([
        icon(name).icon_size(28.0).text_color(tint),
        text(name.name()).small().muted().center_text(),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
    .padding(tokens::SPACE_3)
    .radius(tokens::RADIUS_MD)
}

fn custom_icon_tile(svg: &LazyLock<SvgIcon>, label: &str, size: f32) -> El {
    card([
        icon((**svg).clone()).icon_size(size),
        text(label.to_string()).small().muted().center_text(),
    ])
    .gap(tokens::SPACE_2)
    .align(Align::Center)
    .padding(tokens::SPACE_3)
    .radius(tokens::RADIUS_MD)
}

/// Three tiles, each showing the animated surface under a different
/// `SurfaceAlpha` mode against a colored backdrop. The shared
/// `AppTexture` is cheap to clone (Arc-backed); each tile owns its
/// own composite, so the same pixel data lights up the three blend
/// paths simultaneously.
fn animated_surface_demo(tex: Option<&AppTexture>) -> El {
    let Some(tex) = tex else {
        return paragraph(
            "This demo requires a wgpu host that allocates an `AppTexture` and \
             pushes a frame each tick (see `examples/src/bin/showcase.rs`). \
             Headless render bundles render this card as a placeholder.",
        )
        .muted()
        .small();
    };
    row([
        animated_surface_cell("Premultiplied", tokens::PRIMARY, tex.clone(), SurfaceAlpha::Premultiplied),
        animated_surface_cell("Straight", tokens::SECONDARY, tex.clone(), SurfaceAlpha::Straight),
        animated_surface_cell("Opaque", tokens::ACCENT, tex.clone(), SurfaceAlpha::Opaque),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Stretch)
}

fn animated_surface_cell(
    label: &str,
    backdrop: Color,
    tex: AppTexture,
    alpha: SurfaceAlpha,
) -> El {
    // The cell column uses the default `Align::Stretch` so the stack's
    // `Size::Fill(1.0)` width actually claims the cell's full width —
    // a `Center`-aligned column collapses Fill children to their
    // intrinsic, which is zero here since the stack's own children
    // all Fill recursively.
    column([
        text(label.to_string()).small().muted(),
        stack([
            // Backdrop — Premultiplied / Straight let it show through
            // wherever the texture has alpha < 1; Opaque overwrites.
            El::default()
                .fill(backdrop)
                .radius(tokens::RADIUS_MD)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
            // The animated surface.
            surface(tex)
                .surface_alpha(alpha)
                .width(Size::Fill(1.0))
                .height(Size::Fill(1.0)),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fixed(120.0)),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0))
}
