//! A26.4: Westwood's AdLib music file.
//!
//! The layout, established by reading all ten of Kyrandia's `.ADL`
//! files and corrected against the driver's own address arithmetic:
//!
//! ```text
//!   0..120   one byte per track: which program plays it, 0xFF = none
//!   120..    the sound data: little-endian program offsets first,
//!            then the programs - and the offsets are relative to
//!            byte 120, not to any fixed table end
//! ```
//!
//! The relative base is the fact everything else hangs on. The driver
//! reads `soundData + offset` where `soundData` is the file after its
//! track table, so an offset table only reaches as far as the file's
//! own program count needs and the programs begin right after it.
//! Reading the programs at a fixed 1000-byte header instead - which
//! every one of the ten files *appears* to support, their lowest
//! offset being 880, i.e. 1000 from the file's start - lands 880
//! bytes past every program and executes score data as code. That
//! mistake rendered an entire effects bank as near-silence and took
//! the longest of anything here to find.
//!
//! A program's first two bytes are its header: the channel it runs on
//! (0..9, where 9 is the driver's control channel) and its priority.
//! The stream follows. Programs numbered [`INSTRUMENT_BASE`] and up
//! are instruments - eleven register bytes, not sequences.
//!
//! A program is read **a word at a time**, and that is not a guess: the
//! floppy release ships the driver that plays these files, `ALFX.DRV`,
//! and its interpreter is eleven instructions long.
//!
//! ```text
//!   lds  si, cs:[di+3]        ; the stream pointer
//!   lodsw                     ; two bytes at once
//!   or   al, al
//!   js   command              ; top bit set: a command
//!   ...                       ; otherwise al is a note, ah its length
//! command:
//!   and  al, 0x7f
//!   cmp  al, 0x4a             ; opcodes run 0..74
//!   jle  ok
//!   mov  al, 0x4a             ; anything higher is clamped, not refused
//! ok:
//!   shl  bl, 1
//!   call cs:[bx+0x18ed]       ; 75 handlers
//! ```
//!
//! So every step is a pair: a note and its duration, or a command and
//! its argument. The opcode space is 0 to 74 and nothing else - the 119
//! distinct opcodes an earlier reader found were an artefact of reading
//! a byte at a time and never clamping. Read as words with the clamp,
//! the ten retail files use 73 of the 75, and 72% of their 494 000
//! steps are notes, which is what a score looks like.
//!
//! Some handlers then read further bytes of their own - a jump reads a
//! 16-bit displacement, a patch change reads a patch - so a step is not
//! always two bytes. [`ARGUMENT_BYTES`] says how many each takes, and
//! it is the driver's own dispatch table, not a reconstruction: a
//! first table counted from `ALFX.DRV`'s effect on `si` disagreed in a
//! handful of slots and left 5% of command reads falling past the last
//! handler. With the dispatch table's counts a walk over every
//! referenced track in all ten files stays aligned to its terminator.
//!
//! Two searches were run before the driver turned up and neither
//! settled it. Recorded so they are not repeated: hill-climbing the
//! argument counts to make each program end on its own last byte stalls
//! at 1881 of 3231, because programs share their tails and the premise
//! was wrong; and minimising the count of distinct opcodes gets 116
//! down to 78, still far too many.

