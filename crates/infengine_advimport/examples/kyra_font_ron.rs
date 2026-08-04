//! A74: a game font, exported with its metrics.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_font_ron -- <8FAT.FNT> <project> [language]
//! ```
//!
//! The atlas alone is a picture of letters; what makes it a font is
//! the table saying where each character sits and how wide it draws.
//! The `.FNT` knows both, so this writes the pair the engine reads:
//! `fonts/<stem>.png` and `fonts/<stem>.font.ron`.
//!
//! Glyph slots are bytes in the game's own encoding, so the language
//! decides which characters the high slots mean - a Czech copy's font
//! carries its accents where CP1250 puts them.

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

use infengine_advimport::encoding::Encoding;
use infengine_advimport::kyra::fnt;
use infengine_adventure::pixfont::{PixFont, PixGlyph};

/// Columns of the atlas grid, matching the exported picture.
const COLUMNS: u32 = 16;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(fnt_path), Some(project)) = (args.next(), args.next()) else {
        println!("usage: kyra_font_ron <path to .FNT> <project dir> [language]");
        return;
    };
    let language = args.next().unwrap_or_else(|| "en".into());
    let encoding = if matches!(
        language.split(['-', '_']).next().unwrap_or(""),
        "cs" | "sk" | "pl" | "hu" | "sl" | "hr" | "ro"
    ) {
        Encoding::Cp1250
    } else {
        Encoding::Latin1
    };

    let bytes = std::fs::read(&fnt_path).expect("readable font");
    let font = fnt::decode(&bytes).expect("a decodable .FNT");
    let stem = std::path::Path::new(&fnt_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("font")
        .to_ascii_lowercase();

    // The same grid the picture uses: cells of the widest glyph by the
    // tallest line, sixteen to a row, each glyph at its slot.
    let cell_w = font.max_width().max(1);
    let cell_h = font.line_height().max(1);
    let rows = (font.glyphs.len() as u32).div_ceil(COLUMNS);
    let mut sheet = image::RgbaImage::new(cell_w * COLUMNS, cell_h * rows);
    let mut glyphs: BTreeMap<char, PixGlyph> = BTreeMap::new();

    for (code, glyph) in font.glyphs.iter().enumerate() {
        let (cx, cy) = (
            code as u32 % COLUMNS * cell_w,
            code as u32 / COLUMNS * cell_h,
        );
        for y in 0..glyph.height as u32 {
            for x in 0..glyph.width as u32 {
                let ink = glyph.pixels[(y * glyph.width as u32 + x) as usize];
                if ink == 0 {
                    continue;
                }
                // The nibble picks a palette slot, not an intensity -
                // treating it as alpha made the lettering a ghost of
                // itself. Ink is ink: opaque white, tinted at draw
                // time.
                let _ = ink;
                sheet.put_pixel(
                    cx + x,
                    cy + glyph.top as u32 + y,
                    image::Rgba([255, 255, 255, 255]),
                );
            }
        }
        if glyph.is_empty() {
            continue;
        }
        // The slot number is a byte of text in the game's encoding.
        let (decoded, _) = infengine_advimport::encoding::decode(&[code as u8], encoding);
        if let Some(c) = decoded.chars().next()
            && !c.is_control()
        {
            glyphs.insert(c, PixGlyph { x: cx, y: cy, w: glyph.width as u32 });
        }
    }

    let project = std::path::PathBuf::from(project);
    let dir = project.join(infengine_adventure::pixfont::FONT_DIR);
    std::fs::create_dir_all(&dir).expect("fonts dir");
    sheet.save(dir.join(format!("{stem}.png"))).expect("atlas written");

    let out = PixFont {
        id: stem.clone(),
        image: format!("{}/{stem}.png", infengine_adventure::pixfont::FONT_DIR),
        line_height: cell_h,
        // The glyph widths carry their own breathing room already;
        // adding more made every word read stretched.
        letter_spacing: 0,
        space_width: 0,
        glyphs,
    };
    let count = out.glyphs.len();
    infengine_adventure::pixfont::save(&out, &dir).expect("metrics written");
    println!("{stem}: {count} glyphs, cell {cell_w}x{cell_h}, encoding {language}");
}
