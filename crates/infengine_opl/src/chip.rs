//! A26.3: the YM3812 itself - registers, nine channels, and the mixer.
//!
//! The chip is written to by register number and value, exactly as a
//! DOS driver would have done through ports 0x388 and 0x389, and read
//! from by asking for samples. That interface is the point: a music
//! driver ported from the original can be a faithful port, because the
//! thing it is talking to answers the same way.

use crate::operator::{Operator, Phase};
use crate::tables::VIBRATO;

/// Operators per channel.
const OPS: usize = 2;
/// Melodic channels.
pub const CHANNELS: usize = 9;
/// Operators in total.
pub const OPERATORS: usize = CHANNELS * OPS;

/// The chip's own sample rate: 3.579545 MHz divided by 72.
pub const NATIVE_RATE: u32 = 49716;

/// Register offset to operator index. The chip's operators are laid out
/// in three groups of six with two unused slots between them, which is
/// why this cannot be arithmetic.
const OPERATOR_OF_OFFSET: [i8; 32] = [
    0, 1, 2, 3, 4, 5, -1, -1, 6, 7, 8, 9, 10, 11, -1, -1, 12, 13, 14, 15, 16, 17, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1,
];

/// One channel's pair of operators, in operator indices.
const CHANNEL_OPERATORS: [[usize; OPS]; CHANNELS] = [
    [0, 3],
    [1, 4],
    [2, 5],
    [6, 9],
    [7, 10],
    [8, 11],
    [12, 15],
    [13, 16],
    [14, 17],
];

#[derive(Debug, Clone, Default)]
struct Channel {
    /// Frequency number, 10 bits.
    fnum: u32,
    /// Octave, 0-7.
    block: u32,
    key_on: bool,
    /// Feedback amount, 0 = none.
    feedback: u8,
    /// True for additive (both operators heard), false for FM.
    additive: bool,
}

/// A YM3812 (OPL2).
#[derive(Debug, Clone)]
pub struct Opl2 {
    operators: Vec<Operator>,
    channels: [Channel; CHANNELS],
    /// Waveform select enable. Until a driver sets it, every operator
    /// is a plain sine no matter what the waveform registers say - a
    /// real quirk, and music written for it sounds wrong without it.
    waveform_select: bool,
    tremolo_deep: bool,
    vibrato_deep: bool,
    /// Low-frequency oscillator position, in samples.
    lfo: u32,
    /// Samples produced, only used to drive the LFO.
    registers: [u8; 256],
}

impl Default for Opl2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Opl2 {
    pub fn new() -> Self {
        Self {
            operators: (0..OPERATORS).map(|_| Operator::new()).collect(),
            channels: Default::default(),
            waveform_select: false,
            tremolo_deep: false,
            vibrato_deep: false,
            lfo: 0,
            registers: [0; 256],
        }
    }

    /// Put the chip back the way it powers up.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Last value written to a register, which a driver may read back.
    pub fn register(&self, index: u8) -> u8 {
        self.registers[index as usize]
    }

    /// Write a register, as a driver would through the chip's ports.
    pub fn write(&mut self, index: u8, value: u8) {
        self.registers[index as usize] = value;
        let group = index & 0xF0;
        let offset = (index & 0x1F) as usize;

        match index {
            0x01 => self.waveform_select = value & 0x20 != 0,
            0xBD => {
                self.tremolo_deep = value & 0x80 != 0;
                self.vibrato_deep = value & 0x40 != 0;
            }
            _ => match group {
                0x20 | 0x30 => self.write_control(offset, value),
                0x40 | 0x50 => self.write_level(offset, value),
                0x60 | 0x70 => self.write_attack_decay(offset, value),
                0x80 | 0x90 => self.write_sustain_release(offset, value),
                0xE0 | 0xF0 => self.write_waveform(offset, value),
                0xA0 => self.write_fnum_low(index, value),
                0xB0 => self.write_block_key(index, value),
                0xC0 => self.write_feedback(index, value),
                _ => {}
            },
        }
    }

    fn operator_index(offset: usize) -> Option<usize> {
        let index = OPERATOR_OF_OFFSET[offset & 0x1F];
        (index >= 0).then_some(index as usize)
    }