//! ## What the handlers do
//!
//! Reading the argument lengths was enough to split a program into
//! steps. Turning one into sound needs what each step *means*, and that
//! is in the driver too. Established so far, by disassembling it:
//!
//! ```text
//!   0x0781  write an OPL register: AH is the register, AL the value.
//!           Register to port 0x388, value to 0x389, with the delays
//!           the chip needs between them.
//!   0x069c  channel to operator offset: 00 01 02 08 09 0A 10 11 12,
//!           which is the standard OPL2 layout exactly. Finding it
//!           unchanged is what confirms the rest of this reading.
//!   0x0866  note off: register 0xB0 + channel, bit 5 cleared. The
//!           driver keeps its own copy of that register at state 0x2A
//!           and edits the copy, because the chip cannot be read back.
//!   0x0bfd  apply level: register 0x40 + the carrier's operator
//!           offset, and the modulator's too when state 0x33 says the
//!           voice is one where that matters.
//! ```
//!
//! The channel's state block is what `di` points at, and the handlers
//! are mostly one line long because they only write to it:
//!
//! ```text
//!   0x08  repeats left           set by opcode 0x00, counted by 0x01
//!   0x0A  cleared when a program stops
//!   0x22  level                  set by opcode 0x1E
//!   0x2A  the driver's copy of register 0xB0 + channel
//!   0x32  set by opcode 0x2B
//!   0x33  whether the modulator takes the level as well
//! ```
//!
//! So opcode 0x00 sets a repeat count and 0x01 closes the loop by
//! reading a displacement out of the stream and adding it to the read
//! pointer; 0x08 silences the channel and zeroes the stream pointer;
//! 0x09 rests until the next tick; 0x10 loads an instrument, indexing
//! the file's own program table at +0x1F4 - 500 bytes, i.e. entry 250
//! of a table of words, which is [`INSTRUMENT_BASE`] - so the later
//! programs are patches rather than sequences.
//!
//! What is left is the rest of the register writes. Three opcodes cover
//! 43% of every command the effect programs issue, ten cover 78% and
//! nineteen cover 92%, which the `adl_sfx` example reports; the tail is
//! ornament rather than structure.

use std::fmt;

/// Entries in the track table, which is also where the sound data -
/// the address space every program offset is relative to - begins.
pub const TRACKS: usize = 120;
/// Most program slots any file's table could hold; a read bound, not
/// a claim that any file defines that many.
pub const PROGRAMS: usize = 440;
/// First program number that is an instrument rather than a sequence.
///
/// The driver's instrument lookup is `getProgram(250 + n)`, and the
/// number was confirmed against the data: every instrument reference
/// in the intro and chapter-one banks resolves to instrument-sized
/// bytes at base 250, and none do at the 150 and 190 bases tried
/// before it.
pub const INSTRUMENT_BASE: usize = 250;
/// Marker for an unused track entry.
pub const NO_PROGRAM: u8 = 0xFF;
/// Marker for an unused program offset. Zero is invalid too: offset 0
/// is the offset table itself.
pub const NO_OFFSET: u16 = 0xFFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdlError {
    /// Smaller than the header it must begin with.
    TooSmall { len: usize },
}

impl fmt::Display for AdlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdlError::TooSmall { len } => {
                write!(
                    f,
                    "an ADL file is a {TRACKS}-byte track table and sound data; this is {len} bytes"
                )
            }
        }
    }
}

impl std::error::Error for AdlError {}

/// Highest opcode the driver dispatches. Anything above is clamped to
/// it rather than refused, which is what the driver itself does.
pub const MAX_OPCODE: u8 = 0x4A;

/// Handlers in the driver's dispatch table.
pub const HANDLERS: usize = 75;

/// What each of the driver's 75 handlers is called.
///
/// The numbering is a property of Kyrandia's data and the names are the
/// format's, which is public; how they are used here is ours. Having
/// them turns a decompiled effect from `cmd2b 40` into
/// `setupPrimaryEffectVibrato`, which is the difference between a
/// program you can read and one you can only count.
///
/// Slots that do nothing but stop the channel are the table's padding -
/// forty of the seventy-five, which is why so many opcodes never appear
/// in the data.
pub const HANDLER_NAMES: [&str; HANDLERS] = [
    "setRepeat", "checkRepeat", "setupProgram", // 0
    "setNoteSpacing", "jump", "jumpToSubroutine", // 3
    "returnFromSubroutine", "setBaseOctave", "stopChannel", // 6
    "playRest", "writeAdLib", "setupNoteAndDuration", // 9
    "setBaseNote", "setupSecondaryEffect1", "stopOtherChannel", // 12
    "waitForEndOfProgram", "setupInstrument", "setupPrimaryEffectSlide", // 15
    "removePrimaryEffectSlide", "setBaseFreq", "stopChannel", // 18
    "setupPrimaryEffectVibrato", "stopChannel", "stopChannel", // 21
    "stopChannel", "stopChannel", "setPriority", // 24
    "stopChannel", "setBeat", "waitForNextBeat", // 27
    "setExtraLevel1", "stopChannel", "setupDuration", // 30
    "playNote", "stopChannel", "stopChannel", // 33
    "setFractionalNoteSpacing", "stopChannel", "setTempo", // 36
    "removeSecondaryEffect1", "stopChannel", "setChannelTempo", // 39
    "stopChannel", "setExtraLevel3", "setExtraLevel2", // 42
    "changeExtraLevel2", "setAmDepth", "setVibratoDepth", // 45
    "changeExtraLevel1", "stopChannel", "stopChannel", // 48
    "clearChannel", "stopChannel", "changeNoteRandomly", // 51
    "removePrimaryEffectVibrato", "stopChannel", "stopChannel", // 54
    "pitchBend", "resetToGlobalTempo", "nop", // 57
    "setDurationRandomness", "changeChannelTempo", "stopChannel", // 60
    "callback46", "nop", "setupRhythmSection", // 63
    "playRhythmSection", "removeRhythmSection", "setRhythmLevel2", // 66
    "changeRhythmLevel1", "setRhythmLevel1", "setSoundTrigger", // 69
    "setTempoReset", "callback56", "stopChannel", // 72
];

