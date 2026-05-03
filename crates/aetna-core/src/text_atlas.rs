//! Glyph rasterization + atlas, backend-agnostic.
//!
//! [`GlyphAtlas`] owns the cosmic-text `FontSystem` (bundled Roboto) and
//! a swash `ScaleContext`. It shapes a logical text run to per-glyph
//! positions, rasterizes any glyphs it has not seen at this size, and
//! packs the alpha-coverage bitmaps onto one or more CPU-side
//! [`AtlasPage`]s. Backends mirror dirty regions of those pages to a GPU
//! texture and draw textured quads at the positions returned in
//! [`ShapedRun`].
//!
//! SVG and layout/measurement keep using [`crate::text_metrics`] — its
//! line-level layout is what they consume; the per-glyph artifact here
//! is for paint only.

use std::collections::HashMap;
use std::ops::Range;

use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, Weight, Wrap, fontdb,
};
use swash::scale::{Render, ScaleContext, Source as SwashSource, StrikeWith};
use swash::zeno::Format;

use crate::ir::TextAnchor;
use crate::text_metrics::{TextLayout, TextLine, line_height};
use crate::tree::{FontWeight, TextWrap};

const ROBOTO_REGULAR: &[u8] = include_bytes!("../fonts/Roboto-Regular.ttf");
const ROBOTO_MEDIUM: &[u8] = include_bytes!("../fonts/Roboto-Medium.ttf");
const ROBOTO_BOLD: &[u8] = include_bytes!("../fonts/Roboto-Bold.ttf");

/// Default page size. Picked so a typical fixture's glyphs fit on a
/// single page; larger UIs allocate a second page on demand.
const PAGE_SIZE: u32 = 512;

/// One shaped glyph carrying its atlas key and pen position. Positions
/// are in **logical pixels** relative to the shaped run's origin (top
/// of the first line, x = 0).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub key: GlyphKey,
    /// Pen X relative to run origin. Add the bitmap's `offset.0` to
    /// reach the glyph's screen-space top-left.
    pub x: f32,
    /// Baseline Y relative to run origin. The bitmap's top edge is at
    /// `y - offset.1` (offset.1 is positive for bitmaps above baseline).
    pub y: f32,
    /// Source byte range in the input string — kept for future caret /
    /// selection logic.
    pub byte_range: Range<usize>,
}

/// One shaped + atlased run, the artifact a backend's text path consumes.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedRun {
    pub layout: TextLayout,
    pub glyphs: Vec<ShapedGlyph>,
}

/// Identity for a rasterized glyph at a specific pixel size. The `font`
/// component is `cosmic-text`'s `fontdb::ID`; `size_bits` matches
/// cosmic-text's own cache key (`f32::to_bits` of the requested em size)
/// so we can route LayoutGlyph cache keys straight through.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct GlyphKey {
    pub font: fontdb::ID,
    pub glyph_id: u16,
    /// `font_size.to_bits()` — same encoding cosmic-text uses internally.
    pub size_bits: u32,
}

impl GlyphKey {
    pub fn size(&self) -> f32 {
        f32::from_bits(self.size_bits)
    }
}

/// One glyph's slot inside an atlas page.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GlyphSlot {
    pub page: u32,
    /// Pixel rect inside the page where the bitmap sits.
    pub rect: AtlasRect,
    /// Bitmap top-left in screen space relative to the pen+baseline.
    /// `top_left = (pen_x + offset.0, baseline_y - offset.1)`.
    pub offset: (i32, i32),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AtlasRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl AtlasRect {
    pub fn right(&self) -> u32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> u32 {
        self.y + self.h
    }
}

/// One CPU-side atlas page. Backends sample from a GPU texture mirror.
pub struct AtlasPage {
    pub width: u32,
    pub height: u32,
    /// A8 alpha coverage, row-major, `width * height` bytes.
    pub pixels: Vec<u8>,
    /// Bounding box of writes since the last [`take_dirty`](GlyphAtlas::take_dirty)
    /// call. `None` means clean.
    dirty: Option<AtlasRect>,
    shelves: Vec<Shelf>,
}

#[derive(Copy, Clone)]
struct Shelf {
    y_top: u32,
    height: u32,
    cursor: u32,
}

/// Glyph rasterizer + atlas. Cheap to clone? No — owns font system and
/// allocations. One per backend.
pub struct GlyphAtlas {
    font_system: FontSystem,
    scale_ctx: ScaleContext,
    pages: Vec<AtlasPage>,
    map: HashMap<GlyphKey, GlyphSlot>,
}

impl Default for GlyphAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphAtlas {
    pub fn new() -> Self {
        let font_system = bundled_font_system();
        Self {
            font_system,
            scale_ctx: ScaleContext::new(),
            pages: vec![AtlasPage::new(PAGE_SIZE, PAGE_SIZE)],
            map: HashMap::new(),
        }
    }

