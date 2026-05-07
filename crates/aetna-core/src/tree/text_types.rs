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
