//! A129: Westwood's AdLib driver, ported so the effects render as sound.
//!
//! [`super::adl`] names the file's parts; this executes them. It is a
//! port of the driver the game shipped, following ScummVM's documented
//! reimplementation of it (`drivers/adlib.cpp`) opcode for opcode,
//! because the two previous attempts at "just the parts an effect
//! needs" both produced silence with a shape. The parts an effect needs
//! turned out to be most of the driver:
//!
//! * **Ten channels, not one.** A track's program names a channel and a
//!   priority. Channel 9 is a control channel with no chip voice: the
//!   usual shape of an effect is a conductor on channel 9 whose
//!   `setupProgram` starts the audible voices on channels 6..8, so a
//!   single-channel player renders conductors - which are silent - and
//!   calls the voices unreachable.
//! * **The address space is the file after its 120-byte track table.**
//!   Program offsets are relative to that point, not to the end of some
//!   fixed-size pointer table; reading them any other way executes data
//!   as code. Instruments are programs numbered from
//!   [`adl::INSTRUMENT_BASE`] up.
//! * **Volume is a formula, not a byte.** An operator's total level is
//!   the instrument's level plus two extra levels plus a third scaled by
//!   the channel's volume modifier - and a modifier of zero means "off",
//!   which is exactly what a driver that never sets it renders.
//!
//! Timing is the original's: the driver ticks 72 times a second, each
//! channel advances an 8-bit timer by its tempo and acts when the timer
//! wraps, and note durations count those wraps. The chip is written
//! through [`infengine_opl`] and the samples come back mono at whatever
//! rate the caller wants.
//!
//! What is *not* here: the rhythm-section opcodes (65, 66, 68..70).
//! No program in any of the ten retail `.ADL` files issues them - the
//! census over every referenced track says so - and each is counted in
//! [`Rendered::ignored`] rather than silently skipped, so if a file
//! ever does use them the number says it before anyone's ears have to.

use infengine_opl::{Opl2, render};

use super::adl;

/// Ticks of the driver's timer in a second.
pub const TICKS_PER_SECOND: f32 = 72.0;

/// How long a program may run before it is called a loop.
///
/// A sound effect is seconds at most. A hundred seconds of one is a
/// program looping deliberately, which the music does and an effect
/// does not, and rendering it forever helps nobody.
const MAX_TICKS: usize = (TICKS_PER_SECOND as usize) * 100;
/// How many opcodes a channel may run inside a single tick.
///
/// Control flow costs no time, so a jump to itself with nothing in
/// between never advances the clock at all.
const MAX_STEPS_PER_TICK: usize = 10_000;

/// First operator's register offset per channel; the second is +3.
const REG_OFFSET: [u8; 9] = [0x00, 0x01, 0x02, 0x08, 0x09, 0x0A, 0x10, 0x11, 0x12];

/// F-numbers for the twelve semitones, from the driver.
const FREQ_TABLE: [u16; 12] = [
    0x0134, 0x0147, 0x015A, 0x016F, 0x0184, 0x019C, //
    0x01B4, 0x01CE, 0x01E9, 0x0207, 0x0225, 0x0246,
];

/// Frequency steps for a pitch bend, one table per semitone plus two.
///
/// A bend down indexes the note's own table, a bend up the one two
/// higher; the driver's data, not a synthesis.
const PITCH_BEND_TABLES: [[u8; 32]; 14] = [
    [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
        0x20, 0x21,
    ],
    [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x06, 0x07, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
        0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20,
        0x22, 0x24,
    ],
    [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x06, 0x08, 0x09, 0x0A, 0x0C, 0x0D, 0x0E, 0x0F, 0x11, 0x12,
        0x13, 0x14, 0x15, 0x16, 0x17, 0x19, 0x1A, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x24,
        0x25, 0x26,
    ],
    [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x06, 0x08, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x11, 0x12,
        0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x1A, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x23, 0x25,
        0x27, 0x28,
    ],
    [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x06, 0x08, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x11, 0x13,
        0x15, 0x16, 0x17, 0x18, 0x19, 0x1B, 0x1D, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26,
        0x28, 0x2A,
    ],
    [
        0x00, 0x01, 0x02, 0x03, 0x05, 0x07, 0x09, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x13,
        0x15, 0x16, 0x17, 0x18, 0x19, 0x1B, 0x1D, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x25, 0x27, 0x29,
        0x2B, 0x2D,
    ],
    [
        0x00, 0x01, 0x02, 0x03, 0x05, 0x07, 0x09, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x13,
        0x15, 0x16, 0x17, 0x18, 0x1A, 0x1C, 0x1E, 0x21, 0x24, 0x25, 0x26, 0x27, 0x29, 0x2B, 0x2D,
        0x2F, 0x30,
    ],
    [
        0x00, 0x01, 0x02, 0x04, 0x06, 0x08, 0x0A, 0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x13, 0x15,
        0x18, 0x19, 0x1A, 0x1C, 0x1D, 0x1F, 0x21, 0x23, 0x25, 0x26, 0x27, 0x29, 0x2B, 0x2D, 0x2F,
        0x30, 0x32,
    ],
    [
        0x00, 0x01, 0x02, 0x04, 0x06, 0x08, 0x0A, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x14, 0x17,
        0x1A, 0x19, 0x1A, 0x1C, 0x1E, 0x20, 0x22, 0x25, 0x28, 0x29, 0x2A, 0x2B, 0x2D, 0x2F, 0x31,
        0x33, 0x35,
    ],
    [
        0x00, 0x01, 0x03, 0x05, 0x07, 0x09, 0x0B, 0x0E, 0x0F, 0x10, 0x12, 0x14, 0x16, 0x18, 0x1A,
        0x1B, 0x1C, 0x1D, 0x1E, 0x20, 0x22, 0x24, 0x26, 0x29, 0x2A, 0x2C, 0x2E, 0x30, 0x32, 0x34,
        0x36, 0x39,
    ],
    [
        0x00, 0x01, 0x03, 0x05, 0x07, 0x09, 0x0B, 0x0E, 0x0F, 0x10, 0x12, 0x14, 0x16, 0x19, 0x1B,
        0x1E, 0x1F, 0x21, 0x23, 0x25, 0x27, 0x29, 0x2B, 0x2D, 0x2E, 0x2F, 0x31, 0x32, 0x34, 0x36,
        0x39, 0x3C,
    ],
    [
        0x00, 0x01, 0x03, 0x05, 0x07, 0x0A, 0x0C, 0x0F, 0x10, 0x11, 0x13, 0x15, 0x17, 0x19, 0x1B,
        0x1E, 0x1F, 0x20, 0x22, 0x24, 0x26, 0x28, 0x2B, 0x2E, 0x2F, 0x30, 0x32, 0x34, 0x36, 0x39,
        0x3C, 0x3F,
    ],
    [
        0x00, 0x02, 0x04, 0x06, 0x08, 0x0B, 0x0D, 0x10, 0x11, 0x12, 0x14, 0x16, 0x18, 0x1B, 0x1E,
        0x21, 0x22, 0x23, 0x25, 0x27, 0x29, 0x2C, 0x2F, 0x32, 0x33, 0x34, 0x36, 0x38, 0x3B, 0x34,
        0x41, 0x44,
    ],
    [
        0x00, 0x02, 0x04, 0x06, 0x08, 0x0B, 0x0D, 0x11, 0x12, 0x13, 0x15, 0x17, 0x1A, 0x1D, 0x20,
        0x23, 0x24, 0x25, 0x27, 0x29, 0x2C, 0x2F, 0x32, 0x35, 0x36, 0x37, 0x39, 0x3B, 0x3E, 0x41,
        0x44, 0x47,
    ],
];

