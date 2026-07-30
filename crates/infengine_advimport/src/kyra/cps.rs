//! A3.4: the Kyrandia `.CPS` image.
//!
//! A CPS is a full-screen 320x200 background stored as 8-bit palette
//! indices, usually Format80-compressed, sometimes carrying its own
//! palette. The header is ten bytes:
//!
//! | offset | size | meaning |
//! |--------|------|---------|
//! | 0      | u16  | file size minus the two bytes of this field |
//! | 2      | u16  | compression type |
//! | 4      | u32  | uncompressed size |
//! | 8      | u16  | palette size, 0 or 768 |
//!
//! The palette, when present, follows the header and precedes the pixel
//! data.

use std::fmt;

use super::format80::{self, Format80Error};
use super::palette::{Palette, VGA_PALETTE_BYTES};

/// Native size of a Kyrandia background.
pub const SCREEN_WIDTH: u32 = 320;
pub const SCREEN_HEIGHT: u32 = 200;

const HEADER_LEN: usize = 10;

/// Compression scheme named by the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Lzw12,
    Lzw14,
    Rle,
    /// Westwood Format80 / LCW. What Kyrandia backgrounds actually use.
    Format80,
    Unknown(u16),
}

impl Compression {
    fn from_code(code: u16) -> Self {
        match code {
            0 => Self::None,
            1 => Self::Lzw12,
            2 => Self::Lzw14,
            3 => Self::Rle,
            4 => Self::Format80,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for Compression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "uncompressed"),
            Self::Lzw12 => write!(f, "LZW-12"),
            Self::Lzw14 => write!(f, "LZW-14"),
            Self::Rle => write!(f, "RLE"),
            Self::Format80 => write!(f, "Format80"),
            Self::Unknown(c) => write!(f, "unknown({c})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CpsError {
    TooSmall { len: usize },
    /// Header declared a scheme this importer does not implement. The
    /// Kyrandia data set only ever uses Format80 and uncompressed, but
    /// the header can name others.
    UnsupportedCompression(Compression),
    /// Declared uncompressed size is not a plausible image.
    ImplausibleSize { size: usize },
    /// Header promised a palette the file is too short to hold.
    TruncatedPalette,
    Decompress(Format80Error),
}

impl fmt::Display for CpsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall { len } => write!(f, "CPS is only {len} bytes, shorter than its header"),
            Self::UnsupportedCompression(c) => write!(f, "CPS uses {c}, which is not supported"),
            Self::ImplausibleSize { size } => {
                write!(f, "CPS declares {size} pixels, which is not a sane image")
            }
            Self::TruncatedPalette => write!(f, "CPS header promises a palette the file lacks"),
            Self::Decompress(e) => write!(f, "CPS pixel data failed to decompress: {e}"),
        }
    }
}

