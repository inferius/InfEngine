//! A9.2: the Kyrandia `.WSA` animation.
//!
//! A WSA is a small looping animation - a fluttering banner, a character
//! gesture, the Westwood logo. Frame 0 is stored whole; every later
//! frame is a [`super::format40`] XOR delta from the one before it. Both
//! layers are wrapped in Format80, so each frame is decompressed and
//! then applied on top of the running frame buffer.
//!
//! ```text
//!   u16 frame count
//!   u16 width, u16 height
//!   u16 delta buffer size
//!   u16 flags            bit 0 set means a palette follows the table
//!   u32 offsets[count + 2]
//!   [768-byte VGA palette]
//!   frame data
//! ```
//!
//! What the offsets are relative to is not obvious, and guessing wrong
//! sends every frame into the middle of another one. In the retail data
//! they are absolute file offsets - the first entry equals the end of
//! the header - but other Westwood titles store them relative to the
//! frame data. It does not have to be guessed either way: whatever the
//! convention, the first entry points at the first frame, which is
//! exactly where the header ends. So the base is the header's end minus
//! the first offset, which reads zero for absolute tables and the header
//! size for relative ones.

use std::fmt;

use super::format40::{self, Format40Error};
use super::format80::{self, Format80Error};
use super::palette::{Palette, VGA_PALETTE_BYTES};

/// Bytes before the offset table.
const HEADER_LEN: usize = 10;
/// Flag bit meaning a palette follows the offset table.
const FLAG_HAS_PALETTE: u16 = 1;
/// Frames beyond this mean the header is not a WSA.
const MAX_FRAMES: usize = 4096;
/// Largest edge worth believing.
const MAX_EDGE: u16 = 2048;

#[derive(Debug, Clone, PartialEq)]
pub enum WsaError {
    TooSmall { len: usize },
    /// Header values outside anything a real animation has.
    ImplausibleHeader { frames: usize, width: u16, height: u16 },
    /// The offset table runs past the end of the file.
    TruncatedTable,
    /// A frame's byte range is not inside the file.
    BadFrameRange { frame: usize },
    Decompress { frame: usize, source: Format80Error },
    Delta { frame: usize, source: Format40Error },
}

impl fmt::Display for WsaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooSmall { len } => write!(f, "WSA is only {len} bytes"),
            Self::ImplausibleHeader {
                frames,
                width,
                height,
            } => write!(f, "WSA claims {frames} frames of {width}x{height}"),
            Self::TruncatedTable => write!(f, "WSA offset table runs past the file"),
            Self::BadFrameRange { frame } => write!(f, "WSA frame {frame} lies outside the file"),
            Self::Decompress { frame, source } => {
                write!(f, "WSA frame {frame} did not decompress: {source}")
            }
            Self::Delta { frame, source } => {
                write!(f, "WSA frame {frame} delta failed: {source}")
            }
        }
    }
}

impl std::error::Error for WsaError {}

/// A decoded animation: every frame as 8-bit palette indices.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub width: u32,
    pub height: u32,
    /// One buffer of `width * height` indices per frame.
    pub frames: Vec<Vec<u8>>,
    /// Palette carried by the file, if any.
    pub palette: Option<Palette>,
}

