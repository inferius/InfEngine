//! A26.4: Westwood's AdLib music file.
//!
//! The layout, established by reading all ten of Kyrandia's `.ADL`
//! files rather than by assumption:
//!
//! ```text
//!   0..120     one byte per track: which program plays it, 0xFF = none
//!   120..1000  440 little-endian offsets, one per program, 0xFFFF = none
//!   1000..     the programs themselves
//! ```
//!
//! Every file agrees: the lowest offset in each of the ten is exactly
//! 1000, which is what a 120-byte table followed by 440 words comes to.
//!
//! A program is a byte stream for a small sequencer. A byte under 0x80
//! is a note followed by a duration; a byte with the top bit set is a
//! command. That split is what makes the streams readable at all - the
//! pitch contours are plainly visible in a hex dump once you know to
//! read pairs.
//!
//! **What is not settled yet:** how many arguments each command takes.
//! Reading every command as taking exactly one gets 51% of the ten
//! files' 114 000 steps out as notes and finds 30 650 rests, which is
//! what a score looks like - but it also finds 119 distinct opcodes out
//! of a possible 128, and no sequencer has 119 instructions. That tail
//! is the reader drifting after a command whose argument count is
//! wrong, and turning music data into invented opcodes. Establishing
//! the real argument counts is the next piece of work, and until it is
//! done this is a reader, not a player.
//!
//! The command names below are the ones the data supports on its own:
//! `rest` is over half of all commands, which is what a rest is in
//! music, and `instrument` appears in bursts at the head of a program
//! where a voice is being set up. The rest are reported by number, the
//! same way an untranslated script step is - a disassembly that quietly
//! invents meanings is worse than one that admits what it does not
//! know.

use std::fmt;

/// Bytes of header before the first program.
pub const HEADER_LEN: usize = 1000;
/// Entries in the track table.
pub const TRACKS: usize = 120;
/// Entries in the program offset table.
pub const PROGRAMS: usize = 440;
/// Marker for an unused track entry.
pub const NO_PROGRAM: u8 = 0xFF;
/// Marker for an unused program offset.
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
                write!(f, "an ADL file is at least {HEADER_LEN} bytes; this is {len}")
            }
        }
    }
}

impl std::error::Error for AdlError {}

/// One step of a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Play a note for a number of ticks.
    Note { note: u8, duration: u8 },
    /// A sequencer command and its one argument.
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
/// Derived from how they behave in the data rather than from a table
/// copied from somewhere, which is why there are three of them and not
/// eighty.
fn command_name(opcode: u8) -> Option<&'static str> {
    match opcode {
        0x08 => Some("stop"),
        0x09 => Some("rest"),
        0x10 => Some("instrument"),
        _ => None,
    }
}

/// A parsed `.ADL` file, borrowing its bytes.
#[derive(Debug, Clone, Copy)]
pub struct AdlFile<'a> {
    data: &'a [u8],
}

impl<'a> AdlFile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, AdlError> {
        if data.len() < HEADER_LEN {
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

    /// Byte offset of a program, if it has one.
    pub fn program_offset(&self, program: usize) -> Option<usize> {
        if program >= PROGRAMS {
            return None;
        }
        let at = TRACKS + program * 2;
        let offset = u16::from_le_bytes([*self.data.get(at)?, *self.data.get(at + 1)?]);
        if offset == NO_OFFSET {
            return None;
        }
        let offset = offset as usize;
        // An offset inside the header or past the end is not a program;
        // both happen in the retail files, where the table has room for
        // more programs than any one score uses.
        (HEADER_LEN..self.data.len()).contains(&offset).then_some(offset)
    }

    /// A program's bytes.
    ///
    /// Bounded by the next program that starts after it, which is what
    /// keeps a malformed file from being read to the end of the world.
    /// Programs are not stored in offset order, so the bound has to be
    /// looked up rather than taken from the next table entry.
    pub fn program(&self, program: usize) -> Option<&'a [u8]> {
        let start = self.program_offset(program)?;
        let end = (0..PROGRAMS)
            .filter_map(|other| self.program_offset(other))
            .filter(|offset| *offset > start)
            .min()
            .unwrap_or(self.data.len());
        Some(&self.data[start..end])
    }

    /// How many programs the file actually defines.
    pub fn program_count(&self) -> usize {
        (0..PROGRAMS)
            .filter(|program| self.program_offset(*program).is_some())
            .count()
    }
}

/// Read a program as a list of steps.
pub fn disassemble(program: &[u8]) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut at = 0;
    while at < program.len() {
        let byte = program[at];
        at += 1;
        let Some(argument) = program.get(at).copied() else {
            steps.push(Step::Truncated);
            break;
        };
        at += 1;
        if byte & 0x80 == 0 {
            steps.push(Step::Note {
                note: byte,
                duration: argument,
            });
        } else {
            steps.push(Step::Command {
                opcode: byte & 0x7F,
                argument,
            });
        }
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
    fn build(tracks: &[(usize, u8)], programs: &[(usize, Vec<u8>)]) -> Vec<u8> {
        let mut data = vec![0xFFu8; 1000];
        for byte in data.iter_mut().take(120) {
            *byte = 0xFF;
        }
        for (track, program) in tracks {
            data[*track] = *program;
        }
        let mut body = Vec::new();
        for (index, bytes) in programs {
            let offset = 1000 + body.len();
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
    fn an_offset_pointing_into_the_header_is_not_a_program() {
        let mut data = build(&[(0, 0)], &[(0, vec![0x40, 0x05])]);
        // Point program 1 at the middle of the header.
        data[120 + 2] = 0x10;
        data[120 + 3] = 0x00;
        let file = AdlFile::parse(&data).expect("parse");
        assert_eq!(file.program(1), None);
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
    fn identified_commands_are_named_and_the_rest_are_not() {
        assert_eq!(
            Step::Command {
                opcode: 0x09,
                argument: 0
            }
            .label(),
            "rest 0"
        );
        let mystery = Step::Command {
            opcode: 0x2b,
            argument: 3,
        };
        assert_eq!(mystery.label(), "cmd2b 3");
        assert!(
            mystery.is_unknown(),
            "an unidentified opcode has to be countable"
        );
    }

    #[test]
    fn the_unknown_count_is_what_is_left_to_do() {
        let steps = disassemble(&[0x65, 0x05, 0x89, 0x00, 0xab, 0x08, 0xbb, 0x01]);
        assert_eq!(unknown_count(&steps), 2);
    }
}