    fn write_control(&mut self, offset: usize, value: u8) {
        let Some(index) = Self::operator_index(offset) else {
            return;
        };
        let op = &mut self.operators[index];
        op.tremolo = value & 0x80 != 0;
        op.vibrato = value & 0x40 != 0;
        op.sustaining = value & 0x20 != 0;
        op.key_scale_rate = value & 0x10 != 0;
        // The register counts in half-steps, and 0 means one half - so
        // a multiple of 0 plays an octave *below* the note.
        op.multiple = match value & 0x0F {
            0 => 1,
            11 => 20,
            13 => 24,
            14 => 30,
            15 => 30,
            other => u32::from(other) * 2,
        };
    }

    fn write_level(&mut self, offset: usize, value: u8) {
        let Some(index) = Self::operator_index(offset) else {
            return;
        };
        let op = &mut self.operators[index];
        op.key_scale_level = value >> 6;
        op.total_level = u32::from(value & 0x3F);
    }

    fn write_attack_decay(&mut self, offset: usize, value: u8) {
        let Some(index) = Self::operator_index(offset) else {
            return;
        };
        let op = &mut self.operators[index];
        op.attack = value >> 4;
        op.decay = value & 0x0F;
    }

    fn write_sustain_release(&mut self, offset: usize, value: u8) {
        let Some(index) = Self::operator_index(offset) else {
            return;
        };
        let op = &mut self.operators[index];
        op.sustain = value >> 4;
        op.release = value & 0x0F;
    }

    fn write_waveform(&mut self, offset: usize, value: u8) {
        let Some(index) = Self::operator_index(offset) else {
            return;
        };
        self.operators[index].waveform = value & 0x03;
    }

    fn write_fnum_low(&mut self, index: u8, value: u8) {
        let Some(channel) = Self::channel_of(index) else {
            return;
        };
        let channel = &mut self.channels[channel];
        channel.fnum = (channel.fnum & 0x300) | u32::from(value);
    }

    fn write_block_key(&mut self, index: u8, value: u8) {
        let Some(channel_index) = Self::channel_of(index) else {
            return;
        };
        let was_on = self.channels[channel_index].key_on;
        {
            let channel = &mut self.channels[channel_index];
            channel.fnum = (channel.fnum & 0xFF) | (u32::from(value & 0x03) << 8);
            channel.block = u32::from((value >> 2) & 0x07);
            channel.key_on = value & 0x20 != 0;
        }
        let now_on = self.channels[channel_index].key_on;
        if now_on == was_on {
            return;
        }
        for op in CHANNEL_OPERATORS[channel_index] {
            if now_on {
                self.operators[op].key_on();
            } else {
                self.operators[op].key_off();
            }
        }
    }

    fn write_feedback(&mut self, index: u8, value: u8) {
        let Some(channel) = Self::channel_of(index) else {
            return;
        };
        let channel = &mut self.channels[channel];
        channel.feedback = (value >> 1) & 0x07;
        channel.additive = value & 0x01 != 0;
    }

    fn channel_of(index: u8) -> Option<usize> {
        let channel = (index & 0x0F) as usize;
        (channel < CHANNELS).then_some(channel)
    }

    /// Is anything still sounding?
    ///
    /// What a renderer needs to know when it has stopped feeding the
    /// chip and wants to stop writing a file.
    pub fn is_silent(&self) -> bool {
        self.operators.iter().all(Operator::is_silent)
    }

    /// Produce one sample at the chip's own rate.
    pub fn sample(&mut self) -> i32 {
        self.lfo = self.lfo.wrapping_add(1);
        // The two low-frequency oscillators: about 3.7 Hz for vibrato
        // and 3.1 Hz for tremolo, as the datasheet gives them.
        let vibrato_step = ((self.lfo / (NATIVE_RATE / 30)) & 7) as usize;
        let tremolo = self.tremolo_level();

        let mut mix = 0;
        for index in 0..CHANNELS {
            mix += self.channel_sample(index, vibrato_step, tremolo);
        }
        mix
    }

