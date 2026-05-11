//! Audio playback for InfEngine.
//!
//! Phase 3.3 wires the `kira` crate into the engine and exposes a tiny
//! API for synthesizing and playing short sounds. Asset-backed audio
//! (load from disk via `infengine_assets`) lands in a later phase.

#![forbid(unsafe_code)]

use std::sync::Arc;

use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend};
use tracing::info;

/// Errors raised by the audio system.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("failed to initialize audio backend: {0}")]
    Backend(#[from] kira::backend::cpal::Error),
    #[error("failed to play sound: {0}")]
    Play(#[from] kira::PlaySoundError<()>),
}

/// High-level audio system. Wraps a `kira` `AudioManager`.
pub struct AudioSystem {
    manager: AudioManager<DefaultBackend>,
}

impl std::fmt::Debug for AudioSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioSystem").finish()
    }
}

impl AudioSystem {
    /// Initialize the default audio backend. May fail if no output
    /// device is available.
    pub fn new() -> Result<Self, AudioError> {
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())?;
        info!("audio system initialized");
        Ok(Self { manager })
    }

    /// Play a synthesized sine-wave beep with a short fade in / out.
    ///
    /// * `freq_hz` - tone frequency in hertz (e.g. 440.0 for A4).
    /// * `duration_secs` - total length in seconds.
    /// * `volume` - linear gain in `[0, 1]` for the peak amplitude.
    pub fn play_beep(
        &mut self,
        freq_hz: f32,
        duration_secs: f32,
        volume: f32,
    ) -> Result<(), AudioError> {
        let sample_rate = 44_100u32;
        let total = ((sample_rate as f32) * duration_secs.max(0.01)) as usize;
        let tau = std::f32::consts::TAU;
        let fade = (sample_rate as f32 * 0.005) as usize; // 5 ms fade
        let mut frames = Vec::with_capacity(total);
        for i in 0..total {
            let t = i as f32 / sample_rate as f32;
            let env_in = (i as f32 / fade.max(1) as f32).clamp(0.0, 1.0);
            let env_out = ((total - i) as f32 / fade.max(1) as f32).clamp(0.0, 1.0);
            let env = env_in.min(env_out);
            let v = (freq_hz * tau * t).sin() * env * volume.clamp(0.0, 1.0);
            frames.push(kira::Frame { left: v, right: v });
        }
        let data = StaticSoundData {
            sample_rate,
            frames: Arc::from(frames.into_boxed_slice()),
            settings: StaticSoundSettings::default(),
            slice: None,
        };
        self.manager.play(data)?;
        Ok(())
    }
}