/// The ongoing per-tick effect a channel may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Effect {
    #[default]
    None,
    Slide,
    Vibrato,
}

/// One of the driver's ten channels.
///
/// Nine correspond to the chip's voices; the tenth (index 9) is the
/// control channel, which parses but never sounds.
#[derive(Debug, Clone, Copy, Default)]
struct Channel {
    /// Read position in the sound data; `None` is a stopped channel.
    pos: Option<usize>,
    stack: [usize; 4],
    stack_len: usize,
    repeat: u8,
    priority: u8,

    duration: u8,
    spacing1: u8,
    spacing2: u8,
    fractional_spacing: u8,
    duration_randomness: u8,

    base_octave: u8,
    base_note: u8,
    base_freq: u8,
    raw_note: u8,
    pitch_bend: i8,

    tempo: u8,
    timer: u8,
    tempo_reset: bool,

    /// The driver's copies of the channel's frequency registers.
    reg_ax: u8,
    reg_bx: u8,

    /// Whether the algorithm makes both operators carry volume.
    two_chan: bool,
    op_level1: u8,
    op_level2: u8,
    op_extra_level1: u8,
    op_extra_level2: u8,
    op_extra_level3: u8,
    volume_modifier: u8,

    primary_effect: Effect,
    slide_tempo: u8,
    slide_timer: u8,
    slide_step: i16,
    vibrato_tempo: u8,
    vibrato_timer: u8,
    vibrato_step_range: u8,
    vibrato_steps_countdown: u8,
    vibrato_num_steps: u8,
    vibrato_delay: u8,
    vibrato_delay_countdown: u8,
    vibrato_step: i16,

    secondary_effect: bool,
    secondary_timer: u8,
    secondary_tempo: u8,
    secondary_size: i16,
    secondary_pos: i16,
    secondary_regbase: u8,
    secondary_data: i32,
}

/// The driver: ten channels over one address space and one chip.
struct Driver<'a> {
    sound: &'a [u8],
    chip: Opl2,
    channels: [Channel; 10],
    /// Channel the parser is acting for right now.
    cur: usize,
    tempo: u8,
    vibrato_am_depth_bits: u8,
    rnd: u16,
    /// The beat system a program can synchronize on.
    callback_timer: u8,
    beat_divider: u8,
    beat_div_cnt: u8,
    beat_counter: u8,
    beat_waiting: u8,

    steps: usize,
    ignored: usize,
}

/// Advance an 8-bit timer by a tempo; acting happens on the wrap.
fn advance(timer: &mut u8, tempo: u8) -> bool {
    let old = *timer;
    *timer = timer.wrapping_add(tempo);
    *timer < old
}

impl<'a> Driver<'a> {
    fn new(sound: &'a [u8]) -> Self {
        Self {
            sound,
            chip: Opl2::new(),
            channels: [Channel::default(); 10],
            cur: 0,
            tempo: 0,
            vibrato_am_depth_bits: 0,
            rnd: 0x1234,
            callback_timer: 0xFF,
            beat_divider: 0,
            beat_div_cnt: 0,
            beat_counter: 0,
            beat_waiting: 0,
            steps: 0,
            ignored: 0,
        }
    }

    /// A position `n` bytes further, if it is still inside the data.
    fn check_offset(&self, base: usize, n: i64) -> Option<usize> {
        let to = base as i64 + n;
        (0..self.sound.len() as i64).contains(&to).then_some(to as usize)
    }

    /// Where a program's bytes start: channel, priority, then the stream.
    fn program(&self, id: usize) -> Option<usize> {
        let at = id.checked_mul(2)?;
        let offset =
            u16::from_le_bytes([*self.sound.get(at)?, *self.sound.get(at + 1)?]);
        if offset == 0 || offset == 0xFFFF {
            return None;
        }
        ((offset as usize) < self.sound.len()).then_some(offset as usize)
    }

    fn instrument(&self, id: u8) -> Option<usize> {
        self.program(adl::INSTRUMENT_BASE + id as usize)
    }

    fn write_opl(&mut self, reg: u8, value: u8) {
        self.chip.write(reg, value);
    }

    /// The chip as the driver leaves it before anything plays.
    fn reset_adlib_state(&mut self) {
        self.rnd = 0x1234;
        // Authorize control of the waveforms.
        self.write_opl(0x01, 0x20);
        // FM music mode, rhythm off: nine melodic voices.
        self.write_opl(0x08, 0x00);
        self.write_opl(0xBD, 0x00);
        init_channel(&mut self.channels[9]);
        for chan in (0..9).rev() {
            self.write_opl(0x40 + REG_OFFSET[chan], 0x3F);
            self.write_opl(0x43 + REG_OFFSET[chan], 0x3F);
            init_channel(&mut self.channels[chan]);
        }
    }