    /// Tremolo attenuation right now, in the envelope's units.
    fn tremolo_level(&self) -> u32 {
        // A triangle over about 3.7 Hz, 1 dB deep or 4.8 dB deep.
        let period = NATIVE_RATE / 4;
        let position = self.lfo % period;
        let up = position * 2 < period;
        let ramp = if up {
            position * 2
        } else {
            (period - position) * 2
        };
        let depth = if self.tremolo_deep { 26 } else { 6 };
        ramp * depth / period
    }

    fn channel_sample(&mut self, index: usize, vibrato_step: usize, tremolo: u32) -> i32 {
        let [modulator, carrier] = CHANNEL_OPERATORS[index];
        let channel = self.channels[index].clone();
        if self.operators[modulator].phase() == Phase::Off
            && self.operators[carrier].phase() == Phase::Off
        {
            return 0;
        }

        let fnum_high = channel.fnum >> 6;
        for op in [modulator, carrier] {
            let bonus = self.operators[op].key_scale_rate_bonus(channel.block, fnum_high);
            self.operators[op].tick_envelope(bonus);
            let increment = self.phase_increment(&channel, op, vibrato_step);
            self.operators[op].tick_phase(increment);
        }

        // Feedback is taken from the modulator's own last two outputs,
        // averaged - the chip smooths it, and without that the higher
        // feedback settings scream.
        let feedback = if channel.feedback == 0 {
            0
        } else {
            let history = self.operators[modulator].history;
            (history[0] + history[1]) >> (9 - channel.feedback)
        };

        let modulator_extra =
            self.extra_attenuation(modulator, &channel, fnum_high, tremolo);
        let modulated = self.operators[modulator].output(feedback, modulator_extra);

        let carrier_extra = self.extra_attenuation(carrier, &channel, fnum_high, tremolo);
        if channel.additive {
            // Both operators are heard; the modulator is not modulating.
            let carried = self.operators[carrier].output(0, carrier_extra);
            modulated + carried
        } else {
            self.operators[carrier].output(modulated >> 1, carrier_extra)
        }
    }

    fn extra_attenuation(
        &self,
        op: usize,
        channel: &Channel,
        fnum_high: u32,
        tremolo: u32,
    ) -> u32 {
        let operator = &self.operators[op];
        let mut extra = operator.key_scale(channel.block, fnum_high);
        if operator.tremolo {
            extra += tremolo;
        }
        extra
    }

