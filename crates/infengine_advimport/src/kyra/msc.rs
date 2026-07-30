//! A5.2: the Kyrandia `.MSC` walkable-region mask.
//!
//! Every room ships a mask alongside its background. It uses the same
//! container as a `.CPS` image - Format80 over a ten-byte header - but
//! decodes to 320x144 rather than 320x200, because the bottom 56 rows of
//! a Kyrandia screen are the inventory bar, not the room.
//!
//! One byte per pixel. Bit 7 is what matters: set means blocked, clear
//! means the actor may stand there. The low bits carry per-pixel flags
//! whose meaning is not pinned down - across the retail data they take
//! values 0 and 4..6, and the walkable ones appear on only a handful of
//! pixels per room, so they are not depth zones. They are ignored rather
//! than guessed at.
//!
//! This is why the mask is worth decoding at all: it is the original
//! game's own walkable region, so an imported room arrives with a floor
//! an author would otherwise have to trace by hand.

use std::fmt;

use infengine_adventure::WalkArea;
use infengine_adventure::mask;

use super::cps;
use super::format80;

/// Width of a Kyrandia mask, matching the screen.
pub const MASK_WIDTH: usize = 320;
/// Height of the playable area, above the inventory bar.
pub const MASK_HEIGHT: usize = 144;
/// Bytes in a whole mask.
pub const MASK_BYTES: usize = MASK_WIDTH * MASK_HEIGHT;

/// Bit that marks a pixel as blocked.
const BLOCKED_BIT: u8 = 0x80;

const HEADER_LEN: usize = 10;

#[derive(Debug, Clone, PartialEq)]
pub enum MscError {
    TooSmall { len: usize },
    /// The header declared a size that is not a Kyrandia mask.
    UnexpectedSize { declared: usize },
    /// The header named a compression this importer does not implement.
    UnsupportedCompression(cps::Compression),
    Decompress(format80::Format80Error),
}

impl fmt::Display for MscError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall { len } => write!(f, "MSC is only {len} bytes"),
            Self::UnexpectedSize { declared } => write!(
                f,
                "MSC declares {declared} bytes, not the {MASK_BYTES} of a {MASK_WIDTH}x{MASK_HEIGHT} mask"
            ),
            Self::UnsupportedCompression(c) => write!(f, "MSC uses {c}, which is not supported"),
            Self::Decompress(e) => write!(f, "MSC failed to decompress: {e}"),
        }
    }
}

impl std::error::Error for MscError {}

/// A decoded mask: one byte per pixel of the playable area.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    pub cells: Vec<u8>,
}

impl Mask {
    /// May the actor stand on this pixel?
    pub fn is_walkable(&self, x: usize, y: usize) -> bool {
        if x >= MASK_WIDTH || y >= MASK_HEIGHT {
            return false;
        }
        self.cells[y * MASK_WIDTH + x] & BLOCKED_BIT == 0
    }

    /// The mask as a plain walkability bitmap.
    pub fn to_bitmap(&self) -> Vec<bool> {
        self.cells.iter().map(|c| c & BLOCKED_BIT == 0).collect()
    }

    /// How many pixels are walkable, for sanity checks and reporting.
    pub fn walkable_count(&self) -> usize {
        self.cells.iter().filter(|c| *c & BLOCKED_BIT == 0).count()
    }

    /// Trace the mask into walk polygons.
    pub fn to_walk_area(&self) -> WalkArea {
        mask::trace(
            &self.to_bitmap(),
            MASK_WIDTH,
            MASK_HEIGHT,
            mask::DEFAULT_TOLERANCE,
        )
    }
}

