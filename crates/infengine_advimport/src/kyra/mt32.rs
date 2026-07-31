//! A27.2: rendering the MT-32 soundtrack to audio.
//!
//! Kyrandia's `XMI.PAK` is the Roland MT-32 score - the data says so,
//! not guesswork: each sequence carries a `TIMB` chunk listing the
//! timbres it uses as `(patch, bank)` pairs, with bank 0 for the
//! module's internal patches and 127 for rhythm. That is MT-32
//! numbering, so playing it through a General MIDI bank would fetch the
//! wrong instruments.
//!
//! Which means synthesising it needs the module, and the module needs
//! its two ROMs. Those belong to whoever owns the hardware, so they are
//! supplied at import time and never go near the engine: this runs
//! once, offline, and what the game ships is the audio it produced.
//! That is the same rule the rest of the importer follows.
//!
//! Behind the `mt32` feature, because the emulator is LGPL and the rest
//! of this workspace is not. A default build neither links it nor needs
//! it.

use std::fmt;
use std::path::Path;

use moont::cm32l::{Device, Rom};
use moont::{Frame, Synth, smf};

/// Frames the MT-32 produces per second. Not a choice - it is what the
/// hardware ran at, and resampling belongs downstream if at all.
pub const RATE: u32 = 32_000;

/// Longest tail rendered after the last event, in seconds.
///
/// Long enough for the module's reverb, short enough that a sequence
/// ending on a held note does not run for ever.
const TAIL_SECONDS: f32 = 4.0;

/// Amplitude below which the tail counts as over.
const SILENCE: i16 = 12;

/// Size the control ROM has to be.
const CONTROL_BYTES: u64 = 0x10000;
/// Size the PCM ROM has to be. The CM-32L's is twice the MT-32's.
const PCM_BYTES: u64 = 0x100000;

#[derive(Debug)]
pub enum Mt32Error {
    Rom { path: String, detail: String },
    Midi { detail: String },
}

impl fmt::Display for Mt32Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mt32Error::Rom { path, detail } => write!(f, "{path}: {detail}"),
            Mt32Error::Midi { detail } => write!(f, "not a usable MIDI file: {detail}"),
        }
    }
}

impl std::error::Error for Mt32Error {}

/// The module's ROMs, read once and kept as bytes.
///
/// The synthesiser takes ownership of a decoded ROM, and a track has to
/// start from a module in its power-on state or the previous track's
/// patch edits bleed into it. So the bytes are what is held, and each
/// render builds its own module from them.
pub struct Module {
    control: Vec<u8>,
    pcm: Vec<u8>,
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Module").finish()
    }
}

impl Module {
    /// Load a control ROM and a PCM ROM.
    ///
    /// The file names vary between dumps, so the caller names them
    /// rather than this guessing - a guess that picks the wrong pair
    /// produces a synthesiser that runs and sounds wrong, which is the
    /// worst kind of failure.
    pub fn load(control: &Path, pcm: &Path) -> Result<Self, Mt32Error> {
        let control_bytes = std::fs::read(control).map_err(|err| Mt32Error::Rom {
            path: control.display().to_string(),
            detail: err.to_string(),
        })?;
        let pcm_bytes = std::fs::read(pcm).map_err(|err| Mt32Error::Rom {
            path: pcm.display().to_string(),
            detail: err.to_string(),
        })?;
        // Decoded once here purely to fail early: a bad ROM should be
        // reported before a thousand tracks are queued up.
        Rom::new(&control_bytes, &pcm_bytes).map_err(|err| Mt32Error::Rom {
            path: control.display().to_string(),
            detail: format!("{err:?}"),
        })?;
        Ok(Self {
            control: control_bytes,
            pcm: pcm_bytes,
        })
    }

    /// Find a usable pair of ROMs in a directory of dumps.
    ///
    /// The emulator is a CM-32L, which wants a 1 MB PCM ROM - the plain
    /// MT-32's is half that and is refused outright. That is not a
    /// compromise for Kyrandia: the CM-32L is an MT-32 with extra sound
    /// effects bolted on, so a score addressing the 128 original timbres
    /// plays on it exactly as written.
    ///
    /// Rather than trust a name, every candidate is checked by size.
    /// A dump directory holds several firmware revisions, and picking
    /// one that loads but is the wrong half of a split ROM would give a
    /// synthesiser that runs and sounds wrong - the worst kind of
    /// failure.
    pub fn find(dir: &Path) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
        const CONTROL: &[&str] = &[
            "CM32L_CONTROL.ROM",
            "cm32l_control.rom",
            "cm32l_ctrl_1_02.rom",
            "cm32l_ctrl_1_00.rom",
            "cm32ln_ctrl_1_00.rom",
        ];
        const PCM: &[&str] = &["CM32L_PCM.ROM", "cm32l_pcm.rom"];

