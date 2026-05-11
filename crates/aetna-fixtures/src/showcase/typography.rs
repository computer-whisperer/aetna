//! Typography + long-form content.
//!
//! Demonstrates the heading / paragraph / list / quote / code-block
//! vocabulary by hand, then renders the same vocabulary through
//! `aetna_markdown::md` so the markdown transformer's output is
//! visible alongside the hand-authored variant.

use aetna_core::prelude::*;
use aetna_markdown::{MarkdownOptions, md_with_options};

#[derive(Default)]
pub struct State;

const MARKDOWN_SOURCE: &str = "\
## Markdown

`aetna_markdown::md` walks `pulldown-cmark` events into Aetna widgets — \
the same widget kit the hand-authored examples above compose. Inline runs \
cover **bold**, *italic*, `inline code`, ~~strike~~, and \
[links](https://aetna.dev), all flowing through one wrapping paragraph.

### Lists

- Bullet items wrap under themselves with a hanging indent.
- Inline runs work inside items: **bold**, *italic*, `code`, \
  [links](https://aetna.dev).
- Nested lists live inside a composite item.

42. Numbered lists preserve custom starts.
43. The next marker continues from the source.

- [x] Completed task items use static checkbox markers.
- [ ] Pending task items use the same hanging indent.

### Quote

> Markdown's shape is HTML's shape. Aetna's widget kit already \
> mirrors most of that shape, so the transformer mostly hands events \
> to existing constructors.

### Fenced code

```rust
// aetna-markdown highlights fenced code with a recognised lang tag.
fn render(md: &str) -> El {
    aetna_markdown::md(md)
}
```

### Tables

| Construct  | Maps to            |
|------------|--------------------|
| Heading    | `h1` / `h2` / `h3` |
| List       | `bullet_list` / `numbered_list` |
| Blockquote | `blockquote`       |
| Code block | `code_block`       |
| Table      | `table`            |

### Native math

Inline math shares a prose baseline: $e^{i\\pi}+1=0$, \
$x_1+x_2$, and $\\sqrt{x_1+x_2}$.

$$
\\frac{a^2+b^2}{\\sqrt{x_1+x_2}} + \\begin{bmatrix}1&0\\\\0&1\\end{bmatrix}
$$
";

pub fn view() -> El {
    scroll([column([
        h1("Typography"),
        paragraph(
            "Long-form-content widgets compose the same markdown-shaped \
             vocabulary the `aetna_markdown::md` transformer targets — \
             headings, paragraphs, lists, quotes, code blocks, links. \
             Each is plain Aetna with selectable text and themed surfaces.",
        )
        .muted(),
        h2("Headings"),
        h3("Subheading"),
        text_runs([
            text("Headings stack at "),
            text("h1").code(),
            text(" / "),
            text("h2").code(),
            text(" / "),
            text("h3").code(),
            text(" — the "),
            text("display").italic(),
            text(", "),
            text("heading").italic(),
            text(", and "),
            text("title").italic(),
            text(" text roles, respectively."),
        ])
        .wrap_text()
        .width(Size::Fill(1.0))
        .height(Size::Hug),
        h2("Bulleted list"),
        bullet_list(vec![
            text("Plain string items wrap inside the content column so a long item flows under itself rather than under the bullet."),
            text_runs([
                text("Inline runs work in items: "),
                text("bold").bold(),
                text(", "),
                text("italic").italic(),
                text(", "),
                text("code").code(),
                text(", "),
                text("links").link("https://aetna.dev"),
                text("."),
            ]),
            column([
                paragraph("Composite items host nested blocks — a paragraph, then a sub-list:"),
                bullet_list(["nested one", "nested two"]),
            ])
            .gap(tokens::SPACE_2)
            .width(Size::Fill(1.0)),
        ]),
        h2("Numbered list"),
        numbered_list([
            "Markers right-align so the period sits flush across items.",
            "Marker-slot width grows with the item count — `9.` and `99.` lay out without crowding the content.",
            "Plain-text items wrap inside the content column, same convention as the bullet list.",
        ]),
        h2("Blockquote"),
        blockquote([
            paragraph(
                "Markdown's shape is HTML's shape. Aetna's widget kit \
                 already mirrors most of that shape, so the transformer \
                 mostly hands events to existing constructors.",
            ),
            paragraph("— Aetna design notes").muted(),
        ]),
        h2("Code block"),
        code_block(
            "fn render(md: &str) -> El {\n    \
                 aetna_markdown::md(md)\n}",
        ),
        h2("Inline runs"),
        text_runs([
            text("Inline runs carry "),
            text("underline").underline(),
            text(", "),
            text("strikethrough").strikethrough(),
            text(", and "),
            text("links").link("https://aetna.dev"),
            text(" via per-run flags. The decoration bar tracks each run's "),
            text("color").italic(),
            text(" automatically."),
        ])
        .wrap_text(),
        separator(),
        paragraph(
            "Below: the same vocabulary rendered from a markdown source string \
             through `aetna_markdown::md`, so the transformer's output sits \
             next to the hand-authored variant.",
        )
        .muted()
        .small(),
        md_with_options(MARKDOWN_SOURCE, MarkdownOptions::default().math(true)),
    ])
    .gap(tokens::SPACE_4)
    .align(Align::Start)
    .width(Size::Fill(1.0))])
    .height(Size::Fill(1.0))
}
