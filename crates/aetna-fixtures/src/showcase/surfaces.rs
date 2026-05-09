//! Surfaces — surface roles, drop shadows, custom-shader chrome.
//!
//! Demos how the panel chrome looks at different elevations and via
//! different palette tokens, and includes the liquid-glass custom
//! shader as the showpiece for "any El can mount a custom WGSL surface
//! and the layer ordering still works."

use aetna_core::prelude::*;

const GLASS_NEXT_KEY: &str = "surfaces-glass-next";
const GLASS_DRIFT_KEY: &str = "surfaces-glass-drift";

#[derive(Default)]
pub struct State {
    pub glass_preset: usize,
    pub glass_drift: usize,
}

#[derive(Clone, Copy)]
struct GlassPreset {
    label: &'static str,
    blurb: &'static str,
    blur_px: f32,
    refraction: f32,
    specular: f32,
    tint: Color,
}

const GLASS_PRESETS: &[GlassPreset] = &[
    GlassPreset {
        label: "Soft",
        blurb: "Gentle blur, faint warm tint, soft bevel.",
        blur_px: 4.0,
        refraction: 0.45,
        specular: 0.8,
        tint: Color {
            r: 240,
            g: 240,
            b: 250,
            a: 110,
            token: None,
        },
    },
    GlassPreset {
        label: "Heavy",
        blurb: "Wide blur, stronger refraction at the rim.",
        blur_px: 10.0,
        refraction: 0.85,
        specular: 1.1,
        tint: Color {
            r: 230,
            g: 235,
            b: 250,
            a: 140,
            token: None,
        },
    },
    GlassPreset {
        label: "Cool",
        blurb: "Cool blue tint, crisp specular bevel.",
        blur_px: 6.0,
        refraction: 0.55,
        specular: 1.4,
        tint: Color {
            r: 180,
            g: 215,
            b: 255,
            a: 170,
            token: None,
        },
    },
    GlassPreset {
        label: "Crisp",
        blurb: "Minimal blur, pure refraction lensing.",
        blur_px: 1.5,
        refraction: 0.95,
        specular: 1.6,
        tint: Color {
            r: 250,
            g: 250,
            b: 255,
            a: 60,
            token: None,
        },
    },
];

const DRIFT_OFFSETS: &[f32] = &[0.0, -120.0, 120.0];

pub fn view(state: &State) -> El {
    scroll([column([
        h1("Surfaces"),
        paragraph(
            "How the panel chrome looks. Surface roles slot tokenized \
             palette colours into stock components; drop shadows give \
             layered surfaces a sense of elevation; and the liquid-glass \
             card at the bottom proves any El can mount a custom WGSL \
             shader without losing layer compositing.",
        )
        .muted(),
        section_label("Surface roles"),
        paragraph(
            "Each role binds a palette token to a stock surface — \
             swapping themes via the sidebar picker swaps these live.",
        )
        .small()
        .muted(),
        row([
            surface_role_tile("Panel", "tokens::CARD", tokens::CARD),
            surface_role_tile("Popover", "tokens::POPOVER", tokens::POPOVER),
            surface_role_tile("Muted", "tokens::MUTED", tokens::MUTED),
            surface_role_tile("Accent", "tokens::ACCENT", tokens::ACCENT),
        ])
        .gap(tokens::SPACE_3)
        .align(Align::Stretch),
        section_label("Drop shadows"),
        paragraph(
            "Drop shadows on the dark theme are subtle by design — 30% \
             black on a near-black background only darkens it by a few \
             channel codes. Tiles below cast SHADOW_SM / SHADOW_MD / \
             SHADOW_LG against an ACCENT panel so the falloff stands out.",
        )
        .muted()
        .small(),
        row([
            elevation_tile("shadow_sm", "4 px", tokens::SHADOW_SM),
            elevation_tile("shadow_md", "12 px", tokens::SHADOW_MD),
            elevation_tile("shadow_lg", "24 px", tokens::SHADOW_LG),
        ])
        .gap(tokens::SPACE_4)
        .padding(tokens::SPACE_5)
        .fill(tokens::ACCENT)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_LG),
        paragraph(
            "Stock cards and popovers pin their shadow through SurfaceRole \
             — Panel → SHADOW_SM, Popover → SHADOW_LG — so .shadow(...) \
             on a card is overridden at theme time. Set \
             surface_role(SurfaceRole::None) (or skip card/popover and \
             compose by hand) to paint a custom shadow value verbatim.",
        )
        .muted()
        .small(),
        section_label("Custom-shaded surface"),
        paragraph(
            "`liquid_glass.wgsl` reads the snapshot beneath the card, \
             blurs and refracts it, and tints the result. Any El can \
             mount a custom shader with `.shader(ShaderBinding::custom)` \
             — the runtime orchestrates Pass A → snapshot → Pass B \
             around it.",
        )
        .muted(),
        glass_demo(state),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Stretch)])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if !matches!(e.kind, UiEventKind::Click | UiEventKind::Activate) {
        return;
    }
    match e.route() {
        Some(GLASS_NEXT_KEY) => state.glass_preset = (state.glass_preset + 1) % GLASS_PRESETS.len(),
        Some(GLASS_DRIFT_KEY) => state.glass_drift = (state.glass_drift + 1) % DRIFT_OFFSETS.len(),
        _ => {}
    }
}

