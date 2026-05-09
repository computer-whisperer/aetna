# aetna-fonts

Bundled font assets for Aetna.

Most app crates should not depend on this directly. `aetna-core` enables
the `default_fonts` feature by default, which pulls in Inter,
JetBrains Mono, NotoColorEmoji, and symbols/math fallback faces through
this crate. Roboto is bundled but no longer a default — opt in with the
`roboto` feature if you want the Material-style UI sans.

Use `aetna-core` with `default-features = false` when you want to ship
your own fonts, then register them through the text atlas APIs used by
your host/backend path.

Feature overview (each pulls in a sibling sub-crate that bundles the
font family — the split keeps every published `.crate` artifact under
crates.io's per-crate upload size limit):

- `inter`: Inter Variable UI text — re-exports `aetna-fonts-inter`.
- `jetbrains-mono`: JetBrains Mono Variable code text (with ligatures) — re-exports `aetna-fonts-jetbrains-mono`.
- `roboto`: Roboto UI text — re-exports `aetna-fonts-roboto`.
- `emoji`: NotoColorEmoji color bitmap glyphs — re-exports `aetna-fonts-emoji`.
- `symbols`: arrows, math, and symbols fallback — re-exports `aetna-fonts-symbols`.

CJK was previously available as a `cjk` feature; the bundled font (~16 MB) pushed the published `.crate` over crates.io's upload cap, so it has been removed for this release. Register a CJK face directly into aetna-core's `fontdb` until a bring-your-own-font path returns.
