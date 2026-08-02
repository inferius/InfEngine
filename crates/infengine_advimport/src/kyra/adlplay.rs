//! A54.1: playing an AdLib program, so an effect becomes audio.
//!
//! [`super::adl`] splits a program into steps and names them. This runs
//! them: it keeps one channel's worth of state, executes commands until
//! the channel says it is waiting, and writes to a YM3812 emulator. The
//! result is samples, and samples become a file the engine can play
//! without knowing any of this ever happened.
//!
//! **It is not finished.** The programs run, the control flow is right
//! and the timing is right, but almost everything comes out at a peak of
//! 8 in 32767 - which is silence with a shape. One track in a hundred
//! makes real sound, so the sequencer is reaching the chip and something
//! about the levels or the instrument load is wrong. Ruling the extra
//! levels out changed nothing, so it is the instrument itself or the
//! operator the level lands on. Written down rather than left as a
//! folder of quiet files nobody can explain.
//!
//! What it is not, beyond that, is a faithful driver. Kyrandia's has nine channels,
//! three kinds of ongoing effect, a rhythm section and a tempo system
//! that interacts with all of them. A sound effect is one channel
//! playing a handful of notes over about a second, and this does that
//! part properly and ignores the rest - each unimplemented command
//! consumes its arguments and is counted, so what is missing is a number
//! rather than a silence nobody can explain.
//!
//! Three facts about the format the whole thing rests on:
//!
//! * an instrument is eleven bytes, written to the pairs of operator
//!   registers in the order 0x20, 0xC0, 0xE0, 0x40, 0x60, 0x80;
//! * a note's low nibble indexes a twelve-entry table of F-numbers and
//!   its high nibble, plus the channel's base octave, is the block;
//! * the driver's timer runs 72 times a second, and every duration in a
//!   program is counted in those ticks.

use infengine_opl::{Opl2, render};

use super::adl::{self, Step};

/// Ticks of the driver's timer in a second.
pub const TICKS_PER_SECOND: f32 = 72.0;

/// Channel this renders on.
///
/// One effect at a time, in isolation, so which channel the game would
/// have picked does not matter - and channel 0's operator offsets are
/// the simplest to be right about.
const CHANNEL: u8 = 0;
/// Operator register offsets for channel 0: modulator, then carrier.
const OPERATORS: [u8; 2] = [0x00, 0x03];
/// Bytes an instrument occupies.
const INSTRUMENT_BYTES: usize = 11;
/// The registers those bytes go to, as pairs.
const INSTRUMENT_REGISTERS: [u8; 6] = [0x20, 0xC0, 0xE0, 0x40, 0x60, 0x80];

/// How long a program may run before it is called a loop.
///
/// A sound effect is about a second. A hundred seconds of one is a
/// program jumping to itself, which several of them do deliberately -
/// the music does, at least - and rendering it forever helps nobody.
const MAX_TICKS: usize = (TICKS_PER_SECOND as usize) * 100;
/// How many commands may run inside a single tick.
///
/// Control flow costs no time, so a jump to itself with nothing in
/// between never advances the clock at all.
const MAX_STEPS_PER_TICK: usize = 1000;

/// F-numbers for the twelve semitones of an octave.
///
/// Six of these are known: 0x0134, 0x0147, 0x015A, 0x016F, 0x0184,
/// 0x019C. The rest are the same equal temperament continued, and the
/// fit is not approximate - one base frequency reproduces all six known
/// values exactly when rounded, and these are what it gives for the
/// other six.
const FREQUENCIES: [u16; 12] = [
    0x0134, 0x0147, 0x015A, 0x016F, 0x0184, 0x019C, //
    0x01B4, 0x01CE, 0x01E9, 0x0206, 0x0225, 0x0246,
];

/// What a step did to the channel.
enum Outcome {
    /// Carry on with the next step in the same tick.
    Continue,
    /// Wait this many ticks before the next step.
    Wait(u32),
    /// The program is over.
    Stop,
}

/// One channel running one program.
struct Channel<'a> {
    code: &'a [u8],
    /// The file, for instruments a program asks for by number.
    file: Option<&'a adl::AdlFile<'a>>,
    /// Where the next step starts, in bytes.
    at: usize,
    /// Return addresses, for the subroutine call the format has.
    stack: Vec<usize>,
    /// Times round the loop still to go.
    repeat: u8,

    base_octave: u8,
    base_note: u8,
    base_freq: u8,
    /// The three levels a program can set, which add up into the
    /// operator's total level.
    extra: [u8; 3],
    /// Levels the instrument itself asked for, before the extras.
    instrument_level: [u8; 2],
    /// How long the current note lasts, and how much of that is silence
    /// at its end.
    duration: u8,
    spacing: u8,

    /// Commands run, and commands run that this does not implement.
    pub steps: usize,
    pub ignored: usize,
}

