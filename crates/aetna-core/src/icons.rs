//! Built-in vector icons.
//!
//! The vocabulary intentionally mirrors common shadcn/lucide names.
//! Icons are semantic `El`s that emit vector draw ops for artifact/SVG
//! rendering and a text fallback in GPU backends until the dedicated
//! vector-icon pipeline lands.

use std::panic::Location;

use crate::style::StyleProfile;
use crate::tokens;
use crate::tree::*;

pub trait IntoIconName {
    fn into_icon_name(self) -> IconName;
}

impl IntoIconName for IconName {
    fn into_icon_name(self) -> IconName {
        self
    }
}

impl IntoIconName for &str {
    fn into_icon_name(self) -> IconName {
        IconName::parse(self).unwrap_or_else(|| panic!("unknown Aetna icon name: {self}"))
    }
}

impl IntoIconName for String {
    fn into_icon_name(self) -> IconName {
        IconName::parse(&self).unwrap_or_else(|| panic!("unknown Aetna icon name: {self}"))
    }
}

#[track_caller]
pub fn icon(name: impl IntoIconName) -> El {
    El::new(Kind::Custom("icon"))
        .at_loc(Location::caller())
        .style_profile(StyleProfile::TextOnly)
        .icon_name(name.into_icon_name())
        .icon_size(16.0)
        .icon_stroke_width(2.0)
        .text_color(tokens::TEXT_FOREGROUND)
}

/// SVG path markup in a 24x24 coordinate system. Paths deliberately use
/// `currentColor`; the SVG fallback supplies colour/stroke externally.
pub fn icon_path(name: IconName) -> &'static str {
    match name {
        IconName::Activity => r#"<path d="M3 12h4l3-8 4 16 3-8h4"/>"#,
        IconName::AlertCircle => {
            r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v6"/><path d="M12 17h.01"/>"#
        }
        IconName::BarChart => r#"<path d="M4 20V10"/><path d="M12 20V4"/><path d="M20 20v-7"/>"#,
        IconName::Bell => {
            r#"<path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9"/><path d="M10 21h4"/>"#
        }
        IconName::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
        IconName::ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
        IconName::ChevronRight => r#"<path d="m9 6 6 6-6 6"/>"#,
        IconName::Command => {
            r#"<path d="M9 9h6v6H9z"/><path d="M9 9H6a3 3 0 1 1 3-3v3Z"/><path d="M15 9V6a3 3 0 1 1 3 3h-3Z"/><path d="M15 15h3a3 3 0 1 1-3 3v-3Z"/><path d="M9 15v3a3 3 0 1 1-3-3h3Z"/>"#
        }
        IconName::Download => {
            r#"<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/>"#
        }
        IconName::FileText => {
            r#"<path d="M14 3H6v18h12V7z"/><path d="M14 3v4h4"/><path d="M8 12h8"/><path d="M8 16h6"/>"#
        }
        IconName::Folder => r#"<path d="M3 6h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#,
        IconName::GitBranch => {
            r#"<circle cx="6" cy="5" r="2"/><circle cx="18" cy="19" r="2"/><circle cx="6" cy="19" r="2"/><path d="M6 7v10"/><path d="M8 5h4a6 6 0 0 1 6 6v6"/>"#
        }
        IconName::GitCommit => {
            r#"<circle cx="12" cy="12" r="3"/><path d="M3 12h6"/><path d="M15 12h6"/>"#
        }
        IconName::Info => {
            r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v6"/><path d="M12 7h.01"/>"#
        }
        IconName::LayoutDashboard => {
            r#"<rect x="3" y="3" width="7" height="8"/><rect x="14" y="3" width="7" height="5"/><rect x="14" y="12" width="7" height="9"/><rect x="3" y="15" width="7" height="6"/>"#
        }
        IconName::Menu => r#"<path d="M4 6h16"/><path d="M4 12h16"/><path d="M4 18h16"/>"#,
        IconName::MoreHorizontal => {
            r#"<path d="M6 12h.01"/><path d="M12 12h.01"/><path d="M18 12h.01"/>"#
        }
        IconName::Plus => r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#,
        IconName::RefreshCw => {
            r#"<path d="M21 12a9 9 0 0 1-15.5 6.2"/><path d="M3 12A9 9 0 0 1 18.5 5.8"/><path d="M18 3v4h-4"/><path d="M6 21v-4h4"/>"#
        }
        IconName::Search => r#"<circle cx="11" cy="11" r="7"/><path d="m16 16 5 5"/>"#,
        IconName::Settings => {
            r#"<circle cx="12" cy="12" r="3"/><path d="M19 12a7 7 0 0 0-.1-1l2-1.5-2-3.5-2.4 1a7 7 0 0 0-1.8-1L14.4 3h-4.8L9.3 6a7 7 0 0 0-1.8 1L5.1 6l-2 3.5 2 1.5a7 7 0 0 0 0 2l-2 1.5 2 3.5 2.4-1a7 7 0 0 0 1.8 1l.3 3h4.8l.3-3a7 7 0 0 0 1.8-1l2.4 1 2-3.5-2-1.5a7 7 0 0 0 .1-1Z"/>"#
        }
        IconName::Upload => r#"<path d="M12 21V9"/><path d="m7 14 5-5 5 5"/><path d="M5 3h14"/>"#,
        IconName::Users => {
            r#"<circle cx="9" cy="8" r="3"/><path d="M3 21a6 6 0 0 1 12 0"/><path d="M16 11a3 3 0 0 0 0-6"/><path d="M21 21a5 5 0 0 0-5-5"/>"#
        }
        IconName::X => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_names_parse_to_familiar_icon_names() {
        assert_eq!(IconName::parse("search"), Some(IconName::Search));
        assert_eq!(
            IconName::parse("dashboard"),
            Some(IconName::LayoutDashboard)
        );
        assert_eq!(IconName::parse("refresh"), Some(IconName::RefreshCw));
        assert_eq!(IconName::parse("missing"), None);
    }

    #[test]
    fn icon_builder_sets_vector_icon_defaults() {
        let el = icon("git-branch");
        assert_eq!(el.icon, Some(IconName::GitBranch));
        assert_eq!(el.width, Size::Fixed(16.0));
        assert_eq!(el.height, Size::Fixed(16.0));
        assert_eq!(el.text_color, Some(tokens::TEXT_FOREGROUND));
    }
}
