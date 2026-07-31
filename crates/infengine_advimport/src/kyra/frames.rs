//! A13.2: the sprite frames a Kyrandia room keeps in its own background.
//!
//! A background decodes to 320x200 but the room is only the top of it.
//! The rows below hold the room's animation frames - a river's ripples, a
//! guttering torch - and the room `.DAT` says where each one sits.
//!
//! The frame table is the block between the palette override and the
//! entry points, and it is exactly the length the `u16` in front of it
//! claims. That checks out on all 86 rooms of a retail install, which is
//! what makes the block's bounds trustworthy enough to read records from.
//!
//! Each record is five little-endian `u16`s:
//!
//! ```text
//!   id            1, 2, 3, ... in order
//!   x             in units of 8 pixels
//!   y             in pixels
//!   width         in units of 8 pixels
//!   height        in pixels
//! ```
//!
//! Records are read while they stay plausible and the reader stops at
//! the first one that is not. The block also holds records of other
//! kinds that are not decoded yet, and guessing at those would produce
//! frames cut from the middle of the scene.

use std::fmt;

/// Bytes each record occupies.
const RECORD_LEN: usize = 10;
/// Horizontal positions and widths are stored in units of this many
/// pixels.
const X_UNIT: u16 = 8;
/// Entry points occupy the last twelve bytes of the file.
const ENTRY_BYTES: usize = 12;
/// A frame taller or wider than this is not a frame.
const MAX_EDGE: u16 = 320;

/// One stored frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub id: u16,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Frame {
    /// Does the frame fit inside an image of this size?
    pub fn fits(&self, image_width: u16, image_height: u16) -> bool {
        self.width > 0
            && self.height > 0
            && self.x.saturating_add(self.width) <= image_width
            && self.y.saturating_add(self.height) <= image_height
    }
}

impl fmt::Display for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "frame {} at {},{} {}x{}",
            self.id, self.x, self.y, self.width, self.height
        )
    }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

/// The frame table's bytes: everything between `after_palette` and the
/// entry points, bounded by the length stored in front of it.
///
/// Returns `None` when the stored length does not agree with the file,
/// since a block whose bounds are wrong is not one to read records from.
pub fn table_bytes(data: &[u8], after_palette: usize) -> Option<&[u8]> {
    let size = u16_at(data, after_palette)? as usize;
    let start = after_palette + 2;
    let end = data.len().checked_sub(ENTRY_BYTES)?;
    if end < start || end - start != size {
        return None;
    }
    data.get(start..end)
}

/// Read the frames a room stores, stopping at the first record that is
/// not one.
///
/// `image_width` and `image_height` bound the rectangles; a record
/// describing something outside the picture is where the frame list
/// ends and another kind of record begins.
pub fn read(table: &[u8], image_width: u16, image_height: u16) -> Vec<Frame> {
    // Rooms with frames open the block with this pair; rooms without
    // frames open it with a different one, and reading past that would
    // invent frames out of unrelated records.
    if table.len() < 4 || table[0] != 0x82 || table[1] != 0xFF {
        return Vec::new();
    }

    let mut out: Vec<Frame> = Vec::new();
    let mut pos = 4usize;
    while pos + RECORD_LEN <= table.len() {
        let (Some(id), Some(x), Some(y), Some(w), Some(h)) = (
            u16_at(table, pos),
            u16_at(table, pos + 2),
            u16_at(table, pos + 4),
            u16_at(table, pos + 6),
            u16_at(table, pos + 8),
        ) else {
            break;
        };
        // Ids run 1, 2, 3 with no gaps. A break in the run means this
        // record belongs to something else.
        if id as usize != out.len() + 1 {
            break;
        }
        if w > MAX_EDGE / X_UNIT || h == 0 || h > MAX_EDGE {
            break;
        }
        let frame = Frame {
            id,
            x: x.saturating_mul(X_UNIT),
            y,
            width: w.saturating_mul(X_UNIT),
            height: h,
        };
        if !frame.fits(image_width, image_height) {
            break;
        }
        out.push(frame);
        pos += RECORD_LEN;
    }
    out
}

/// The row where frame storage begins, and so where the visible room
/// ends.
///
/// A room with no frames has nothing to say about it, and the caller
/// falls back to the play area's height.
pub fn storage_top(frames: &[Frame]) -> Option<u16> {
    frames.iter().map(|f| f.y).min()
}