impl<'a> Channel<'a> {
    fn new(code: &'a [u8], file: Option<&'a adl::AdlFile<'a>>) -> Self {
        Self {
            code,
            file,
            at: 0,
            stack: Vec::new(),
            repeat: 0,
            base_octave: 0,
            base_note: 0,
            base_freq: 0,
            extra: [0; 3],
            instrument_level: [0x3F; 2],
            duration: 0,
            spacing: 0,
            steps: 0,
            ignored: 0,
        }
    }

    /// Read the next step and how many bytes it took.
    fn next_step(&self) -> Option<(Step, usize)> {
        let word = self.code.get(self.at..self.at + 2)?;
        let (first, second) = (word[0], word[1]);
        if first & 0x80 == 0 {
            return Some((
                Step::Note {
                    note: first,
                    duration: second,
                },
                2,
            ));
        }
        let opcode = (first & 0x7F).min(adl::MAX_OPCODE);
        let extra = adl::argument_bytes(opcode);
        // The dispatcher's word read already took one byte of argument,
        // so a command wanting one takes no more than the word.
        let taken = 2 + extra.saturating_sub(1);
        Some((
            Step::Command {
                opcode,
                argument: second,
            },
            taken,
        ))
    }

    /// A byte of a command's arguments beyond the one in the word.
    fn arg(&self, index: usize) -> u8 {
        self.code.get(self.at + 2 + index).copied().unwrap_or(0)
    }

    /// Where a jump's displacement points.
    ///
    /// The low byte is the one the dispatcher already read as the
    /// command's argument, and only the high byte follows - so a jump is
    /// three bytes, not four. Reading both bytes from after the word
    /// takes the high byte from whatever comes next and lands the jump
    /// somewhere arbitrary, which reads as a program that will not stop
    /// rather than as a misread displacement.
    fn target(&self, argument: u8, taken: usize) -> usize {
        let offset = argument as u16 | ((self.arg(0) as u16) << 8);
        // Relative to the end of the instruction, and signed.
        self.at
            .saturating_add(taken)
            .saturating_add_signed(offset as i16 as isize)
    }

    /// Push the instrument's registers, and remember its levels.
    fn load_instrument(&mut self, number: u8, chip: &mut Opl2) {
        // Instruments live in the same pointer array as the tracks, past
        // the 190 that are tracks - which is why the driver indexes them
        // at an offset rather than from the start.
        let Some(file) = self.file else { return };
        let Some(data) = file.instrument(number as usize) else {
            return;
        };
        if data.len() < INSTRUMENT_BYTES {
            return;
        }
        let mut byte = 0usize;
        for register in INSTRUMENT_REGISTERS {
            for (op, offset) in OPERATORS.iter().enumerate() {
                // Feedback and algorithm are one register for the whole
                // channel rather than one per operator.
                if register == 0xC0 {
                    if op == 0 {
                        chip.write(0xC0 + CHANNEL, data[byte]);
                        byte += 1;
                    }
                    continue;
                }
                let value = data[byte];
                byte += 1;
                if register == 0x40 {
                    self.instrument_level[op] = value & 0x3F;
                }
                chip.write(register + offset, value);
            }
        }
        self.apply_level(chip);
    }

    /// Write the carrier's total level from the instrument's and the
    /// program's.
    ///
    /// A level is attenuation, so louder is a smaller number and the
    /// extras add. Only the carrier is touched: raising the modulator
    /// changes the timbre rather than the volume, which is why a program
    /// that thinks it is fading would instead sound like a different
    /// instrument.
    fn apply_level(&self, chip: &mut Opl2) {
        let extra: u32 = self.extra.iter().map(|e| *e as u32).sum();
        let level = (self.instrument_level[1] as u32 + extra).min(0x3F) as u8;
        let keep = chip.register(0x40 + OPERATORS[1]) & 0xC0;
        chip.write(0x40 + OPERATORS[1], keep | level);
    }

    /// Start a note, or stop one.
    fn play_note(&mut self, note: u8, chip: &mut Opl2) {
        let semitone = (note & 0x0F) as usize % FREQUENCIES.len();
        let octave = ((note >> 4) as u32 + self.base_octave as u32) & 0x07;
        let frequency = FREQUENCIES[semitone] as u32 + self.base_freq as u32;
        chip.write(0xA0 + CHANNEL, (frequency & 0xFF) as u8);
        // Key on, with the block and the top two bits of the F-number.
        let high = ((frequency >> 8) & 0x03) as u8 | ((octave as u8) << 2) | 0x20;
        chip.write(0xB0 + CHANNEL, high);
    }

    fn note_off(&self, chip: &mut Opl2) {
        let held = chip.register(0xB0 + CHANNEL) & 0xDF;
        chip.write(0xB0 + CHANNEL, held);
    }

