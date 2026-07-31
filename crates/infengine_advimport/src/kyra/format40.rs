//! A9.1: Westwood's Format40, the XOR delta between animation frames.
//!
//! A `.WSA` stores frame 0 whole and every later frame as the difference
//! from the one before it, expressed as XOR operations over the frame
//! buffer. That is why an animation cannot be decoded frame-at-random:
//! each frame is only meaningful applied on top of its predecessor.
//!
//! The command set is small but has an awkward corner. A lead byte of
//! `0x80` escapes to a 16-bit count whose top two bits pick the
//! operation, so the same value means "skip", "XOR from the stream" or
//! "XOR with one repeated byte" depending on bits that are not part of
//! the count. Masking those off before using the number is the whole
//! trick.

use std::fmt;

/// A malformed delta stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format40Error {
    /// The stream ended in the middle of a command.
    UnexpectedEnd { at: usize },
    /// A command would write past the end of the frame.
    Overrun { at: usize, position: usize },
}

impl fmt::Display for Format40Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { at } => {
                write!(f, "format40 stream ended mid-command at byte {at}")
            }
            Self::Overrun { at, position } => write!(
                f,
                "format40 command at byte {at} writes past the frame at {position}"
            ),
        }
    }
}

impl std::error::Error for Format40Error {}

/// Apply a Format40 delta to `frame` in place.
///
/// `frame` holds the previous image and receives the next one. Commands
/// address it as one flat run of pixels, so the caller supplies a buffer
/// of exactly width times height bytes.
pub fn apply(delta: &[u8], frame: &mut [u8]) -> Result<(), Format40Error> {
    let mut src = 0usize;
    let mut dst = 0usize;

    macro_rules! byte {
        ($at:expr) => {
            match delta.get(src) {
                Some(b) => {
                    src += 1;
                    *b
                }
                None => return Err(Format40Error::UnexpectedEnd { at: $at }),
            }
        };
    }

    while src < delta.len() {
        let at = src;
        let cmd = byte!(at);

        if cmd == 0 {
            // 0x00 count value: XOR a run with one repeated byte.
            let count = byte!(at) as usize;
            let value = byte!(at);
            xor_fill(frame, &mut dst, count, value, at)?;
        } else if cmd & 0x80 == 0 {
            // 0x01..0x7F: XOR that many bytes straight from the stream.
            let count = cmd as usize;
            xor_from_stream(frame, &mut dst, delta, &mut src, count, at)?;
        } else if cmd == 0x80 {
            // Escape to a 16-bit operand.
            let lo = byte!(at) as u16;
            let hi = byte!(at) as u16;
            let word = lo | (hi << 8);
            if word == 0 {
                break;
            }
            if word & 0x8000 == 0 {
                // Skip: leave those pixels as they were.
                dst = dst.saturating_add(word as usize);
            } else if word & 0x4000 == 0 {
                let count = (word & 0x3FFF) as usize;
                xor_from_stream(frame, &mut dst, delta, &mut src, count, at)?;
            } else {
                let count = (word & 0x3FFF) as usize;
                let value = byte!(at);
                xor_fill(frame, &mut dst, count, value, at)?;
            }
        } else {
            // 0x81..0xFF: skip the low seven bits' worth of pixels.
            dst = dst.saturating_add((cmd & 0x7F) as usize);
        }

        if dst > frame.len() {
            return Err(Format40Error::Overrun { at, position: dst });
        }
    }
    Ok(())
}

fn xor_fill(
    frame: &mut [u8],
    dst: &mut usize,
    count: usize,
    value: u8,
    at: usize,
) -> Result<(), Format40Error> {
    let end = dst.checked_add(count).ok_or(Format40Error::Overrun {
        at,
        position: usize::MAX,
    })?;
    if end > frame.len() {
        return Err(Format40Error::Overrun { at, position: end });
    }
    for byte in &mut frame[*dst..end] {
        *byte ^= value;
    }
    *dst = end;
    Ok(())
}