/// Argument bytes each command takes after its opcode byte.
///
/// This is the driver's dispatch table entry for entry - the `values`
/// count each handler declares. An earlier table was reconstructed by
/// counting `ALFX.DRV`'s effect on `si` and disagreed in a handful of
/// slots (`setupProgram` took three there, one here), and each
/// disagreement desynchronized every stream that used the opcode:
/// those were the 5% of reads that kept falling past the last
/// handler. With these counts a walk over every referenced track in
/// all ten retail files stays aligned to its terminator.
pub const ARGUMENT_BYTES: [u8; HANDLERS] = [
    1, 2, 1, 1, 2, 2, 0, 1, 0, 1, //
    2, 2, 1, 5, 1, 1, 1, 3, 0, 1, //
    0, 4, 0, 0, 0, 0, 1, 0, 1, 1, //
    1, 0, 1, 1, 0, 0, 1, 0, 1, 0, //
    0, 1, 0, 1, 2, 2, 1, 1, 1, 0, //
    0, 1, 0, 2, 0, 0, 0, 1, 0, 0, //
    1, 1, 0, 2, 0, 9, 1, 0, 2, 2, //
    2, 1, 1, 2, 0,
];

/// How many bytes a command's arguments take.
pub fn argument_bytes(opcode: u8) -> usize {
    ARGUMENT_BYTES
        .get(usize::from(opcode.min(MAX_OPCODE)))
        .copied()
        .unwrap_or(1) as usize
}

/// One step of a program: always a pair of bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Play a note for a number of ticks.
    Note { note: u8, duration: u8 },
    /// A sequencer command and its argument.
    Command { opcode: u8, argument: u8 },
    /// The stream ended in the middle of a step.
    Truncated,
}

impl Step {
    /// What this step is called, where that is known.
    ///
    /// `None` means the opcode has not been identified. Those are left
    /// unnamed on purpose rather than guessed at.
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Step::Note { .. } => Some("note"),
            Step::Truncated => Some("truncated"),
            Step::Command { opcode, .. } => command_name(*opcode),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Step::Note { note, duration } => format!("note {note} for {duration}"),
            Step::Truncated => "truncated".to_string(),
            Step::Command { opcode, argument } => match command_name(*opcode) {
                Some(name) => format!("{name} {argument}"),
                None => format!("cmd{opcode:02x} {argument}"),
            },
        }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Step::Command { opcode, .. } if command_name(*opcode).is_none())
    }
}

/// Commands this importer has identified.
///
/// All seventy-five of them. Six were read out of the driver by hand
/// before it turned out the format is documented, and those six agree
/// with the table - which is the only reason to trust the other
/// sixty-nine.
fn command_name(opcode: u8) -> Option<&'static str> {
    HANDLER_NAMES.get(opcode as usize).copied()
}

/// A parsed `.ADL` file, borrowing its bytes.
#[derive(Debug, Clone, Copy)]
pub struct AdlFile<'a> {
    data: &'a [u8],
}