    /// Run one step.
    fn step(&mut self, chip: &mut Opl2) -> Outcome {
        let Some((step, taken)) = self.next_step() else {
            return Outcome::Stop;
        };
        self.steps += 1;
        let (note, opcode, argument) = match step {
            Step::Note { note, duration } => {
                self.at += taken;
                self.play_note(note.wrapping_add(self.base_note), chip);
                return Outcome::Wait(duration.max(1) as u32);
            }
            Step::Command { opcode, argument } => (0u8, opcode, argument),
            Step::Truncated => return Outcome::Stop,
        };
        let _ = note;

        match opcode {
            // Control flow.
            0x00 => {
                self.repeat = argument;
                self.at += taken;
            }
            0x01 => {
                let back = self.target(argument, taken);
                self.at += taken;
                if self.repeat > 0 {
                    self.repeat -= 1;
                    self.at = back;
                }
            }
            0x04 => self.at = self.target(argument, taken),
            0x05 => {
                let to = self.target(argument, taken);
                self.stack.push(self.at + taken);
                self.at = to;
            }
            0x06 => {
                self.at = match self.stack.pop() {
                    Some(back) => back,
                    None => return Outcome::Stop,
                };
            }
            0x08 | 0x4A => {
                self.note_off(chip);
                return Outcome::Stop;
            }

            // Waiting.
            0x09 => {
                self.at += taken;
                self.note_off(chip);
                return Outcome::Wait(argument.max(1) as u32);
            }
            0x1D => {
                self.at += taken;
                return Outcome::Wait(argument.max(1) as u32);
            }

            // Notes and their shape.
            0x03 => {
                self.spacing = argument;
                self.at += taken;
            }
            0x07 => {
                self.base_octave = argument;
                self.at += taken;
            }
            0x0B => {
                let duration = self.arg(0);
                self.at += taken;
                self.duration = duration;
                self.play_note(argument.wrapping_add(self.base_note), chip);
                return Outcome::Wait(duration.max(1) as u32);
            }
            0x0C => {
                self.base_note = argument;
                self.at += taken;
            }
            0x13 => {
                self.base_freq = argument;
                self.at += taken;
            }
            0x20 => {
                self.duration = argument;
                self.at += taken;
            }
            0x21 => {
                self.at += taken;
                self.play_note(argument.wrapping_add(self.base_note), chip);
                return Outcome::Wait(self.duration.max(1) as u32);
            }

            // Volume.
            0x1E => {
                self.extra[0] = argument;
                self.at += taken;
                self.apply_level(chip);
            }
            0x2C => {
                self.extra[1] = argument;
                self.at += taken;
                self.apply_level(chip);
            }
            0x2B => {
                self.extra[2] = argument;
                self.at += taken;
                self.apply_level(chip);
            }
            0x2D => {
                self.extra[1] = self.extra[1].wrapping_add(argument);
                self.at += taken;
                self.apply_level(chip);
            }
            0x30 => {
                self.extra[0] = self.extra[0].wrapping_add(argument);
                self.at += taken;
                self.apply_level(chip);
            }

            // The chip, directly.
            0x0A => {
                let value = self.arg(0);
                chip.write(argument, value);
                self.at += taken;
            }
            0x10 => {
                self.at += taken;
                self.load_instrument(argument, chip);
            }
            0x33 => {
                self.note_off(chip);
                self.extra = [0; 3];
                self.at += taken;
            }

            // Everything else: the ongoing effects, the rhythm section,
            // the tempo system. Counted rather than pretended away.
            _ => {
                self.ignored += 1;
                self.at += taken;
            }
        }
        Outcome::Continue
    }
}

/// What rendering a program produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// Mono samples at the rate asked for.
    pub samples: Vec<i16>,
    /// Commands executed.
    pub steps: usize,
    /// Of those, ones this does not implement.
    pub ignored: usize,
    /// Ticks the program ran for.
    pub ticks: usize,
}

impl Rendered {
    pub fn seconds(&self, rate: u32) -> f32 {
        self.samples.len() as f32 / rate.max(1) as f32
    }

    /// Did anything come out?
    pub fn is_silent(&self) -> bool {
        self.samples.iter().all(|s| *s == 0)
    }
}

