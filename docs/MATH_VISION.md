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
- `Root`
- `Scripts`
- `UnderOver`
- `Accent`
- `Fenced`
- `Table`
- `Error`

Expected expansions:

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

The current first slice exposes `MathLayout` and `MathAtom` with glyph, rule,
and radical atoms. That is sufficient for simple text, scripts, fractions, and
basic square-root rendering. It is not the final representation for
stretchy delimiters, matrices, large operators, or OpenType-MATH-quality
radical assemblies.

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
- an internal `MathMetrics` layer that centralizes current math sizing
  constants before they are replaced with OpenType MATH values
- a small `parse_tex` helper in core for the initial vertical slice
- a small `parse_mathml` / `parse_mathml_with_display` adapter for the
  matching Presentation MathML subset
- OpenType MATH-backed radical glyph variants with a native vector fallback
- `Kind::Math`
- `math`, `math_inline`, and `math_block` constructors
- layout support for standalone math nodes
- mixed inline support inside `text_runs`
- markdown opt-in through `MarkdownOptions::math(true)`
- a visual bundle fixture: `cargo run -p aetna-markdown --example markdown_math`

The supported TeX subset is deliberately small:

- rows and grouped expressions
- identifiers, numbers, operators, and common Greek/operator commands
- text groups such as `\text`, `\mathrm`, and `\operatorname`
- `\frac`
- `\sqrt` and indexed `\sqrt[n]{...}`
- accents such as `\hat`, `\bar`, `\overline`, `\vec`, and `\tilde`
- superscripts and subscripts
- display-style limits for common large operators such as `\sum`
- simple `\left...\right` fences
- matrix-like environments lowered to the shared table IR: `matrix`,
  `pmatrix`, `bmatrix`, `Bmatrix`, `vmatrix`, `Vmatrix`, and `cases`
- `array` environments with simple `l`, `c`, and `r` column alignment specs

The supported MathML subset mirrors that same IR:

- `math`, `mrow`, `semantics` with annotations ignored
- `mi`, `mn`, `mo`, `mtext`, `mspace`
- `mfrac`
- `msqrt`, `mroot`
- `msub`, `msup`, `msubsup`
- `munder`, `mover`, `munderover`, with explicit accent movers
- `mfenced`
- `mtable`, `mtr`, `mtd`
- table-level `columnalign` values `left`, `center`, `right`, and `decimal`
- table-level `columnspacing` and `rowspacing` when expressed as `em` values

This is enough to render smoke examples such as:

```text
$e^{i\pi}+1=0$
$$\frac{a^2+b^2}{\sqrt{x_1+x_2}}$$
\left(\frac{a}{b}\right)
\begin{bmatrix} a & b \\ c & d \end{bmatrix}
\begin{array}{lr} x & 100 \\ xx & 2 \end{array}
```

## Visual Findings So Far

The first visual fixture was valuable. It exposed two issues immediately:

- Mixed inline paragraphs cannot treat text runs as atomic when math appears
  inside them. The current fix tokenizes text into word/space chunks while
  keeping math expressions atomic, then batches same-style text back into
  line-local glyph runs so ordinary prose spacing is not degraded by the math
  embed path.
- Inline built-up fractions need a math axis above the prose baseline. The
  current fraction layout raises the rule and tightens inline fraction spacing.
- SVG fixture PNGs must render with the same bundled font family that core
  layout measured. The SVG bundle path emits resolved Aetna family names, and
  `tools/svg_to_png.sh` supplies fontconfig paths for the bundled faces.

The radical rendering has moved past the first `√` glyph plus separate overbar
rule and the later heuristic-only vector radical. The current implementation
queries the bundled Noto Sans Math MATH table for a vertical radical variant,
emits that exact glyph outline through the same glyph-id vector path used by
stretchy delimiters, then extends the overbar as a rule over the radicand. The
native vector radical remains as the fallback when a font does not expose a
usable radical variant.

Display-style large operators have started moving onto that same exact-glyph
path. Sums, products, big intersections, and big unions with limits now prefer
OpenType MATH vertical variants from Noto Sans Math instead of scaling a text
glyph, so the operator grows without getting artificially heavy.

Fenced delimiters follow the same bootstrap pattern: simple stretchable
parentheses, brackets, braces, bars, angles, floors, and ceilings emit native
vector atoms instead of scaled text glyphs when the enclosed expression crosses
the font's `DelimitedSubFormulaMinHeight`, so tall fences do not become
artificially bold while ordinary inline fences remain native glyphs. They still
fall back to the bootstrap vector shape only when the font does not expose a
usable delimiter variant or assembly. Moderately stretched fences render
through exact OpenType delimiter variant glyph outlines, and taller fences can
now be built from the font's assembly pieces and extender glyphs.

The TeX matrix adapter is now intentionally narrow: it recognizes the common
LaTeX matrix environments, treats `&` as a cell separator and `\\` as a row
separator inside those environments, then lowers the result into `Table` plus
optional `Fenced` nodes. This keeps markdown, MathML, and future importers on
the same core layout path. The same table path now carries per-column
alignment and table gap metadata, with TeX `array`, TeX `cases`, MathML
`columnalign`, and MathML spacing attributes as the first importers. TeX
table-like environments accept a trailing row separator, but otherwise require
consistent row widths; malformed table source becomes a math parse error
instead of being guessed into shape.

Display math currently derives its bootstrap math axis from the rendered `+`
glyph, so fractions, display operators, and table-like structures share the
same visual centerline while math glyphs are still routed through the text
stack. Once math glyph selection moves fully onto the bundled math face, this
should be replaced with the OpenType MATH `AxisHeight` constant from that same
rendered face.

## Next Work Packages

### 1. Radical Construction

The first OpenType-backed radical path is in place:

- Prefer Noto Sans Math vertical radical variants when available.
- Extend the overbar with a native rule over arbitrary radicand width.
- Fall back to the native vector radical when the font lacks MATH-table
  radical variants.

Remaining work is polish: use more of the MATH radical constants, validate
larger nested roots, and decide whether radical assemblies are needed for cases
where variants cannot cover the requested height.

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

The first preparatory step is in place: current heuristic values flow through
an internal metrics helper instead of being embedded directly in every layout
function. That helper now reads the bundled Noto Sans Math MATH table for the
low-risk values that are already close to the tuned heuristics: script scale,
axis height, fraction rule thickness, fraction numerator/denominator gaps,
script shifts/gaps, upper/lower limit placement, radical rule thickness, and
radical vertical gaps. Fraction numerator/denominator shifts are also applied
as minimum baseline placement constraints while the fraction rule remains on
the shared math axis. The next step is widening that bridge to delimiter
assemblies. The parser now verifies that Noto Sans Math exposes delimiter
variants, assemblies, connector overlap data, and the delimiter stretch
threshold for common fences, and it preserves variant glyph IDs plus assembly
part connector/extender metadata. Rendering now has a first exact-glyph bridge:
when a discrete delimiter or radical variant covers the requested height, the
math layout emits that glyph ID and draw-op resolution converts the bundled
Noto Sans Math outline into a mask vector. When no delimiter variant is tall
enough, the layout repeats OpenType assembly extender parts, distributes
connector overlap so the final height tracks the target expression, and emits
each assembly piece through the same glyph-id outline path. Display-style sums
and sibling large operators with limits now use the same variant glyph bridge.

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
