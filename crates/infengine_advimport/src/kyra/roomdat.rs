//! A3.9: the per-room `.DAT`, and why backgrounds need it.
//!
//! Kyrandia does not give each room a full palette. Every screen starts
//! from the game-wide `PALETTE.COL` and then overwrites a 28-colour
//! window near the top of it with shades chosen for that room - the
//! particular browns of a wooden floor, the greens of one set of
//! curtains. Decode a background without that override and the image is
//! structurally perfect but miscoloured, because those 28 indices still
//! hold whatever the previous room left there.
//!
//! Layout, worked out from a retail install and confirmed against the
//! running game:
//!
//! ```text
//!   "BRINS.MSK\0"        mask file for the walkable region
//!   ...                  variable header
//!   88 <n> 87            marker that ends the header
//!   84 bytes             28 RGB triples, 6-bit VGA
//!   u16                  size of what follows
//!   ...                  object and script records
//!   12 bytes             four entry points, (u16 x, u8 y) each
//! ```
//!
//! The trailing twelve bytes read as four screen positions rather than
//! as room ids: x runs to 300 and y to 132, which is the play area, not
//! an index into 87 rooms. Checked against the walk masks, 340 of 344 of
//! them land on walkable floor - so they are where the actor stands on
//! entering, one per direction. Which slot is which direction is not
//! established, so they are numbered rather than labelled north-east.

use std::fmt;

use glam::Vec2;

use super::palette::Palette;

/// Colours the room override supplies.
pub const OVERRIDE_COLORS: usize = 28;
/// Bytes those colours occupy: three 6-bit components each.
pub const OVERRIDE_BYTES: usize = OVERRIDE_COLORS * 3;
/// Palette index the override lands on. The window sits at the top of
/// the 256-colour table, just below the entries the engine reserves.
pub const OVERRIDE_START: u8 = 228;

/// Entry points stored at the tail: four of them, three bytes each.
pub const ENTRY_POINTS: usize = 4;
const ENTRY_BYTES: usize = ENTRY_POINTS * 3;

/// Byte that opens the marker ending the header.
const MARKER_HEAD: u8 = 0x88;
/// Byte that closes it. The value between the two varies per room.
const MARKER_TAIL: u8 = 0x87;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomDatError {
    /// No NUL-terminated mask name at the start.
    NoMaskName,
    /// The `88 <n> 87` marker never appeared.
    NoPaletteMarker,
    /// The marker was found but the file ends before 84 palette bytes.
    TruncatedPalette { available: usize },
    /// A byte in the palette window exceeded the 6-bit VGA range, so the
    /// window is not where we think it is.
    NotSixBit { at: usize, value: u8 },
}

impl fmt::Display for RoomDatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMaskName => write!(f, "room .DAT does not start with a mask name"),
            Self::NoPaletteMarker => write!(f, "room .DAT has no 88 xx 87 palette marker"),
            Self::TruncatedPalette { available } => write!(
                f,
                "room .DAT palette window holds {available} of {OVERRIDE_BYTES} bytes"
            ),
            Self::NotSixBit { at, value } => write!(
                f,
                "room .DAT palette byte {at} is {value}, above the 6-bit VGA range"
            ),
        }
    }
}

impl std::error::Error for RoomDatError {}

/// What the importer needs out of a room `.DAT`.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomData {
    /// Name of the `.MSK` holding the room's walkable region.
    pub mask_name: String,
    /// 28 RGB triples in 6-bit VGA form.
    pub palette_override: Vec<u8>,
    /// Where the actor stands on entering, one per direction. Rooms
    /// with a single way in repeat the same position four times.
    pub entry_points: Vec<Vec2>,
}

impl RoomData {
    /// Apply this room's colours on top of a base palette.
    pub fn apply(&self, base: &Palette) -> Palette {
        base.with_override(OVERRIDE_START, &self.palette_override)
    }
}

