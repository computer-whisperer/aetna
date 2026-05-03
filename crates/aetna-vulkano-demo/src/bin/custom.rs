//! Smoke fixture for v5.3 step 7 — register a custom WGSL shader and
//! render through it. Same gradient.wgsl that
//! `aetna-demo/src/bin/render_custom.rs` exercises on the wgpu side, so
//! you can A/B both backends against the same custom shader.
//!
//! What this proves: `Runner::register_shader` runs naga on the WGSL,
//! installs a graphics pipeline against the shared `QuadInstance`
//! layout + descriptor set, and the paint stream picks it up for any
//! `El::shader(ShaderBinding::custom(name))` node — without any
//! aetna-core or aetna-vulkano changes per shader.
//!
//! Run: `cargo run -p aetna-vulkano-demo --bin custom`

use aetna_core::*;

const GRADIENT_WGSL: &str = include_str!("../../../aetna-core/shaders/gradient.wgsl");

fn gradient_button(label: &str, top: Color, bottom: Color, radius: f32) -> El {
    button(label).text_color(tokens::TEXT_ON_SOLID_DARK).shader(
        ShaderBinding::custom("gradient")
            .color("vec_a", top)
            .color("vec_b", bottom)
            .f32("vec_c", radius),
    )
}

struct Custom;

impl App for Custom {
    fn build(&self) -> El {
        column([
            h1("Custom shader (vulkano)"),
            paragraph(
                "Three buttons paint via a registered custom shader \
                 (gradient.wgsl). The right-hand button is stock \
                 rounded_rect for contrast.",
            )
            .muted(),
            row([
                gradient_button(
                    "Sunrise",
                    Color::rgb(255, 200, 90),
                    Color::rgb(245, 95, 110),
                    tokens::RADIUS_MD,
                ),
                gradient_button(
                    "Ocean",
                    Color::rgb(120, 200, 255),
                    Color::rgb(40, 90, 200),
                    tokens::RADIUS_MD,
                ),
                gradient_button(
                    "Forest",
                    Color::rgb(180, 230, 140),
                    Color::rgb(40, 110, 80),
                    tokens::RADIUS_MD,
                ),
                spacer(),
                button("Stock").secondary(),
            ])
            .gap(tokens::SPACE_MD),
        ])
        .gap(tokens::SPACE_LG)
        .padding(tokens::SPACE_XL)
    }

    fn on_event(&mut self, _event: UiEvent) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Rect::new(0.0, 0.0, 720.0, 280.0);
    aetna_vulkano_demo::run_with_init(
        "Aetna — custom shader (vulkano)",
        viewport,
        Custom,
        |runner| runner.register_shader("gradient", GRADIENT_WGSL),
    )
}