    /// Start a program the way `setupPrograms` does: only if its
    /// priority beats what its channel is already playing.
    fn start_program(&mut self, id: usize) -> bool {
        let Some(at) = self.program(id) else {
            return false;
        };
        if at + 2 > self.sound.len() {
            return false;
        }
        let chan = self.sound[at] as usize;
        let priority = self.sound[at + 1];
        if chan > 9 {
            return false;
        }
        let channel = &mut self.channels[chan];
        if priority < channel.priority {
            return false;
        }
        init_channel(channel);
        channel.priority = priority;
        channel.pos = Some(at + 2);
        channel.tempo = 0xFF;
        channel.timer = 0xFF;
        channel.duration = 1;
        // Every effect renders at full volume; the game's mixer is
        // where a quieter setting would live.
        channel.volume_modifier = 0xFF;
        self.init_adlib_channel(chan);
        true
    }

    /// Clear a chip voice before a program takes it over.
    fn init_adlib_channel(&mut self, chan: usize) {
        if chan >= 9 {
            return;
        }
        let offset = REG_OFFSET[chan];
        self.write_opl(0x60 + offset, 0xFF);
        self.write_opl(0x63 + offset, 0xFF);
        self.write_opl(0x80 + offset, 0xFF);
        self.write_opl(0x83 + offset, 0xFF);
        self.write_opl(0xB0 + chan as u8, 0x00);
        // The original turns the note back on with everything but the
        // low frequency byte zeroed; kept because the driver's own
        // state tracking assumes it happened.
        self.write_opl(0xB0 + chan as u8, 0x20);
    }

    fn get_random(&mut self) -> u16 {
        self.rnd = self.rnd.wrapping_add(0x9248);
        let low = self.rnd & 7;
        self.rnd = (self.rnd >> 3) | (low << 13);
        self.rnd
    }

    /// Note number and octave to F-number, into the channel's registers.
    fn setup_note(&mut self, raw_note: u8, flag: bool) {
        if self.cur >= 9 {
            return;
        }
        let cur = self.cur;
        self.channels[cur].raw_note = raw_note;

        let mut note =
            ((raw_note & 0x0F) as i8).wrapping_add(self.channels[cur].base_note as i8) as i32;
        let mut octave = (((raw_note as i32 + self.channels[cur].base_octave as i32) >> 4)
            & 0x0F) as i8 as i32;

        // Twelve notes; anything outside moves the octave.
        if note >= 12 {
            octave += note / 12;
            note %= 12;
        } else if note < 0 {
            let octaves = -(note + 1) / 12 + 1;
            octave -= octaves;
            note += 12 * octaves;
        }

        let mut freq = FREQ_TABLE[note as usize] as i32 + self.channels[cur].base_freq as i32;

        let bend = self.channels[cur].pitch_bend;
        if bend != 0 || flag {
            let index_note = (raw_note & 0x0F).min(11) as usize;
            if bend >= 0 {
                let table = &PITCH_BEND_TABLES[index_note + 2];
                freq += table[(bend as i32).clamp(0, 31) as usize] as i32;
            } else {
                let table = &PITCH_BEND_TABLES[index_note];
                freq -= table[(-(bend as i32)).clamp(0, 31) as usize] as i32;
            }
        }

        // Update octave and frequency, but keep the on/off state.
        let channel = &mut self.channels[cur];
        channel.reg_ax = (freq & 0xFF) as u8;
        channel.reg_bx =
            ((channel.reg_bx as i32 & 0x20) | (octave << 2) | ((freq >> 8) & 0x03)) as u8;
        let (ax, bx) = (channel.reg_ax, channel.reg_bx);
        self.write_opl(0xA0 + cur as u8, ax);
        self.write_opl(0xB0 + cur as u8, bx);
    }

    /// Key on, and prime the vibrato variables for the new note.
    fn note_on(&mut self) {
        if self.cur >= 9 {
            return;
        }
        let cur = self.cur;
        self.channels[cur].reg_bx |= 0x20;
        let bx = self.channels[cur].reg_bx;
        self.write_opl(0xB0 + cur as u8, bx);

        let channel = &mut self.channels[cur];
        let shift = 9 - (channel.vibrato_step_range.min(9) as i32);
        let freq = (((channel.reg_bx as u16) << 8) | channel.reg_ax as u16) & 0x3FF;
        channel.vibrato_step = ((freq >> shift) & 0xFF) as i16;
        channel.vibrato_delay_countdown = channel.vibrato_delay;
    }

    fn note_off(&mut self) {
        if self.cur >= 9 {
            return;
        }
        let cur = self.cur;
        self.channels[cur].reg_bx &= 0xDF;
        let bx = self.channels[cur].reg_bx;
        self.write_opl(0xB0 + cur as u8, bx);
    }

    fn setup_duration(&mut self, duration: u8) {
        let channel = &mut self.channels[self.cur];
        if channel.duration_randomness != 0 {
            let extra = (self.get_random() & self.channels[self.cur].duration_randomness as u16)
                as u8;
            self.channels[self.cur].duration = duration.wrapping_add(extra);
            return;
        }
        if channel.fractional_spacing != 0 {
            channel.spacing2 = (duration >> 3).wrapping_mul(channel.fractional_spacing);
        }
        channel.duration = duration;
    }

    /// The instrument's eleven bytes to the channel's two operators.
    fn setup_instrument(&mut self, at: usize) {
        if self.cur >= 9 {
            return;
        }
        let Some(data) = self.sound.get(at..at + 11) else {
            return;
        };
        let data: [u8; 11] = data.try_into().expect("sliced to length");
        let reg = REG_OFFSET[self.cur];
        let cur = self.cur as u8;
        // Characteristics for both operators, byte-interleaved.
        self.write_opl(0x20 + reg, data[0]);
        self.write_opl(0x23 + reg, data[1]);
        // Feedback and algorithm; the algorithm bit decides whether
        // both operators carry volume or only the carrier.
        self.write_opl(0xC0 + cur, data[2]);
        self.channels[self.cur].two_chan = data[2] & 1 != 0;
        // Waveforms.
        self.write_opl(0xE0 + reg, data[3]);
        self.write_opl(0xE3 + reg, data[4]);
        self.channels[self.cur].op_level1 = data[5];
        self.channels[self.cur].op_level2 = data[6];
        let l1 = self.calculate_op_level1();
        let l2 = self.calculate_op_level2();
        self.write_opl(0x40 + reg, l1);
        self.write_opl(0x43 + reg, l2);
        // Attack/decay, then sustain/release.
        self.write_opl(0x60 + reg, data[7]);
        self.write_opl(0x63 + reg, data[8]);
        self.write_opl(0x80 + reg, data[9]);
        self.write_opl(0x83 + reg, data[10]);
    }