        let control = CONTROL
            .iter()
            .map(|name| dir.join(name))
            .find(|path| has_size(path, CONTROL_BYTES))?;
        let pcm = PCM
            .iter()
            .map(|name| dir.join(name))
            .find(|path| has_size(path, PCM_BYTES))?;
        Some((control, pcm))
    }

    /// Render one standard MIDI file to interleaved stereo samples.
    pub fn render(&self, midi: &[u8]) -> Result<Vec<i16>, Mt32Error> {
        let events = smf::parse(midi).map_err(|err| Mt32Error::Midi {
            detail: format!("{err:?}"),
        })?;

        let rom = Rom::new(&self.control, &self.pcm).map_err(|err| Mt32Error::Rom {
            path: "<loaded>".to_string(),
            detail: format!("{err:?}"),
        })?;
        // The MT-32 maps its first part to MIDI channel 2, which is what
        // a score written for the module expects. Using the General MIDI
        // mapping here would silently shift every part by one.
        let mut synth = Device::new(rom);

        let last_event = events.iter().map(smf::Event::time).max().unwrap_or(0);
        for event in &events {
            match event {
                smf::Event::Msg { time, msg } => {
                    synth.play_msg_at(*msg, *time);
                }
                smf::Event::Sysex { time, data } => {
                    synth.play_sysex_at(data, *time);
                }
                // The event enum is non-exhaustive; anything added later
                // is something this importer does not know how to send.
                _ => {}
            }
        }

        let tail = (RATE as f32 * TAIL_SECONDS) as u32;
        let total = (last_event + tail) as usize;
        let mut frames = vec![Frame(0, 0); total];
        synth.render(&mut frames);

        let mut samples = Vec::with_capacity(total * 2);
        for frame in frames {
            samples.push(frame.0);
            samples.push(frame.1);
        }
        let trimmed = crate::wav::trim_trailing_silence(&samples, 2, SILENCE).len();
        samples.truncate(trimmed);
        Ok(samples)
    }
}

fn has_size(path: &Path, expected: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_rom_names_the_file_it_could_not_read() {
        let err = Module::load(
            Path::new("no/such/control.rom"),
            Path::new("no/such/pcm.rom"),
        )
        .expect_err("there is no such file");
        assert!(err.to_string().contains("control.rom"), "{err}");
    }

    #[test]
    fn finding_roms_in_an_empty_directory_gives_nothing() {
        let dir = std::env::temp_dir().join("infengine_mt32_empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        assert!(Module::find(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rom_of_the_wrong_size_is_passed_over() {
        let dir = std::env::temp_dir().join("infengine_mt32_sizes");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");

        // A dump directory holds several revisions and the halves of
        // split ROMs. Only the whole ones are usable, and a name is not
        // enough to tell them apart.
        std::fs::write(dir.join("cm32l_ctrl_1_00.rom"), vec![0u8; 32_768]).expect("write");
        std::fs::write(dir.join("cm32l_ctrl_1_02.rom"), vec![0u8; 65_536]).expect("write");
        std::fs::write(dir.join("cm32l_pcm.rom"), vec![0u8; 1_048_576]).expect("write");

        let (control, pcm) = Module::find(&dir).expect("found a usable pair");
        // Compared without case: on Windows a lookup for CM32L_PCM.ROM
        // finds a file written as cm32l_pcm.rom and hands back the name
        // that was asked for, not the one on disk.
        let name = |path: &std::path::Path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
        };
        assert_eq!(name(&control), "cm32l_ctrl_1_02.rom");
        assert_eq!(name(&pcm), "cm32l_pcm.rom");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_with_only_half_a_rom_finds_nothing() {
        let dir = std::env::temp_dir().join("infengine_mt32_half");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("cm32l_ctrl_1_02.rom"), vec![0u8; 65_536]).expect("write");
        std::fs::write(dir.join("cm32l_pcm_h.rom"), vec![0u8; 524_288]).expect("write");
        assert!(Module::find(&dir).is_none(), "half a PCM ROM is not a ROM");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