/// Decode a `.MSC` file.
pub fn decode(data: &[u8]) -> Result<Mask, MscError> {
    if data.len() < HEADER_LEN {
        return Err(MscError::TooSmall { len: data.len() });
    }
    let compression = cps::Compression::from_code(u16::from_le_bytes([data[2], data[3]]));
    let declared = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if declared != MASK_BYTES {
        return Err(MscError::UnexpectedSize { declared });
    }

    let body = &data[HEADER_LEN..];
    let cells = match compression {
        cps::Compression::Format80 => {
            format80::decompress(body, MASK_BYTES).map_err(MscError::Decompress)?
        }
        cps::Compression::None if body.len() >= MASK_BYTES => body[..MASK_BYTES].to_vec(),
        other => return Err(MscError::UnsupportedCompression(other)),
    };
    Ok(Mask { cells })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    fn header(comp: u16, size: u32) -> Vec<u8> {
        let mut h = Vec::new();
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(&comp.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h
    }

    /// A mask with a walkable rectangle near the bottom, like a room
    /// floor, and everything else blocked.
    fn floor_mask() -> Vec<u8> {
        let mut cells = vec![BLOCKED_BIT; MASK_BYTES];
        for y in 100..140 {
            for x in 40..280 {
                cells[y * MASK_WIDTH + x] = 0;
            }
        }
        cells
    }

    fn uncompressed_file(cells: &[u8]) -> Vec<u8> {
        let mut file = header(0, MASK_BYTES as u32);
        file.extend_from_slice(cells);
        file
    }

    #[test]
    fn uncompressed_mask_decodes() {
        let mask = decode(&uncompressed_file(&floor_mask())).expect("decode");
        assert_eq!(mask.cells.len(), MASK_BYTES);
        assert!(mask.is_walkable(100, 120));
        assert!(!mask.is_walkable(10, 10));
    }

    #[test]
    fn only_bit_seven_decides_walkability() {
        // The low bits carry flags; a pixel with flags set but bit 7
        // clear is still floor.
        let mut cells = vec![BLOCKED_BIT; MASK_BYTES];
        cells[0] = 0x06;
        cells[1] = 0x86;
        let mask = decode(&uncompressed_file(&cells)).expect("decode");
        assert!(mask.is_walkable(0, 0), "0x06 is walkable with flags");
        assert!(!mask.is_walkable(1, 0), "0x86 is blocked with flags");
    }

    #[test]
    fn out_of_bounds_is_not_walkable() {
        let mask = decode(&uncompressed_file(&floor_mask())).expect("decode");
        assert!(!mask.is_walkable(MASK_WIDTH, 0));
        assert!(!mask.is_walkable(0, MASK_HEIGHT));
    }

    #[test]
    fn walkable_count_matches_the_painted_area() {
        let mask = decode(&uncompressed_file(&floor_mask())).expect("decode");
        assert_eq!(mask.walkable_count(), 40 * 240);
    }

    #[test]
    fn tracing_produces_a_floor_covering_the_walkable_pixels() {
        let mask = decode(&uncompressed_file(&floor_mask())).expect("decode");
        let area = mask.to_walk_area();
        assert_eq!(area.solids.len(), 1);
        assert!(area.contains(Vec2::new(160.0, 120.0)));
        assert!(!area.contains(Vec2::new(10.0, 10.0)));
    }

    #[test]
    fn a_mask_with_a_blocked_island_produces_a_hole() {
        let mut cells = floor_mask();
        for y in 110..125 {
            for x in 140..180 {
                cells[y * MASK_WIDTH + x] = BLOCKED_BIT;
            }
        }
        let mask = decode(&uncompressed_file(&cells)).expect("decode");
        let area = mask.to_walk_area();
        assert_eq!(area.holes.len(), 1, "the blocked island is a hole");
        assert!(!area.contains(Vec2::new(160.0, 117.0)));
        assert!(area.contains(Vec2::new(60.0, 120.0)));
    }

    #[test]
    fn format80_body_decodes() {
        // Run-length fill of the whole mask with a walkable value.
        let mut file = header(4, MASK_BYTES as u32);
        let mut remaining = MASK_BYTES;
        while remaining > 0 {
            let run = remaining.min(0xFFFF);
            file.push(0xFE);
            file.extend_from_slice(&(run as u16).to_le_bytes());
            file.push(0x00);
            remaining -= run;
        }
        let mask = decode(&file).expect("decode");
        assert_eq!(mask.walkable_count(), MASK_BYTES);
    }

    #[test]
    fn a_short_file_is_rejected() {
        assert!(matches!(decode(&[0, 0]), Err(MscError::TooSmall { .. })));
    }

    #[test]
    fn a_declared_size_that_is_not_a_mask_is_rejected() {
        // 320x200 would be a background, not a mask; accepting it would
        // silently mis-shape the floor.
        let file = header(4, 64000);
        assert_eq!(
            decode(&file),
            Err(MscError::UnexpectedSize { declared: 64000 })
        );
    }

    #[test]
    fn an_unsupported_compression_is_named() {
        let file = header(2, MASK_BYTES as u32);
        assert_eq!(
            decode(&file),
            Err(MscError::UnsupportedCompression(cps::Compression::Lzw14))
        );
    }
}
