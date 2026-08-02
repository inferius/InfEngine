//! A52.1: Westwood's bitmap font.
//!
//! The game draws its own text, so its letters are a file rather than a
//! system font. Reading it is what lets an imported game keep looking
//! like itself instead of being retyped in whatever the engine happens
//! to ship.
//!
//! The header is eight little-endian words, and which is which was
//! worked out by measuring rather than assumed:
//!
//! ```text
//!   0x00  file size          6747, which the file is
//!   0x02  0x0500             the same in every Westwood font
//!   0x06  bitmap offsets     256 words, 248 of 255 ascending and
//!                            every one inside the file
//!   0x08  widths             256 bytes, none above 16
//!   0x0c  rows               256 pairs: how far down a glyph starts,
//!                            and how tall it is
//! ```
//!
//! Two of those tables are bytes and two are words, so reading them the
//! wrong way round gives offsets in the thousands where widths should
//! be - which is what the first attempt did, and it is why the tables
//! are identified by the shape of their contents here.
//!
//! A glyph is four bits per pixel: two pixels to a byte, low nibble
//! first, and zero is transparent rather than black. The nibble is an
//! index into whatever palette the text is being drawn with, so a font
//! carries no colours of its own.

use std::fmt;

/// Characters a font describes.
pub const GLYPHS: usize = 256;
/// The word every Westwood font has at offset 2.
const MAGIC: u16 = 0x0500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FntError {
    TooSmall { len: usize },
    /// The second word is not the one every Westwood font has.
    NotAFont { magic: u16 },
    /// A table the header points at runs past the end.
    TableOutside { at: usize },
}

impl fmt::Display for FntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall { len } => write!(f, "a font is at least 16 bytes; this is {len}"),
            Self::NotAFont { magic } => {
                write!(f, "font magic is {magic:#06x}, not {MAGIC:#06x}")
            }
            Self::TableOutside { at } => {
                write!(f, "a font table at {at:#x} runs past the end of the file")
            }
        }
    }
}

impl std::error::Error for FntError {}

/// One character's picture.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Glyph {
    pub width: u8,
    /// How far below the line's top the glyph starts.
    pub top: u8,
    pub height: u8,
    /// One palette index per pixel, row by row, `width * height` of them.
    /// Zero is transparent.
    pub pixels: Vec<u8>,
}

impl Glyph {
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// How tall the glyph sits, counting the gap above it.
    pub fn extent(&self) -> u32 {
        self.top as u32 + self.height as u32
    }
}

/// A decoded font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Font {
    pub glyphs: Vec<Glyph>,
}

impl Font {
    /// Tallest a line of this font can be.
    pub fn line_height(&self) -> u32 {
        self.glyphs.iter().map(Glyph::extent).max().unwrap_or(0)
    }

    /// Widest glyph, which is what a fixed cell would have to be.
    pub fn max_width(&self) -> u32 {
        self.glyphs.iter().map(|g| g.width as u32).max().unwrap_or(0)
    }

    /// Glyphs that draw something.
    pub fn drawn(&self) -> usize {
        self.glyphs.iter().filter(|g| !g.is_empty()).count()
    }
}

/// Read a `.FNT`.
pub fn decode(data: &[u8]) -> Result<Font, FntError> {
    if data.len() < 16 {
        return Err(FntError::TooSmall { len: data.len() });
    }
    let word = |at: usize| u16::from_le_bytes([data[at], data[at + 1]]) as usize;

    let magic = word(2) as u16;
    if magic != MAGIC {
        return Err(FntError::NotAFont { magic });
    }

    let offsets = word(6);
    let widths = word(8);
    let rows = word(12);
    for at in [offsets, widths, rows] {
        if at == 0 || at >= data.len() {
            return Err(FntError::TableOutside { at });
        }
    }
    // A table may stop short of 256 entries. Kyrandia's own font ends
    // its row table two bytes before the end of the file, so the last
    // character has no entry at all - and demanding a full table
    // rejects the one font this was written for.
    let byte = |table: usize, index: usize| data.get(table + index).copied().unwrap_or(0);
    let short = |table: usize, index: usize, stride: usize| table + index * stride < data.len();

    let mut glyphs = Vec::with_capacity(GLYPHS);
    for index in 0..GLYPHS {
        let width = byte(widths, index);
        let top = byte(rows, index * 2);
        let height = byte(rows, index * 2 + 1);
        let at = if short(offsets, index, 2) && offsets + index * 2 + 1 < data.len() {
            word(offsets + index * 2)
        } else {
            0
        };

        // Four bits a pixel, so a row takes half its width rounded up.
        let stride = (width as usize).div_ceil(2);
        let needed = stride * height as usize;
        let pixels = if width == 0 || height == 0 || at == 0 || at + needed > data.len() {
            Vec::new()
        } else {
            unpack(&data[at..at + needed], width, height)
        };
        glyphs.push(Glyph {
            width: if pixels.is_empty() { 0 } else { width },
            top,
            height: if pixels.is_empty() { 0 } else { height },
            pixels,
        });
    }
    Ok(Font { glyphs })
}