    /// The modulator's total level - volume only in additive voices.
    fn calculate_op_level1(&self) -> u8 {
        let channel = &self.channels[self.cur];
        let mut value = channel.op_level1 & 0x3F;
        if channel.two_chan {
            value = value.wrapping_add(channel.op_extra_level1);
            value = value.wrapping_add(channel.op_extra_level2);
            let mut level3 =
                ((channel.op_extra_level3 ^ 0x3F) as u16) * channel.volume_modifier as u16;
            if level3 != 0 {
                level3 += 0x3F;
                level3 >>= 8;
            }
            value = value.wrapping_add((level3 as u8) ^ 0x3F);
        }
        // The original clips exactly like this, overflow bugs included.
        value = value.min(0x3F);
        if channel.volume_modifier == 0 {
            value = 0x3F;
        }
        value | (channel.op_level1 & 0xC0)
    }

    /// The carrier's total level: instrument plus the extra levels.
    fn calculate_op_level2(&self) -> u8 {
        let channel = &self.channels[self.cur];
        let mut value = channel.op_level2 & 0x3F;
        value = value.wrapping_add(channel.op_extra_level1);
        value = value.wrapping_add(channel.op_extra_level2);
        let mut level3 =
            ((channel.op_extra_level3 ^ 0x3F) as u16) * channel.volume_modifier as u16;
        if level3 != 0 {
            level3 += 0x3F;
            level3 >>= 8;
        }
        value = value.wrapping_add((level3 as u8) ^ 0x3F);
        value = value.min(0x3F);
        if channel.volume_modifier == 0 {
            value = 0x3F;
        }
        value | (channel.op_level2 & 0xC0)
    }

    fn adjust_volume(&mut self) {
        if self.cur >= 9 {
            return;
        }
        let reg = REG_OFFSET[self.cur];
        let l2 = self.calculate_op_level2();
        self.write_opl(0x43 + reg, l2);
        if self.channels[self.cur].two_chan {
            let l1 = self.calculate_op_level1();
            self.write_opl(0x40 + reg, l1);
        }
    }

    /// The frequency slide, run once per tick while it is active.
    fn primary_effect_slide(&mut self) {
        if self.cur >= 9 {
            return;
        }
        let cur = self.cur;
        let channel = &mut self.channels[cur];
        if !advance(&mut channel.slide_timer, channel.slide_tempo) {
            return;
        }
        let mut freq =
            (((channel.reg_bx & 0x03) as i16) << 8) | channel.reg_ax as i16;
        let mut octave = channel.reg_bx & 0x1C;
        let note_on = channel.reg_bx & 0x20;

        freq += channel.slide_step.clamp(-0x3FF, 0x3FF);
        if channel.slide_step >= 0 && freq >= 734 {
            // Too high: halve it and go up an octave.
            freq >>= 1;
            if freq & 0x3FF == 0 {
                freq += 1;
            }
            octave = octave.wrapping_add(4);
        } else if channel.slide_step < 0 && freq < 388 {
            // Too low: double it and go down an octave.
            if freq < 0 {
                freq = 0;
            }
            freq <<= 1;
            if freq & 0x3FF == 0 {
                freq -= 1;
            }
            octave = octave.wrapping_sub(4);
        }
        channel.reg_ax = (freq & 0xFF) as u8;
        channel.reg_bx = note_on | (octave & 0x1C) | (((freq >> 8) & 0x03) as u8);
        let (ax, bx) = (channel.reg_ax, channel.reg_bx);
        self.write_opl(0xA0 + cur as u8, ax);
        self.write_opl(0xB0 + cur as u8, bx);
    }

    /// The vibrato, run once per tick while it is active.
    fn primary_effect_vibrato(&mut self) {
        if self.cur >= 9 {
            return;
        }
        let cur = self.cur;
        let channel = &mut self.channels[cur];
        // A new note delays the effect.
        if channel.vibrato_delay_countdown != 0 {
            channel.vibrato_delay_countdown -= 1;
            return;
        }
        if advance(&mut channel.vibrato_timer, channel.vibrato_tempo) {
            // Reverse direction every so many updates.
            channel.vibrato_steps_countdown = channel.vibrato_steps_countdown.wrapping_sub(1);
            if channel.vibrato_steps_countdown == 0 {
                channel.vibrato_step = -channel.vibrato_step;
                channel.vibrato_steps_countdown = channel.vibrato_num_steps;
            }
            let freq = ((((channel.reg_bx as u16) << 8) | channel.reg_ax as u16) & 0x3FF)
                .wrapping_add(channel.vibrato_step as u16);
            channel.reg_ax = (freq & 0xFF) as u8;
            channel.reg_bx = (channel.reg_bx & 0xFC) | ((freq >> 8) as u8);
            let (ax, bx) = (channel.reg_ax, channel.reg_bx);
            self.write_opl(0xA0 + cur as u8, ax);
            self.write_opl(0xB0 + cur as u8, bx);
        }
    }

    /// The secondary effect: an operator register modulated from a
    /// table inside the sound data itself.
    fn secondary_effect_1(&mut self) {
        if self.cur >= 9 {
            return;
        }
        let cur = self.cur;
        let channel = &mut self.channels[cur];
        if advance(&mut channel.secondary_timer, channel.secondary_tempo) {
            channel.secondary_pos -= 1;
            if channel.secondary_pos < 0 {
                channel.secondary_pos = channel.secondary_size;
            }
            let index = channel.secondary_data + channel.secondary_pos as i32;
            let regbase = channel.secondary_regbase;
            let value = usize::try_from(index)
                .ok()
                .and_then(|index| self.sound.get(index).copied());
            if let Some(value) = value {
                let reg = regbase.wrapping_add(REG_OFFSET[cur]);
                self.write_opl(reg, value);
            }
        }
    }