/// Parse a room `.DAT`.
pub fn parse(data: &[u8]) -> Result<RoomData, RoomDatError> {
    let nul = data
        .iter()
        .position(|&b| b == 0)
        .ok_or(RoomDatError::NoMaskName)?;
    if nul == 0 {
        return Err(RoomDatError::NoMaskName);
    }
    let mask_name = String::from_utf8_lossy(&data[..nul]).into_owned();

    // Scan for the marker. The byte between head and tail is a per-room
    // value, so only the two ends identify it.
    let body = &data[nul + 1..];
    let marker = (0..body.len().saturating_sub(2))
        .find(|&i| body[i] == MARKER_HEAD && body[i + 2] == MARKER_TAIL)
        .ok_or(RoomDatError::NoPaletteMarker)?;

    let start = marker + 3;
    let window = body
        .get(start..start + OVERRIDE_BYTES)
        .ok_or(RoomDatError::TruncatedPalette {
            available: body.len().saturating_sub(start),
        })?;

    // Every byte must be a legal 6-bit component. If one is not, the
    // marker we locked onto was a coincidence, and silently accepting it
    // would recolour the room with garbage.
    if let Some((at, &value)) = window.iter().enumerate().find(|&(_, &b)| b > 0x3F) {
        return Err(RoomDatError::NotSixBit { at, value });
    }

    Ok(RoomData {
        mask_name,
        palette_override: window.to_vec(),
        entry_points: read_entry_points(data, nul + 1 + start + OVERRIDE_BYTES),
    })
}