/// Four bits a pixel into one byte a pixel.
///
/// Low nibble first: the left pixel of a pair is in the bottom half of
/// the byte, which is the opposite of what reading hex suggests and is
/// the sort of thing that produces a font written backwards.
fn unpack(packed: &[u8], width: u8, height: u8) -> Vec<u8> {
    let stride = (width as usize).div_ceil(2);
    let mut out = Vec::with_capacity(width as usize * height as usize);
    for row in 0..height as usize {
        for x in 0..width as usize {
            let byte = packed.get(row * stride + x / 2).copied().unwrap_or(0);
            out.push(if x % 2 == 0 { byte & 0x0F } else { byte >> 4 });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a font holding one glyph at `code`.
    fn font_with(code: usize, width: u8, height: u8, packed: &[u8]) -> Vec<u8> {
        let header = 16usize;
        let offsets = header;
        let widths = offsets + GLYPHS * 2;
        let rows = widths + GLYPHS;
        let bitmaps = rows + GLYPHS * 2;
        let mut data = vec![0u8; bitmaps + packed.len()];

        let put = |data: &mut Vec<u8>, at: usize, value: u16| {
            data[at..at + 2].copy_from_slice(&value.to_le_bytes());
        };
        put(&mut data, 0, (bitmaps + packed.len()) as u16);
        put(&mut data, 2, MAGIC);
        put(&mut data, 6, offsets as u16);
        put(&mut data, 8, widths as u16);
        put(&mut data, 12, rows as u16);

        put(&mut data, offsets + code * 2, bitmaps as u16);
        data[widths + code] = width;
        data[rows + code * 2] = 2; // top
        data[rows + code * 2 + 1] = height;
        data[bitmaps..bitmaps + packed.len()].copy_from_slice(packed);
        data
    }

    #[test]
    fn a_glyph_is_two_pixels_to_a_byte_low_half_first() {
        // Reading the halves the other way round writes the font
        // backwards, which looks like a broken decoder rather than a
        // swapped nibble.
        let data = font_with(b'A' as usize, 4, 2, &[0x21, 0x43, 0x65, 0x87]);
        let font = decode(&data).expect("decode");
        let glyph = &font.glyphs[b'A' as usize];
        assert_eq!(glyph.width, 4);
        assert_eq!(glyph.height, 2);
        assert_eq!(glyph.pixels, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn an_odd_width_still_fills_whole_bytes() {
        let data = font_with(b'i' as usize, 3, 1, &[0x21, 0x03]);
        let font = decode(&data).expect("decode");
        assert_eq!(font.glyphs[b'i' as usize].pixels, vec![1, 2, 3]);
    }

    #[test]
    fn a_character_the_font_does_not_draw_is_empty_rather_than_missing() {
        // Every code has a slot, so indexing by a byte is total.
        let data = font_with(b'A' as usize, 4, 2, &[0; 4]);
        let font = decode(&data).expect("decode");
        assert_eq!(font.glyphs.len(), GLYPHS);
        assert!(font.glyphs[b'Z' as usize].is_empty());
        assert_eq!(font.drawn(), 1);
    }

    #[test]
    fn a_glyph_pointing_past_the_end_is_dropped_not_read() {
        let mut data = font_with(b'A' as usize, 4, 2, &[0; 4]);
        // Point it somewhere impossible.
        let offsets = 16;
        data[offsets + b'A' as usize * 2..offsets + b'A' as usize * 2 + 2]
            .copy_from_slice(&0xFFFFu16.to_le_bytes());
        let font = decode(&data).expect("decode");
        assert!(font.glyphs[b'A' as usize].is_empty());
    }

    #[test]
    fn the_line_height_counts_the_gap_above_a_glyph() {
        // A letter that starts two rows down and is two tall needs four
        // rows of line, not two.
        let data = font_with(b'A' as usize, 4, 2, &[0; 4]);
        let font = decode(&data).expect("decode");
        assert_eq!(font.line_height(), 4);
        assert_eq!(font.max_width(), 4);
    }

    #[test]
    fn something_that_is_not_a_font_is_refused() {
        assert_eq!(decode(&[]), Err(FntError::TooSmall { len: 0 }));
        let mut data = font_with(b'A' as usize, 1, 1, &[0]);
        data[2] = 0xAB;
        data[3] = 0xCD;
        assert!(matches!(decode(&data), Err(FntError::NotAFont { .. })));
    }

    #[test]
    fn a_table_outside_the_file_is_refused_rather_than_read() {
        let mut data = font_with(b'A' as usize, 1, 1, &[0]);
        data[8..10].copy_from_slice(&0xFFFEu16.to_le_bytes());
        assert!(matches!(decode(&data), Err(FntError::TableOutside { .. })));
    }
}
