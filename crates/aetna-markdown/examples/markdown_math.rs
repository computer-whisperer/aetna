//! markdown_math — visual fixture for native math in markdown.
//!
//! Run: `cargo run -p aetna-markdown --example markdown_math`

use aetna_core::prelude::*;
use aetna_markdown::{MarkdownOptions, md_with_options};

const SOURCE: &str = "\
# Native Math

Inline math should share a baseline with prose: Euler's identity \
$e^{i\\pi}+1=0$ sits inside this paragraph, followed by a nested \
subscript example $x_1+x_2$ and a square root $\\sqrt{x_1+x_2}$.

Display math should center in the available width:

$$
\\sum_{i=1}^{n} x_i + \\frac{a^2+b^2}{\\sqrt{x_1+x_2}}
$$

The first TeX slice intentionally covers the structural basics:
$\\frac{1}{2}$, $\\alpha+\\beta\\to\\gamma$, $\\sqrt[3]{x+1}$, and $y_{n+1}=y_n+x^2$.
";

const MATHML_SOURCE: &str = r#"
<math display="block">
  <mrow>
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
    <mo>+</mo>
    <mroot>
      <mrow>
        <mi>x</mi>
        <mo>+</mo>
        <mn>1</mn>
      </mrow>
      <mn>3</mn>
    </mroot>
    <mo>+</mo>
    <munderover>
      <mo>∑</mo>
      <mrow><mi>i</mi><mo>=</mo><mn>1</mn></mrow>
      <mi>n</mi>
    </munderover>
  </mrow>
</math>
"#;

fn fixture() -> El {
    let (mathml_expr, mathml_display) =
        parse_mathml_with_display(MATHML_SOURCE).expect("fixture MathML parses");
    column([
        md_with_options(SOURCE, MarkdownOptions::default().math(true)),
        divider(),
        h2("MathML input"),
        paragraph("The expression below comes from Presentation MathML and lands on the same native math renderer."),
        math_block(mathml_expr).math_display(mathml_display),
    ])
    .padding(tokens::SPACE_7)
    .width(Size::Fixed(680.0))
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 680.0, 760.0);
    let bundle = render_bundle(&mut root, viewport);

    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");
    let written = write_bundle(&bundle, &out_dir, "markdown_math")?;
    for p in &written {
        println!("wrote {}", p.display());
    }

    if !bundle.lint.findings.is_empty() {
        eprintln!("\nlint findings ({}):", bundle.lint.findings.len());
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}