    /// One opcode. The return value is the parser's: zero keeps
    /// parsing, anything else ends the channel's turn this tick.
    fn run_opcode(&mut self, opcode: u8, at: usize) -> i32 {
        let cur = self.cur;
        let value = |i: usize| self.sound.get(at + i).copied().unwrap_or(0);
        let value16_le = |i: usize| i16::from_le_bytes([value(i), value(i + 1)]);
        let value16_be = |i: usize| i16::from_be_bytes([value(i), value(i + 1)]);
        match opcode {
            // setRepeat
            0 => {
                self.channels[cur].repeat = value(0);
                0
            }
            // checkRepeat: jump back while the counter is not spent.
            1 => {
                self.channels[cur].repeat = self.channels[cur].repeat.wrapping_sub(1);
                if self.channels[cur].repeat != 0
                    && let Some(pos) = self.channels[cur].pos
                    && let Some(to) = self.check_offset(pos, value16_le(0) as i64)
                {
                    self.channels[cur].pos = Some(to);
                }
                0
            }
            // setupProgram: start another program on its own channel.
            2 => {
                let id = value(0);
                if id == 0xFF {
                    return 0;
                }
                let Some(target) = self.program(id as usize) else {
                    return 0;
                };
                if target + 2 > self.sound.len() {
                    return 0;
                }
                let chan = self.sound[target] as usize;
                if chan > 9 {
                    return 0;
                }
                if self.sound[target + 1] >= self.channels[chan].priority {
                    // Starting a program must not move this channel's
                    // own read position, even onto itself.
                    let own = self.channels[cur].pos;
                    let priority = self.sound[target + 1];
                    init_channel(&mut self.channels[chan]);
                    self.channels[chan].priority = priority;
                    self.channels[chan].pos = Some(target + 2);
                    self.channels[chan].tempo = 0xFF;
                    self.channels[chan].timer = 0xFF;
                    self.channels[chan].duration = 1;
                    self.channels[chan].volume_modifier = 0xFF;
                    self.init_adlib_channel(chan);
                    self.channels[cur].pos = own;
                }
                0
            }
            // setNoteSpacing
            3 => {
                self.channels[cur].spacing1 = value(0);
                0
            }
            // jump, relative to after the displacement.
            4 => {
                let to = self.channels[cur]
                    .pos
                    .and_then(|pos| self.check_offset(pos, value16_le(0) as i64));
                match to {
                    Some(to) => {
                        self.channels[cur].pos = Some(to);
                        0
                    }
                    None => self.stop_channel(),
                }
            }
            // jumpToSubroutine
            5 => {
                let channel = &mut self.channels[cur];
                if channel.stack_len >= channel.stack.len() {
                    return 0;
                }
                let Some(pos) = channel.pos else { return 0 };
                channel.stack[channel.stack_len] = pos;
                channel.stack_len += 1;
                match self.check_offset(pos, value16_le(0) as i64) {
                    Some(to) => self.channels[cur].pos = Some(to),
                    None => {
                        self.channels[cur].stack_len -= 1;
                    }
                }
                0
            }
            // returnFromSubroutine
            6 => {
                let channel = &mut self.channels[cur];
                if channel.stack_len == 0 {
                    return self.stop_channel();
                }
                channel.stack_len -= 1;
                channel.pos = Some(channel.stack[channel.stack_len]);
                0
            }
            // setBaseOctave
            7 => {
                self.channels[cur].base_octave = value(0);
                0
            }
            // playRest: silence for a duration.
            9 => {
                self.setup_duration(value(0));
                self.note_off();
                (value(0) != 0) as i32
            }
            // writeAdLib
            10 => {
                self.write_opl(value(0), value(1));
                0
            }
            // setupNoteAndDuration
            11 => {
                self.setup_note(value(0), false);
                self.setup_duration(value(1));
                (value(1) != 0) as i32
            }
            // setBaseNote
            12 => {
                self.channels[cur].base_note = value(0);
                0
            }
            // setupSecondaryEffect1
            13 => {
                let channel = &mut self.channels[cur];
                channel.secondary_timer = value(0);
                channel.secondary_tempo = value(0);
                channel.secondary_size = value(1) as i16;
                channel.secondary_pos = value(1) as i16;
                channel.secondary_regbase = value(2);
                // The offset the sequencer assumes; see ScummVM's note
                // on the xlat bug the original drivers share.
                channel.secondary_data = value16_le(3) as i32 - 191;
                channel.secondary_effect = true;
                let start = channel.secondary_data + channel.secondary_size as i32;
                if start < 0 || start as usize >= self.sound.len() {
                    channel.secondary_effect = false;
                }
                0
            }
            // stopOtherChannel
            14 => {
                let target = value(0) as usize;
                if target > 9 {
                    return 0;
                }
                let own = self.channels[cur].pos;
                self.channels[target].duration = 0;
                self.channels[target].priority = 0;
                self.channels[target].pos = None;
                self.channels[cur].pos = own;
                0
            }
            // waitForEndOfProgram: re-run this opcode until the other
            // program's channel goes quiet.
            15 => {
                let Some(target) = self.program(value(0) as usize) else {
                    return 0;
                };
                let chan = self.sound[target] as usize;
                if chan > 9 || self.channels[chan].pos.is_none() {
                    return 0;
                }
                if let Some(pos) = self.channels[cur].pos {
                    self.channels[cur].pos = Some(pos.saturating_sub(2));
                }
                2
            }
            // setupInstrument
            16 => {
                if let Some(instrument) = self.instrument(value(0)) {
                    self.setup_instrument(instrument);
                }
                0
            }
            // setupPrimaryEffectSlide; the step is big-endian.
            17 => {
                let channel = &mut self.channels[cur];
                channel.slide_tempo = value(0);
                channel.slide_step = value16_be(1);
                channel.primary_effect = Effect::Slide;
                channel.slide_timer = 0xFF;
                0
            }
            // removePrimaryEffectSlide
            18 => {
                let channel = &mut self.channels[cur];
                channel.primary_effect = Effect::None;
                channel.slide_step = 0;
                0
            }
            // setBaseFreq
            19 => {
                self.channels[cur].base_freq = value(0);
                0
            }
            // setupPrimaryEffectVibrato
            21 => {
                let channel = &mut self.channels[cur];
                channel.vibrato_tempo = value(0);
                channel.vibrato_step_range = value(1);
                channel.vibrato_steps_countdown = value(2).wrapping_add(1);
                channel.vibrato_num_steps = value(2) << 1;
                channel.vibrato_delay = value(3);
                channel.primary_effect = Effect::Vibrato;
                0
            }
            // setPriority
            26 => {
                self.channels[cur].priority = value(0);
                0
            }
            // setBeat
            28 => {
                self.beat_divider = value(0) >> 1;
                self.beat_div_cnt = self.beat_divider;
                self.callback_timer = 0xFF;
                self.beat_counter = 0;
                self.beat_waiting = 0;
                0
            }
            // waitForNextBeat
            29 => {
                if self.beat_counter & value(0) != 0 && self.beat_waiting != 0 {
                    self.beat_waiting = 0;
                    return 0;
                }
                if self.beat_counter & value(0) == 0 {
                    self.beat_waiting = self.beat_waiting.wrapping_add(1);
                }
                if let Some(pos) = self.channels[cur].pos {
                    self.channels[cur].pos = Some(pos.saturating_sub(2));
                }
                self.channels[cur].duration = 1;
                2
            }
            // setExtraLevel1
            30 => {
                self.channels[cur].op_extra_level1 = value(0);
                self.adjust_volume();
                0
            }
            // setupDuration
            32 => {
                self.setup_duration(value(0));
                (value(0) != 0) as i32
            }
            // playNote: the note already set up, for a fresh duration.
            33 => {
                self.setup_duration(value(0));
                self.note_on();
                (value(0) != 0) as i32
            }
            // setFractionalNoteSpacing
            36 => {
                self.channels[cur].fractional_spacing = value(0) & 7;
                0
            }
            // setTempo
            38 => {
                self.tempo = value(0);
                0
            }
            // removeSecondaryEffect1
            39 => {
                self.channels[cur].secondary_effect = false;
                0
            }
            // setChannelTempo
            41 => {
                self.channels[cur].tempo = value(0);
                0
            }
            // setExtraLevel3
            43 => {
                self.channels[cur].op_extra_level3 = value(0);
                0
            }
            // setExtraLevel2, on whichever channel it names.
            44 => {
                let target = value(0) as usize;
                if target > 9 {
                    return 0;
                }
                let backup = self.cur;
                self.cur = target;
                self.channels[target].op_extra_level2 = value(1);
                self.adjust_volume();
                self.cur = backup;
                0
            }
            // changeExtraLevel2
            45 => {
                let target = value(0) as usize;
                if target > 9 {
                    return 0;
                }
                let backup = self.cur;
                self.cur = target;
                self.channels[target].op_extra_level2 =
                    self.channels[target].op_extra_level2.wrapping_add(value(1));
                self.adjust_volume();
                self.cur = backup;
                0
            }
            // setAMDepth
            46 => {
                if value(0) & 1 != 0 {
                    self.vibrato_am_depth_bits |= 0x80;
                } else {
                    self.vibrato_am_depth_bits &= 0x7F;
                }
                let bits = self.vibrato_am_depth_bits;
                self.write_opl(0xBD, bits);
                0
            }
            // setVibratoDepth
            47 => {
                if value(0) & 1 != 0 {
                    self.vibrato_am_depth_bits |= 0x40;
                } else {
                    self.vibrato_am_depth_bits &= 0xBF;
                }
                let bits = self.vibrato_am_depth_bits;
                self.write_opl(0xBD, bits);
                0
            }
            // changeExtraLevel1
            48 => {
                self.channels[cur].op_extra_level1 =
                    self.channels[cur].op_extra_level1.wrapping_add(value(0));
                self.adjust_volume();
                0
            }
            // clearChannel: stop it and silence its voice.
            51 => {
                let target = value(0) as usize;
                if target > 9 {
                    return 0;
                }
                let own = self.channels[cur].pos;
                self.channels[target].duration = 0;
                self.channels[target].priority = 0;
                self.channels[target].pos = None;
                self.channels[target].op_extra_level2 = 0;
                if target != 9 {
                    let reg = REG_OFFSET[target];
                    self.write_opl(0xC0 + target as u8, 0x00);
                    self.write_opl(0x43 + reg, 0x3F);
                    self.write_opl(0x83 + reg, 0xFF);
                    self.write_opl(0xB0 + target as u8, 0x00);
                }
                self.channels[cur].pos = own;
                0
            }
            // changeNoteRandomly
            53 => {
                if cur >= 9 {
                    return 0;
                }
                let mask = value16_be(0) as u16;
                let channel = &self.channels[cur];
                let mut note =
                    (((channel.reg_bx & 0x1F) as u16) << 8) | channel.reg_ax as u16;
                note = note.wrapping_add(mask & self.get_random());
                note |= ((self.channels[cur].reg_bx & 0x20) as u16) << 8;
                self.write_opl(0xA0 + cur as u8, (note & 0xFF) as u8);
                self.write_opl(0xB0 + cur as u8, (note >> 8) as u8);
                0
            }
            // removePrimaryEffectVibrato
            54 => {
                self.channels[cur].primary_effect = Effect::None;
                0
            }
            // pitchBend
            57 => {
                self.channels[cur].pitch_bend = value(0) as i8;
                let raw = self.channels[cur].raw_note;
                self.setup_note(raw, true);
                0
            }
            // resetToGlobalTempo
            58 => {
                self.channels[cur].tempo = self.tempo;
                0
            }
            // nop slots the table really has.
            59 | 64 => 0,
            // setDurationRandomness
            60 => {
                self.channels[cur].duration_randomness = value(0);
                0
            }
            // changeChannelTempo
            61 => {
                let tempo =
                    (self.channels[cur].tempo as i32 + value(0) as i8 as i32).clamp(1, 255);
                self.channels[cur].tempo = tempo as u8;
                0
            }
            // removeRhythmSection: nothing sets one up (no retail
            // program does), so only the mode bits need clearing.
            67 => {
                let bits = self.vibrato_am_depth_bits;
                self.write_opl(0xBD, bits & 0xC0);
                0
            }
            // setSoundTrigger: the game reads it; rendering has no one
            // to tell.
            71 => 0,
            // setTempoReset
            72 => {
                self.channels[cur].tempo_reset = value(0) != 0;
                0
            }
            // The rhythm section (65, 66, 68..70) and the two unnamed
            // callbacks (63, 73): unused by every retail file, counted
            // when that stops being true.
            63 | 65 | 66 | 68 | 69 | 70 | 73 => {
                self.ignored += 1;
                0
            }
            // stopChannel, canonical and the table's padding slots.
            _ => self.stop_channel(),
        }
    }