    /// How far the phase moves in one sample.
    fn phase_increment(&self, channel: &Channel, op: usize, vibrato_step: usize) -> u32 {
        let mut fnum = channel.fnum;
        if self.operators[op].vibrato {
            let depth = usize::from(self.vibrato_deep);
            let cents = i32::from(VIBRATO[depth][vibrato_step]);
            // Vibrato is a fraction of the frequency number, which is
            // why its depth in cents changes with the octave - as on the
            // real chip.
            fnum = (fnum as i32 + cents * (fnum as i32 >> 7)).max(0) as u32;
        }
        let base = (fnum << channel.block) >> 1;
        base * self.operators[op].multiple / 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain loud sine on channel 0, keyed on.
    fn sine_chip() -> Opl2 {
        let mut chip = Opl2::new();
        chip.write(0x01, 0x20); // waveform select on
        chip.write(0x20, 0x01); // modulator: multiple 1
        chip.write(0x23, 0x01); // carrier: multiple 1
        chip.write(0x40, 0x3F); // modulator silent
        chip.write(0x43, 0x00); // carrier at full volume
        chip.write(0x60, 0xF0); // fastest attack
        chip.write(0x63, 0xF0);
        chip.write(0x80, 0x0F); // no decay, slow release
        chip.write(0x83, 0x0F);
        chip.write(0xC0, 0x01); // additive
        chip.write(0xA0, 0x98); // fnum
        chip.write(0xB0, 0x31); // block 4, key on
        chip
    }

    fn run(chip: &mut Opl2, samples: usize) -> Vec<i32> {
        (0..samples).map(|_| chip.sample()).collect()
    }

    #[test]
    fn a_chip_at_reset_is_silent() {
        let mut chip = Opl2::new();
        assert!(chip.is_silent());
        assert!(run(&mut chip, 512).iter().all(|s| *s == 0));
    }

    #[test]
    fn a_keyed_note_makes_a_sound() {
        let mut chip = sine_chip();
        let samples = run(&mut chip, 4096);
        let peak = samples.iter().map(|s| s.abs()).max().unwrap_or(0);
        assert!(peak > 100, "peak was {peak}");
        assert!(!chip.is_silent());
    }

    #[test]
    fn a_key_off_eventually_goes_quiet() {
        let mut chip = sine_chip();
        run(&mut chip, 4096);
        chip.write(0xB0, 0x11); // key off, same block
        // Release rate 15 is the fastest; give it a generous second.
        chip.write(0x83, 0xFF);
        run(&mut chip, NATIVE_RATE as usize);
        assert!(chip.is_silent(), "a released note has to stop");
    }

    #[test]
    fn a_higher_block_plays_a_higher_note() {
        // Count zero crossings, which is frequency without a Fourier
        // transform.
        fn crossings(block_bits: u8) -> usize {
            let mut chip = sine_chip();
            chip.write(0xB0, block_bits);
            let samples = run(&mut chip, 8192);
            samples
                .windows(2)
                .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
                .count()
        }
        let low = crossings(0x29); // block 2
        let high = crossings(0x31); // block 4
        assert!(high > low * 2, "{high} should be about four times {low}");
    }

    #[test]
    fn the_waveform_select_bit_really_gates_the_waveforms() {
        let mut chip = Opl2::new();
        chip.write(0xE3, 0x01); // ask for the half sine
        assert_eq!(
            chip.operators[3].waveform, 1,
            "the register still records it"
        );
        assert!(
            !chip.waveform_select,
            "but until the enable bit is set the chip plays a plain sine"
        );
        chip.write(0x01, 0x20);
        assert!(chip.waveform_select);
    }

    #[test]
    fn additive_and_fm_sound_different() {
        let mut fm = sine_chip();
        fm.write(0x40, 0x00); // modulator loud
        fm.write(0xC0, 0x00); // FM
        let fm_out = run(&mut fm, 4096);

        let mut additive = sine_chip();
        additive.write(0x40, 0x00);
        additive.write(0xC0, 0x01);
        let add_out = run(&mut additive, 4096);

        assert_ne!(fm_out, add_out, "the connection bit has to do something");
    }

    #[test]
    fn the_operator_map_covers_every_operator_exactly_once() {
        let mut seen = vec![0; OPERATORS];
        for offset in 0..32 {
            if let Some(index) = Opl2::operator_index(offset) {
                seen[index] += 1;
            }
        }
        assert!(
            seen.iter().all(|count| *count == 1),
            "each operator has exactly one register slot: {seen:?}"
        );
    }

    #[test]
    fn every_channels_operators_are_its_own() {
        let mut seen = vec![0; OPERATORS];
        for channel in CHANNEL_OPERATORS {
            for op in channel {
                seen[op] += 1;
            }
        }
        assert!(seen.iter().all(|count| *count == 1), "{seen:?}");
    }

    #[test]
    fn writing_a_register_out_of_range_does_not_panic() {
        let mut chip = Opl2::new();
        for index in 0..=255u8 {
            chip.write(index, 0xFF);
        }
        run(&mut chip, 64);
    }

    #[test]
    fn a_multiple_of_zero_plays_an_octave_down() {
        let mut chip = Opl2::new();
        chip.write(0x20, 0x00);
        assert_eq!(chip.operators[0].multiple, 1);
        chip.write(0x20, 0x01);
        assert_eq!(chip.operators[0].multiple, 2, "and one is the note itself");
    }

    #[test]
    fn tremolo_swings_and_comes_back() {
        let mut chip = Opl2::new();
        chip.write(0xBD, 0x80);
        let mut levels = Vec::new();
        for _ in 0..NATIVE_RATE / 2 {
            chip.lfo = chip.lfo.wrapping_add(1);
            levels.push(chip.tremolo_level());
        }
        let peak = levels.iter().copied().max().unwrap_or(0);
        let floor = levels.iter().copied().min().unwrap_or(0);
        assert!(peak > floor, "tremolo has to move: {floor}..{peak}");
        assert!(peak <= 26, "and stay within its depth");
    }
}