impl std::error::Error for CpsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Decompress(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Format80Error> for CpsError {
    fn from(e: Format80Error) -> Self {
        Self::Decompress(e)
    }
}

/// A decoded CPS: 8-bit indices plus whatever palette shipped with it.
#[derive(Debug, Clone, PartialEq)]
pub struct CpsImage {
    pub width: u32,
    pub height: u32,
    /// One palette index per pixel, row-major from the top.
    pub indices: Vec<u8>,
    /// The palette embedded in this file, if any. Room backgrounds
    /// often carry none and borrow the palette from a sibling file.
    pub palette: Option<Palette>,
    pub compression: Compression,
}

impl CpsImage {
    /// Expand to RGBA8 using the embedded palette, or `fallback` when
    /// the file carried none.
    pub fn to_rgba(&self, fallback: Option<&Palette>) -> Vec<u8> {
        let owned;
        let palette = match (self.palette.as_ref(), fallback) {
            (Some(p), _) => p,
            (None, Some(p)) => p,
            (None, None) => {
                owned = Palette::default();
                &owned
            }
        };
        palette.expand_rgba(&self.indices, None)
    }
}

/// Guard against a corrupt header asking for a gigabyte allocation:
/// the largest thing a CPS legitimately holds is a full screen plus a
/// little slack for the odd oversized asset.
const MAX_PIXELS: usize = 1024 * 1024;

/// Decode a CPS file.
pub fn decode(data: &[u8]) -> Result<CpsImage, CpsError> {
    if data.len() < HEADER_LEN {
        return Err(CpsError::TooSmall { len: data.len() });
    }

    let compression = Compression::from_code(u16::from_le_bytes([data[2], data[3]]));
    let uncompressed_size =
        u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let palette_size = u16::from_le_bytes([data[8], data[9]]) as usize;

    if uncompressed_size == 0 || uncompressed_size > MAX_PIXELS {
        return Err(CpsError::ImplausibleSize {
            size: uncompressed_size,
        });
    }

    let mut pos = HEADER_LEN;
    let palette = if palette_size == VGA_PALETTE_BYTES {
        let end = pos + VGA_PALETTE_BYTES;
        if end > data.len() {
            return Err(CpsError::TruncatedPalette);
        }
        let pal = Palette::from_vga_6bit(&data[pos..end]);
        pos = end;
        pal
    } else {
        if palette_size != 0 {
            // A non-zero, non-768 palette size means the header is not
            // what we think it is; skipping that many bytes anyway would
            // silently decode garbage.
            return Err(CpsError::TruncatedPalette);
        }
        None
    };

    let body = &data[pos.min(data.len())..];
    let indices = match compression {
        Compression::Format80 => format80::decompress(body, uncompressed_size)?,
        Compression::None => {
            if body.len() < uncompressed_size {
                return Err(CpsError::Decompress(Format80Error::OutputTruncated {
                    got: body.len(),
                    expected: uncompressed_size,
                }));
            }
            body[..uncompressed_size].to_vec()
        }
        other => return Err(CpsError::UnsupportedCompression(other)),
    };

    // Backgrounds are full-screen. Anything else keeps the declared
    // pixel count as a single row rather than guessing a shape.
    let (width, height) = if uncompressed_size == (SCREEN_WIDTH * SCREEN_HEIGHT) as usize {
        (SCREEN_WIDTH, SCREEN_HEIGHT)
    } else {
        (uncompressed_size as u32, 1)
    };

    Ok(CpsImage {
        width,
        height,
        indices,
        palette,
        compression,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(comp: u16, size: u32, pal: u16) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&0u16.to_le_bytes()); // file size, unused by the decoder
        h.extend_from_slice(&comp.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&pal.to_le_bytes());
        h
    }

    #[test]
    fn uncompressed_image_decodes() {
        let mut file = header(0, 4, 0);
        file.extend_from_slice(&[1, 2, 3, 4]);
        let img = decode(&file).expect("decode");
        assert_eq!(img.indices, vec![1, 2, 3, 4]);
        assert_eq!(img.compression, Compression::None);
        assert!(img.palette.is_none());
    }

    #[test]
    fn format80_image_decodes() {
        let mut file = header(4, 5, 0);
        // Literal "AB", then copy 3 bytes from 2 back.
        file.extend_from_slice(&[0x82, b'A', b'B', 0x00, 0x02, 0x80]);
        let img = decode(&file).expect("decode");
        assert_eq!(img.indices, b"ABABA");
        assert_eq!(img.compression, Compression::Format80);
    }

    #[test]
    fn embedded_palette_is_parsed_and_skipped() {
        let mut file = header(0, 2, VGA_PALETTE_BYTES as u16);
        let mut pal = vec![0u8; VGA_PALETTE_BYTES];
        pal[3] = 63; // colour 1 = red
        file.extend_from_slice(&pal);
        file.extend_from_slice(&[1, 0]);
        let img = decode(&file).expect("decode");
        assert_eq!(img.indices, vec![1, 0], "palette must not leak into pixels");
        assert_eq!(
            img.palette.as_ref().expect("palette").color(1),
            [255, 0, 0]
        );
    }

    #[test]
    fn full_screen_size_is_recognised() {
        let pixels = (SCREEN_WIDTH * SCREEN_HEIGHT) as usize;
        let mut file = header(0, pixels as u32, 0);
        file.extend_from_slice(&vec![7u8; pixels]);
        let img = decode(&file).expect("decode");
        assert_eq!((img.width, img.height), (SCREEN_WIDTH, SCREEN_HEIGHT));
    }

    #[test]
    fn rgba_expansion_prefers_the_embedded_palette() {
        let mut file = header(0, 1, VGA_PALETTE_BYTES as u16);
        let mut pal = vec![0u8; VGA_PALETTE_BYTES];
        pal[3] = 63;
        file.extend_from_slice(&pal);
        file.extend_from_slice(&[1]);
        let img = decode(&file).expect("decode");
        assert_eq!(img.to_rgba(None), vec![255, 0, 0, 255]);
    }

    #[test]
    fn rgba_expansion_falls_back_when_no_palette_is_embedded() {
        let mut file = header(0, 1, 0);
        file.extend_from_slice(&[1]);
        let img = decode(&file).expect("decode");

        let mut bytes = vec![0u8; VGA_PALETTE_BYTES];
        bytes[4] = 63; // colour 1 = green
        let fallback = Palette::from_vga_6bit(&bytes).expect("palette");
        assert_eq!(img.to_rgba(Some(&fallback)), vec![0, 255, 0, 255]);
    }

    #[test]
    fn short_file_is_rejected() {
        assert!(matches!(
            decode(&[0, 0, 0]),
            Err(CpsError::TooSmall { len: 3 })
        ));
    }

    #[test]
    fn unsupported_compression_is_named_not_guessed() {
        let file = header(2, 16, 0);
        assert_eq!(
            decode(&file),
            Err(CpsError::UnsupportedCompression(Compression::Lzw14))
        );
    }

    #[test]
    fn absurd_declared_size_is_rejected_before_allocating() {
        let file = header(4, u32::MAX, 0);
        assert!(matches!(decode(&file), Err(CpsError::ImplausibleSize { .. })));
        let file = header(4, 0, 0);
        assert!(matches!(decode(&file), Err(CpsError::ImplausibleSize { .. })));
    }

    #[test]
    fn promised_palette_that_is_missing_is_rejected() {
        let file = header(0, 4, VGA_PALETTE_BYTES as u16);
        assert_eq!(decode(&file), Err(CpsError::TruncatedPalette));
    }

    #[test]
    fn nonsense_palette_size_is_rejected_rather_than_skipped() {
        let mut file = header(0, 4, 100);
        file.extend_from_slice(&[0u8; 200]);
        assert_eq!(decode(&file), Err(CpsError::TruncatedPalette));
    }
}