impl<'a> AdlFile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, AdlError> {
        // A track table and at least one offset slot; the retail files
        // are all far past this.
        if data.len() < TRACKS + 2 {
            return Err(AdlError::TooSmall { len: data.len() });
        }
        Ok(Self { data })
    }

    /// Program number that plays `track`, if any.
    pub fn program_of_track(&self, track: usize) -> Option<u8> {
        let value = *self.data.get(track)?;
        (value != NO_PROGRAM && track < TRACKS).then_some(value)
    }

    /// Every track that plays something, as `(track, program)`.
    pub fn tracks(&self) -> Vec<(usize, u8)> {
        (0..TRACKS)
            .filter_map(|track| self.program_of_track(track).map(|prog| (track, prog)))
            .collect()
    }

    /// The sound data: the file past its track table, which is the
    /// address space every program offset is relative to.
    pub fn sound(&self) -> &'a [u8] {
        &self.data[TRACKS..]
    }

    /// A program's offset into [`Self::sound`], if it has one.
    pub fn program_offset(&self, program: usize) -> Option<usize> {
        if program >= PROGRAMS {
            return None;
        }
        let at = TRACKS + program * 2;
        let offset = u16::from_le_bytes([*self.data.get(at)?, *self.data.get(at + 1)?]);
        // Zero would be the offset table itself; 0xFFFF is the
        // explicit "none". Past the end is a slot the file never
        // wrote, read as table only because the program before it is
        // long enough to reach it.
        if offset == NO_OFFSET || offset == 0 {
            return None;
        }
        let offset = offset as usize;
        (offset < self.sound().len()).then_some(offset)
    }

    /// A program's bytes: channel, priority, then its stream.
    ///
    /// Bounded by the next program that starts after it, which is what
    /// keeps a malformed file from being read to the end of the world.
    /// Programs are not stored in offset order, so the bound has to be
    /// looked up rather than taken from the next table entry. The
    /// driver itself does not use this bound - its parser stops at the
    /// program's own terminator - so this is for tools that list.
    pub fn program(&self, program: usize) -> Option<&'a [u8]> {
        let start = self.program_offset(program)?;
        let end = (0..PROGRAMS)
            .filter_map(|other| self.program_offset(other))
            .filter(|offset| *offset > start)
            .min()
            .unwrap_or(self.sound().len());
        Some(&self.sound()[start..end])
    }

    /// One instrument's register bytes.
    ///
    /// Instruments are the programs from [`INSTRUMENT_BASE`] up;
    /// eleven bytes is what the driver reads of one.
    pub fn instrument(&self, instrument: usize) -> Option<&'a [u8]> {
        self.program(INSTRUMENT_BASE + instrument)
            .filter(|data| data.len() >= 11)
    }

    /// How many programs the file actually defines.
    pub fn program_count(&self) -> usize {
        (0..PROGRAMS)
            .filter(|program| self.program_offset(*program).is_some())
            .count()
    }
}

/// Read a program as a list of steps.
///
/// A note is always its pitch and a duration. A command is its opcode
/// and however many bytes its handler reads, which is usually the one
/// the dispatcher pre-loads and sometimes more.
pub fn disassemble(program: &[u8]) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut at = 0;
    while at < program.len() {
        let byte = program[at];
        if byte & 0x80 == 0 {
            let Some(duration) = program.get(at + 1).copied() else {
                steps.push(Step::Truncated);
                break;
            };
            at += 2;
            steps.push(Step::Note {
                note: byte,
                duration,
            });
            continue;
        }

        let opcode = (byte & 0x7F).min(MAX_OPCODE);
        let width = 1 + argument_bytes(opcode);
        if at + width > program.len() {
            steps.push(Step::Truncated);
            break;
        }
        // Commands with no value bytes - the terminator above all -
        // still get shown with a zero rather than with the byte after
        // them, which belongs to the next step.
        let argument = if width > 1 { program[at + 1] } else { 0 };
        at += width;
        steps.push(Step::Command { opcode, argument });
    }
    steps
}

