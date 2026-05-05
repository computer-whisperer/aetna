# aetna-fonts

Bundled font assets for Aetna.

Most app crates should not depend on this directly. `aetna-core` enables
the `default_fonts` feature by default, which pulls in Roboto,
NotoColorEmoji, and symbols/math fallback faces through this crate.

Use `aetna-core` with `default-features = false` when you want to ship
your own fonts, then register them through the text atlas APIs used by
your host/backend path.

Feature overview:

- `roboto`: Latin UI text.
- `emoji`: NotoColorEmoji color bitmap glyphs.
- `symbols`: arrows, math, and symbols fallback.
- `cjk`: optional CJK fallback; larger binary footprint.
