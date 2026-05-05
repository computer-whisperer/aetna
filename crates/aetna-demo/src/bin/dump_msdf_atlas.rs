//! Dump the MSDF text atlas after rasterizing the test phrase, so we
//! can see what's actually in each slot — useful for chasing visual
//! artifacts that look like atlas bleed.

use std::path::PathBuf;

use aetna_core::text::msdf_atlas::{MsdfAtlas, MsdfGlyphKey};
use cosmic_text::fontdb;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out: PathBuf = std::env::current_dir()?.join("crates/aetna-demo/out/msdf_atlas.png");

    // Build a fontdb with Roboto and pull a face.
    let mut db = fontdb::Database::new();
    db.load_font_data(aetna_fonts::ROBOTO_REGULAR.to_vec());
    let id = db.faces().next().expect("Roboto").id;
    let face = ttf_parser::Face::parse(aetna_fonts::ROBOTO_REGULAR, 0)?;

    let mut atlas = MsdfAtlas::default();
    let phrase = "The quick brown fox jumps over the lazy dog 0123456789";
    for ch in phrase.chars() {
        let gid = face.glyph_index(ch).map(|g| g.0).unwrap_or(0);
        atlas.ensure(
            MsdfGlyphKey {
                font: id,
                glyph_id: gid,
            },
            &face,
        );
    }

    let page = &atlas.pages()[0];
    println!("page 0: {}×{}", page.width, page.height);
    let writer = std::io::BufWriter::new(std::fs::File::create(&out)?);
    let mut enc = png::Encoder::new(writer, page.width, page.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&page.pixels)?;
    println!("wrote {}", out.display());
    Ok(())
}