    fn stop_channel(&mut self) -> i32 {
        self.channels[self.cur].priority = 0;
        if self.cur != 9 {
            self.note_off();
        }
        self.channels[self.cur].pos = None;
        2
    }

    /// One 72 Hz tick over every channel, highest first.
    fn execute_programs(&mut self) {
        for cur in (0..=9usize).rev() {
            self.cur = cur;
            if self.channels[cur].pos.is_none() {
                continue;
            }
            if self.channels[cur].tempo_reset {
                self.channels[cur].tempo = self.tempo;
            }

            let mut result: i32 = 1;
            let tempo = self.channels[cur].tempo;
            let mut timer = self.channels[cur].timer;
            let ticked = advance(&mut timer, tempo);
            self.channels[cur].timer = timer;
            if ticked {
                self.channels[cur].duration = self.channels[cur].duration.wrapping_sub(1);
                if self.channels[cur].duration != 0 {
                    if self.channels[cur].duration == self.channels[cur].spacing2 {
                        self.note_off();
                    }
                    if self.channels[cur].duration == self.channels[cur].spacing1 && cur != 9 {
                        self.note_off();
                    }
                } else {
                    result = 0;
                }
            }

            let mut guard = 0usize;
            while result == 0 && self.channels[cur].pos.is_some() {
                guard += 1;
                if guard > MAX_STEPS_PER_TICK {
                    // A program spinning without ever waiting is a
                    // loop; end it rather than hang the render.
                    result = self.stop_channel();
                    break;
                }
                let pos = self.channels[cur].pos.expect("checked in the loop");
                let Some(&opcode) = self.sound.get(pos) else {
                    result = self.stop_channel();
                    break;
                };
                self.steps += 1;
                if opcode & 0x80 != 0 {
                    let index = (opcode & 0x7F).min(adl::MAX_OPCODE);
                    let values = pos + 1;
                    let taken = adl::argument_bytes(index);
                    if values + taken > self.sound.len() {
                        result = self.stop_channel();
                        break;
                    }
                    self.channels[cur].pos = Some(values + taken);
                    result = self.run_opcode(index, values);
                } else {
                    let Some(&duration) = self.sound.get(pos + 1) else {
                        result = self.stop_channel();
                        break;
                    };
                    self.channels[cur].pos = Some(pos + 2);
                    self.setup_note(opcode, false);
                    self.note_on();
                    self.setup_duration(duration);
                    result = (duration != 0) as i32;
                }
            }

            if result == 1 && self.channels[cur].pos.is_some() {
                match self.channels[cur].primary_effect {
                    Effect::None => {}
                    Effect::Slide => self.primary_effect_slide(),
                    Effect::Vibrato => self.primary_effect_vibrato(),
                }
                if self.channels[cur].secondary_effect {
                    self.secondary_effect_1();
                }
            }
        }
    }