fn xor_from_stream(
    frame: &mut [u8],
    dst: &mut usize,
    delta: &[u8],
    src: &mut usize,
    count: usize,
    at: usize,
) -> Result<(), Format40Error> {
    let end = dst.checked_add(count).ok_or(Format40Error::Overrun {
        at,
        position: usize::MAX,
    })?;
    if end > frame.len() {
        return Err(Format40Error::Overrun { at, position: end });
    }
    let source = delta
        .get(*src..*src + count)
        .ok_or(Format40Error::UnexpectedEnd { at })?;
    for (slot, value) in frame[*dst..end].iter_mut().zip(source) {
        *slot ^= *value;
    }
    *src += count;
    *dst = end;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// End-of-stream marker: the 0x80 escape with a zero operand.
    const END: [u8; 3] = [0x80, 0x00, 0x00];

    #[test]
    fn a_short_run_xors_from_the_stream() {
        let mut frame = vec![0x00, 0x00, 0x00, 0x00];
        let mut delta = vec![0x03, 0xAA, 0xBB, 0xCC];
        delta.extend_from_slice(&END);
        apply(&delta, &mut frame).expect("apply");
        assert_eq!(frame, vec![0xAA, 0xBB, 0xCC, 0x00]);
    }

    #[test]
    fn xoring_twice_returns_the_original() {
        // The whole format rests on this: a delta is its own inverse.
        let original = vec![0x11, 0x22, 0x33, 0x44];
        let mut frame = original.clone();
        let mut delta = vec![0x04, 0xF0, 0x0F, 0xFF, 0x01];
        delta.extend_from_slice(&END);
        apply(&delta, &mut frame).expect("first");
        assert_ne!(frame, original);
        apply(&delta, &mut frame).expect("second");
        assert_eq!(frame, original);
    }

    #[test]
    fn a_fill_run_xors_one_value_repeatedly() {
        let mut frame = vec![0x00; 5];
        let mut delta = vec![0x00, 0x04, 0x7F];
        delta.extend_from_slice(&END);
        apply(&delta, &mut frame).expect("apply");
        assert_eq!(frame, vec![0x7F, 0x7F, 0x7F, 0x7F, 0x00]);
    }

    #[test]
    fn a_short_skip_leaves_pixels_untouched() {
        let mut frame = vec![0x11, 0x22, 0x33, 0x44];
        // 0x82 skips two, then XOR one byte.
        let mut delta = vec![0x82, 0x01, 0xFF];
        delta.extend_from_slice(&END);
        apply(&delta, &mut frame).expect("apply");
        assert_eq!(frame, vec![0x11, 0x22, 0x33 ^ 0xFF, 0x44]);
    }

    #[test]
    fn the_escape_can_skip_a_long_run() {
        let mut frame = vec![0x01; 300];
        // Skip 256 pixels, then XOR one.
        let mut delta = vec![0x80, 0x00, 0x01, 0x01, 0xFF];
        delta.extend_from_slice(&END);
        apply(&delta, &mut frame).expect("apply");
        assert_eq!(frame[255], 0x01, "still untouched");
        assert_eq!(frame[256], 0x01 ^ 0xFF);
    }

    #[test]
    fn the_escape_can_xor_a_long_run_from_the_stream() {
        let mut frame = vec![0x00; 4];
        // 0x8000 | 0x0003: three bytes from the stream.
        let mut delta = vec![0x80, 0x03, 0x80, 0x0A, 0x0B, 0x0C];
        delta.extend_from_slice(&END);
        apply(&delta, &mut frame).expect("apply");
        assert_eq!(frame, vec![0x0A, 0x0B, 0x0C, 0x00]);
    }

    #[test]
    fn the_escape_can_fill_a_long_run() {
        let mut frame = vec![0x00; 5];
        // 0xC000 | 0x0004: four bytes of one value.
        let mut delta = vec![0x80, 0x04, 0xC0, 0x3C];
        delta.extend_from_slice(&END);
        apply(&delta, &mut frame).expect("apply");
        assert_eq!(frame, vec![0x3C, 0x3C, 0x3C, 0x3C, 0x00]);
    }

    #[test]
    fn the_operation_bits_are_masked_out_of_the_count() {
        // 0xC004 must mean four pixels, not 49156 of them. Forgetting
        // the mask turns a small fill into an instant overrun.
        let mut frame = vec![0u8; 8];
        let delta = vec![0x80, 0x04, 0xC0, 0x01, 0x80, 0x00, 0x00];
        apply(&delta, &mut frame).expect("apply");
        assert_eq!(&frame[..4], &[1, 1, 1, 1]);
        assert_eq!(&frame[4..], &[0, 0, 0, 0]);
    }

    #[test]
    fn a_zero_operand_ends_the_stream() {
        let mut frame = vec![0u8; 4];
        // The trailing bytes must be ignored once the marker is seen.
        let delta = vec![0x80, 0x00, 0x00, 0x01, 0xFF];
        apply(&delta, &mut frame).expect("apply");
        assert_eq!(frame, vec![0, 0, 0, 0]);
    }

    #[test]
    fn writing_past_the_frame_is_rejected() {
        let mut frame = vec![0u8; 2];
        let delta = vec![0x00, 0x10, 0xFF];
        assert!(matches!(
            apply(&delta, &mut frame),
            Err(Format40Error::Overrun { .. })
        ));
    }

    #[test]
    fn a_truncated_command_is_rejected() {
        let mut frame = vec![0u8; 8];
        assert!(matches!(
            apply(&[0x04, 0xAA], &mut frame),
            Err(Format40Error::UnexpectedEnd { .. })
        ));
        assert!(matches!(
            apply(&[0x00], &mut frame),
            Err(Format40Error::UnexpectedEnd { .. })
        ));
    }

    #[test]
    fn an_empty_delta_leaves_the_frame_alone() {
        let mut frame = vec![1, 2, 3];
        apply(&[], &mut frame).expect("apply");
        assert_eq!(frame, vec![1, 2, 3]);
    }
}