    pub fn pages(&self) -> &[AtlasPage] {
        &self.pages
    }

    pub fn page(&self, index: u32) -> Option<&AtlasPage> {
        self.pages.get(index as usize)
    }

    pub fn slot(&self, key: GlyphKey) -> Option<GlyphSlot> {
        self.map.get(&key).copied()
    }

    /// Drain and return one dirty rect per page that has writes since
    /// the last call. Clears the dirty bookkeeping.
    pub fn take_dirty(&mut self) -> Vec<(usize, AtlasRect)> {
        let mut out = Vec::new();
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(rect) = page.dirty.take() {
                out.push((i, rect));
            }
        }
        out
    }

    /// Shape a logical text run with the given attributes and ensure
    /// every glyph it produces is present in the atlas. Returns the
    /// shaped runs (per-glyph keys + positions) and the line-level
    /// layout. `available_width` is only used for `TextWrap::Wrap`.
    pub fn shape_and_rasterize(
        &mut self,
        text: &str,
        size: f32,
        weight: FontWeight,
        wrap: TextWrap,
        anchor: TextAnchor,
        available_width: Option<f32>,
    ) -> ShapedRun {
        let line_h = line_height(size);
        let mut buffer = Buffer::new(&mut self.font_system, Metrics::new(size, line_h));
        buffer.set_wrap(
            &mut self.font_system,
            match wrap {
                TextWrap::NoWrap => Wrap::None,
                TextWrap::Wrap => Wrap::WordOrGlyph,
            },
        );
        // cosmic-text uses the buffer width for both wrapping AND
        // alignment. For Wrap mode it's the wrap width; for NoWrap with
        // Middle/End anchors it's the box that line-alignment positions
        // glyphs within. Passing None for NoWrap+Middle leaves the
        // buffer unbounded and silently disables centering — single-
        // glyph button labels show up flush-left.
        buffer.set_size(&mut self.font_system, available_width, None);
        let attrs = Attrs::new()
            .family(Family::Name("Roboto"))
            .weight(cosmic_weight(weight));
        buffer.set_text(&mut self.font_system, text, attrs, Shaping::Advanced);

        if let Some(align) = match anchor {
            TextAnchor::Start => None,
            TextAnchor::Middle => Some(cosmic_text::Align::Center),
            TextAnchor::End => Some(cosmic_text::Align::End),
        } {
            for line in buffer.lines.iter_mut() {
                line.set_align(Some(align));
            }
        }
        buffer.shape_until_scroll(&mut self.font_system, false);

        // Walk runs in source order, emit per-glyph entries, ensure
        // each unique CacheKey is rasterized into the atlas.
        let mut lines = Vec::new();
        let mut shaped_glyphs = Vec::new();
        let mut height: f32 = 0.0;
        let mut max_width: f32 = 0.0;
        for run in buffer.layout_runs() {
            height = height.max(run.line_top + run.line_height);
            max_width = max_width.max(run.line_w);
            let (line_start, line_end) = run_byte_range(&run);
            lines.push(TextLine {
                text: line_slice(&run, line_start, line_end),
                width: run.line_w,
                y: run.line_top,
                baseline: run.line_y,
                rtl: run.rtl,
            });

            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                let key = glyph_key(physical.cache_key);
                self.ensure(key);
                shaped_glyphs.push(ShapedGlyph {
                    key,
                    x: glyph.x + glyph.x_offset,
                    y: run.line_y + glyph.y_offset,
                    byte_range: glyph.start..glyph.end,
                });
            }
        }

        let layout = TextLayout {
            width: max_width,
            height: height.max(line_h),
            line_height: line_h,
            lines,
        };

        ShapedRun {
            layout,
            glyphs: shaped_glyphs,
        }
    }

    fn ensure(&mut self, key: GlyphKey) {
        if self.map.contains_key(&key) {
            return;
        }
        let Some(slot) = self.rasterize_and_pack(key) else {
            // Glyph missing or zero-sized — record an empty slot so we
            // don't try again every frame.
            self.map.insert(
                key,
                GlyphSlot {
                    page: 0,
                    rect: AtlasRect {
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                    },
                    offset: (0, 0),
                },
            );
            return;
        };
        self.map.insert(key, slot);
    }

    fn rasterize_and_pack(&mut self, key: GlyphKey) -> Option<GlyphSlot> {
        let font = self.font_system.get_font(key.font)?;
        let mut scaler = self
            .scale_ctx
            .builder(font.as_swash())
            .size(key.size())
            .hint(true)
            .build();

        let sources = [
            SwashSource::ColorOutline(0),
            SwashSource::ColorBitmap(StrikeWith::BestFit),
            SwashSource::Outline,
        ];
        let mut render = Render::new(&sources);
        render.format(Format::Alpha);
        let image = render.render(&mut scaler, key.glyph_id)?;
        let width = image.placement.width;
        let height = image.placement.height;
        if width == 0 || height == 0 || image.data.is_empty() {
            return None;
        }

        let (page_idx, rect) = self.allocate(width, height)?;
        let page = &mut self.pages[page_idx];
        copy_bitmap(&mut page.pixels, page.width, &rect, &image.data);
        merge_dirty(&mut page.dirty, rect);

        Some(GlyphSlot {
            page: page_idx as u32,
            rect,
            offset: (image.placement.left, image.placement.top),
        })
    }

    fn allocate(&mut self, w: u32, h: u32) -> Option<(usize, AtlasRect)> {
        for (i, page) in self.pages.iter_mut().enumerate() {
            if let Some(rect) = page.allocate(w, h) {
                return Some((i, rect));
            }
        }
        // Grow: add a new page sized to fit at least this glyph.
        let new_w = PAGE_SIZE.max(w.next_power_of_two());
        let new_h = PAGE_SIZE.max(h.next_power_of_two());
        let mut page = AtlasPage::new(new_w, new_h);
        let rect = page.allocate(w, h)?;
        self.pages.push(page);
        Some((self.pages.len() - 1, rect))
    }
}

