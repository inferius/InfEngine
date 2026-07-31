//! A7.1: the DDS tiles a Black Mirror scene stores its art in.
//!
//! Textures are not stored whole. Each one is cut into a grid of
//! power-of-two tiles, laid out row by row with widths descending across
//! a row (1024, then 256, then 32), and each tile is a complete DDS file
//! written back to back with a `u32` length in front:
//!
//! ```text
//!   [u32 total size]["DDS " + 124-byte header + pixels]  x N
//! ```
//!
//! Three pixel formats show up in a retail install: uncompressed BGRA32
//! for backgrounds, DXT5 for anything with soft alpha, and DXT1 for a
//! handful of opaque overlays. All three are decoded here to RGBA8.

use std::fmt;

/// Bytes of a DDS header, not counting the four-byte magic.
const HEADER_LEN: usize = 124;
/// Magic plus header.
const PREAMBLE: usize = 4 + HEADER_LEN;

/// Offsets into the file, measured from the magic. The DDS_PIXELFORMAT
/// block sits at header offset 72, so its fields land at 76 and up -
/// four bytes earlier than the surrounding fields would suggest, which
/// is easy to get wrong and produced a reader that decoded nothing.
const OFF_HEIGHT: usize = 4 + 8;
const OFF_WIDTH: usize = 4 + 12;
const OFF_FOURCC: usize = 4 + 80;
const OFF_BITCOUNT: usize = 4 + 84;
const OFF_MASKS: usize = 4 + 88;

/// Largest tile edge worth believing.
const MAX_TILE_EDGE: u32 = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DdsError {
    /// Not a DDS at this position.
    NotDds,
    /// Header present but the file ends inside it.
    Truncated,
    /// Dimensions outside anything a scene tile plausibly has.
    ImplausibleSize { width: u32, height: u32 },
    /// A pixel format this importer does not decode.
    UnsupportedFormat { fourcc: [u8; 4], bits: u32 },
    /// The header promises more pixels than the blob holds.
    ShortPayload { have: usize, need: usize },
}

impl fmt::Display for DdsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDds => write!(f, "no DDS magic at this position"),
            Self::Truncated => write!(f, "DDS header is cut short"),
            Self::ImplausibleSize { width, height } => {
                write!(f, "DDS tile claims {width}x{height}")
            }
            Self::UnsupportedFormat { fourcc, bits } => {
                let name = String::from_utf8_lossy(fourcc);
                write!(f, "DDS pixel format {name:?} at {bits} bpp is not supported")
            }
            Self::ShortPayload { have, need } => {
                write!(f, "DDS payload holds {have} of {need} bytes")
            }
        }
    }
}

impl std::error::Error for DdsError {}

/// How a tile's pixels are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Uncompressed, with channel masks read from the header.
    Uncompressed {
        bits: u32,
        r: u32,
        g: u32,
        b: u32,
        a: u32,
    },
    /// BC1, four bits per pixel.
    Dxt1,
    /// BC3: BC1 colour plus an interpolated alpha block.
    Dxt5,
}

/// One decoded tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    /// RGBA8, row-major from the top.
    pub rgba: Vec<u8>,
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Read the pixel format out of a header.
fn format_of(data: &[u8]) -> Result<Format, DdsError> {
    let fourcc: [u8; 4] = data
        .get(OFF_FOURCC..OFF_FOURCC + 4)
        .ok_or(DdsError::Truncated)?
        .try_into()
        .map_err(|_| DdsError::Truncated)?;
    let bits = u32_at(data, OFF_BITCOUNT).ok_or(DdsError::Truncated)?;

    match &fourcc {
        b"DXT1" => Ok(Format::Dxt1),
        b"DXT5" => Ok(Format::Dxt5),
        [0, 0, 0, 0] => {
            let r = u32_at(data, OFF_MASKS).ok_or(DdsError::Truncated)?;
            let g = u32_at(data, OFF_MASKS + 4).ok_or(DdsError::Truncated)?;
            let b = u32_at(data, OFF_MASKS + 8).ok_or(DdsError::Truncated)?;
            let a = u32_at(data, OFF_MASKS + 12).ok_or(DdsError::Truncated)?;
            if bits != 32 && bits != 24 {
                return Err(DdsError::UnsupportedFormat { fourcc, bits });
            }
            Ok(Format::Uncompressed { bits, r, g, b, a })
        }
        _ => Err(DdsError::UnsupportedFormat { fourcc, bits }),
    }
}

