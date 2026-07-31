//! A26.2: one FM operator - a phase, an envelope, and a waveform.
//!
//! The envelope is a faithful *model* of the YM3812's, not a gate-level
//! reproduction: four phases over a 9-bit attenuation counter, with the
//! published rate-to-time relationship (each step of four in the rate
//! halves the time, and rate 0 never moves). It is not bit-exact against
//! silicon, and where that shows is in the first few milliseconds of a
//! fast attack. Everything a listener would call the instrument - its
//! decay shape, its sustain level, how key scaling brightens the top
//! octaves - comes out of the same numbers the hardware uses.

use crate::tables::{self, KEY_SCALE_LEVEL};

/// Loudest the envelope counter goes, in the chip's 1/8 dB steps.
pub const ENVELOPE_MAX: u32 = 0x1FF;

/// Which part of the envelope an operator is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Silent, waiting for a key-on.
    #[default]
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// One of the chip's eighteen operators.
#[derive(Debug, Clone, Default)]
pub struct Operator {
    // Register state, named after the datasheet's fields.
    pub tremolo: bool,
    pub vibrato: bool,
    /// Envelope type: true holds at the sustain level, false decays away.
    pub sustaining: bool,
    /// Key scale rate: makes high notes envelope faster.
    pub key_scale_rate: bool,
    /// Frequency multiplier, already expanded from the register's
    /// half-steps (the register's 0 means one half).
    pub multiple: u32,
    /// Key scale level: how much high notes are attenuated.
    pub key_scale_level: u8,
    /// Total level, 0 loudest.
    pub total_level: u32,
    pub attack: u8,
    pub decay: u8,
    /// Sustain level in the register's units, 0 loudest.
    pub sustain: u8,
    pub release: u8,
    /// Waveform 0-3. Only 0 exists until the chip's waveform-select bit
    /// is enabled, which is a real and audible OPL2 quirk.
    pub waveform: u8,

    phase: Phase,
    /// Phase accumulator, 10.10 fixed point over one period.
    phase_counter: u32,
    /// Envelope attenuation, 0 loudest.
    envelope: u32,
    /// Fractional envelope progress, so slow rates still move.
    envelope_fraction: u32,
    /// Last two outputs, for feedback.
    pub history: [i32; 2],
}