/// Read the four entry positions from the tail of the file.
///
/// `after_palette` is where the palette window ends; the entry points
/// sit past the script records that follow it. Bounding the read that
/// way matters: taking the last twelve bytes unconditionally would, on a
/// short file, read back into the palette and place the actor at a
/// position made of colour components.
fn read_entry_points(data: &[u8], after_palette: usize) -> Vec<Vec2> {
    let Some(at) = data.len().checked_sub(ENTRY_BYTES) else {
        return Vec::new();
    };
    if at < after_palette {
        return Vec::new();
    }
    let tail = &data[at..];
    (0..ENTRY_POINTS)
        .map(|i| {
            let x = u16::from_le_bytes([tail[i * 3], tail[i * 3 + 1]]);
            Vec2::new(x as f32, tail[i * 3 + 2] as f32)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `.DAT` the way the retail files are laid out.
    fn build(mask: &str, header: &[u8], palette: &[u8], tail: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(mask.as_bytes());
        v.push(0);
        v.extend_from_slice(header);
        v.extend_from_slice(&[MARKER_HEAD, 0x40, MARKER_TAIL]);
        v.extend_from_slice(palette);
        v.extend_from_slice(tail);
        v
    }

    fn ramp() -> Vec<u8> {
        // 28 triples of legal 6-bit values.
        (0..OVERRIDE_BYTES).map(|i| (i % 64) as u8).collect()
    }

    #[test]
    fn parses_mask_name_and_palette() {
        let data = build("BRINS.MSK", &[0, 0, 0x3F, 0x3F], &ramp(), &[0x02, 0x00, 0x81, 0xFF]);
        let room = parse(&data).expect("parse");
        assert_eq!(room.mask_name, "BRINS.MSK");
        assert_eq!(room.palette_override.len(), OVERRIDE_BYTES);
        assert_eq!(room.palette_override, ramp());
    }

    #[test]
    fn header_bytes_above_the_six_bit_range_do_not_confuse_the_scan() {
        // Real headers carry values like 0x4A and 0x74 before the marker.
        let data = build("ARCH.MSK", &[0x00, 0x4A, 0x4A, 0x65, 0x74], &ramp(), &[]);
        let room = parse(&data).expect("parse");
        assert_eq!(room.mask_name, "ARCH.MSK");
    }

    #[test]
    fn a_lone_marker_head_without_its_tail_is_skipped() {
        // 0x88 appears in the header without a 0x87 two bytes later; the
        // scan must keep looking rather than lock onto it.
        let data = build("X.MSK", &[0x88, 0x01, 0x02], &ramp(), &[]);
        let room = parse(&data).expect("parse");
        assert_eq!(room.palette_override, ramp());
    }

    #[test]
    fn missing_mask_name_is_rejected() {
        assert_eq!(parse(&[]), Err(RoomDatError::NoMaskName));
        assert_eq!(parse(&[0, 1, 2]), Err(RoomDatError::NoMaskName));
        assert_eq!(parse(b"NOTERMINATOR"), Err(RoomDatError::NoMaskName));
    }

    #[test]
    fn missing_marker_is_rejected() {
        let mut data = b"X.MSK\0".to_vec();
        data.extend_from_slice(&[1, 2, 3, 4, 5]);
        assert_eq!(parse(&data), Err(RoomDatError::NoPaletteMarker));
    }

    #[test]
    fn truncated_palette_window_is_rejected() {
        let data = build("X.MSK", &[], &[0u8; 10], &[]);
        assert_eq!(
            parse(&data),
            Err(RoomDatError::TruncatedPalette { available: 10 })
        );
    }

    #[test]
    fn a_coincidental_marker_is_caught_by_the_six_bit_check() {
        // Enough bytes follow, but they are not palette data.
        let data = build("X.MSK", &[], &[0xFFu8; OVERRIDE_BYTES], &[]);
        assert_eq!(
            parse(&data),
            Err(RoomDatError::NotSixBit {
                at: 0,
                value: 0xFF
            })
        );
    }

    /// Twelve bytes reading as four (u16 x, u8 y) pairs.
    fn entry_tail(points: &[(u16, u8)]) -> Vec<u8> {
        let mut v = Vec::new();
        for (x, y) in points {
            v.extend_from_slice(&x.to_le_bytes());
            v.push(*y);
        }
        v
    }

    #[test]
    fn the_tail_yields_four_entry_positions() {
        let tail = entry_tail(&[(296, 106), (296, 106), (40, 120), (12, 130)]);
        let data = build("BRINS.MSK", &[], &ramp(), &tail);
        let room = parse(&data).expect("parse");
        assert_eq!(room.entry_points.len(), 4);
        assert_eq!(room.entry_points[0], glam::Vec2::new(296.0, 106.0));
        assert_eq!(room.entry_points[3], glam::Vec2::new(12.0, 130.0));
    }

    #[test]
    fn a_room_with_one_way_in_repeats_the_same_position() {
        let tail = entry_tail(&[(24, 108); 4]);
        let data = build("ALCHEMY.MSK", &[], &ramp(), &tail);
        let room = parse(&data).expect("parse");
        assert!(room.entry_points.iter().all(|p| *p == room.entry_points[0]));
    }

    #[test]
    fn a_file_too_short_for_the_tail_yields_no_entry_points() {
        // Reading them anyway would take bytes out of the palette and
        // place the actor at a position made of colour components.
        let data = build("X.MSK", &[], &ramp(), &[1, 2, 3]);
        let room = parse(&data).expect("parse");
        assert!(room.entry_points.is_empty());
    }

    #[test]
    fn applying_the_override_changes_only_its_own_window() {
        let base = Palette::default();
        let data = build("X.MSK", &[], &ramp(), &[]);
        let room = parse(&data).expect("parse");
        let result = room.apply(&base);

        // Below the window: untouched grey ramp.
        assert_eq!(result.color(0), base.color(0));
        assert_eq!(result.color(OVERRIDE_START - 1), base.color(OVERRIDE_START - 1));
        // Inside: the room's colours, widened from 6 bits.
        assert_ne!(result.color(OVERRIDE_START), base.color(OVERRIDE_START));
        // Above: the window is 28 colours, so 228 + 28 = 256 exactly.
        assert_eq!(OVERRIDE_START as usize + OVERRIDE_COLORS, 256);
    }
}