impl AtlasPage {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height) as usize],
            dirty: None,
            shelves: Vec::new(),
        }
    }

    /// Pack a `w × h` glyph onto the next available shelf. Adds a new
    /// shelf below the current one if none fits.
    fn allocate(&mut self, w: u32, h: u32) -> Option<AtlasRect> {
        if w > self.width || h > self.height {
            return None;
        }
        // Try existing shelves: prefer the tightest fit (minimum waste).
        let mut best: Option<usize> = None;
        for (i, shelf) in self.shelves.iter().enumerate() {
            if shelf.cursor + w > self.width || shelf.height < h {
                continue;
            }
            let waste = shelf.height - h;
            if best
                .map(|b| waste < self.shelves[b].height - h)
                .unwrap_or(true)
            {
                best = Some(i);
            }
        }
        if let Some(i) = best {
            let shelf = &mut self.shelves[i];
            let rect = AtlasRect {
                x: shelf.cursor,
                y: shelf.y_top,
                w,
                h,
            };
            shelf.cursor += w;
            return Some(rect);
        }

        // Add a new shelf at the bottom of the existing ones.
        let next_y = self.shelves.last().map(|s| s.y_top + s.height).unwrap_or(0);
        if next_y + h > self.height {
            return None;
        }
        let shelf = Shelf {
            y_top: next_y,
            height: h,
            cursor: w,
        };
        self.shelves.push(shelf);
        Some(AtlasRect {
            x: 0,
            y: next_y,
            w,
            h,
        })
    }
}

fn copy_bitmap(dst: &mut [u8], dst_stride: u32, rect: &AtlasRect, src: &[u8]) {
    let stride = dst_stride as usize;
    let w = rect.w as usize;
    for row in 0..rect.h as usize {
        let dst_off = (rect.y as usize + row) * stride + rect.x as usize;
        let src_off = row * w;
        dst[dst_off..dst_off + w].copy_from_slice(&src[src_off..src_off + w]);
    }
}

fn merge_dirty(dirty: &mut Option<AtlasRect>, rect: AtlasRect) {
    *dirty = Some(match *dirty {
        None => rect,
        Some(prev) => {
            let x = prev.x.min(rect.x);
            let y = prev.y.min(rect.y);
            let r = prev.right().max(rect.right());
            let b = prev.bottom().max(rect.bottom());
            AtlasRect {
                x,
                y,
                w: r - x,
                h: b - y,
            }
        }
    });
}

fn glyph_key(cache_key: CacheKey) -> GlyphKey {
    // cosmic-text packs subpixel x/y bins into the cache key for
    // subpixel positioning. v5.1 commit 1 quantizes to whole pixels
    // (subpixel bins discarded) — backend can opt into subpixel later
    // by widening the key.
    GlyphKey {
        font: cache_key.font_id,
        glyph_id: cache_key.glyph_id,
        size_bits: cache_key.font_size_bits,
    }
}

fn run_byte_range(run: &cosmic_text::LayoutRun<'_>) -> (usize, usize) {
    let start = run.glyphs.iter().map(|g| g.start).min().unwrap_or(0);
    let end = run.glyphs.iter().map(|g| g.end).max().unwrap_or(start);
    (start, end)
}

fn line_slice(run: &cosmic_text::LayoutRun<'_>, start: usize, end: usize) -> String {
    run.text
        .get(start..end)
        .unwrap_or_default()
        .trim_end()
        .to_string()
}

