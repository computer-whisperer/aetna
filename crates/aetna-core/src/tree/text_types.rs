//! Text styling enums carried by [`El`](crate::El).

/// Font weight. The renderer maps these to font-loading or to
/// font-weight CSS / SVG attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Semibold,
    Bold,
}

/// Proportional UI font family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FontFamily {
    /// Inter Variable, the closest bundled match for modern shadcn /
    /// Tailwind SaaS-dashboard typography.
    #[default]
    Inter,
    /// Roboto, retained for Material-style applications and backward
    /// compatibility with early Aetna typography.
    Roboto,
}

impl FontFamily {
    pub fn family_name(self) -> &'static str {
        match self {
            FontFamily::Inter => "Inter Variable",
            FontFamily::Roboto => "Roboto",
        }
    }

    pub fn css_stack(self) -> &'static str {
        match self {
            FontFamily::Inter => {
                "'Inter Variable', Inter, ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, sans-serif"
            }
            FontFamily::Roboto => {
                "Roboto, ui-sans-serif, system-ui, -apple-system, Segoe UI, sans-serif"
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TextWrap {
    #[default]
    NoWrap,
    Wrap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextOverflow {
    #[default]
    Clip,
    Ellipsis,
}

/// Semantic typography role for text-bearing nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TextRole {
    #[default]
    Body,
    Caption,
    Label,
    Title,
    Heading,
    Display,
    Code,
}

impl TextRole {
    pub fn name(self) -> &'static str {
        match self {
            TextRole::Body => "body",
            TextRole::Caption => "caption",
            TextRole::Label => "label",
            TextRole::Title => "title",
            TextRole::Heading => "heading",
            TextRole::Display => "display",
            TextRole::Code => "code",
        }
    }
}
