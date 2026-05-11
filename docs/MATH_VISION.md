# Aetna Math Vision

This is the maintainer-facing architecture note for native math rendering in
Aetna. Public author guidance belongs in crate READMEs and rustdoc once the
surface is stable enough to document as supported API.

## Goal

Aetna should have a good native math renderer that can serve markdown first,
but is not markdown-specific. The feature should accept common math sources,
normalize them into an Aetna-owned presentation IR, lay them out through native
Rust code, and render through the existing backend-neutral paint stream.

The main expected inputs are:

- markdown math: `$...$` and `$$...$$`, conventionally TeX,
- raw presentation MathML / MathML Core,
- programmatic math builders from app code,
- possible later adapters such as AsciiMath or Typst-like syntax.

The core renderer should not treat TeX as the internal truth. TeX is an
authoring language. MathML Core is the best conceptual normalization target,
but Aetna should own its IR instead of exposing an external MathML crate's type
as public API.

## Architecture

The intended stack is:

```text
input syntax -> parser/adapter -> Aetna math IR -> math box layout -> Aetna draw ops
```

### Input Adapters

Input adapters parse source notation and produce Aetna math IR:

- TeX adapter for markdown and common authoring workflows.
- MathML Core adapter for browser/platform-style interchange.
- Programmatic builders for code-authored formulae.

Adapters should be allowed to live outside `aetna-core` if they pull parser
dependencies. `aetna-core` should only need the normalized math IR, layout, and
rendering path.

### Normalized Math IR

The current first slice uses `aetna_core::math::MathExpr`. It is a
presentation-math IR, not a semantic algebra system. It is intentionally shaped
like a small subset of MathML Core:

- `Row`
- `Identifier`
- `Number`
- `Operator`
- `Text`
- `Space`
- `Fraction`
- `Sqrt`
- `Scripts`
- `Error`

Expected expansions:

- `Root`
- `UnderOver`
- `Table`
- `Style`
- `Phantom`
- explicit operator metadata
- source spans / original source for copy and diagnostics

### Layout IR

Math layout should lower the expression tree into TeX/OpenType-MATH-style
boxes, not into `El` children. The important intermediate values are:

- `width`
- `ascent`
- `descent`
- baseline-relative positioned atoms

The current first slice exposes `MathLayout` and `MathAtom` with glyph and rule
atoms. That is sufficient for simple text, scripts, fractions, and temporary
sqrt rendering. It is not the final representation for high-quality radicals,
stretchy delimiters, matrices, or large operators.

### Draw Ops

Math should render through normal Aetna paint plumbing. The first slice lowers
math atoms directly to existing draw ops:

- glyph atoms -> `DrawOp::GlyphRun`
- rule atoms -> rounded-rect quads with zero radius

This was enough to avoid backend-specific pipeline work. A future dedicated
`DrawOp::Math` can still be introduced if the paint atoms become too rich to
lower cleanly during `draw_ops`.

## Current First Slice

The current in-progress implementation has:

- `aetna_core::math::{MathExpr, MathDisplay, MathLayout, MathAtom}`
- a small `parse_tex` helper in core for the initial vertical slice
- a small `parse_mathml` / `parse_mathml_with_display` adapter for the
  matching Presentation MathML subset
- `Kind::Math`
- `math`, `math_inline`, and `math_block` constructors
- layout support for standalone math nodes
- mixed inline support inside `text_runs`
- markdown opt-in through `MarkdownOptions::math(true)`
- a visual bundle fixture: `cargo run -p aetna-markdown --example markdown_math`

The supported TeX subset is deliberately small:

- rows and grouped expressions
- identifiers, numbers, operators, and common Greek/operator commands
- `\frac`
- `\sqrt`
- superscripts and subscripts

The supported MathML subset mirrors that same IR:

- `math`, `mrow`
- `mi`, `mn`, `mo`, `mtext`, `mspace`
- `mfrac`
- `msqrt`
- `msub`, `msup`, `msubsup`

This is enough to render smoke examples such as:

```text
$e^{i\pi}+1=0$
$$\frac{a^2+b^2}{\sqrt{x_1+x_2}}$$
```

## Visual Findings So Far

The first visual fixture was valuable. It exposed two issues immediately:

- Mixed inline paragraphs cannot treat text runs as atomic when math appears
  inside them. The current fix tokenizes text into word/space chunks while
  keeping math expressions atomic.
- Inline built-up fractions need a math axis above the prose baseline. The
  current fraction layout raises the rule and tightens inline fraction spacing.

The radical rendering is intentionally not considered solved. The current
implementation paints `√` plus a separate overbar rule. Attempts to overlap the
rule into the glyph made the result worse. The right fix is a proper radical
construction, not more hand-tuning of the normal square-root glyph.

## Next Work Packages

### 1. Radical Construction

Replace the temporary `√` plus rule approach with a real radical assembly:

- Prefer OpenType MATH glyph variants / assemblies from Noto Sans Math when
  available.
- If the text stack does not expose enough MATH-table data yet, add a native
  vector radical path sized from the box metrics.
- Keep the overbar and radical check visually joined at multiple font sizes.

This should be treated as part of the math layout project, not as a markdown
transformer concern.

### 2. Font And Math Constants

Move layout constants toward OpenType MATH data:

- math axis height
- fraction rule thickness
- numerator/denominator gaps
- script shifts
- radical vertical gap and rule thickness
- delimiter variants / assemblies

The bundled Noto Sans Math face is already available through the default
symbols font bundle, so the missing piece is reading and applying math-table
metrics.

### 3. Inline Layout Quality

The current mixed inline path is intentionally small and separate from the
normal attributed-text path. It should converge toward a real inline item
layout:

- text shaping segments,
- atomic math embeds,
- baseline alignment per line,
- wrapping across words and embeds,
- link and selection metadata,
- compatible SVG/bundle output.

The normal `Kind::Inlines` attributed text path remains better for pure prose.
The mixed path should become good enough that adding one formula to a paragraph
does not materially degrade prose layout.

### 4. Parser Boundaries

The current `parse_tex` helper is a bootstrap parser, not a full TeX engine.
Before widening support, decide where parser crates belong:

- Keep `aetna-core` parser-light.
- Move richer TeX / MathML parsing into a future `aetna-math` crate if the
  dependency surface grows.
- Keep `MathExpr` or its successor as the stable normalized API boundary.

MathML Core import should target the Aetna IR, not bypass it.

### 5. Coverage And Fixtures

Add visual fixtures for:

- inline fractions at several font sizes,
- display fractions,
- nested scripts,
- radicals with short and long radicands,
- Greek and operator fallback coverage,
- wrapping paragraphs with math near line boundaries.

The bundle pipeline is the right first gate. GPU screenshots can follow once
the SVG/bundle artifact looks plausible.

## Non-Goals For The First Phase

- Full TeX macro expansion.
- Semantic math / CAS behavior.
- Complete historical MathML.
- Perfect browser parity.
- Editable equation UI.
- Accessibility beyond preserving source and future semantic metadata.

## Acceptance Bar

The first practical acceptance target is:

- markdown can opt into native math,
- common simple formulae render without tofu,
- inline formulae keep reasonable prose wrapping and baselines,
- display formulae center and reserve enough vertical space,
- SVG bundle output and wgpu/vulkano paths use the same core layout decisions,
- unsupported syntax degrades into an explicit math error expression rather
  than panicking.
