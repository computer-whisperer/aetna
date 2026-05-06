# aetna-fonts-emoji

NotoColorEmoji (CBDT color bitmaps) bundled for Aetna.

Most consumers should depend on `aetna-fonts` (with the `emoji` feature, which is on by default) rather than this crate directly. This crate exists so the published `.crate` artifact for each font family stays under crates.io's per-crate upload size limit; `aetna-fonts` re-exports the byte slice when the matching feature is enabled.

Color rendering requires aetna-core's RGBA atlas path. Loading this directly into a non-aetna `fontdb` will render color glyphs as B&W silhouettes.
