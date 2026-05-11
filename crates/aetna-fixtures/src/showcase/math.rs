//! Math — editable markdown math playground.
//!
//! The Typography page carries a compact markdown math sample. This page is
//! the live workbench: edit markdown/TeX on the left and the preview re-renders
//! through the same native math path on the right.

use aetna_core::prelude::*;
use aetna_markdown::{MarkdownOptions, md_with_options};

const SOURCE_KEY: &str = "math-source";

const EULER_KEY: &str = "math-preset-euler";
const FRACTION_KEY: &str = "math-preset-fraction";
const MATRIX_KEY: &str = "math-preset-matrix";
const STRESS_KEY: &str = "math-preset-stress";

const DEFAULT_SOURCE: &str = "\
# Native math

Inline math rides the same baseline as prose: $e^{i\\pi}+1=0$ and \
$x_1+x_2$.

$$
\\frac{a^2+b^2}{\\sqrt{x_1+x_2}} + \\begin{bmatrix}1&0\\\\0&1\\end{bmatrix}
$$
";

const EULER_SOURCE: &str = "\
# Euler identity

Inline: $e^{i\\pi}+1=0$.

$$
e^{i\\pi}+1=0
$$
";

const FRACTION_SOURCE: &str = "\
# Fractions and radicals

Inline: $\\frac{1}{2}$, $\\sqrt{x_1+x_2}$, and $\\sqrt[3]{x+1}$.

$$
\\frac{\\sum_{i=1}^{n}x_i^2}{\\sqrt{x_1+x_2+x_3+x_4}}
$$
";

const MATRIX_SOURCE: &str = "\
# Matrices and fences

$$
\\left[\\begin{array}{lcr}x&10&\\alpha\\\\xx&2&\\beta\\\\xxx&300&\\gamma\\end{array}\\right]
$$

$$
\\left\\{\\begin{matrix}a+b\\\\\\frac{c}{d}\\\\\\sqrt{x+y}\\\\\\sum_{i=1}^{n}i\\end{matrix}\\right.
$$
";

const STRESS_SOURCE: &str = "\
# Inline wrapping

This paragraph puts built-up math near line boundaries: \
$\\frac{a+b}{c+d}$ should keep its baseline, \
$\\sqrt{x_1+x_2+x_3+x_4}$ should stay atomic, and \
$\\left(\\frac{a}{b}\\right)$ should not create odd prose gaps.

Symbols: $\\alpha+\\beta\\to\\gamma$, $\\Delta\\le\\Omega$, and \
$A\\cup B\\cap C\\ne\\emptyset$.
";

#[derive(Clone, Copy)]
struct Preset {
    key: &'static str,
    label: &'static str,
    source: &'static str,
}

const PRESETS: &[Preset] = &[
    Preset {
        key: EULER_KEY,
        label: "Euler",
        source: EULER_SOURCE,
    },
    Preset {
        key: FRACTION_KEY,
        label: "Fractions",
        source: FRACTION_SOURCE,
    },
    Preset {
        key: MATRIX_KEY,
        label: "Matrices",
        source: MATRIX_SOURCE,
    },
    Preset {
        key: STRESS_KEY,
        label: "Wrapping",
        source: STRESS_SOURCE,
    },
];

pub struct State {
    pub source: String,
    pub selection: Selection,
}

impl Default for State {
    fn default() -> Self {
        Self {
            source: DEFAULT_SOURCE.into(),
            selection: Selection::default(),
        }
    }
}

pub fn view(state: &State) -> El {
    scroll([column([
        h1("Math"),
        paragraph(
            "Native math starts in markdown but renders through Aetna's \
             own presentation IR, box layout, and draw ops. Edit the \
             source to exercise inline math, display math, fences, \
             matrices, scripts, and parser errors.",
        )
        .muted(),
        preset_bar(state),
        row([editor_card(state), preview_card(state)])
            .gap(tokens::SPACE_4)
            .align(Align::Stretch)
            .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_4)
    .width(Size::Fill(1.0))])
    .height(Size::Fill(1.0))
}

pub fn on_event(state: &mut State, e: UiEvent) {
    if matches!(e.kind, UiEventKind::Click | UiEventKind::Activate)
        && let Some(route) = e.route()
        && let Some(preset) = PRESETS.iter().find(|preset| preset.key == route)
    {
        state.source = preset.source.into();
        state.selection = Selection::default();
        return;
    }

    if e.target_key() == Some(SOURCE_KEY) {
        text_area::apply_event(&mut state.source, &mut state.selection, SOURCE_KEY, &e);
    }
}

fn preset_bar(state: &State) -> El {
    row([
        text("Presets").label().muted(),
        row(PRESETS.iter().map(|preset| {
            let active = state.source == preset.source;
            let button = button(preset.label).key(preset.key);
            if active {
                button.primary()
            } else {
                button.secondary()
            }
        }))
        .gap(tokens::SPACE_2)
        .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_3)
    .align(Align::Center)
    .width(Size::Fill(1.0))
}

fn editor_card(state: &State) -> El {
    card([
        card_header([
            card_title("Source"),
            card_description("Markdown with TeX math enabled."),
        ]),
        card_content([
            text_area(&state.source, &state.selection, SOURCE_KEY).height(Size::Fixed(330.0))
        ]),
    ])
    .width(Size::Fill(1.0))
}

fn preview_card(state: &State) -> El {
    card([
        card_header([
            card_title("Preview"),
            card_description("Rendered with MarkdownOptions::math(true)."),
        ]),
        card_content([scroll([md_with_options(
            &state.source,
            MarkdownOptions::default().math(true),
        )])
        .key("math-preview")
        .height(Size::Fixed(330.0))]),
    ])
    .width(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(key: &'static str) -> UiEvent {
        UiEvent::synthetic_click(key)
    }

    #[test]
    fn preset_click_replaces_source() {
        let mut state = State::default();

        on_event(&mut state, click(MATRIX_KEY));

        assert_eq!(state.source, MATRIX_SOURCE);
        assert_eq!(state.selection, Selection::default());
    }
}
