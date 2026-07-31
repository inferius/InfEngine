//! A14.1: the Creative Voice File, and the CD release's dialogue.
//!
//! Kyrandia's `.VRM` archives are ordinary PAK files holding numbered
//! `.VOC` recordings - the CD release's spoken dialogue, one per line.
//! VOC is Creative Labs' old format: a text signature, then a chain of
//! typed blocks.
//!
//! ```text
//!   "Creative Voice File\x1A"
//!   u16 header size, u16 version, u16 checksum
//!   repeated: u8 type, u24 size, payload
//!     type 1  time constant, codec, then PCM
//!     type 2  more PCM, continuing the block before it
//!     type 9  explicit rate, bit depth and channel count, then PCM
//!     type 0  end
//! ```
//!
//! The old block type states its rate as a *time constant* rather than
//! in hertz: the sample rate is `1000000 / (256 - constant)`. Reading it
//! as a frequency yields audio that plays at roughly a two-hundredth of
//! the right speed.
//!
//! Output is a WAV, because it needs no encoder and every tool opens it.

use std::fmt;

/// Signature every VOC starts with, including its terminator.
const SIGNATURE: &[u8] = b"Creative Voice File\x1A";

/// Block that carries PCM with a time constant.
const BLOCK_SOUND: u8 = 1;
/// Block that continues the previous one.
const BLOCK_CONTINUE: u8 = 2;
/// Block that carries PCM with an explicit format.
const BLOCK_SOUND_NEW: u8 = 9;
/// Block that ends the file.
const BLOCK_END: u8 = 0;

/// Codec id for unsigned 8-bit PCM, the only one these files use.
const CODEC_PCM8: u8 = 0;

/// Rates outside this range mean the header was misread.
const MIN_RATE: u32 = 1000;
const MAX_RATE: u32 = 192_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VocError {
    /// Missing or wrong signature.
    NotVoc,
    /// The header names a data offset outside the file.
    BadHeader,
    /// A block ran past the end of the file.
    TruncatedBlock { at: usize },
    /// A codec other than unsigned 8-bit PCM.
    UnsupportedCodec(u8),
    /// The header's rate is not a plausible sample rate.
    ImplausibleRate(u32),
    /// No audio in the file.
    NoSamples,
}

impl fmt::Display for VocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotVoc => write!(f, "not a Creative Voice File"),
            Self::BadHeader => write!(f, "VOC header points outside the file"),
            Self::TruncatedBlock { at } => write!(f, "VOC block at byte {at} runs past the file"),
            Self::UnsupportedCodec(c) => write!(f, "VOC codec {c} is not 8-bit PCM"),
            Self::ImplausibleRate(r) => write!(f, "VOC claims a sample rate of {r} Hz"),
            Self::NoSamples => write!(f, "VOC holds no audio"),
        }
    }
}

impl std::error::Error for VocError {}

/// Decoded audio: unsigned 8-bit mono PCM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    pub sample_rate: u32,
    pub samples: Vec<u8>,
}

impl Voice {
    /// Length in milliseconds.
    pub fn duration_ms(&self) -> u32 {
        if self.sample_rate == 0 {
            return 0;
        }
        (self.samples.len() as u64 * 1000 / self.sample_rate as u64) as u32
    }

    /// Wrap the samples in a RIFF WAV container.
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = self.samples.len() as u32;
        let mut out = Vec::with_capacity(44 + self.samples.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // chunk size
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&self.sample_rate.to_le_bytes()); // byte rate
        out.extend_from_slice(&1u16.to_le_bytes()); // block align
        out.extend_from_slice(&8u16.to_le_bytes()); // bits per sample
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(&self.samples);
        out
    }
}

/// Sample rate from an old block's time constant.
fn rate_from_constant(constant: u8) -> u32 {
    // 256 - 255 is 1, so the divisor is never zero.
    1_000_000 / (256 - constant as u32)
}

fn u24(data: &[u8], at: usize) -> Option<usize> {
    let b = data.get(at..at + 3)?;
    Some(b[0] as usize | ((b[1] as usize) << 8) | ((b[2] as usize) << 16))
}

