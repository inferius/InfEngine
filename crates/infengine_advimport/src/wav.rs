//! A27.1: writing a RIFF WAVE file.
//!
//! Small enough to own rather than depend on. Everything this importer
//! produces has to be something the engine can already read, and a
//! 16-bit PCM wave is the one audio format nothing has an opinion about.

use std::fs;
use std::io;
use std::path::Path;

/// Bytes of header before the samples begin.
pub const HEADER_LEN: usize = 44;

/// Encode interleaved 16-bit samples as a RIFF WAVE file.
///
/// `channels` is 1 for mono, 2 for interleaved stereo. The sample slice
/// must hold whole frames; a trailing half-frame is dropped rather than
/// written, since a file whose data chunk is not a multiple of its block
/// alignment is one some decoders refuse outright.
pub fn encode(samples: &[i16], rate: u32, channels: u16) -> Vec<u8> {
    let channels = channels.max(1);
    let frames = samples.len() / usize::from(channels);
    let samples = &samples[..frames * usize::from(channels)];

    let block_align = channels * 2;
    let byte_rate = rate * u32::from(block_align);
    let data_len = (samples.len() * 2) as u32;

    let mut out = Vec::with_capacity(HEADER_LEN + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// Write a wave file, creating the directories above it.
pub fn write(path: &Path, samples: &[i16], rate: u32, channels: u16) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, encode(samples, rate, channels))
}

/// Trim silence from the end of a rendered buffer.
///
/// A synthesiser asked to run until nothing is sounding leaves a tail of
/// digital zeroes, and a thousand of those add up. `threshold` is the
/// amplitude below which a frame counts as silence - not zero, because a
/// reverb tail decays into single-digit values that are inaudible and
/// still not zero.
pub fn trim_trailing_silence(samples: &[i16], channels: u16, threshold: i16) -> &[i16] {
    let channels = usize::from(channels.max(1));
    let mut end = samples.len();
    while end >= channels {
        let frame = &samples[end - channels..end];
        if frame.iter().any(|s| s.saturating_abs() > threshold) {
            break;
        }
        end -= channels;
    }
    &samples[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read a little-endian u32 out of the encoded header, by spelling
    /// the offset out rather than reusing the writer's arithmetic.
    fn u32_at(data: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
    }

    fn u16_at(data: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([data[at], data[at + 1]])
    }

    #[test]
    fn the_header_says_what_the_specification_says_it_should() {
        let wav = encode(&[1, -1, 2, -2], 32_000, 2);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(u32_at(&wav, 16), 16, "a PCM format chunk is 16 bytes");
        assert_eq!(u16_at(&wav, 20), 1, "format 1 is PCM");
        assert_eq!(u16_at(&wav, 22), 2, "channels");
        assert_eq!(u32_at(&wav, 24), 32_000, "sample rate");
        assert_eq!(u32_at(&wav, 28), 32_000 * 4, "bytes per second");
        assert_eq!(u16_at(&wav, 32), 4, "block align");
        assert_eq!(u16_at(&wav, 34), 16, "bits per sample");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32_at(&wav, 40), 8, "four samples of two bytes");
        assert_eq!(wav.len(), 44 + 8);
    }

    #[test]
    fn the_riff_size_counts_everything_after_it() {
        let wav = encode(&[0; 100], 8_000, 1);
        assert_eq!(
            u32_at(&wav, 4) as usize,
            wav.len() - 8,
            "the RIFF size excludes the tag and itself"
        );
    }

    #[test]
    fn samples_survive_the_round_trip() {
        let samples = [0i16, 1, -1, i16::MAX, i16::MIN, 1234];
        let wav = encode(&samples, 44_100, 1);
        let decoded: Vec<i16> = wav[44..]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(decoded, samples);
    }

    #[test]
    fn a_half_frame_is_dropped_rather_than_written() {
        // Three samples cannot be whole stereo frames.
        let wav = encode(&[1, 2, 3], 8_000, 2);
        assert_eq!(u32_at(&wav, 40), 4, "one frame, not one and a half");
    }

    #[test]
    fn an_empty_buffer_still_makes_a_valid_file() {
        let wav = encode(&[], 8_000, 1);
        assert_eq!(wav.len(), HEADER_LEN);
        assert_eq!(u32_at(&wav, 40), 0);
    }

    #[test]
    fn trailing_silence_goes_but_the_music_stays() {
        let samples = [500i16, -500, 3, -2, 0, 0, 0, 0];
        let trimmed = trim_trailing_silence(&samples, 2, 8);
        assert_eq!(trimmed, &[500, -500], "the quiet tail is cut");

        let loud = [100i16, 100, 200, 200];
        assert_eq!(
            trim_trailing_silence(&loud, 2, 8).len(),
            4,
            "and nothing audible is lost"
        );
    }

    #[test]
    fn silence_all_the_way_down_trims_to_nothing() {
        assert!(trim_trailing_silence(&[0, 0, 0, 0], 2, 8).is_empty());
    }

    #[test]
    fn trimming_never_leaves_a_partial_frame() {
        let samples = [900i16, 0, 0, 0, 0, 0];
        let trimmed = trim_trailing_silence(&samples, 2, 8);
        assert_eq!(trimmed.len() % 2, 0, "frames stay whole");
        assert_eq!(trimmed, &[900, 0]);
    }
}