fn section_label(s: &str) -> El {
    h3(s).label()
}

fn surface_role_tile(title: &str, token_name: &str, fill: Color) -> El {
    card([text(title).label(), text(token_name).caption().muted()])
        .gap(tokens::SPACE_1)
        .padding(tokens::SPACE_3)
        .fill(fill)
        .radius(tokens::RADIUS_MD)
        .height(Size::Fixed(76.0))
}

fn elevation_tile(label: &str, sub: &str, shadow: f32) -> El {
    card([text(label).title(), text(sub).muted().small()])
        .shadow(shadow)
        .padding(tokens::SPACE_4)
        .gap(tokens::SPACE_1)
        .height(Size::Fixed(120.0))
}

fn glass_backdrop() -> El {
    // Stripes use status tokens — they swap with the theme so the glass
    // demo stays vivid under any palette without hard-coding colors.
    fn stripe(c: Color) -> El {
        column(Vec::<El>::new()).fill(c).width(Size::Fill(1.0))
    }
    row([
        stripe(tokens::DESTRUCTIVE),
        stripe(tokens::SUCCESS),
        stripe(tokens::INFO),
        stripe(tokens::WARNING),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn glass_card(preset: &GlassPreset, drift_x: f32) -> El {
    column([
        text("Liquid glass")
            .bold()
            .font_size(22.0)
            .text_color(tokens::PRIMARY_FOREGROUND),
        text(preset.blurb).text_color(tokens::PRIMARY_FOREGROUND),
        spacer(),
        row([
            text(format!("preset: {}", preset.label))
                .bold()
                .text_color(tokens::PRIMARY_FOREGROUND),
            spacer(),
            button("Next preset").key(GLASS_NEXT_KEY).secondary(),
            button("Drift →").key(GLASS_DRIFT_KEY).primary(),
        ])
        .gap(tokens::SPACE_2),
    ])
    .gap(tokens::SPACE_2)
    .padding(tokens::SPACE_4)
    .shader(
        ShaderBinding::custom("liquid_glass")
            .color("vec_a", preset.tint)
            .vec4(
                "vec_b",
                [preset.blur_px, preset.refraction, preset.specular, 0.0],
            )
            .vec4("vec_c", [28.0, 0.0, 0.0, 0.0]),
    )
    .width(Size::Fixed(420.0))
    .height(Size::Fixed(220.0))
    .translate(drift_x, 0.0)
    .animate(Timing::SPRING_BOUNCY)
}

fn glass_demo(state: &State) -> El {
    let preset = &GLASS_PRESETS[state.glass_preset % GLASS_PRESETS.len()];
    let drift_x = DRIFT_OFFSETS[state.glass_drift % DRIFT_OFFSETS.len()];
    stack([glass_backdrop(), glass_card(preset, drift_x)])
        .align(Align::Center)
        .justify(Justify::Center)
        .height(Size::Fixed(280.0))
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_LG)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn glass_next_cycles_through_presets() {
        let mut s = State::default();
        assert_eq!(s.glass_preset, 0);
        on_event(&mut s, click(GLASS_NEXT_KEY));
        assert_eq!(s.glass_preset, 1);
        for _ in 0..GLASS_PRESETS.len() - 1 {
            on_event(&mut s, click(GLASS_NEXT_KEY));
        }
        assert_eq!(s.glass_preset, 0);
    }

    #[test]
    fn glass_drift_cycles_horizontal_offsets() {
        let mut s = State::default();
        assert_eq!(DRIFT_OFFSETS[s.glass_drift], 0.0);
        on_event(&mut s, click(GLASS_DRIFT_KEY));
        assert_ne!(DRIFT_OFFSETS[s.glass_drift], 0.0);
        for _ in 0..DRIFT_OFFSETS.len() - 1 {
            on_event(&mut s, click(GLASS_DRIFT_KEY));
        }
        assert_eq!(DRIFT_OFFSETS[s.glass_drift], 0.0);
    }

    #[test]
    fn drift_offsets_stay_inside_content_bounds() {
        // Glass card is 420 wide; showcase content area is ~720 wide
        // (900 viewport − 180 sidebar). Half the spare room is 150 —
        // any drift offset beyond that pushes the card past the panel
        // edge or into the sidebar.
        for &offset in DRIFT_OFFSETS {
            assert!(
                offset.abs() <= 150.0,
                "drift offset {offset} exceeds safe range"
            );
        }
    }
}
