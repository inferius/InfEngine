//! A27.3: encoding rendered audio as Ogg Vorbis.
//!
//! Ninety minutes of 32 kHz stereo is 668 MB as a wave file and about
//! forty as Vorbis. That difference is the difference between a
//! soundtrack you can ship and one you cannot, and it is why the
//! importer's last step is a lossy encoder rather than a copy.
//!
//! Behind the same feature as the synthesiser, since nothing else in
//! this importer produces audio.

use std::fs;
use std::io;
use std::num::{NonZeroU8, NonZeroU32};
use std::path::Path;

use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

/// Samples handed to the encoder at a time.
///
/// Vorbis wants planar float blocks; this is a compromise between
/// allocating one buffer the size of the track and calling into the
/// encoder for every frame.
const BLOCK: usize = 4096;

/// Quality, on libvorbis's -0.2 to 1.0 scale.
///
/// 0.5 is roughly 160 kbit/s at this sample rate. The source is a
/// synthesiser at 32 kHz, so there is nothing above 16 kHz to preserve
/// and spending bits on transparency past this point buys silence.
const QUALITY: f32 = 0.5;

#[derive(Debug)]
pub enum OggError {
    Encode(String),
    Write { path: String, source: io::Error },
}

impl std::fmt::Display for OggError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OggError::Encode(detail) => write!(f, "{detail}"),
            OggError::Write { path, source } => write!(f, "{path}: {source}"),
        }
    }
}

impl std::error::Error for OggError {}

/// Encode interleaved 16-bit samples as Ogg Vorbis.
pub fn encode(samples: &[i16], rate: u32, channels: u16) -> Result<Vec<u8>, OggError> {
    let channels = channels.max(1);
    let lanes = usize::from(channels);
    let frames = samples.len() / lanes;

    let rate = NonZeroU32::new(rate.max(1)).expect("rate is at least one");
    let channel_count = NonZeroU8::new(channels.min(255) as u8).expect("at least one channel");

    let mut builder = VorbisEncoderBuilder::new(rate, channel_count, Vec::new())
        .map_err(|err| OggError::Encode(err.to_string()))?;
    builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: QUALITY,
    });
    let mut encoder = builder
        .build()
        .map_err(|err| OggError::Encode(err.to_string()))?;

    // Vorbis takes one buffer per channel, not interleaved frames.
    let mut planes: Vec<Vec<f32>> = vec![Vec::with_capacity(BLOCK); lanes];
    let mut at = 0;
    while at < frames {
        let end = (at + BLOCK).min(frames);
        for plane in &mut planes {
            plane.clear();
        }
        for frame in at..end {
            for (lane, plane) in planes.iter_mut().enumerate() {
                plane.push(f32::from(samples[frame * lanes + lane]) / 32768.0);
            }
        }
        encoder
            .encode_audio_block(&planes)
            .map_err(|err| OggError::Encode(err.to_string()))?;
        at = end;
    }

    encoder
        .finish()
        .map_err(|err| OggError::Encode(err.to_string()))
}

/// Write an Ogg Vorbis file, creating the directories above it.
pub fn write(path: &Path, samples: &[i16], rate: u32, channels: u16) -> Result<(), OggError> {
    let data = encode(samples, rate, channels)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| OggError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    fs::write(path, data).map_err(|source| OggError::Write {
        path: path.display().to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second of a tone, so the encoder has something real to chew on.
    fn tone(rate: u32, seconds: f32, channels: u16) -> Vec<i16> {
        let frames = (rate as f32 * seconds) as usize;
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for frame in 0..frames {
            let t = frame as f32 / rate as f32;
            let value = (t * 440.0 * std::f32::consts::TAU).sin() * 12_000.0;
            for _ in 0..channels {
                samples.push(value as i16);
            }
        }
        samples
    }

    #[test]
    fn the_output_is_an_ogg_stream() {
        let data = encode(&tone(32_000, 0.2, 2), 32_000, 2).expect("encode");
        assert_eq!(&data[0..4], b"OggS", "an Ogg page starts with its tag");
        assert!(
            data.windows(6).any(|w| w == b"vorbis"),
            "and the codec identifies itself"
        );
    }

    #[test]
    fn a_tone_compresses_far_below_its_wave_size() {
        let samples = tone(32_000, 1.0, 2);
        let raw = samples.len() * 2;
        let encoded = encode(&samples, 32_000, 2).expect("encode").len();
        assert!(
            encoded * 4 < raw,
            "{encoded} bytes should be well under a quarter of {raw}"
        );
    }

    #[test]
    fn mono_and_stereo_both_encode() {
        assert!(!encode(&tone(22_050, 0.1, 1), 22_050, 1).expect("mono").is_empty());
        assert!(!encode(&tone(44_100, 0.1, 2), 44_100, 2).expect("stereo").is_empty());
    }

    #[test]
    fn an_empty_track_still_produces_a_readable_stream() {
        // Headers and an end-of-stream page, which is what a player
        // needs to see rather than a zero-byte file.
        let data = encode(&[], 32_000, 2).expect("encode");
        assert_eq!(&data[0..4], b"OggS");
    }

    #[test]
    fn a_trailing_half_frame_does_not_derail_the_encoder() {
        let mut samples = tone(32_000, 0.05, 2);
        samples.push(1234);
        assert!(!encode(&samples, 32_000, 2).expect("encode").is_empty());
    }
}