/// Decode a `.VOC` file.
pub fn decode(data: &[u8]) -> Result<Voice, VocError> {
    if data.len() < SIGNATURE.len() + 6 || !data.starts_with(SIGNATURE) {
        return Err(VocError::NotVoc);
    }
    let header_len = u16::from_le_bytes([data[20], data[21]]) as usize;
    if header_len < SIGNATURE.len() || header_len > data.len() {
        return Err(VocError::BadHeader);
    }

    let mut samples = Vec::new();
    let mut rate = 0u32;
    let mut pos = header_len;

    while pos < data.len() {
        let at = pos;
        let block_type = data[pos];
        pos += 1;
        if block_type == BLOCK_END {
            break;
        }
        let Some(size) = u24(data, pos) else {
            return Err(VocError::TruncatedBlock { at });
        };
        pos += 3;
        let Some(payload) = data.get(pos..pos + size) else {
            return Err(VocError::TruncatedBlock { at });
        };
        pos += size;

        match block_type {
            BLOCK_SOUND => {
                if payload.len() < 2 {
                    return Err(VocError::TruncatedBlock { at });
                }
                if payload[1] != CODEC_PCM8 {
                    return Err(VocError::UnsupportedCodec(payload[1]));
                }
                if rate == 0 {
                    rate = rate_from_constant(payload[0]);
                }
                samples.extend_from_slice(&payload[2..]);
            }
            // A continuation carries nothing but more of the same PCM.
            BLOCK_CONTINUE => samples.extend_from_slice(payload),
            BLOCK_SOUND_NEW => {
                if payload.len() < 12 {
                    return Err(VocError::TruncatedBlock { at });
                }
                if payload[5] != CODEC_PCM8 && payload[4] != 8 {
                    return Err(VocError::UnsupportedCodec(payload[5]));
                }
                if rate == 0 {
                    rate = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                }
                samples.extend_from_slice(&payload[12..]);
            }
            // Silence, markers, repeats and text blocks carry no audio
            // this importer needs; skipping keeps one odd block from
            // costing the whole recording.
            _ => {}
        }
    }

    if samples.is_empty() {
        return Err(VocError::NoSamples);
    }
    if !(MIN_RATE..=MAX_RATE).contains(&rate) {
        return Err(VocError::ImplausibleRate(rate));
    }
    Ok(Voice {
        sample_rate: rate,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a VOC with one old-style sound block.
    fn build(constant: u8, pcm: &[u8]) -> Vec<u8> {
        let mut v = SIGNATURE.to_vec();
        v.extend_from_slice(&26u16.to_le_bytes()); // header size
        v.extend_from_slice(&0x010Au16.to_le_bytes()); // version
        v.extend_from_slice(&0x1129u16.to_le_bytes()); // checksum
        v.push(BLOCK_SOUND);
        let size = pcm.len() + 2;
        v.extend_from_slice(&size.to_le_bytes()[..3]);
        v.push(constant);
        v.push(CODEC_PCM8);
        v.extend_from_slice(pcm);
        v.push(BLOCK_END);
        v
    }

    #[test]
    fn a_single_block_decodes() {
        let voice = decode(&build(0xD2, &[10, 20, 30, 40])).expect("decode");
        assert_eq!(voice.samples, vec![10, 20, 30, 40]);
    }

    #[test]
    fn the_time_constant_becomes_a_sample_rate() {
        // The retail files use 0xD2, which is about 21.7 kHz. Reading
        // the byte as a frequency would give 210 Hz.
        assert_eq!(rate_from_constant(0xD2), 21739);
        let voice = decode(&build(0xD2, &[0; 8])).expect("decode");
        assert_eq!(voice.sample_rate, 21739);
    }

    #[test]
    fn the_constant_never_divides_by_zero() {
        assert_eq!(rate_from_constant(255), 1_000_000);
    }

    #[test]
    fn a_continuation_block_appends_to_the_same_recording() {
        let mut v = build(0xD2, &[1, 2]);
        v.pop(); // drop the end marker
        v.push(BLOCK_CONTINUE);
        v.extend_from_slice(&3usize.to_le_bytes()[..3]);
        v.extend_from_slice(&[3, 4, 5]);
        v.push(BLOCK_END);
        let voice = decode(&v).expect("decode");
        assert_eq!(voice.samples, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn an_unknown_block_is_skipped_rather_than_fatal() {
        // Silence and marker blocks appear in real files; one of them
        // must not cost the whole recording.
        let mut v = build(0xD2, &[7, 7]);
        v.pop();
        v.push(3); // silence block
        v.extend_from_slice(&3usize.to_le_bytes()[..3]);
        v.extend_from_slice(&[0, 0, 0]);
        v.push(BLOCK_CONTINUE);
        v.extend_from_slice(&1usize.to_le_bytes()[..3]);
        v.push(9);
        v.push(BLOCK_END);
        let voice = decode(&v).expect("decode");
        assert_eq!(voice.samples, vec![7, 7, 9]);
    }

    #[test]
    fn a_new_style_block_states_its_rate_directly() {
        let mut v = SIGNATURE.to_vec();
        v.extend_from_slice(&26u16.to_le_bytes());
        v.extend_from_slice(&0x0114u16.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.push(BLOCK_SOUND_NEW);
        let pcm = [5u8, 6, 7];
        v.extend_from_slice(&(12 + pcm.len()).to_le_bytes()[..3]);
        v.extend_from_slice(&22050u32.to_le_bytes());
        v.push(8); // bits
        v.push(1); // channels
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(&[0; 4]);
        v.extend_from_slice(&pcm);
        v.push(BLOCK_END);
        let voice = decode(&v).expect("decode");
        assert_eq!(voice.sample_rate, 22050);
        assert_eq!(voice.samples, pcm);
    }

    #[test]
    fn duration_follows_from_rate_and_length() {
        let voice = Voice {
            sample_rate: 1000,
            samples: vec![0; 2500],
        };
        assert_eq!(voice.duration_ms(), 2500);
        assert_eq!(
            Voice {
                sample_rate: 0,
                samples: vec![0; 10]
            }
            .duration_ms(),
            0
        );
    }

    #[test]
    fn the_wav_header_describes_the_samples() {
        let voice = Voice {
            sample_rate: 21739,
            samples: vec![1, 2, 3, 4],
        };
        let wav = voice.to_wav();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(wav[40..44].try_into().expect("len")),
            4,
            "data chunk length"
        );
        assert_eq!(
            u32::from_le_bytes(wav[24..28].try_into().expect("rate")),
            21739
        );
        assert_eq!(wav.len(), 44 + 4);
    }

    #[test]
    fn a_file_without_the_signature_is_rejected() {
        assert_eq!(decode(b"nope"), Err(VocError::NotVoc));
        let mut v = build(0xD2, &[1]);
        v[0] = b'X';
        assert_eq!(decode(&v), Err(VocError::NotVoc));
    }

    #[test]
    fn a_header_pointing_outside_the_file_is_rejected() {
        let mut v = build(0xD2, &[1, 2]);
        v[20..22].copy_from_slice(&9999u16.to_le_bytes());
        assert_eq!(decode(&v), Err(VocError::BadHeader));
    }

    #[test]
    fn a_truncated_block_is_rejected() {
        let mut v = build(0xD2, &[1, 2, 3, 4]);
        v.truncate(v.len() - 3);
        assert!(matches!(
            decode(&v),
            Err(VocError::TruncatedBlock { .. }) | Err(VocError::NoSamples)
        ));
    }

    #[test]
    fn a_compressed_recording_is_named_not_mangled() {
        let mut v = build(0xD2, &[1, 2]);
        // Codec byte sits right after the time constant.
        let at = SIGNATURE.len() + 6 + 4 + 1;
        v[at] = 1; // 4-bit ADPCM
        assert_eq!(decode(&v), Err(VocError::UnsupportedCodec(1)));
    }

    #[test]
    fn a_file_with_no_audio_is_rejected() {
        let mut v = SIGNATURE.to_vec();
        v.extend_from_slice(&26u16.to_le_bytes());
        v.extend_from_slice(&0x010Au16.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes());
        v.push(BLOCK_END);
        assert_eq!(decode(&v), Err(VocError::NoSamples));
    }
}
