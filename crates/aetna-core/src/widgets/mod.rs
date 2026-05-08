//! Stock widget vocabulary — pure compositions of the public widget-kit
//! surface (`El` builders, style profiles, focus opt-in). These modules
//! ship no privileged internals: an app crate can fork any of them and
//! produce an equivalent widget against the same public API. The
//! invariant — *stock widgets get no APIs that user widgets don't* — is
//! what makes the library a substrate rather than a fixed component
//! library; everything here is its proof.

pub mod accordion;
pub mod alert;
pub mod avatar;
pub mod badge;
pub mod breadcrumb;
pub mod button;
pub mod card;
pub mod checkbox;
pub mod command;
pub mod dialog;
pub mod dropdown_menu;
pub mod form;
pub mod overlay;
pub mod pagination;
pub mod popover;
pub mod progress;
pub mod radio;
pub mod resize_handle;
pub mod select;
pub mod separator;
pub mod sheet;
pub mod sidebar;
pub mod skeleton;
pub mod slider;
pub mod switch;
pub mod table;
pub mod tabs;
pub mod text;
pub mod text_area;
pub mod text_input;
pub mod toolbar;
