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
\\frac{a^2+b^2}{\\sqrt{x_1+x_2}}
$$

The first TeX slice intentionally covers the structural basics:
$\\frac{1}{2}$, $\\alpha+\\beta\\to\\gamma$, and $y_{n+1}=y_n+x^2$.
";

fn fixture() -> El {
    column([md_with_options(
        SOURCE,
        MarkdownOptions::default().math(true),
    )])
    .padding(tokens::SPACE_7)
    .width(Size::Fixed(680.0))
}

fn main() -> std::io::Result<()> {
    let mut root = fixture();

    let viewport = Rect::new(0.0, 0.0, 680.0, 560.0);
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
