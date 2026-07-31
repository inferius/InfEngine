//! A3.1: Westwood's Format80 (also known as LCW) decompressor.
//!
//! Format80 is the LZ variant Westwood shipped in everything from Dune II
//! through the Kyrandia series. A stream is a sequence of commands, each
//! either copying literal bytes from the source or repeating bytes
//! already written to the destination.
//!
//! The subtlety that trips up naive implementations: back-references may
//! overlap the region being written. Copying `count` bytes from an offset
//! two bytes behind the write head is how the format encodes a repeating
//! two-byte pattern, so the copy must proceed one byte at a time and
//! observe its own output. `copy_from_slice` would produce the wrong
//! bytes.

use std::fmt;

/// A malformed or truncated Format80 stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Format80Error {
    /// The stream ended in the middle of a command.
    UnexpectedEnd { at: usize },
    /// A back-reference pointed before the start of the output.
    BadBackReference { at: usize, offset: usize },
    /// Output grew past the size the container promised.
    OutputOverflow { limit: usize },
    /// The stream ran out before producing the promised output.
    OutputTruncated { got: usize, expected: usize },
}

impl fmt::Display for Format80Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { at } => {
                write!(f, "format80 stream ended mid-command at byte {at}")
            }
            Self::BadBackReference { at, offset } => write!(
                f,
                "format80 back-reference at byte {at} points {offset} before the output start"
            ),
            Self::OutputOverflow { limit } => {
                write!(f, "format80 output grew past the declared {limit} bytes")
            }
            Self::OutputTruncated { got, expected } => {
                write!(f, "format80 produced {got} of {expected} declared bytes")
            }
        }
    }
}

impl std::error::Error for Format80Error {}

/// Read a little-endian u16 at `pos`, advancing it.
fn read_u16(src: &[u8], pos: &mut usize) -> Result<usize, Format80Error> {
    if *pos + 1 >= src.len() {
        return Err(Format80Error::UnexpectedEnd { at: *pos });
    }
    let v = u16::from_le_bytes([src[*pos], src[*pos + 1]]) as usize;
    *pos += 2;
    Ok(v)
}

/// Copy `count` bytes into `out`, starting from absolute index `from`.
///
/// Byte at a time and re-reading `out` as it grows, so an overlapping
/// range repeats the pattern rather than reading stale bytes.
fn copy_within(
    out: &mut Vec<u8>,
    from: usize,
    count: usize,
    limit: usize,
    at: usize,
) -> Result<(), Format80Error> {
    if from >= out.len() {
        return Err(Format80Error::BadBackReference {
            at,
            offset: from.saturating_sub(out.len()),
        });
    }
    if out.len() + count > limit {
        return Err(Format80Error::OutputOverflow { limit });
    }
    for i in 0..count {
        let byte = out[from + i];
        out.push(byte);
    }
    Ok(())
}

/// Decompress a Format80 stream into exactly `expected_size` bytes.
///
/// `expected_size` comes from the container header (a CPS image declares
/// its uncompressed size), and doubles as the guard against a corrupt
/// stream allocating without bound.
pub fn decompress(src: &[u8], expected_size: usize) -> Result<Vec<u8>, Format80Error> {
    let out = run(src, expected_size)?;
    if out.len() != expected_size {
        return Err(Format80Error::OutputTruncated {
            got: out.len(),
            expected: expected_size,
        });
    }
    Ok(out)
}

/// Decompress until the stream ends, producing at most `limit` bytes.
///
/// Some containers do not record the decompressed size - a WSA frame is
/// a delta whose length is whatever it turns out to be - so the caller
/// supplies only an upper bound and takes what comes out. A short result
/// is legitimate here rather than an error.
pub fn decompress_up_to(src: &[u8], limit: usize) -> Result<Vec<u8>, Format80Error> {
    run(src, limit)
}

