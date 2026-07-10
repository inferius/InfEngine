//! Audio playback for InfEngine.
//!
//! Phase 3.3 wires the `kira` crate into the engine and exposes a tiny
//! API for synthesizing and playing short sounds. Asset-backed audio
//! (load from disk via `infengine_assets`) lands in a later phase.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::path::Path;
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
    #[error("failed to load sound from file: {0}")]
    LoadFile(#[from] kira::sound::FromFileError),
}

/// A loaded sound that can be played multiple times. Cloning is cheap
/// (the underlying frame buffer is `Arc<[Frame]>`).
#[derive(Debug, Clone)]
pub struct Sound {
    data: StaticSoundData,
}

/// W232: decode an audio file into a playable [`Sound`] without an
/// [`AudioSystem`]. Decoding is device-independent, so this can run
/// on a background thread; only playback needs the audio manager.
pub fn decode_sound(path: impl AsRef<Path>) -> Result<Sound, AudioError> {
    let data = StaticSoundData::from_file(path)?;
    Ok(Sound { data })
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

    /// Load a sound from disk (wav, ogg, flac, mp3 - whatever the active
    /// kira features support). The resulting handle can be played as
    /// many times as needed.
    pub fn load_sound(&self, path: impl AsRef<Path>) -> Result<Sound, AudioError> {
        decode_sound(path)
    }

    /// Play a previously loaded `Sound` once at default volume.
    pub fn play_sound(&mut self, sound: &Sound) -> Result<(), AudioError> {
        self.manager.play(sound.data.clone())?;
        Ok(())
    }

    /// Play `sound` with a linear gain in `[0, 1]`. Below ~1e-3
    /// gain we floor the conversion to a quiet -60 dB rather than
    /// `-inf` so distant SFX still trigger if they happen to clip
    /// past the audible-range guard.
    pub fn play_sound_with_volume(
        &mut self,
        sound: &Sound,
        gain: f32,
    ) -> Result<(), AudioError> {
        let g = gain.clamp(0.0, 1.5);
        let db = if g <= 1e-3 { -60.0 } else { 20.0 * g.log10() };
        let settings = sound.data.settings.volume(kira::Decibels::from(db));
        let mut data = sound.data.clone();
        data.settings = settings;
        self.manager.play(data)?;
        Ok(())
    }

    /// Build + play a "thud" sting: a low tone layered with a
    /// pitched noise burst, shaped by an exponential decay
    /// envelope. Good for break / impact SFX. `freq` is the
    /// fundamental tone frequency; `vol` is linear gain in `[0, 1]`.
    pub fn play_thud(&mut self, freq: f32, vol: f32) -> Result<(), AudioError> {
        let sample_rate = 44_100u32;
        let dur = 0.16_f32;
        let total = ((sample_rate as f32) * dur) as usize;
        let tau = std::f32::consts::TAU;
        let mut rng: u32 = 0xA5A5_5A5A ^ (freq.to_bits());
        let mut frames = Vec::with_capacity(total);
        for i in 0..total {
            let t = i as f32 / sample_rate as f32;
            let env = (-t * 18.0).exp();
            // Noise component: simple LCG -> [-1, 1]. The noise is
            // band-shaped by mixing the current sample with the
            // last two (cheap low-pass).
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let n = ((rng >> 8) as f32 / (u32::MAX >> 8) as f32) * 2.0 - 1.0;
            let tone = (freq * tau * t).sin() * 0.7
                + (freq * 0.5 * tau * t).sin() * 0.4;
            let s = (tone * 0.55 + n * 0.45) * env * vol.clamp(0.0, 1.0);
            frames.push(kira::Frame { left: s, right: s });
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

    /// Build + play a "click" sting: a short tone burst with a
    /// fast attack + decay. Good for place / UI confirmation
    /// SFX. `freq` is the tone, `vol` linear gain in `[0, 1]`.
    pub fn play_click(&mut self, freq: f32, vol: f32) -> Result<(), AudioError> {
        let sample_rate = 44_100u32;
        let dur = 0.08_f32;
        let total = ((sample_rate as f32) * dur) as usize;
        let tau = std::f32::consts::TAU;
        let mut frames = Vec::with_capacity(total);
        for i in 0..total {
            let t = i as f32 / sample_rate as f32;
            // 5 ms attack + exponential decay.
            let attack = (t / 0.005).clamp(0.0, 1.0);
            let decay = (-t * 22.0).exp();
            let env = attack * decay;
            let s = ((freq * tau * t).sin() * 0.6
                + (freq * 2.0 * tau * t).sin() * 0.3)
                * env
                * vol.clamp(0.0, 1.0);
            frames.push(kira::Frame { left: s, right: s });
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

    /// Build + play a soft noise burst for footsteps / cloth-rustle
    /// effects. Filtered noise only, no tone. `vol` linear gain.
    pub fn play_swish(&mut self, vol: f32) -> Result<(), AudioError> {
        let sample_rate = 44_100u32;
        let dur = 0.08_f32;
        let total = ((sample_rate as f32) * dur) as usize;
        let mut rng: u32 = 0x1357_9BDF;
        let mut last = 0.0_f32;
        let mut frames = Vec::with_capacity(total);
        for i in 0..total {
            let t = i as f32 / sample_rate as f32;
            let env = (-t * 28.0).exp();
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let n = ((rng >> 8) as f32 / (u32::MAX >> 8) as f32) * 2.0 - 1.0;
            // One-pole low-pass to take the harshness off raw
            // white noise.
            last = last * 0.75 + n * 0.25;
            let s = last * env * vol.clamp(0.0, 1.0);
            frames.push(kira::Frame { left: s, right: s });
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

/// A keyed collection of `Sound` variants. Designed for game SFX
/// where one logical event (e.g. "step:grass") has multiple
/// recordings + you want to pick a random one on each fire so the
/// playback doesn't sound mechanical.
///
/// The bank is decoupled from `AudioSystem` so it can be built on
/// a background thread or shared across audio frontends. Playback
/// methods take a `&mut AudioSystem` to keep mutation explicit.
#[derive(Default)]
pub struct AudioBank {
    variants: HashMap<String, Vec<Sound>>,
}

impl std::fmt::Debug for AudioBank {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioBank")
            .field("keys", &self.variants.len())
            .finish()
    }
}

impl AudioBank {
    /// Create an empty bank. Call `load_variants` to populate.
    pub fn new() -> Self {
        Self {
            variants: HashMap::new(),
        }
    }

    /// W232: attach pre-decoded sounds as the variants of `key`,
    /// replacing any previous set. Pairs with [`decode_sound`] so a
    /// caller can decode on a background thread (a full music track
    /// takes hundreds of ms - a synchronous decode inside a frame is
    /// a visible hitch) and hand the result over on the main thread.
    pub fn insert_variants(&mut self, key: &str, sounds: Vec<Sound>) -> usize {
        let n = sounds.len();
        if n > 0 {
            self.variants.insert(key.to_string(), sounds);
        }
        n
    }

    /// Load every file in `paths` as a variant of `key`. Files
    /// that fail to load are logged + skipped. Returns the number
    /// of variants successfully attached. Idempotent: calling
    /// again with the same key replaces previous variants.
    pub fn load_variants<P: AsRef<Path>>(
        &mut self,
        audio: &AudioSystem,
        key: &str,
        paths: &[P],
    ) -> usize {
        let mut sounds = Vec::with_capacity(paths.len());
        for p in paths {
            match audio.load_sound(p.as_ref()) {
                Ok(s) => sounds.push(s),
                Err(err) => {
                    tracing::warn!(
                        error = %err,
                        path = %p.as_ref().display(),
                        key,
                        "audio bank variant load failed"
                    );
                }
            }
        }
        let n = sounds.len();
        if n > 0 {
            self.variants.insert(key.to_string(), sounds);
        }
        n
    }

    /// True when the bank has at least one variant for `key`.
    pub fn has(&self, key: &str) -> bool {
        self.variants.get(key).is_some_and(|v| !v.is_empty())
    }

    /// Pick + play a variant of `key`. `seed` selects which one
    /// (`seed % variant_count`); callers can drive the seed with
    /// a frame counter / position hash to shuffle deterministically.
    /// `gain` is linear in `[0, 1]`. Returns `true` if a variant
    /// was played, `false` if the key was unknown or empty.
    pub fn play(
        &self,
        audio: &mut AudioSystem,
        key: &str,
        seed: u64,
        gain: f32,
    ) -> bool {
        let Some(variants) = self.variants.get(key) else {
            return false;
        };
        if variants.is_empty() {
            return false;
        }
        let idx = (seed as usize) % variants.len();
        if let Err(err) = audio.play_sound_with_volume(&variants[idx], gain) {
            tracing::debug!(error = %err, key, "audio bank play failed");
        }
        true
    }

    /// Play `key` at world-space `source`, with the listener at
    /// `listener`. Volume falls off linearly with distance, cutting
    /// to zero at `max_range`, then scales by `volume` (a per-sound
    /// mix level - e.g. MC plays footsteps at 0.15). Returns the
    /// computed gain when a sample fired, `None` when out of range or
    /// the key is unknown.
    #[allow(clippy::too_many_arguments)]
    pub fn play_at(
        &self,
        audio: &mut AudioSystem,
        key: &str,
        listener: [f32; 3],
        source: [f32; 3],
        max_range: f32,
        seed: u64,
        volume: f32,
    ) -> Option<f32> {
        let dx = source[0] - listener[0];
        let dy = source[1] - listener[1];
        let dz = source[2] - listener[2];
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d >= max_range || max_range <= 0.0 {
            return None;
        }
        let gain = (1.0 - d / max_range).clamp(0.0, 1.0) * volume.clamp(0.0, 1.5);
        if self.play(audio, key, seed, gain) {
            Some(gain)
        } else {
            None
        }
    }

    /// Pick the variant index that would be used for `(key, seed)`
    /// without playing. Useful for unit tests / shuffling that
    /// stays in sync across replicas.
    pub fn pick_index(&self, key: &str, seed: u64) -> Option<usize> {
        let variants = self.variants.get(key)?;
        if variants.is_empty() {
            return None;
        }
        Some((seed as usize) % variants.len())
    }

    /// Number of keys currently registered.
    pub fn key_count(&self) -> usize {
        self.variants.len()
    }
}