    /// The driver's timer callback: programs, then the beat counter.
    fn callback(&mut self) {
        self.execute_programs();
        let old = self.callback_timer;
        self.callback_timer = self.callback_timer.wrapping_add(self.tempo);
        if self.callback_timer < old {
            self.beat_div_cnt = self.beat_div_cnt.wrapping_sub(1);
            if self.beat_div_cnt == 0 {
                self.beat_div_cnt = self.beat_divider;
                self.beat_counter = self.beat_counter.wrapping_add(1);
            }
        }
    }

    fn any_running(&self) -> bool {
        self.channels.iter().any(|channel| channel.pos.is_some())
    }

    /// Tick until every channel stops, rendering as it goes.
    fn run(mut self, rate: u32) -> Rendered {
        let mut samples = Vec::new();
        let mut ticks = 0usize;
        while self.any_running() && ticks < MAX_TICKS {
            self.callback();
            ticks += 1;
            // Fractional-exact pacing: emit up to where this tick ends.
            let due = (ticks as u64 * rate as u64) / TICKS_PER_SECOND as u64;
            let missing = (due as usize).saturating_sub(samples.len());
            if missing > 0 {
                samples.extend(render(&mut self.chip, rate, missing));
            }
        }
        // Let every release phase ring out rather than cutting it off.
        for cur in 0..9 {
            self.cur = cur;
            self.note_off();
        }
        samples.extend(infengine_opl::render_until_silent(&mut self.chip, rate, 2.0));
        Rendered {
            samples,
            steps: self.steps,
            ignored: self.ignored,
            ticks,
        }
    }
}

/// Reset a channel the way `initChannel` does: everything to zero
/// except the second extra level, which survives on purpose.
fn init_channel(channel: &mut Channel) {
    let keep = channel.op_extra_level2;
    *channel = Channel::default();
    channel.op_extra_level2 = keep;
    channel.tempo = 0xFF;
    channel.spacing1 = 1;
}

/// What rendering a program produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// Mono samples at the rate asked for.
    pub samples: Vec<i16>,
    /// Opcodes and notes executed.
    pub steps: usize,
    /// Of those, ones this driver does not implement.
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

/// Render one of a file's track programs.
///
/// `None` when the program table has no such program. The program's
/// own header says which channel it conducts from; anything it starts
/// via `setupProgram` plays too, which is most of what an effect is.
pub fn render_track(file: &adl::AdlFile<'_>, program: usize, rate: u32) -> Option<Rendered> {
    let sound = file.sound();
    let mut driver = Driver::new(sound);
    driver.reset_adlib_state();
    if !driver.start_program(program) {
        return None;
    }
    Some(driver.run(rate))
}

