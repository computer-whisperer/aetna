//! Stock widget vocabulary — pure compositions of the public widget-kit
//! surface (`El` builders, style profiles, focus opt-in). These modules
//! ship no privileged internals: an app crate can fork any of them and
//! produce an equivalent widget against the same public API. That
//! invariant — *stock widgets get no APIs that user widgets don't* — is
//! what makes the library a substrate rather than a fixed component
//! library. v0.7.5 is the slice that locks the rule down; everything
//! here is its proof.

pub mod badge;
pub mod button;
pub mod card;
pub mod overlay;
pub mod popover;
pub mod text;
pub mod text_area;
pub mod text_input;
