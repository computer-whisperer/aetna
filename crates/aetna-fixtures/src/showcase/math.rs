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
const LIMITS_KEY: &str = "math-preset-limits";
const ERRORS_KEY: &str = "math-preset-errors";
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

const LIMITS_SOURCE: &str = "\
# Large operator limits

Inline large operators stay compact beside prose: $\\sum_{i=1}^{n}x_i$.

$$
\\sum_{i=1}^{n}x_i + \\prod_{k=1}^{m}(1+x_k) + \\bigcup_{j=1}^{r}A_j
$$

$$
\\int_0^1 f(x)dx
$$
";

const ERRORS_SOURCE: &str = "\
# Parser errors

Malformed inline source becomes an explicit math error: $\\frac{a}{$.

Malformed display source does the same:

$$
\\begin{bmatrix}a&b\\\\c\\end{bmatrix}
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
        key: LIMITS_KEY,
        label: "Limits",
        source: LIMITS_SOURCE,
    },
    Preset {
        key: ERRORS_KEY,
        label: "Errors",
        source: ERRORS_SOURCE,
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
        h2("Coverage"),
        coverage_grid(),
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

fn coverage_grid() -> El {
    column([
        row([nested_roots_card(), large_operator_card()])
            .gap(tokens::SPACE_4)
            .align(Align::Stretch)
            .width(Size::Fill(1.0)),
        row([mathml_import_card(), malformed_source_card()])
            .gap(tokens::SPACE_4)
            .align(Align::Stretch)
            .width(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_4)
    .width(Size::Fill(1.0))
}

fn nested_roots_card() -> El {
    demo_card(
        "Nested roots",
        "Radical variants, root indices, and overbars through nested TeX roots.",
        [
            math_block(tex_or_error(r"\sqrt{1+\sqrt{x+\sqrt[3]{y+1}}}")),
            math_block(tex_or_error(
                r"\sqrt[12]{\frac{1+\sqrt{x}}{1+\sqrt[3]{y+\sqrt{z}}}}",
            ))
            .font_size(18.0),
        ],
    )
}

fn large_operator_card() -> El {
    let expr = tex_or_error(r"\sum_{i=1}^{n}x_i+\prod_{k=1}^{m}(1+x_k)");
    let integral = tex_or_error(r"\int_0^1 f(x)dx");
    demo_card(
        "Limit sizing",
        "Display operators use font variants; integrals keep side scripts.",
        [
            math_block(expr.clone()).font_size(18.0),
            math_block(expr).font_size(26.0),
            math_block(integral).font_size(26.0),
        ],
    )
}

fn mathml_import_card() -> El {
    let source = r#"
<math display="block">
  <mfenced open="[" close="]">
    <mtable columnalign="left right" columnspacing="0.6em" rowspacing="0.2em">
      <mtr><mtd><mi>x</mi></mtd><mtd><mn>10</mn></mtd></mtr>
      <mtr><mtd><msup><mi>x</mi><mn>2</mn></msup></mtd><mtd><mn>200</mn></mtd></mtr>
    </mtable>
  </mfenced>
</math>
"#;
    demo_card(
        "MathML import",
        "Presentation MathML table, spacing, alignment, and fence.",
        [math_block(mathml_or_error(source))],
    )
}

fn malformed_source_card() -> El {
    demo_card(
        "Malformed source",
        "Parser failures render as math error expressions.",
        [math_block(tex_or_error(r"\sqrt{1+\frac{x}{"))],
    )
}

fn demo_card<const N: usize>(title: &'static str, description: &'static str, body: [El; N]) -> El {
    card([
        card_header([card_title(title), card_description(description)]),
        card_content([column(body)
            .gap(tokens::SPACE_3)
            .align(Align::Center)
            .width(Size::Fill(1.0))]),
    ])
    .width(Size::Fill(1.0))
}

fn tex_or_error(source: &str) -> MathExpr {
    parse_tex(source)
        .unwrap_or_else(|err| MathExpr::Error(format!("math parse error: {}", err.message)))
}

fn mathml_or_error(source: &str) -> MathExpr {
    parse_mathml(source)
        .unwrap_or_else(|err| MathExpr::Error(format!("mathml parse error: {}", err.message)))
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

    #[test]
    fn error_preset_is_available_from_the_preset_bar() {
        let mut state = State::default();

        on_event(&mut state, click(ERRORS_KEY));

        assert_eq!(state.source, ERRORS_SOURCE);
    }

    #[test]
    fn mathml_showcase_sample_imports_successfully() {
        let expr = mathml_or_error(
            r#"<math><mfenced open="[" close="]"><mtable><mtr><mtd><mi>x</mi></mtd></mtr></mtable></mfenced></math>"#,
        );

        assert!(!matches!(expr, MathExpr::Error(_)));
    }
}