/// Decode one DDS blob starting at its magic.
pub fn decode(data: &[u8]) -> Result<Tile, DdsError> {
    if data.len() < PREAMBLE {
        return Err(DdsError::Truncated);
    }
    if &data[..4] != b"DDS " {
        return Err(DdsError::NotDds);
    }
    let height = u32_at(data, OFF_HEIGHT).ok_or(DdsError::Truncated)?;
    let width = u32_at(data, OFF_WIDTH).ok_or(DdsError::Truncated)?;
    if width == 0 || height == 0 || width > MAX_TILE_EDGE || height > MAX_TILE_EDGE {
        return Err(DdsError::ImplausibleSize { width, height });
    }

    let format = format_of(data)?;
    let payload = &data[PREAMBLE..];
    let rgba = match format {
        Format::Uncompressed { bits, r, g, b, a } => {
            decode_uncompressed(payload, width, height, bits, [r, g, b, a])?
        }
        Format::Dxt1 => decode_bc(payload, width, height, false)?,
        Format::Dxt5 => decode_bc(payload, width, height, true)?,
    };

    Ok(Tile {
        width,
        height,
        format,
        rgba,
    })
}

/// Shift and scale a masked channel to 0..=255.
fn channel(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 255;
    }
    let shift = mask.trailing_zeros();
    let width = mask.count_ones();
    let raw = (value & mask) >> shift;
    let max = (1u32 << width) - 1;
    if max == 0 {
        return 255;
    }
    ((raw * 255) / max) as u8
}

fn decode_uncompressed(
    payload: &[u8],
    width: u32,
    height: u32,
    bits: u32,
    masks: [u32; 4],
) -> Result<Vec<u8>, DdsError> {
    let bytes_per_pixel = (bits / 8) as usize;
    let need = width as usize * height as usize * bytes_per_pixel;
    if payload.len() < need {
        return Err(DdsError::ShortPayload {
            have: payload.len(),
            need,
        });
    }
    let [rm, gm, bm, am] = masks;
    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for chunk in payload[..need].chunks_exact(bytes_per_pixel) {
        let mut value = 0u32;
        for (i, byte) in chunk.iter().enumerate() {
            value |= (*byte as u32) << (8 * i);
        }
        out.push(channel(value, rm));
        out.push(channel(value, gm));
        out.push(channel(value, bm));
        // A zero alpha mask means the format carries no alpha, so the
        // tile is opaque. Treating a missing mask as alpha 0 would make
        // every background invisible.
        out.push(if am == 0 { 255 } else { channel(value, am) });
    }
    Ok(out)
}

/// Expand a 5-6-5 colour to RGB8.
fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 0x1F) as u32;
    let g = ((c >> 5) & 0x3F) as u32;
    let b = (c & 0x1F) as u32;
    [
        ((r * 255 + 15) / 31) as u8,
        ((g * 255 + 31) / 63) as u8,
        ((b * 255 + 15) / 31) as u8,
    ]
}