impl Animation {
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Lay the frames out left to right as one sheet, and return it as
    /// RGBA8 with its dimensions.
    ///
    /// A sheet is what an editor and a sprite renderer both want; the
    /// individual frames stay available for anything that needs them
    /// separately.
    pub fn to_sheet_rgba(&self, fallback: Option<&Palette>) -> (u32, u32, Vec<u8>) {
        let owned;
        let palette = match (self.palette.as_ref(), fallback) {
            (Some(p), _) => p,
            (None, Some(p)) => p,
            (None, None) => {
                owned = Palette::default();
                &owned
            }
        };

        let count = self.frames.len().max(1) as u32;
        let sheet_w = self.width * count;
        let mut rgba = vec![0u8; sheet_w as usize * self.height as usize * 4];
        for (index, frame) in self.frames.iter().enumerate() {
            let expanded = palette.expand_rgba(frame, Some(0));
            let ox = index as u32 * self.width;
            for y in 0..self.height as usize {
                let src = y * self.width as usize * 4;
                let dst = (y * sheet_w as usize + ox as usize) * 4;
                let span = self.width as usize * 4;
                rgba[dst..dst + span].copy_from_slice(&expanded[src..src + span]);
            }
        }
        (sheet_w, self.height, rgba)
    }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Decode a `.WSA` file.
pub fn decode(data: &[u8]) -> Result<Animation, WsaError> {
    if data.len() < HEADER_LEN {
        return Err(WsaError::TooSmall { len: data.len() });
    }
    let frames = u16_at(data, 0).unwrap_or(0) as usize;
    let width = u16_at(data, 2).unwrap_or(0);
    let height = u16_at(data, 4).unwrap_or(0);
    let delta_buffer = u16_at(data, 6).unwrap_or(0) as usize;
    let flags = u16_at(data, 8).unwrap_or(0);

    if frames == 0
        || frames > MAX_FRAMES
        || width == 0
        || height == 0
        || width > MAX_EDGE
        || height > MAX_EDGE
    {
        return Err(WsaError::ImplausibleHeader {
            frames,
            width,
            height,
        });
    }

    // The table carries two extra entries past the frames: the end of
    // the last frame, and a trailing zero.
    let table_len = (frames + 2) * 4;
    let table_end = HEADER_LEN + table_len;
    if table_end > data.len() {
        return Err(WsaError::TruncatedTable);
    }

    let palette = if flags & FLAG_HAS_PALETTE != 0 {
        data.get(table_end..table_end + VGA_PALETTE_BYTES)
            .and_then(Palette::from_vga_6bit)
    } else {
        None
    };

    // Where the frame data starts, derived rather than assumed: the
    // first table entry points at the first frame, and the first frame
    // begins right after the header, so the difference is the base.
    let laid_out = table_end + if palette.is_some() { VGA_PALETTE_BYTES } else { 0 };
    let first = u32_at(data, HEADER_LEN).unwrap_or(0) as usize;
    let base = laid_out.saturating_sub(first);

    let pixels = width as usize * height as usize;
    let mut buffer = vec![0u8; pixels];
    let mut out = Vec::with_capacity(frames);

    for frame in 0..frames {
        let start = u32_at(data, HEADER_LEN + frame * 4).unwrap_or(0) as usize;
        let end = u32_at(data, HEADER_LEN + (frame + 1) * 4).unwrap_or(0) as usize;
        // An empty range is a frame the file does not store: the buffer
        // carries over, which is how a WSA holds a still. Frame 0
        // legitimately starts at offset 0, so emptiness has to be judged
        // by the range being empty, not by the start being zero.
        if end <= start {
            out.push(buffer.clone());
            continue;
        }
        let (from, to) = (base + start, base + end);
        let Some(chunk) = data.get(from..to.min(data.len())) else {
            return Err(WsaError::BadFrameRange { frame });
        };

        // A frame's decompressed size is not in the stream, so the
        // header's delta buffer size bounds it - that field exists for
        // exactly this. It is only an allocation guard; a small frame's
        // command stream can be longer than the frame itself, so the
        // fallback is deliberately generous rather than tight.
        let limit = if delta_buffer > 0 {
            delta_buffer.max(pixels)
        } else {
            pixels * 2 + 256
        };
        let delta = format80::decompress_up_to(chunk, limit)
            .map_err(|source| WsaError::Decompress { frame, source })?;
        format40::apply(&delta, &mut buffer)
            .map_err(|source| WsaError::Delta { frame, source })?;
        out.push(buffer.clone());
    }

    Ok(Animation {
        width: width as u32,
        height: height as u32,
        frames: out,
        palette,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a WSA whose frames are given as raw Format40 deltas, each
    /// wrapped in a Format80 literal run.
    fn build(width: u16, height: u16, deltas: &[Vec<u8>], palette: Option<&[u8]>) -> Vec<u8> {
        let mut bodies = Vec::new();
        let mut offsets = Vec::new();
        let mut cursor = 0u32;
        for delta in deltas {
            offsets.push(cursor);
            // Format80: a literal run, then the end marker.
            let mut wrapped = Vec::new();
            for chunk in delta.chunks(0x3F) {
                wrapped.push(0x80 | chunk.len() as u8);
                wrapped.extend_from_slice(chunk);
            }
            wrapped.push(0x80);
            cursor += wrapped.len() as u32;
            bodies.push(wrapped);
        }
        offsets.push(cursor);
        offsets.push(0);

        let mut file = Vec::new();
        file.extend_from_slice(&(deltas.len() as u16).to_le_bytes());
        file.extend_from_slice(&width.to_le_bytes());
        file.extend_from_slice(&height.to_le_bytes());
        file.extend_from_slice(&0u16.to_le_bytes()); // delta buffer size
        file.extend_from_slice(&(if palette.is_some() { 1u16 } else { 0 }).to_le_bytes());
        for offset in &offsets {
            file.extend_from_slice(&offset.to_le_bytes());
        }
        if let Some(p) = palette {
            file.extend_from_slice(p);
        }
        for body in bodies {
            file.extend_from_slice(&body);
        }
        file
    }

    /// A delta that XORs `values` over the start of the frame.
    fn xor_run(values: &[u8]) -> Vec<u8> {
        let mut d = vec![values.len() as u8];
        d.extend_from_slice(values);
        d.extend_from_slice(&[0x80, 0x00, 0x00]);
        d
    }

    #[test]
    fn a_single_frame_animation_decodes() {
        let file = build(2, 2, &[xor_run(&[1, 2, 3, 4])], None);
        let anim = decode(&file).expect("decode");
        assert_eq!((anim.width, anim.height), (2, 2));
        assert_eq!(anim.frame_count(), 1);
        assert_eq!(anim.frames[0], vec![1, 2, 3, 4]);
    }

    #[test]
    fn later_frames_accumulate_on_top_of_earlier_ones() {
        // This is the whole point of a delta format: frame 1 is only
        // meaningful applied to frame 0.
        let file = build(2, 2, &[xor_run(&[1, 2, 3, 4]), xor_run(&[1, 0, 0, 0])], None);
        let anim = decode(&file).expect("decode");
        assert_eq!(anim.frame_count(), 2);
        assert_eq!(anim.frames[0], vec![1, 2, 3, 4]);
        assert_eq!(anim.frames[1], vec![0, 2, 3, 4], "frame 1 builds on frame 0");
    }

    #[test]
    fn an_empty_frame_range_repeats_the_previous_frame() {
        let mut file = build(2, 2, &[xor_run(&[5, 6, 7, 8]), xor_run(&[1, 1, 1, 1])], None);
        // Collapse frame 1 to a zero-length range by pointing its start
        // at its own end.
        let start_at = HEADER_LEN + 4;
        let end = u32_at(&file, HEADER_LEN + 8).expect("table entry");
        file[start_at..start_at + 4].copy_from_slice(&end.to_le_bytes());
        let anim = decode(&file).expect("decode");
        assert_eq!(anim.frames[1], anim.frames[0]);
    }

    #[test]
    fn an_absolute_offset_table_decodes_the_same_as_a_relative_one() {
        // Retail Kyrandia stores absolute file offsets; the builder here
        // writes relative ones. Both must give the same frames.
        let relative = build(2, 2, &[xor_run(&[1, 2, 3, 4])], None);
        let mut absolute = relative.clone();
        let table_end = HEADER_LEN + (1 + 2) * 4;
        for i in 0..3 {
            let at = HEADER_LEN + i * 4;
            let value = u32_at(&absolute, at).expect("entry");
            let shifted = if i == 2 { value } else { value + table_end as u32 };
            absolute[at..at + 4].copy_from_slice(&shifted.to_le_bytes());
        }
        assert_eq!(
            decode(&relative).expect("relative").frames,
            decode(&absolute).expect("absolute").frames
        );
    }

    #[test]
    fn frame_zero_starting_at_offset_zero_is_still_decoded() {
        // Every WSA has frame 0 at offset 0; reading that as "no frame"
        // makes the whole animation start blank.
        let file = build(2, 1, &[xor_run(&[9, 9])], None);
        let anim = decode(&file).expect("decode");
        assert_eq!(anim.frames[0], vec![9, 9]);
    }

    #[test]
    fn an_embedded_palette_is_read_and_skipped() {
        // If the palette were not accounted for, every frame offset
        // would land 768 bytes early and decode garbage.
        let mut pal = vec![0u8; VGA_PALETTE_BYTES];
        pal[3] = 63; // colour 1 = red
        let file = build(2, 2, &[xor_run(&[1, 1, 1, 1])], Some(&pal));
        let anim = decode(&file).expect("decode");
        assert_eq!(anim.frames[0], vec![1, 1, 1, 1]);
        assert_eq!(
            anim.palette.as_ref().expect("palette").color(1),
            [255, 0, 0]
        );
    }

    #[test]
    fn a_sheet_lays_frames_out_left_to_right() {
        let file = build(2, 2, &[xor_run(&[1, 2, 3, 4]), xor_run(&[1, 0, 0, 0])], None);
        let anim = decode(&file).expect("decode");
        let (w, h, rgba) = anim.to_sheet_rgba(None);
        assert_eq!((w, h), (4, 2), "two frames of two pixels wide");
        assert_eq!(rgba.len(), 4 * 2 * 4);
        // Frame 0 pixel 0 is index 1, frame 1 pixel 0 is index 0.
        assert_eq!(rgba[3], 255, "index 1 is opaque");
        assert_eq!(rgba[2 * 4 + 3], 0, "index 0 is the transparent key");
    }

    #[test]
    fn index_zero_is_treated_as_transparent() {
        // Sprites key their background on colour zero; drawing it opaque
        // would put a black box behind every animation.
        let file = build(2, 1, &[xor_run(&[0, 5])], None);
        let anim = decode(&file).expect("decode");
        let (_, _, rgba) = anim.to_sheet_rgba(None);
        assert_eq!(rgba[3], 0);
        assert_eq!(rgba[7], 255);
    }

    #[test]
    fn a_short_file_is_rejected() {
        assert!(matches!(decode(&[0, 0]), Err(WsaError::TooSmall { .. })));
    }

    #[test]
    fn an_implausible_header_is_rejected_before_allocating() {
        let mut file = build(2, 2, &[xor_run(&[1])], None);
        file[2..4].copy_from_slice(&9999u16.to_le_bytes());
        assert!(matches!(
            decode(&file),
            Err(WsaError::ImplausibleHeader { .. })
        ));

        let mut file = build(2, 2, &[xor_run(&[1])], None);
        file[0..2].copy_from_slice(&0u16.to_le_bytes());
        assert!(matches!(
            decode(&file),
            Err(WsaError::ImplausibleHeader { .. })
        ));
    }

    #[test]
    fn a_truncated_offset_table_is_rejected() {
        let mut file = build(2, 2, &[xor_run(&[1])], None);
        file.truncate(HEADER_LEN + 4);
        assert_eq!(decode(&file), Err(WsaError::TruncatedTable));
    }
}