/// How many steps in a program are commands nobody has identified.
pub fn unknown_count(steps: &[Step]) -> usize {
    steps.iter().filter(|step| step.is_unknown()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a file by spelling the layout out, not by using the
    /// constants the parser uses - a builder that shares its numbers
    /// with the code under test cannot catch an offset being wrong.
    ///
    /// 120 track bytes, then an 880-byte offset area (what a real
    /// file's table region looks like), then the programs. Offsets are
    /// written relative to byte 120, because that is what the driver
    /// reads them as.
    fn build(tracks: &[(usize, u8)], programs: &[(usize, Vec<u8>)]) -> Vec<u8> {
        let mut data = vec![0xFFu8; 1000];
        for (track, program) in tracks {
            data[*track] = *program;
        }
        let mut body = Vec::new();
        for (index, bytes) in programs {
            let offset = (1000 - 120) + body.len();
            let at = 120 + index * 2;
            data[at] = (offset & 0xFF) as u8;
            data[at + 1] = (offset >> 8) as u8;
            body.extend_from_slice(bytes);
        }
        data.extend_from_slice(&body);
        data
    }

    #[test]
    fn a_file_shorter_than_its_header_is_refused() {
        assert_eq!(
            AdlFile::parse(&[0u8; 40]).err(),
            Some(AdlError::TooSmall { len: 40 })
        );
    }

    #[test]
    fn tracks_name_the_programs_that_play_them() {
        let data = build(&[(0, 0), (3, 9), (20, 200)], &[]);
        let file = AdlFile::parse(&data).expect("parse");
        assert_eq!(file.tracks(), vec![(0, 0), (3, 9), (20, 200)]);
        assert_eq!(file.program_of_track(1), None, "0xFF means no program");
        assert_eq!(file.program_of_track(500), None, "and 120 is the end");
    }

    #[test]
    fn a_program_is_bounded_by_the_next_one_along() {
        let data = build(
            &[(0, 0)],
            &[(0, vec![0x40, 0x05, 0x89, 0x00]), (7, vec![0x41, 0x06])],
        );
        let file = AdlFile::parse(&data).expect("parse");
        assert_eq!(file.program(0), Some(&[0x40, 0x05, 0x89, 0x00][..]));
        assert_eq!(file.program(7), Some(&[0x41, 0x06][..]));
        assert_eq!(file.program(1), None);
        assert_eq!(file.program_count(), 2);
    }

    #[test]
    fn the_zero_and_ffff_offsets_are_not_programs() {
        let mut data = build(&[(0, 0)], &[(0, vec![0x40, 0x05])]);
        // Zero would point a program at the offset table itself.
        data[120 + 2] = 0x00;
        data[120 + 3] = 0x00;
        let file = AdlFile::parse(&data).expect("parse");
        assert_eq!(file.program(1), None);
        assert_eq!(file.program(3), None, "0xFFFF is the explicit none");
    }

    #[test]
    fn an_offset_early_in_the_sound_data_is_a_program() {
        // Real files put programs right after however much offset
        // table they actually use, so a small offset is data, not an
        // error - there is no fixed table size to be "inside" of.
        let mut data = build(&[(0, 0)], &[]);
        data[120] = 0x10; // program 0 at sound offset 0x10
        data[121] = 0x00;
        data[120 + 0x10] = 0x07;
        data[120 + 0x11] = 0x42;
        let file = AdlFile::parse(&data).expect("parse");
        let program = file.program(0).expect("a small offset is valid");
        assert_eq!(&program[..2], &[0x07, 0x42], "channel 7, priority 0x42");
    }

    #[test]
    fn an_offset_past_the_end_is_not_a_program() {
        let mut data = build(&[(0, 0)], &[(0, vec![0x40, 0x05])]);
        data[120 + 4] = 0xF0;
        data[120 + 5] = 0xFF - 1;
        let file = AdlFile::parse(&data).expect("parse");
        assert_eq!(file.program(2), None);
    }

    #[test]
    fn notes_and_commands_are_told_apart_by_the_top_bit() {
        let steps = disassemble(&[0x65, 0x05, 0x89, 0x00, 0x10, 0x02]);
        assert_eq!(
            steps,
            vec![
                Step::Note {
                    note: 0x65,
                    duration: 5
                },
                Step::Command {
                    opcode: 0x09,
                    argument: 0
                },
                Step::Note {
                    note: 0x10,
                    duration: 2
                },
            ]
        );
    }

    #[test]
    fn a_stream_that_ends_mid_step_says_so() {
        let steps = disassemble(&[0x65, 0x05, 0x89]);
        assert_eq!(steps.last(), Some(&Step::Truncated));
    }

    #[test]
    fn a_command_consumes_what_its_handler_reads() {
        // The slide setup reads three bytes after its opcode, so the
        // note after it starts four bytes along and not two.
        assert_eq!(argument_bytes(0x11), 3);
        let steps = disassemble(&[0x91, 0x11, 0x22, 0x33, 0x40, 0x05]);
        assert_eq!(
            steps,
            vec![
                Step::Command {
                    opcode: 0x11,
                    argument: 0x11
                },
                Step::Note {
                    note: 0x40,
                    duration: 5
                },
            ]
        );
    }

    #[test]
    fn a_terminator_at_the_end_of_a_stream_is_a_step_not_a_truncation() {
        // stopChannel takes no values, so a program may end on its
        // very last byte - and nearly every real one does.
        let steps = disassemble(&[0x40, 0x05, 0x88]);
        assert_eq!(
            steps,
            vec![
                Step::Note {
                    note: 0x40,
                    duration: 5
                },
                Step::Command {
                    opcode: 0x08,
                    argument: 0
                },
            ]
        );
    }

    #[test]
    fn a_command_whose_arguments_run_off_the_end_says_so() {
        // Opcode 0x0D takes five; four bytes cannot hold it.
        assert_eq!(argument_bytes(0x0D), 5);
        assert_eq!(disassemble(&[0x8D, 0x01, 0x02, 0x03]).last(), Some(&Step::Truncated));
    }

    #[test]
    fn an_opcode_past_the_drivers_last_handler_is_clamped() {
        // The driver does `cmp al, 0x4a` then `mov al, 0x4a` rather
        // than refusing, so 0xFF is opcode 74 and not opcode 127.
        assert_eq!(
            disassemble(&[0xFF]),
            vec![Step::Command {
                opcode: MAX_OPCODE,
                argument: 0,
            }]
        );
        assert_eq!(
            disassemble(&[0x89, 0x00]),
            vec![Step::Command {
                opcode: 0x09,
                argument: 0,
            }],
            "and an opcode inside the range is left alone"
        );
    }

    #[test]
    fn every_command_in_range_has_a_name() {
        // Six of these were read out of the driver by hand before the
        // format turned out to be documented, and all six agreed with
        // the table - which is the only reason to trust the rest.
        assert_eq!(
            Step::Command {
                opcode: 0x09,
                argument: 0
            }
            .label(),
            "playRest 0"
        );
        assert_eq!(
            Step::Command {
                opcode: 0x1e,
                argument: 3
            }
            .label(),
            "setExtraLevel1 3",
            "read by hand as the one that writes register 0x40 + operator"
        );
        for opcode in 0..=MAX_OPCODE {
            let step = Step::Command { opcode, argument: 0 };
            assert!(step.name().is_some(), "{opcode:#04x} has no name");
            assert!(!step.is_unknown());
        }
    }

    #[test]
    fn an_opcode_past_the_table_is_still_countable() {
        // Nothing in the data reaches here, because the driver clamps -
        // but a reader that stops naming things has to say so rather
        // than print a number as if it were a name.
        let step = Step::Command {
            opcode: MAX_OPCODE + 1,
            argument: 3,
        };
        assert!(step.is_unknown());
        assert_eq!(step.label(), "cmd4b 3");
        assert_eq!(unknown_count(&[step]), 1);
    }

    #[test]
    fn nothing_the_data_holds_is_left_unnamed() {
        let steps = disassemble(&[0x65, 0x05, 0x89, 0x00, 0xab, 0x08, 0xbb, 0x01]);
        assert_eq!(unknown_count(&steps), 0);
    }
}