/// The shared decoding loop. Stops at the end marker, at the end of the
/// input, or once `limit` bytes have been produced.
fn run(src: &[u8], expected_size: usize) -> Result<Vec<u8>, Format80Error> {
    let mut out: Vec<u8> = Vec::with_capacity(expected_size);
    let mut pos = 0usize;

    while pos < src.len() {
        let at = pos;
        let cmd = src[pos];
        pos += 1;

        if cmd == 0x80 {
            // Explicit end of stream.
            break;
        } else if cmd & 0x80 == 0 {
            // 0cccpppp pppppppp - short back-reference, offset relative
            // to the current write head.
            let count = ((cmd as usize & 0x70) >> 4) + 3;
            if pos >= src.len() {
                return Err(Format80Error::UnexpectedEnd { at });
            }
            let offset = ((cmd as usize & 0x0F) << 8) | src[pos] as usize;
            pos += 1;
            if offset > out.len() {
                return Err(Format80Error::BadBackReference { at, offset });
            }
            let from = out.len() - offset;
            copy_within(&mut out, from, count, expected_size, at)?;
        } else if cmd & 0x40 == 0 {
            // 10cccccc - copy `count` literal bytes from the source.
            let count = cmd as usize & 0x3F;
            if pos + count > src.len() {
                return Err(Format80Error::UnexpectedEnd { at });
            }
            if out.len() + count > expected_size {
                return Err(Format80Error::OutputOverflow {
                    limit: expected_size,
                });
            }
            out.extend_from_slice(&src[pos..pos + count]);
            pos += count;
        } else {
            // 11cccccc - long forms, distinguished by the low six bits.
            let selector = cmd & 0x3F;
            match selector {
                // 0xFE: run-length fill.
                0x3E => {
                    let count = read_u16(src, &mut pos)?;
                    if pos >= src.len() {
                        return Err(Format80Error::UnexpectedEnd { at });
                    }
                    let value = src[pos];
                    pos += 1;
                    if out.len() + count > expected_size {
                        return Err(Format80Error::OutputOverflow {
                            limit: expected_size,
                        });
                    }
                    out.resize(out.len() + count, value);
                }
                // 0xFF: long absolute back-reference.
                0x3F => {
                    let count = read_u16(src, &mut pos)?;
                    let from = read_u16(src, &mut pos)?;
                    copy_within(&mut out, from, count, expected_size, at)?;
                }
                // Medium absolute back-reference.
                _ => {
                    let count = selector as usize + 3;
                    let from = read_u16(src, &mut pos)?;
                    copy_within(&mut out, from, count, expected_size, at)?;
                }
            }
        }

        if out.len() >= expected_size {
            break;
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Command 0x80 | n: copy n literal bytes.
    fn literal(bytes: &[u8]) -> Vec<u8> {
        let mut v = vec![0x80 | bytes.len() as u8];
        v.extend_from_slice(bytes);
        v
    }

    #[test]
    fn literal_run_round_trips() {
        let mut stream = literal(b"KYRA");
        stream.push(0x80);
        assert_eq!(decompress(&stream, 4).expect("decode"), b"KYRA");
    }

    #[test]
    fn short_back_reference_repeats_earlier_bytes() {
        // Write "AB", then copy 3 bytes from 2 back: "ABABA".
        let mut stream = literal(b"AB");
        stream.push(0x00); // count = 3, high offset bits = 0
        stream.push(0x02); // offset = 2
        stream.push(0x80);
        assert_eq!(decompress(&stream, 5).expect("decode"), b"ABABA");
    }

    #[test]
    fn overlapping_back_reference_repeats_the_pattern() {
        // This is the case a slice copy gets wrong: reading one byte back
        // for eight bytes must smear that byte, not read past the head.
        let mut stream = literal(b"X");
        stream.push(0x50); // count = ((0x50 & 0x70) >> 4) + 3 = 8
        stream.push(0x01); // offset = 1
        stream.push(0x80);
        assert_eq!(decompress(&stream, 9).expect("decode"), b"XXXXXXXXX");
    }

    #[test]
    fn run_length_fill_expands() {
        let mut stream = vec![0xFE, 0x10, 0x00, 0x2A]; // 16 bytes of 0x2A
        stream.push(0x80);
        assert_eq!(decompress(&stream, 16).expect("decode"), vec![0x2A; 16]);
    }

    #[test]
    fn long_absolute_back_reference() {
        let mut stream = literal(b"HELLO");
        stream.extend_from_slice(&[0xFF, 0x03, 0x00, 0x00, 0x00]); // 3 bytes from index 0
        stream.push(0x80);
        assert_eq!(decompress(&stream, 8).expect("decode"), b"HELLOHEL");
    }

    #[test]
    fn medium_absolute_back_reference() {
        let mut stream = literal(b"HELLO");
        // 0xC0 | 1 -> count = 1 + 3 = 4, then absolute offset 1.
        stream.extend_from_slice(&[0xC1, 0x01, 0x00]);
        stream.push(0x80);
        assert_eq!(decompress(&stream, 9).expect("decode"), b"HELLOELLO");
    }

    #[test]
    fn end_marker_stops_before_declared_size_is_an_error() {
        let mut stream = literal(b"AB");
        stream.push(0x80);
        assert_eq!(
            decompress(&stream, 100),
            Err(Format80Error::OutputTruncated {
                got: 2,
                expected: 100
            })
        );
    }

    #[test]
    fn back_reference_before_output_start_is_rejected() {
        // Offset 5 with nothing written yet.
        let stream = vec![0x00, 0x05, 0x80];
        assert!(matches!(
            decompress(&stream, 10),
            Err(Format80Error::BadBackReference { .. })
        ));
    }

    #[test]
    fn truncated_literal_is_rejected() {
        // Promises 10 literal bytes, supplies 2.
        let stream = vec![0x8A, b'A', b'B'];
        assert!(matches!(
            decompress(&stream, 10),
            Err(Format80Error::UnexpectedEnd { .. })
        ));
    }

    #[test]
    fn overflow_past_declared_size_is_rejected() {
        let stream = vec![0xFE, 0xFF, 0xFF, 0x00];
        assert!(matches!(
            decompress(&stream, 16),
            Err(Format80Error::OutputOverflow { .. })
        ));
    }

    #[test]
    fn stream_stops_once_declared_size_is_reached() {
        // Trailing garbage after the output is complete must not matter:
        // real CPS files pad to a sector boundary.
        let mut stream = literal(b"AB");
        stream.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(decompress(&stream, 2).expect("decode"), b"AB");
    }

    #[test]
    fn decompress_up_to_accepts_a_short_result() {
        // A WSA frame's delta has no declared size, so stopping early is
        // the normal case rather than a truncation.
        let mut stream = literal(b"AB");
        stream.push(0x80);
        assert_eq!(decompress_up_to(&stream, 4096).expect("decode"), b"AB");
    }

    #[test]
    fn decompress_up_to_still_honours_its_limit() {
        let stream = vec![0xFE, 0xFF, 0xFF, 0x00];
        assert!(matches!(
            decompress_up_to(&stream, 16),
            Err(Format80Error::OutputOverflow { .. })
        ));
    }

    #[test]
    fn empty_stream_producing_nothing_is_fine() {
        assert_eq!(decompress(&[], 0).expect("decode"), Vec::<u8>::new());
    }
}
