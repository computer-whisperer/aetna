//! Font-backed text measurement and simple word wrapping.
//!
//! The production wgpu path uses [`crate::text_atlas::GlyphAtlas`] for
//! shaping + rasterization; layout, lint, SVG artifacts, and draw-op IR
//! all share this core layout artifact for measurement. Proportional
//! text is shaped through `cosmic-text` using bundled Roboto; the older
//! TTF-advance path remains as a fallback and for monospace until Aetna
//! has a bundled mono font.

use crate::tree::{FontWeight, TextWrap};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, Wrap, fontdb};

const ROBOTO_REGULAR: &[u8] = include_bytes!("../fonts/Roboto-Regular.ttf");
const ROBOTO_MEDIUM: &[u8] = include_bytes!("../fonts/Roboto-Medium.ttf");
const ROBOTO_BOLD: &[u8] = include_bytes!("../fonts/Roboto-Bold.ttf");
const LINE_HEIGHT_MULTIPLIER: f32 = 1.4;
const MONO_CHAR_WIDTH_FACTOR: f32 = 0.62;

const BASELINE_MULTIPLIER: f32 = 0.93;

#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    pub text: String,
    pub width: f32,
    /// Top offset from the text layout origin, in logical pixels.
    pub y: f32,
    /// Baseline offset from the text layout origin, in logical pixels.
    pub baseline: f32,
    /// Paragraph direction as resolved by the shaping engine.
    pub rtl: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLayout {
    pub lines: Vec<TextLine>,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
}

impl TextLayout {
    pub fn line_count(&self) -> usize {
        self.lines.len().max(1)
    }