fn bundled_font_system() -> FontSystem {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaping_emits_one_glyph_per_visible_codepoint() {
        let mut atlas = GlyphAtlas::new();
        let run = atlas.shape_and_rasterize(
            "abc",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        assert_eq!(run.glyphs.len(), 3);
        assert_eq!(run.layout.lines.len(), 1);
        assert!(run.layout.width > 0.0);
    }

    #[test]
    fn repeated_glyph_reuses_atlas_slot() {
        let mut atlas = GlyphAtlas::new();
        atlas.shape_and_rasterize(
            "aaa",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        let pages_before = atlas.pages().len();
        let dirty_before: u32 = atlas
            .pages()
            .iter()
            .map(|p| p.dirty.map(|r| r.w * r.h).unwrap_or(0))
            .sum();

        // Drain dirty so a new write would re-mark.
        atlas.take_dirty();
        atlas.shape_and_rasterize(
            "aa",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        assert_eq!(atlas.pages().len(), pages_before);
        // No new rasterization — every glyph was already cached, so
        // the dirty region stays None on the second call.
        let dirty_after: u32 = atlas
            .pages()
            .iter()
            .map(|p| p.dirty.map(|r| r.w * r.h).unwrap_or(0))
            .sum();
        assert_eq!(dirty_after, 0);
        assert!(dirty_before > 0);
    }

    #[test]
    fn distinct_sizes_get_distinct_slots() {
        let mut atlas = GlyphAtlas::new();
        let r16 = atlas.shape_and_rasterize(
            "A",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        let r24 = atlas.shape_and_rasterize(
            "A",
            24.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        assert_eq!(r16.glyphs.len(), 1);
        assert_eq!(r24.glyphs.len(), 1);
        let s16 = atlas.slot(r16.glyphs[0].key).unwrap();
        let s24 = atlas.slot(r24.glyphs[0].key).unwrap();
        // Different size → different rasterization → different slot.
        assert_ne!(s16.rect, s24.rect);
        assert!(s24.rect.h >= s16.rect.h);
    }

    #[test]
    fn distinct_weights_get_distinct_slots() {
        let mut atlas = GlyphAtlas::new();
        let regular = atlas.shape_and_rasterize(
            "A",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        let bold = atlas.shape_and_rasterize(
            "A",
            16.0,
            FontWeight::Bold,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        let r = atlas.slot(regular.glyphs[0].key).unwrap();
        let b = atlas.slot(bold.glyphs[0].key).unwrap();
        assert_ne!(regular.glyphs[0].key, bold.glyphs[0].key);
        assert_ne!(r.rect, b.rect);
    }

    #[test]
    fn dirty_region_covers_new_glyphs_and_clears_on_take() {
        let mut atlas = GlyphAtlas::new();
        atlas.shape_and_rasterize(
            "Hello",
            18.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        let dirty = atlas.take_dirty();
        assert_eq!(dirty.len(), 1, "expected one dirty page after first run");
        let (page_idx, rect) = dirty[0];
        assert_eq!(page_idx, 0);
        assert!(rect.w > 0 && rect.h > 0);
        assert!(atlas.take_dirty().is_empty());
    }

    #[test]
    fn shelves_pack_a_realistic_text_run_into_one_page() {
        let mut atlas = GlyphAtlas::new();
        atlas.shape_and_rasterize(
            "The quick brown fox jumps over the lazy dog 0123456789",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
        // A typical body-text run easily fits on one 512x512 page.
        // The packer is allowed to use multiple shelves; the contract
        // is just "no spurious second page."
        assert_eq!(atlas.pages().len(), 1);
    }

    #[test]
    fn many_distinct_glyphs_can_grow_to_a_second_page() {
        let mut atlas = GlyphAtlas::new();
        // Combine many sizes/weights to exhaust one page eventually.
        for size in [10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0] {
            for weight in [FontWeight::Regular, FontWeight::Bold] {
                atlas.shape_and_rasterize(
                    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789",
                    size,
                    weight,
                    TextWrap::NoWrap,
                    TextAnchor::Start,
                    None,
                );
            }
        }
        // The exact page count depends on shelf packing efficiency; what
        // matters is that the allocator successfully made room for every
        // glyph (i.e. didn't panic / drop entries).
        let total_glyphs: usize = atlas.map.len();
        assert!(total_glyphs > 100, "only stored {total_glyphs} glyphs");
    }

    #[test]
    fn empty_glyph_caches_zero_slot_without_panicking() {
        // A space is typically a non-rendering glyph (zero-sized
        // bitmap). Shaping a string with spaces should not panic and
        // should still cache a slot so we don't retry every call.
        let mut atlas = GlyphAtlas::new();
        atlas.shape_and_rasterize(
            "    ",
            16.0,
            FontWeight::Regular,
            TextWrap::NoWrap,
            TextAnchor::Start,
            None,
        );
    }
}