/// Decode BC1 or BC3.
///
/// BC3 prefixes each block with eight bytes of interpolated alpha and
/// then uses the BC1 colour block, except that the "one bit alpha" mode
/// BC1 has when `c0 <= c1` does not apply - alpha comes from the alpha
/// block instead.
fn decode_bc(
    payload: &[u8],
    width: u32,
    height: u32,
    with_alpha: bool,
) -> Result<Vec<u8>, DdsError> {
    let block_bytes = if with_alpha { 16 } else { 8 };
    let bx = width.div_ceil(4) as usize;
    let by = height.div_ceil(4) as usize;
    let need = bx * by * block_bytes;
    if payload.len() < need {
        return Err(DdsError::ShortPayload {
            have: payload.len(),
            need,
        });
    }

    let w = width as usize;
    let h = height as usize;
    let mut out = vec![0u8; w * h * 4];

    for byi in 0..by {
        for bxi in 0..bx {
            let base = (byi * bx + bxi) * block_bytes;
            let (alpha, color_at) = if with_alpha {
                (Some(&payload[base..base + 8]), base + 8)
            } else {
                (None, base)
            };

            let c0 = u16::from_le_bytes([payload[color_at], payload[color_at + 1]]);
            let c1 = u16::from_le_bytes([payload[color_at + 2], payload[color_at + 3]]);
            let bits = u32::from_le_bytes([
                payload[color_at + 4],
                payload[color_at + 5],
                payload[color_at + 6],
                payload[color_at + 7],
            ]);

            let a = rgb565(c0);
            let b = rgb565(c1);
            let mut palette = [[0u8; 4]; 4];
            palette[0] = [a[0], a[1], a[2], 255];
            palette[1] = [b[0], b[1], b[2], 255];
            if c0 > c1 || with_alpha {
                for i in 0..3 {
                    palette[2][i] = ((2 * a[i] as u32 + b[i] as u32) / 3) as u8;
                    palette[3][i] = ((a[i] as u32 + 2 * b[i] as u32) / 3) as u8;
                }
                palette[2][3] = 255;
                palette[3][3] = 255;
            } else {
                for i in 0..3 {
                    palette[2][i] = ((a[i] as u32 + b[i] as u32) / 2) as u8;
                    palette[3][i] = 0;
                }
                palette[2][3] = 255;
                palette[3][3] = 0;
            }

            let alpha_palette = alpha.map(|block| alpha_ramp(block[0], block[1]));
            let alpha_bits = alpha.map(|block| {
                let mut v = 0u64;
                for (i, byte) in block[2..8].iter().enumerate() {
                    v |= (*byte as u64) << (8 * i);
                }
                v
            });

            for py in 0..4 {
                for px in 0..4 {
                    let x = bxi * 4 + px;
                    let y = byi * 4 + py;
                    if x >= w || y >= h {
                        continue;
                    }
                    let index = ((bits >> (2 * (py * 4 + px))) & 3) as usize;
                    let mut texel = palette[index];
                    if let (Some(ramp), Some(abits)) = (alpha_palette, alpha_bits) {
                        let ai = ((abits >> (3 * (py * 4 + px))) & 7) as usize;
                        texel[3] = ramp[ai];
                    }
                    let dst = (y * w + x) * 4;
                    out[dst..dst + 4].copy_from_slice(&texel);
                }
            }
        }
    }
    Ok(out)
}