    pub fn measured(&self) -> MeasuredText {
        MeasuredText {
            width: self.width,
            height: self.height,
            line_count: self.line_count(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredText {
    pub width: f32,
    pub height: f32,
    pub line_count: usize,
}

/// Measure text in logical pixels. `available_width` is only used when
/// `wrap == TextWrap::Wrap`; `None` means measure explicit newlines only.
pub fn measure_text(
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> MeasuredText {
    layout_text(text, size, weight, mono, wrap, available_width).measured()
}

/// Lay out text into measured lines. Coordinates in [`TextLine`] are
/// relative to the layout origin; callers place the layout inside a
/// rectangle and apply alignment/vertical centering as needed.
pub fn layout_text(
    text: &str,
    size: f32,
    weight: FontWeight,
    mono: bool,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> TextLayout {
    if !mono && let Some(layout) = layout_text_cosmic(text, size, weight, wrap, available_width) {
        return layout;
    }

    let raw_lines = match (wrap, available_width) {
        (TextWrap::Wrap, Some(width)) => wrap_lines_by_width(text, width, size, weight, mono),
        _ => text.split('\n').map(str::to_string).collect(),
    };
    build_layout(raw_lines, size, weight, mono)
}

/// Word-wrap text into lines whose measured width stays within
/// `max_width` whenever possible. Explicit newlines always split
/// paragraphs. Oversized words are split by character.
pub fn wrap_lines(
    text: &str,
    max_width: f32,
    size: f32,
    weight: FontWeight,
    mono: bool,
) -> Vec<String> {
    if !mono
        && let Some(layout) =
            layout_text_cosmic(text, size, weight, TextWrap::Wrap, Some(max_width))
    {
        return layout.lines.into_iter().map(|line| line.text).collect();
    }
    wrap_lines_by_width(text, max_width, size, weight, mono)
}

fn wrap_lines_by_width(
    text: &str,
    max_width: f32,
    size: f32,
    weight: FontWeight,
    mono: bool,
) -> Vec<String> {
    if max_width <= 0.0 {
        return vec![String::new()];
    }

    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }

        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if line.is_empty() {
                push_word_wrapped(&mut out, &mut line, word, max_width, size, weight, mono);
                continue;
            }

            let candidate = format!("{line} {word}");
            if line_width(&candidate, size, weight, mono) <= max_width {
                line = candidate;
            } else {
                out.push(std::mem::take(&mut line));
                push_word_wrapped(&mut out, &mut line, word, max_width, size, weight, mono);
            }
        }

        if !line.is_empty() {
            out.push(line);
        }
    }

    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Measure one single-line string. Newline characters are ignored; use
/// [`measure_text`] for multi-line text.
pub fn line_width(text: &str, size: f32, weight: FontWeight, mono: bool) -> f32 {
    if !mono && let Some(layout) = layout_text_cosmic(text, size, weight, TextWrap::NoWrap, None) {
        return layout.width;
    }
    line_width_by_ttf(text, size, weight, mono)
}

fn line_width_by_ttf(text: &str, size: f32, weight: FontWeight, mono: bool) -> f32 {
    if mono {
        return text
            .chars()
            .filter(|c| *c != '\n' && *c != '\r')
            .map(|c| if c == '\t' { 4.0 } else { 1.0 })
            .sum::<f32>()
            * size
            * MONO_CHAR_WIDTH_FACTOR;
    }

    let Ok(face) = ttf_parser::Face::parse(font_bytes(weight), 0) else {
        return fallback_line_width(text, size, mono);
    };
    let scale = size / face.units_per_em() as f32;
    let fallback_advance = face.units_per_em() as f32 * 0.5;
    let mut width = 0.0;
    let mut prev = None;

    for c in text.chars() {
        if c == '\n' || c == '\r' {
            continue;
        }
        if c == '\t' {
            width += line_width("    ", size, weight, mono);
            prev = None;
            continue;
        }

        let Some(glyph) = glyph_for(&face, c) else {
            continue;
        };
        if let Some(left) = prev {
            width += kern(&face, left, glyph) * scale;
        }
        width += face
            .glyph_hor_advance(glyph)
            .map(|advance| advance as f32)
            .unwrap_or(fallback_advance)
            * scale;
        prev = Some(glyph);
    }
    width
}

pub fn line_height(size: f32) -> f32 {
    size * LINE_HEIGHT_MULTIPLIER
}

fn build_layout(lines: Vec<String>, size: f32, weight: FontWeight, mono: bool) -> TextLayout {
    let raw_lines = if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    };
    let line_height = line_height(size);
    let lines: Vec<TextLine> = raw_lines
        .into_iter()
        .enumerate()
        .map(|(i, text)| {
            let y = i as f32 * line_height;
            TextLine {
                width: line_width(&text, size, weight, mono),
                text,
                y,
                baseline: y + size * BASELINE_MULTIPLIER,
                rtl: false,
            }
        })
        .collect();
    let width = lines.iter().map(|line| line.width).fold(0.0, f32::max);
    TextLayout {
        width,
        height: lines.len().max(1) as f32 * line_height,
        line_height,
        lines,
    }
}

fn layout_text_cosmic(
    text: &str,
    size: f32,
    weight: FontWeight,
    wrap: TextWrap,
    available_width: Option<f32>,
) -> Option<TextLayout> {
    let line_height = line_height(size);
    let mut font_system = roboto_font_system();
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(size, line_height));
    buffer.set_wrap(
        &mut font_system,
        match wrap {
            TextWrap::NoWrap => Wrap::None,
            TextWrap::Wrap => Wrap::WordOrGlyph,
        },
    );
    buffer.set_size(
        &mut font_system,
        match wrap {
            TextWrap::NoWrap => None,
            TextWrap::Wrap => available_width,
        },
        None,
    );
    let attrs = Attrs::new()
        .family(Family::Name("Roboto"))
        .weight(cosmic_weight(weight));
    buffer.set_text(&mut font_system, text, attrs, Shaping::Advanced);
    buffer.shape_until_scroll(&mut font_system, false);

    let mut lines = Vec::new();
    let mut height: f32 = 0.0;
    for run in buffer.layout_runs() {
        height = height.max(run.line_top + run.line_height);
        lines.push(TextLine {
            text: layout_run_text(&run),
            width: run.line_w,
            y: run.line_top,
            baseline: run.line_y,
            rtl: run.rtl,
        });
    }

    if lines.is_empty() {
        return None;
    }

    let width = lines.iter().map(|line| line.width).fold(0.0, f32::max);
    Some(TextLayout {
        lines,
        width,
        height: height.max(line_height),
        line_height,
    })
}

fn roboto_font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.set_sans_serif_family("Roboto");
    db.load_font_data(ROBOTO_REGULAR.to_vec());
    db.load_font_data(ROBOTO_MEDIUM.to_vec());
    db.load_font_data(ROBOTO_BOLD.to_vec());
    FontSystem::new_with_locale_and_db("en-US".to_string(), db)
}

fn cosmic_weight(weight: FontWeight) -> Weight {
    match weight {
        FontWeight::Regular => Weight::NORMAL,
        FontWeight::Medium => Weight::MEDIUM,
        FontWeight::Semibold => Weight::SEMIBOLD,
        FontWeight::Bold => Weight::BOLD,
    }
}

fn layout_run_text(run: &cosmic_text::LayoutRun<'_>) -> String {
    let Some(start) = run.glyphs.iter().map(|glyph| glyph.start).min() else {
        return String::new();
    };
    let end = run
        .glyphs
        .iter()
        .map(|glyph| glyph.end)
        .max()
        .unwrap_or(start);
    run.text
        .get(start..end)
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

fn push_word_wrapped(
    out: &mut Vec<String>,
    line: &mut String,
    word: &str,
    max_width: f32,
    size: f32,
    weight: FontWeight,
    mono: bool,
) {
    if line_width(word, size, weight, mono) <= max_width {
        line.push_str(word);
        return;
    }

    for ch in word.chars() {
        let candidate = format!("{line}{ch}");
        if !line.is_empty() && line_width(&candidate, size, weight, mono) > max_width {
            out.push(std::mem::take(line));
        }
        line.push(ch);
    }
}

fn glyph_for(face: &ttf_parser::Face<'_>, c: char) -> Option<ttf_parser::GlyphId> {
    face.glyph_index(c)
        .or_else(|| face.glyph_index('\u{FFFD}'))
        .or_else(|| face.glyph_index('?'))
        .or_else(|| face.glyph_index(' '))
}

fn kern(face: &ttf_parser::Face<'_>, left: ttf_parser::GlyphId, right: ttf_parser::GlyphId) -> f32 {
    let Some(kern) = &face.tables().kern else {
        return 0.0;
    };
    kern.subtables
        .into_iter()
        .filter(|subtable| subtable.horizontal && !subtable.has_cross_stream)
        .find_map(|subtable| subtable.glyphs_kerning(left, right))
        .map(|value| value as f32)
        .unwrap_or(0.0)
}

fn font_bytes(weight: FontWeight) -> &'static [u8] {
    match weight {
        FontWeight::Regular => ROBOTO_REGULAR,
        FontWeight::Medium => ROBOTO_MEDIUM,
        FontWeight::Semibold | FontWeight::Bold => ROBOTO_BOLD,
    }
}

fn fallback_line_width(text: &str, size: f32, mono: bool) -> f32 {
    let char_w = size * if mono { MONO_CHAR_WIDTH_FACTOR } else { 0.60 };
    text.chars().count() as f32 * char_w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proportional_measurement_distinguishes_narrow_and_wide_glyphs() {
        let narrow = line_width("iiiiii", 16.0, FontWeight::Regular, false);
        let wide = line_width("WWWWWW", 16.0, FontWeight::Regular, false);

        assert!(wide > narrow * 2.0, "wide={wide} narrow={narrow}");
    }

    #[test]
    fn wrap_lines_respects_measured_widths() {
        let lines = wrap_lines(
            "wide WWW words stay measured",
            120.0,
            16.0,
            FontWeight::Regular,
            false,
        );

        assert!(lines.len() > 1);
        for line in lines {
            assert!(
                line_width(&line, 16.0, FontWeight::Regular, false) <= 121.0,
                "{line:?} overflowed"
            );
        }
    }

    #[test]
    fn layout_text_carries_line_positions_and_measurement() {
        let layout = layout_text(
            "alpha beta gamma",
            16.0,
            FontWeight::Regular,
            false,
            TextWrap::Wrap,
            Some(80.0),
        );

        assert!(layout.lines.len() > 1);
        assert_eq!(layout.measured().line_count, layout.lines.len());
        assert_eq!(layout.lines[0].y, 0.0);
        assert_eq!(layout.lines[1].y, layout.line_height);
        assert!(layout.lines[0].baseline > layout.lines[0].y);
        assert!(layout.height >= layout.line_height * 2.0);
    }

    #[test]
    fn proportional_layout_uses_cosmic_shaping_widths() {
        let layout = layout_text(
            "Roboto shaping",
            18.0,
            FontWeight::Medium,
            false,
            TextWrap::NoWrap,
            None,
        );

        assert_eq!(layout.lines.len(), 1);
        assert!((layout.lines[0].width - layout.width).abs() < 0.01);
        assert!(layout.lines[0].baseline > layout.lines[0].y);
    }
}