/// Render a bare command stream on channel 0.
///
/// For tests and probes: the stream is its own address space, so
/// relative jumps stay inside it, and there is no program table for
/// `setupProgram` or instruments to reach.
pub fn render_stream(code: &[u8], rate: u32) -> Rendered {
    let mut driver = Driver::new(code);
    driver.reset_adlib_state();
    let channel = &mut driver.channels[0];
    channel.pos = Some(0);
    channel.tempo = 0xFF;
    channel.timer = 0xFF;
    channel.duration = 1;
    channel.volume_modifier = 0xFF;
    driver.init_adlib_channel(0);
    driver.run(rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command: the opcode with its top bit, then its values.
    fn cmd(opcode: u8, values: &[u8]) -> Vec<u8> {
        let mut out = vec![0x80 | opcode];
        out.extend_from_slice(values);
        out
    }

    fn note(note: u8, duration: u8) -> Vec<u8> {
        vec![note & 0x7F, duration]
    }

    #[test]
    fn a_program_that_only_stops_makes_no_sound() {
        let out = render_stream(&cmd(8, &[]), 22050);
        assert!(out.is_silent());
        assert!(out.ticks <= 1);
    }

    #[test]
    fn a_note_takes_the_ticks_it_asks_for() {
        // Seventy-two ticks to the second, so a note of 36 is half of
        // one. Tempo 0xFF misses one wrap in 256, which is the slack.
        let mut code = note(0x20, 36);
        code.extend(cmd(8, &[]));
        let out = render_stream(&code, 22050);
        assert!((37..=39).contains(&out.ticks), "{}", out.ticks);
        // The tail is the release, which is why this is over half.
        assert!(out.seconds(22050) >= 0.5, "{}", out.seconds(22050));
    }

    #[test]
    fn a_loop_runs_the_body_the_number_of_times_it_was_given() {
        // setRepeat 2: the body plays, then checkRepeat spends one
        // count per pass - so two passes in all.
        let mut code = cmd(0, &[2]);
        code.extend(note(0x20, 4));
        // The displacement is measured from after its own two bytes:
        // the note starts at byte 2 and the jump is taken at byte 7.
        let back = (-5i16).to_le_bytes();
        code.extend(cmd(1, &back));
        code.extend(cmd(8, &[]));

        let out = render_stream(&code, 22050);
        assert!((9..=11).contains(&out.ticks), "two passes of four: {}", out.ticks);
    }

    #[test]
    fn a_program_that_jumps_to_itself_is_stopped_rather_than_rendered_forever() {
        let back = (-3i16).to_le_bytes();
        let out = render_stream(&cmd(4, &back), 22050);
        assert!(out.samples.len() < 22050 * 4, "it ran away");
    }

    #[test]
    fn a_rhythm_command_is_counted_not_hidden() {
        // setupRhythmSection: nine values, no retail program uses it.
        let mut code = cmd(65, &[0; 9]);
        code.extend(note(0x20, 1));
        code.extend(cmd(8, &[]));
        let out = render_stream(&code, 22050);
        assert_eq!(out.ignored, 1);
        assert!(out.steps >= 3);
    }

    #[test]
    fn writing_a_register_reaches_the_chip() {
        // The direct write is the escape hatch for anything not
        // modelled, so it has to work even when nothing else does.
        let mut code = cmd(10, &[0x40, 0x00]);
        code.extend(note(0x20, 2));
        code.extend(cmd(8, &[]));
        let out = render_stream(&code, 22050);
        assert_eq!(out.ignored, 0);
        assert!(!out.is_silent(), "level 0 on the carrier should sound");
    }

    #[test]
    fn a_note_with_an_instrument_is_loud() {
        // A plausible instrument in a fake program table, the way the
        // files lay one out: entry 250 pointing at eleven bytes.
        let mut sound = vec![0u8; 600];
        // Program 0 at offset 0x20: channel 0, priority 1, then code.
        sound[0] = 0x20;
        sound[1] = 0x00;
        let instrument_entry = 2 * (adl::INSTRUMENT_BASE + 3);
        sound[instrument_entry] = 0x10;
        sound[instrument_entry + 1] = 0x02; // offset 0x210
        let code: Vec<u8> = [
            cmd(16, &[3]),        // setupInstrument 3
            cmd(7, &[2]),         // base octave
            note(0x23, 24),       // a real note
            cmd(8, &[]),          // stop
        ]
        .concat();
        sound[0x20..0x20 + 2].copy_from_slice(&[0, 1]);
        sound[0x22..0x22 + code.len()].copy_from_slice(&code);
        // AM/VIB off, sustain on, multiple 1; feedback 0 FM; sine;
        // levels loud; fast attack, slow release.
        let instrument = [0x21, 0x21, 0x00, 0x00, 0x00, 0x10, 0x08, 0xF4, 0xF4, 0x2F, 0x2F];
        sound[0x210..0x210 + 11].copy_from_slice(&instrument);

        let file_bytes: Vec<u8> = {
            let mut all = vec![0xFF; 120];
            all[0] = 0; // track 0 -> program 0
            all.extend_from_slice(&sound);
            all
        };
        let file = adl::AdlFile::parse(&file_bytes).expect("parses");
        let out = render_track(&file, 0, 22050).expect("program 0 exists");
        let peak = out.samples.iter().map(|s| s.abs()).max().unwrap_or(0);
        assert!(peak > 500, "an instrument at level 0x08 is audible: {peak}");
    }

    #[test]
    fn a_conductor_spawns_a_voice_on_another_channel() {
        // Program 0: channel 9 (control), setupProgram 1, wait, stop.
        // Program 1: channel 6, a note. The sound must come from the
        // voice the conductor started.
        let mut sound = vec![0u8; 0x300];
        sound[0] = 0x40;
        sound[1] = 0;
        sound[2] = 0x60;
        sound[3] = 0;
        let conductor: Vec<u8> =
            [vec![9, 1], cmd(2, &[1]), cmd(9, &[30]), cmd(8, &[])].concat();
        let voice: Vec<u8> = [
            vec![6, 1],
            cmd(10, &[0x53, 0x00]), // carrier level loud on channel 6
            cmd(7, &[3]),
            note(0x21, 20),
            cmd(8, &[]),
        ]
        .concat();
        sound[0x40..0x40 + conductor.len()].copy_from_slice(&conductor);
        sound[0x60..0x60 + voice.len()].copy_from_slice(&voice);

        let mut file_bytes = vec![0xFF; 120];
        file_bytes[0] = 0;
        file_bytes.extend_from_slice(&sound);
        let file = adl::AdlFile::parse(&file_bytes).expect("parses");
        let out = render_track(&file, 0, 22050).expect("program 0 exists");
        assert!(!out.is_silent(), "the spawned voice must sound");
    }

    #[test]
    fn every_semitone_has_a_frequency_and_they_rise() {
        // The driver's own table, all twelve.
        assert_eq!(FREQ_TABLE[0], 0x0134);
        assert_eq!(FREQ_TABLE[9], 0x0207);
        for pair in FREQ_TABLE.windows(2) {
            assert!(pair[1] > pair[0], "{pair:?}");
        }
        // An octave up is a doubling, near enough.
        let ratio = FREQ_TABLE[11] as f32 / FREQ_TABLE[0] as f32;
        assert!((ratio - 1.887).abs() < 0.01, "{ratio}");
    }

    #[test]
    fn the_values_table_matches_the_dispatchers_arithmetic() {
        // The census over the retail files walks cleanly only with
        // these counts, so they are load-bearing.
        assert_eq!(adl::argument_bytes(2), 1, "setupProgram takes one value");
        assert_eq!(adl::argument_bytes(13), 5, "setupSecondaryEffect1 takes five");
        assert_eq!(adl::argument_bytes(65), 9, "setupRhythmSection takes nine");
        assert_eq!(adl::argument_bytes(21), 4, "vibrato takes four");
        assert_eq!(adl::argument_bytes(8), 0, "stopChannel takes none");
    }
}
