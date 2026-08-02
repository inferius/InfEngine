//! Decode a Westwood `.FNT` and write it out as a picture.
//!
//! ```text
//! cargo run -p infengine_advimport --example kyra_font -- <font.fnt> [out.png]
//! ```
//!
//! The sheet is one glyph per cell, sixteen to a row, so the whole
//! character set can be checked by looking at it - which is the only way
//! to tell a font read correctly from one read with its nibbles the
//! wrong way round.

#![allow(clippy::print_stdout)]

use infengine_advimport::kyra::fnt;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        println!("usage: kyra_font <font.fnt> [out.png]");
        return;
    };
    let out = args.next();

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            println!("could not read {path}: {err}");
            return;
        }
    };
    let font = match fnt::decode(&bytes) {
        Ok(font) => font,
        Err(err) => {
            println!("{path}: {err}");
            return;
        }
    };

    println!(
        "{} glyphs drawn of {}, line {} tall, widest {}",
        font.drawn(),
        fnt::GLYPHS,
        font.line_height(),
        font.max_width()
    );
    let sample: String = (32..127u8)
        .filter(|c| !font.glyphs[*c as usize].is_empty())
        .map(|c| c as char)
        .collect();
    println!("drawn in ASCII: {sample}");

    let Some(out) = out else { return };
    // Scaled up, because the point of writing a sheet is to look at it
    // and a ten-pixel letter settles nothing at its own size.
    const ZOOM: u32 = 5;
    let (cell_w, cell_h) = (font.max_width().max(1), font.line_height().max(1));
    let (cols, rows) = (16u32, 16u32);
    let (width, height) = (cell_w * cols * ZOOM, cell_h * rows * ZOOM);
    let mut image = vec![0u8; (width * height * 4) as usize];

    for (code, glyph) in font.glyphs.iter().enumerate() {
        let (cx, cy) = (code as u32 % cols * cell_w, code as u32 / cols * cell_h);
        for y in 0..glyph.height as u32 {
            for x in 0..glyph.width as u32 {
                let index = glyph.pixels[(y * glyph.width as u32 + x) as usize];
                if index == 0 {
                    continue;
                }
                // No palette in a font, so the nibble is drawn as a grey.
                // What matters here is the shape, not the colour.
                let shade = 32 + index * 14;
                let (px, py) = (cx + x, cy + glyph.top as u32 + y);
                for zy in 0..ZOOM {
                    for zx in 0..ZOOM {
                        let at = (((py * ZOOM + zy) * width + px * ZOOM + zx) * 4) as usize;
                        if at + 4 <= image.len() {
                            image[at..at + 4].copy_from_slice(&[shade, shade, shade, 255]);
                        }
                    }
                }
            }
        }
    }

    match image::RgbaImage::from_raw(width, height, image) {
        Some(sheet) => match sheet.save(&out) {
            Ok(()) => println!("wrote {out} ({width}x{height})"),
            Err(err) => println!("could not write {out}: {err}"),
        },
        None => println!("the sheet does not match its size"),
    }
}