/// Group frames into runs that share a size.
///
/// A room usually stores more than one animation - the bridge keeps
/// three frames of river beside three of something else - and the only
/// thing separating them in the table is that the rectangles change
/// shape. Grouping on that recovers the animations instead of leaving
/// one undifferentiated pile.
pub fn group_by_size(frames: &[Frame]) -> Vec<Vec<Frame>> {
    let mut groups: Vec<Vec<Frame>> = Vec::new();
    for frame in frames {
        let same = groups
            .last()
            .and_then(|g| g.last())
            .is_some_and(|prev| prev.width == frame.width && prev.height == frame.height);
        if !same {
            groups.push(Vec::new());
        }
        if let Some(group) = groups.last_mut() {
            group.push(*frame);
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a frame table: the opening pair, then records.
    fn table(records: &[(u16, u16, u16, u16, u16)]) -> Vec<u8> {
        let mut v = vec![0x82, 0xFF, 0x84, 0xFF];
        for (id, x, y, w, h) in records {
            for value in [id, x, y, w, h] {
                v.extend_from_slice(&value.to_le_bytes());
            }
        }
        v
    }

    /// The bridge room's real table: three river frames and three of
    /// something else.
    fn bridge() -> Vec<u8> {
        table(&[
            (1, 1, 136, 7, 32),
            (2, 8, 136, 7, 32),
            (3, 15, 136, 7, 32),
            (4, 1, 168, 5, 16),
            (5, 7, 168, 5, 16),
            (6, 13, 168, 5, 16),
        ])
    }

    #[test]
    fn records_decode_into_pixel_rectangles() {
        let frames = read(&bridge(), 320, 200);
        assert_eq!(frames.len(), 6);
        assert_eq!(
            frames[0],
            Frame {
                id: 1,
                x: 8,
                y: 136,
                width: 56,
                height: 32
            }
        );
        assert_eq!(frames[3].y, 168);
        assert_eq!(frames[3].width, 40);
    }

    #[test]
    fn frames_group_into_the_animations_they_belong_to() {
        // Six frames, two animations - the whole reason for grouping.
        let groups = group_by_size(&read(&bridge(), 320, 200));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 3);
        assert_eq!(groups[1].len(), 3);
        assert_eq!(groups[0][0].height, 32);
        assert_eq!(groups[1][0].height, 16);
    }

    #[test]
    fn storage_top_is_where_the_visible_room_ends() {
        assert_eq!(storage_top(&read(&bridge(), 320, 200)), Some(136));
        assert_eq!(storage_top(&[]), None);
    }

    #[test]
    fn a_table_that_does_not_open_with_the_frame_pair_yields_nothing() {
        // Rooms without frames open the block differently; reading on
        // would invent frames out of unrelated records.
        let mut t = bridge();
        t[0] = 0x81;
        assert!(read(&t, 320, 200).is_empty());
        assert!(read(&[], 320, 200).is_empty());
    }

    #[test]
    fn reading_stops_where_the_ids_stop_counting() {
        // The block holds other kinds of record after the frames.
        let t = table(&[(1, 1, 136, 7, 32), (2, 8, 136, 7, 32), (9, 0, 0, 1, 1)]);
        assert_eq!(read(&t, 320, 200).len(), 2);
    }

    #[test]
    fn a_rectangle_outside_the_picture_ends_the_list() {
        let t = table(&[(1, 1, 136, 7, 32), (2, 60, 136, 7, 32)]);
        // 60 * 8 + 56 is past 320.
        assert_eq!(read(&t, 320, 200).len(), 1);
    }

    #[test]
    fn a_zero_sized_record_ends_the_list() {
        let t = table(&[(1, 1, 136, 7, 32), (2, 8, 136, 0, 0)]);
        assert_eq!(read(&t, 320, 200).len(), 1);
    }

    #[test]
    fn fits_checks_both_axes() {
        let f = Frame {
            id: 1,
            x: 300,
            y: 190,
            width: 32,
            height: 32,
        };
        assert!(!f.fits(320, 200));
        assert!(f.fits(400, 300));
    }

    #[test]
    fn the_table_block_is_bounded_by_its_own_length() {
        // after_palette, then u16 length, then that many bytes, then
        // twelve bytes of entry points.
        let mut data = vec![0u8; 4];
        let body = bridge();
        data.extend_from_slice(&(body.len() as u16).to_le_bytes());
        data.extend_from_slice(&body);
        data.extend_from_slice(&[0u8; ENTRY_BYTES]);
        let table = table_bytes(&data, 4).expect("bounds agree");
        assert_eq!(table, &body[..]);
        assert_eq!(read(table, 320, 200).len(), 6);
    }

    #[test]
    fn a_length_that_disagrees_with_the_file_is_refused() {
        // Reading records from a block whose bounds are wrong would
        // slice them out of the middle of something else.
        let mut data = vec![0u8; 4];
        data.extend_from_slice(&99u16.to_le_bytes());
        data.extend_from_slice(&bridge());
        data.extend_from_slice(&[0u8; ENTRY_BYTES]);
        assert!(table_bytes(&data, 4).is_none());
    }

    #[test]
    fn a_file_too_short_for_entry_points_has_no_table() {
        assert!(table_bytes(&[0, 0, 0, 0], 0).is_none());
    }

    #[test]
    fn a_frame_prints_readably() {
        let f = Frame {
            id: 2,
            x: 64,
            y: 136,
            width: 56,
            height: 32,
        };
        assert_eq!(f.to_string(), "frame 2 at 64,136 56x32");
    }
}