impl Operator {
    pub fn new() -> Self {
        Self {
            multiple: 1,
            envelope: ENVELOPE_MAX,
            phase: Phase::Off,
            ..Default::default()
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_silent(&self) -> bool {
        self.phase == Phase::Off || self.envelope >= ENVELOPE_MAX
    }

    /// Start a note.
    ///
    /// The phase counter is reset, which is what makes two operators
    /// keyed together sound like one instrument rather than two.
    pub fn key_on(&mut self) {
        if self.phase == Phase::Off || self.phase == Phase::Release {
            self.phase_counter = 0;
        }
        self.phase = Phase::Attack;
        self.envelope_fraction = 0;
    }

    pub fn key_off(&mut self) {
        if self.phase != Phase::Off {
            self.phase = Phase::Release;
        }
    }

    /// Advance the envelope by one sample.
    ///
    /// `rate_bonus` is the key-scale contribution worked out by the
    /// channel, which knows the block and frequency this operator is
    /// playing at.
    pub fn tick_envelope(&mut self, rate_bonus: u32) {
        let (target_rate, going_up) = match self.phase {
            Phase::Off => return,
            Phase::Attack => (self.attack, true),
            Phase::Decay => (self.decay, false),
            Phase::Sustain => {
                // A sustaining envelope holds; a percussive one keeps
                // falling at the release rate even with the key down.
                if self.sustaining {
                    return;
                }
                (self.release, false)
            }
            Phase::Release => (self.release, false),
        };

        if target_rate == 0 {
            // Rate zero means "never", which is how a pad holds for ever
            // and how a key-off can leave a note ringing.
            return;
        }
        let rate = (u32::from(target_rate) * 4 + rate_bonus).min(63);

        // Each four steps of rate doubles the speed. Below rate 4 the
        // step is a fraction, so it is accumulated rather than rounded
        // to zero - otherwise the slowest envelopes never move at all.
        let shift = 15u32.saturating_sub(rate / 4);
        self.envelope_fraction += 1 << (rate % 4);
        let steps = self.envelope_fraction >> shift;
        if steps == 0 {
            return;
        }
        self.envelope_fraction -= steps << shift;

        if going_up {
            // Attack approaches zero attenuation proportionally, which
            // is what gives the chip its characteristic curved attack
            // rather than a straight ramp.
            let step = ((self.envelope + 1) * steps / 16).max(steps);
            self.envelope = self.envelope.saturating_sub(step);
            if self.envelope == 0 {
                self.phase = Phase::Decay;
            }
        } else {
            self.envelope = (self.envelope + steps).min(ENVELOPE_MAX);
            let sustain_level = sustain_attenuation(self.sustain);
            if self.phase == Phase::Decay && self.envelope >= sustain_level {
                self.envelope = sustain_level;
                self.phase = Phase::Sustain;
            }
            if self.envelope >= ENVELOPE_MAX && self.phase == Phase::Release {
                self.phase = Phase::Off;
            }
        }
    }

    /// Move the phase on by one sample.
    pub fn tick_phase(&mut self, increment: u32) {
        self.phase_counter = self.phase_counter.wrapping_add(increment);
    }

    /// This operator's output.
    ///
    /// `modulation` shifts the phase - that is all frequency modulation
    /// is here. `extra` is the attenuation the channel adds for key
    /// scaling and tremolo.
    pub fn output(&mut self, modulation: i32, extra: u32) -> i32 {
        if self.phase == Phase::Off {
            self.history = [0, 0];
            return 0;
        }
        let phase = ((self.phase_counter >> 10) as i32).wrapping_add(modulation) as u32 & 0x3FF;
        let attenuation = (self.envelope + self.total_level * 4 + extra).min(0x1FFF) << 2;
        let sample = wave(self.waveform, phase, attenuation);

        self.history[1] = self.history[0];
        self.history[0] = sample;
        sample
    }

    /// Key scale attenuation for the note this operator plays.
    pub fn key_scale(&self, block: u32, fnum_high: u32) -> u32 {
        if self.key_scale_level == 0 {
            return 0;
        }
        let level = u32::from(KEY_SCALE_LEVEL[(fnum_high & 0xF) as usize]);
        let attenuation = level.saturating_sub(8 * (7 - block));
        // The register's three settings are 3, 1.5 and 6 dB per octave,
        // which the hardware expresses as these shifts.
        match self.key_scale_level {
            1 => attenuation >> 1,
            2 => attenuation,
            _ => attenuation << 1,
        }
    }

    /// Extra envelope rate from the note's pitch.
    pub fn key_scale_rate_bonus(&self, block: u32, fnum_high: u32) -> u32 {
        let raw = block * 2 + (fnum_high >> 3);
        if self.key_scale_rate { raw } else { raw >> 2 }
    }

    #[cfg(test)]
    pub fn envelope(&self) -> u32 {
        self.envelope
    }
}

/// Attenuation the sustain level setting means.
///
/// The register counts 3 dB steps, except that 15 means "as quiet as it
/// goes" rather than 45 dB - a special case in the hardware and the
/// reason a sustain of 15 is silence, not merely faint.
fn sustain_attenuation(sustain: u8) -> u32 {
    if sustain >= 15 {
        ENVELOPE_MAX
    } else {
        u32::from(sustain) * 16
    }
}

/// Sample one of the four waveforms at `phase`, given an attenuation.
fn wave(waveform: u8, phase: u32, attenuation: u32) -> i32 {
    let quarter = phase & 0xFF;
    let half = phase & 0x1FF;
    match waveform & 3 {
        // Full sine.
        0 => {
            let negative = phase & 0x200 != 0;
            let index = if phase & 0x100 != 0 { 255 - quarter } else { quarter };
            tables::attenuation_to_sample(
                u32::from(tables::log_sin(index as usize)) + attenuation,
                negative,
            )
        }
        // Half sine: the negative half is muted.
        1 => {
            if phase & 0x200 != 0 {
                return 0;
            }
            let index = if phase & 0x100 != 0 { 255 - quarter } else { quarter };
            tables::attenuation_to_sample(
                u32::from(tables::log_sin(index as usize)) + attenuation,
                false,
            )
        }
        // Absolute sine: both humps positive.
        2 => {
            let index = if half & 0x100 != 0 { 255 - quarter } else { quarter };
            tables::attenuation_to_sample(
                u32::from(tables::log_sin(index as usize)) + attenuation,
                false,
            )
        }
        // Pulse sine: only the rising quarters, which is where the
        // OPL2's harsher patches get their bite.
        _ => {
            if phase & 0x100 != 0 {
                return 0;
            }
            tables::attenuation_to_sample(
                u32::from(tables::log_sin(quarter as usize)) + attenuation,
                false,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyed(attack: u8, decay: u8, sustain: u8, release: u8) -> Operator {
        let mut op = Operator::new();
        op.attack = attack;
        op.decay = decay;
        op.sustain = sustain;
        op.release = release;
        op.key_on();
        op
    }

    #[test]
    fn a_new_operator_is_silent() {
        let op = Operator::new();
        assert!(op.is_silent());
        assert_eq!(op.phase(), Phase::Off);
    }

    #[test]
    fn an_attack_reaches_full_volume_and_moves_on() {
        let mut op = keyed(15, 0, 0, 0);
        for _ in 0..2000 {
            op.tick_envelope(0);
            if op.phase() != Phase::Attack {
                break;
            }
        }
        assert_eq!(op.phase(), Phase::Decay);
        assert_eq!(op.envelope(), 0, "attack ends at full volume");
    }

    #[test]
    fn a_faster_attack_takes_fewer_samples() {
        let mut count = Vec::new();
        for rate in [4u8, 8, 12] {
            let mut op = keyed(rate, 0, 0, 0);
            let mut samples = 0;
            while op.phase() == Phase::Attack && samples < 2_000_000 {
                op.tick_envelope(0);
                samples += 1;
            }
            count.push(samples);
        }
        assert!(count[0] > count[1] && count[1] > count[2], "{count:?}");
    }

    #[test]
    fn a_rate_of_zero_never_moves() {
        let mut op = keyed(0, 0, 0, 0);
        for _ in 0..100_000 {
            op.tick_envelope(0);
        }
        assert_eq!(op.phase(), Phase::Attack, "rate zero means never");
        assert_eq!(op.envelope(), ENVELOPE_MAX);
    }

    #[test]
    fn a_decay_stops_at_the_sustain_level() {
        let mut op = keyed(15, 10, 4, 0);
        op.sustaining = true;
        for _ in 0..200_000 {
            op.tick_envelope(0);
            if op.phase() == Phase::Sustain {
                break;
            }
        }
        assert_eq!(op.phase(), Phase::Sustain);
        assert_eq!(op.envelope(), 4 * 16);

        // And it holds there for as long as the key is down.
        for _ in 0..100_000 {
            op.tick_envelope(0);
        }
        assert_eq!(op.envelope(), 4 * 16);
    }

    #[test]
    fn a_percussive_envelope_keeps_falling_through_sustain() {
        let mut op = keyed(15, 15, 4, 10);
        op.sustaining = false;
        for _ in 0..500_000 {
            op.tick_envelope(0);
        }
        assert!(
            op.envelope() > 4 * 16,
            "without the sustain bit the note dies away by itself"
        );
    }

    #[test]
    fn a_release_ends_in_silence() {
        let mut op = keyed(15, 15, 0, 12);
        op.sustaining = true;
        for _ in 0..10_000 {
            op.tick_envelope(0);
        }
        op.key_off();
        for _ in 0..500_000 {
            op.tick_envelope(0);
            if op.phase() == Phase::Off {
                break;
            }
        }
        assert_eq!(op.phase(), Phase::Off);
        assert!(op.is_silent());
    }

    #[test]
    fn a_sustain_of_fifteen_is_silence_not_merely_faint() {
        assert_eq!(sustain_attenuation(15), ENVELOPE_MAX);
        assert_eq!(sustain_attenuation(14), 14 * 16);
    }

    #[test]
    fn the_half_sine_wave_mutes_its_negative_half() {
        // Second half of the period.
        assert_eq!(wave(1, 0x300, 0), 0);
        assert!(wave(1, 0x100, 0) > 0, "and keeps the first");
    }

    #[test]
    fn the_absolute_sine_never_goes_negative() {
        for phase in (0..0x400).step_by(7) {
            assert!(wave(2, phase, 0) >= 0);
        }
    }

    #[test]
    fn the_plain_sine_is_symmetric_about_the_half_period() {
        for phase in (0..0x200).step_by(11) {
            let a = wave(0, phase, 0);
            let b = wave(0, phase + 0x200, 0);
            assert_eq!(a, -b, "phase {phase:#x}");
        }
    }

    #[test]
    fn key_scaling_attenuates_the_top_octaves_more() {
        let mut op = Operator::new();
        op.key_scale_level = 2;
        let low = op.key_scale(1, 8);
        let high = op.key_scale(6, 8);
        assert!(high > low, "{high} should exceed {low}");
        op.key_scale_level = 0;
        assert_eq!(op.key_scale(6, 8), 0, "and off means off");
    }

    #[test]
    fn key_scale_rate_makes_high_notes_envelope_faster() {
        let mut op = Operator::new();
        op.key_scale_rate = true;
        assert!(op.key_scale_rate_bonus(6, 8) > op.key_scale_rate_bonus(1, 8));
        op.key_scale_rate = false;
        assert!(
            op.key_scale_rate_bonus(6, 8) < 6 * 2,
            "with the bit clear the effect is a quarter as strong"
        );
    }
}