/// Run a program and record what the chip makes of it.
pub fn render_program(
    code: &[u8],
    file: Option<&adl::AdlFile<'_>>,
    rate: u32,
) -> Rendered {
    let mut chip = Opl2::new();
    // A YM3812 starts with its waveform select disabled, and every
    // Westwood instrument assumes otherwise.
    chip.write(0x01, 0x20);
    let mut channel = Channel::new(code, file);
    let mut samples = Vec::new();
    let samples_per_tick = (rate as f32 / TICKS_PER_SECOND).max(1.0) as usize;

    let mut ticks = 0usize;
    'outer: while ticks < MAX_TICKS {
        // Commands until the channel wants to wait.
        let mut wait = 0u32;
        for _ in 0..MAX_STEPS_PER_TICK {
            match channel.step(&mut chip) {
                Outcome::Continue => continue,
                Outcome::Wait(t) => {
                    wait = t;
                    break;
                }
                Outcome::Stop => break 'outer,
            }
        }
        if wait == 0 {
            // Nothing asked to wait, so the program is spinning.
            break;
        }
        for _ in 0..wait {
            samples.extend(render(&mut chip, rate, samples_per_tick));
            ticks += 1;
            if ticks >= MAX_TICKS {
                break;
            }
        }
    }

    // Let the release phase finish rather than cutting the tail off.
    channel.note_off(&mut chip);
    samples.extend(infengine_opl::render_until_silent(&mut chip, rate, 2.0));

    Rendered {
        samples,
        steps: channel.steps,
        ignored: channel.ignored,
        ticks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command word: the opcode with its top bit, then its argument.
    fn cmd(opcode: u8, argument: u8) -> Vec<u8> {
        vec![0x80 | opcode, argument]
    }

    fn note(note: u8, duration: u8) -> Vec<u8> {
        vec![note & 0x7F, duration]
    }

    #[test]
    fn a_program_that_only_stops_makes_no_sound() {
        let out = render_program(&cmd(0x08, 0), None, 22050);
        assert!(out.is_silent());
        assert_eq!(out.ticks, 0);
    }

    #[test]
    fn a_note_takes_the_ticks_it_asks_for() {
        // Seventy-two ticks to the second, so a note of 36 is half of one.
        let mut code = note(0x20, 36);
        code.extend(cmd(0x08, 0));
        let out = render_program(&code, None, 22050);
        assert_eq!(out.ticks, 36);
        // The tail is the release, which is why this is not exactly half.
        assert!(out.seconds(22050) >= 0.5, "{}", out.seconds(22050));
    }

    #[test]
    fn a_loop_runs_the_body_the_number_of_times_it_was_given() {
        // setRepeat 2, note, checkRepeat back to the note, stop.
        let mut code = cmd(0x00, 2);
        code.extend(note(0x20, 1));
        // Three bytes: the word, then the displacement's high byte.
        // It is measured from the end of the instruction.
        let back = (-5i16) as u16;
        let mut check = cmd(0x01, (back & 0xFF) as u8);
        check.push((back >> 8) as u8);
        code.extend(check);
        code.extend(cmd(0x08, 0));

        let out = render_program(&code, None, 22050);
        assert_eq!(out.ticks, 3, "the body runs once plus twice more");
    }

    #[test]
    fn a_program_that_jumps_to_itself_is_stopped_rather_than_rendered_forever() {
        let back = (-3i16) as u16;
        let mut code = cmd(0x04, (back & 0xFF) as u8);
        code.push((back >> 8) as u8);
        let out = render_program(&code, None, 22050);
        assert!(out.samples.len() < 22050 * 2, "it ran away");
    }

    #[test]
    fn a_command_this_does_not_implement_is_counted_not_hidden() {
        // setupRhythmSection, which this does not do.
        let mut code = cmd(0x41, 0);
        code.extend_from_slice(&[0; 8]);
        code.extend(note(0x20, 1));
        code.extend(cmd(0x08, 0));
        let out = render_program(&code, None, 22050);
        assert_eq!(out.ignored, 1);
        assert!(out.steps >= 3);
    }

    #[test]
    fn writing_a_register_reaches_the_chip() {
        // The direct write is how a program reaches anything this does
        // not model, so it has to work even when nothing else does.
        let mut code = cmd(0x0A, 0x40);
        code.push(0x00); // level 0, which is as loud as an operator gets
        code.extend(note(0x20, 2));
        code.extend(cmd(0x08, 0));
        let out = render_program(&code, None, 22050);
        assert_eq!(out.ignored, 0);
        assert_eq!(out.ticks, 2);
    }

    #[test]
    fn every_semitone_has_a_frequency_and_they_rise() {
        // Six of these are known from the format; the rest continue the
        // same tempering, and a table that did not rise would be a typo
        // nobody would hear as one.
        assert_eq!(FREQUENCIES[0], 0x0134);
        assert_eq!(FREQUENCIES[5], 0x019C);
        for pair in FREQUENCIES.windows(2) {
            assert!(pair[1] > pair[0], "{pair:?}");
        }
        // An octave up is a doubling, so the twelfth step is close to it.
        let ratio = FREQUENCIES[11] as f32 / FREQUENCIES[0] as f32;
        assert!((ratio - 1.887).abs() < 0.01, "{ratio}");
    }
}
