# aetna-fonts

Bundled font assets for Aetna.

Most app crates should not depend on this directly. `aetna-core` enables
the `default_fonts` feature by default, which pulls in Inter, Roboto,
NotoColorEmoji, and symbols/math fallback faces through this crate.

Use `aetna-core` with `default-features = false` when you want to ship
your own fonts, then register them through the text atlas APIs used by
your host/backend path.

Feature overview (each pulls in a sibling sub-crate that bundles the
font family — the split keeps every published `.crate` artifact under
crates.io's per-crate upload size limit):

- `inter`: Inter Variable UI text — re-exports `aetna-fonts-inter`.
- `roboto`: Roboto UI text — re-exports `aetna-fonts-roboto`.
- `emoji`: NotoColorEmoji color bitmap glyphs — re-exports `aetna-fonts-emoji`.
- `symbols`: arrows, math, and symbols fallback — re-exports `aetna-fonts-symbols`.

CJK was previously available as a `cjk` feature; the bundled font (~16 MB) pushed the published `.crate` over crates.io's upload cap, so it has been removed for this release. Register a CJK face directly into aetna-core's `fontdb` until a bring-your-own-font path returns.