/// The eight alpha values a BC3 block interpolates between.
fn alpha_ramp(a0: u8, a1: u8) -> [u8; 8] {
    let mut ramp = [0u8; 8];
    ramp[0] = a0;
    ramp[1] = a1;
    if a0 > a1 {
        for (i, slot) in ramp.iter_mut().enumerate().take(8).skip(2) {
            let k = i as u32 - 1;
            *slot = (((7 - k) * a0 as u32 + k * a1 as u32) / 7) as u8;
        }
    } else {
        for (i, slot) in ramp.iter_mut().enumerate().take(6).skip(2) {
            let k = i as u32 - 1;
            *slot = (((5 - k) * a0 as u32 + k * a1 as u32) / 5) as u8;
        }
        ramp[6] = 0;
        ramp[7] = 255;
    }
    ramp
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal DDS blob.
    fn dds(width: u32, height: u32, fourcc: &[u8; 4], bits: u32, masks: [u32; 4], payload: &[u8]) -> Vec<u8> {
        // Offsets are spelled out from the DDS specification rather
        // than reusing the reader's constants. Sharing them let a
        // four-byte error in the pixel-format block pass every test
        // while decoding nothing at all in practice.
        let mut v = vec![0u8; PREAMBLE];
        v[..4].copy_from_slice(b"DDS ");
        v[4..8].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        v[12..16].copy_from_slice(&height.to_le_bytes()); // dwHeight
        v[16..20].copy_from_slice(&width.to_le_bytes()); // dwWidth
        v[84..88].copy_from_slice(fourcc); // ddspf.dwFourCC
        v[88..92].copy_from_slice(&bits.to_le_bytes()); // ddspf.dwRGBBitCount
        for (i, m) in masks.iter().enumerate() {
            let at = 92 + i * 4; // ddspf.dwRBitMask onwards
            v[at..at + 4].copy_from_slice(&m.to_le_bytes());
        }
        v.extend_from_slice(payload);
        v
    }

    /// The mask set a Black Mirror background uses.
    const BGRA: [u32; 4] = [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000];

    #[test]
    fn bgra32_decodes_with_the_channels_in_the_right_order() {
        // One pixel, stored B G R A.
        let file = dds(1, 1, &[0; 4], 32, BGRA, &[0x10, 0x20, 0x30, 0x40]);
        let tile = decode(&file).expect("decode");
        assert_eq!(tile.width, 1);
        assert_eq!(tile.rgba, vec![0x30, 0x20, 0x10, 0x40]);
    }

    #[test]
    fn a_missing_alpha_mask_yields_an_opaque_tile() {
        // Treating a zero mask as alpha 0 would make every background
        // invisible, which is exactly the bug this guards.
        let file = dds(1, 1, &[0; 4], 32, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0], &[1, 2, 3, 0]);
        let tile = decode(&file).expect("decode");
        assert_eq!(tile.rgba[3], 255);
    }

    #[test]
    fn channel_scaling_spans_the_full_range() {
        assert_eq!(channel(0x00FF_0000, 0x00FF_0000), 255);
        assert_eq!(channel(0x0000_0000, 0x00FF_0000), 0);
        assert_eq!(channel(0xF800, 0xF800), 255, "a 5-bit channel reaches white");
    }

    #[test]
    fn rgb565_endpoints_are_exact() {
        assert_eq!(rgb565(0xFFFF), [255, 255, 255]);
        assert_eq!(rgb565(0x0000), [0, 0, 0]);
        assert_eq!(rgb565(0xF800), [255, 0, 0]);
    }

    #[test]
    fn dxt1_solid_block_decodes_to_that_colour() {
        // c0 = white, c1 = black, every texel index 0.
        let mut block = Vec::new();
        block.extend_from_slice(&0xFFFFu16.to_le_bytes());
        block.extend_from_slice(&0x0000u16.to_le_bytes());
        block.extend_from_slice(&0u32.to_le_bytes());
        let file = dds(4, 4, b"DXT1", 0, [0; 4], &block);
        let tile = decode(&file).expect("decode");
        assert_eq!(tile.format, Format::Dxt1);
        assert_eq!(&tile.rgba[..4], &[255, 255, 255, 255]);
        assert_eq!(tile.rgba.len(), 4 * 4 * 4);
    }

    #[test]
    fn dxt1_punchthrough_alpha_applies_when_c0_is_not_greater() {
        // c0 <= c1 selects the three-colour mode where index 3 is
        // transparent black.
        let mut block = Vec::new();
        block.extend_from_slice(&0x0000u16.to_le_bytes());
        block.extend_from_slice(&0xFFFFu16.to_le_bytes());
        block.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // all index 3
        let file = dds(4, 4, b"DXT1", 0, [0; 4], &block);
        let tile = decode(&file).expect("decode");
        assert_eq!(tile.rgba[3], 0, "index 3 is transparent in this mode");
    }

    #[test]
    fn dxt5_alpha_endpoints_are_taken_from_the_alpha_block() {
        let mut block = Vec::new();
        block.push(200); // a0
        block.push(10); // a1
        block.extend_from_slice(&[0u8; 6]); // every texel index 0 -> a0
        block.extend_from_slice(&0xFFFFu16.to_le_bytes());
        block.extend_from_slice(&0x0000u16.to_le_bytes());
        block.extend_from_slice(&0u32.to_le_bytes());
        let file = dds(4, 4, b"DXT5", 0, [0; 4], &block);
        let tile = decode(&file).expect("decode");
        assert_eq!(tile.format, Format::Dxt5);
        assert_eq!(tile.rgba[3], 200);
        assert_eq!(&tile.rgba[..3], &[255, 255, 255]);
    }

    #[test]
    fn dxt5_never_uses_the_punchthrough_colour_mode() {
        // In BC3 the colour block is always four-colour, even when
        // c0 <= c1; treating it as BC1 would punch holes in the art.
        let mut block = Vec::new();
        block.extend_from_slice(&[255, 255, 0, 0, 0, 0, 0, 0]);
        block.extend_from_slice(&0x0000u16.to_le_bytes());
        block.extend_from_slice(&0xFFFFu16.to_le_bytes());
        block.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // index 3
        let file = dds(4, 4, b"DXT5", 0, [0; 4], &block);
        let tile = decode(&file).expect("decode");
        assert_eq!(tile.rgba[3], 255, "alpha comes from the alpha block");
        assert!(tile.rgba[0] > 0, "index 3 must not be forced to black");
    }

    #[test]
    fn alpha_ramp_covers_both_interpolation_modes() {
        let wide = alpha_ramp(255, 0);
        assert_eq!(wide[0], 255);
        assert_eq!(wide[1], 0);
        assert!(wide[2] > wide[3], "the eight-value ramp descends");

        let narrow = alpha_ramp(0, 255);
        assert_eq!(narrow[6], 0, "six-value mode pins index 6 to zero");
        assert_eq!(narrow[7], 255, "and index 7 to full");
    }

    #[test]
    fn a_partial_block_at_the_edge_is_clipped_not_written_past() {
        // 3x3 image occupies one 4x4 block; the extra texels must be
        // dropped rather than overflowing the buffer.
        let mut block = Vec::new();
        block.extend_from_slice(&0xFFFFu16.to_le_bytes());
        block.extend_from_slice(&0x0000u16.to_le_bytes());
        block.extend_from_slice(&0u32.to_le_bytes());
        let file = dds(3, 3, b"DXT1", 0, [0; 4], &block);
        let tile = decode(&file).expect("decode");
        assert_eq!(tile.rgba.len(), 3 * 3 * 4);
    }

    #[test]
    fn a_non_dds_blob_is_rejected() {
        let mut file = dds(4, 4, b"DXT1", 0, [0; 4], &[0u8; 8]);
        file[0] = b'X';
        assert_eq!(decode(&file), Err(DdsError::NotDds));
        assert_eq!(decode(&[]), Err(DdsError::Truncated));
    }

    #[test]
    fn an_absurd_size_is_rejected_before_allocating() {
        let file = dds(99999, 99999, b"DXT1", 0, [0; 4], &[]);
        assert!(matches!(
            decode(&file),
            Err(DdsError::ImplausibleSize { .. })
        ));
    }

    #[test]
    fn a_short_payload_is_reported_not_padded() {
        let file = dds(64, 64, b"DXT1", 0, [0; 4], &[0u8; 16]);
        assert!(matches!(decode(&file), Err(DdsError::ShortPayload { .. })));
    }

    #[test]
    fn an_unknown_fourcc_is_named() {
        let file = dds(4, 4, b"DXT3", 0, [0; 4], &[0u8; 16]);
        assert!(matches!(
            decode(&file),
            Err(DdsError::UnsupportedFormat { .. })
        ));
    }
}
