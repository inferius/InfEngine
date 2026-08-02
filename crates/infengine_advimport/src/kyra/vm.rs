//! A16.2: reading EMC bytecode, so it can become our actions.
//!
//! This is a *disassembler*, not an interpreter. The goal is to turn a
//! room's script into [`infengine_adventure::Action`]s the editor can
//! show and a person can change - not to keep a foreign virtual machine
//! running inside the engine forever.
//!
//! The instruction encoding is compact and easy to get wrong. Every
//! instruction is one big-endian 16-bit word, and only five bits of it
//! are the opcode; the top three bits pick where the operand comes
//! from:
//!
//! ```text
//!   0x8000 set   the low fifteen bits are the operand, opcode is 0
//!   0x4000 set   the low eight bits are a signed operand
//!   0x2000 set   the operand is the whole word that follows
//!   none set     no operand
//!   opcode = (word >> 8) & 0x1F
//! ```
//!
//! Reading the high byte as the opcode - the obvious guess - yields 37
//! distinct "opcodes" that are really flag combinations, and nothing
//! decodes.

use std::fmt;

/// Instructions of the EMC virtual machine.
///
/// Named after what they do to the stack rather than after any
/// particular engine's source, since all this module needs is enough
/// structure to find the calls and their arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    JumpOrConst,
    SetReturn,
    PushReturn,
    Push,
    PushAlt,
    PushReg,
    PushFrameNeg,
    PushFrame,
    PopReturn,
    PopReg,
    PopFrameNeg,
    PopFrame,
    AddStack,
    SubStack,
    /// Call into the engine's own function table.
    Call,
    JumpIfFalse,
    Negate,
    Eval,
    SetReturnAndJump,
    /// An opcode this disassembler does not name.
    Other(u8),
}

impl Op {
    fn from_code(code: u8) -> Self {
        match code {
            0 => Op::JumpOrConst,
            1 => Op::SetReturn,
            2 => Op::PushReturn,
            3 => Op::Push,
            4 => Op::PushAlt,
            5 => Op::PushReg,
            6 => Op::PushFrameNeg,
            7 => Op::PushFrame,
            8 => Op::PopReturn,
            9 => Op::PopReg,
            10 => Op::PopFrameNeg,
            11 => Op::PopFrame,
            12 => Op::AddStack,
            13 => Op::SubStack,
            14 => Op::Call,
            15 => Op::JumpIfFalse,
            16 => Op::Negate,
            17 => Op::Eval,
            18 => Op::SetReturnAndJump,
            other => Op::Other(other),
        }
    }

    /// Does this instruction put a value on the stack?
    pub fn is_push(self) -> bool {
        matches!(self, Op::Push | Op::PushAlt)
    }
}

/// One decoded instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instruction {
    /// Byte offset in the code.
    pub at: usize,
    pub op: Op,
    /// Operand, when the encoding carried one.
    pub operand: Option<i32>,
}

impl fmt::Display for Instruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.operand {
            Some(value) => write!(f, "{:>5}: {:?} {value}", self.at, self.op),
            None => write!(f, "{:>5}: {:?}", self.at, self.op),
        }
    }
}

/// Decode a whole code chunk.
///
/// Decoding never fails: a word that makes no sense still decodes to
/// some instruction, and stopping the whole room's script over one
/// would lose the rest of it. Meaning is judged later, by whether a
/// call is recognised.
pub fn disassemble(code: &[u8]) -> Vec<Instruction> {
    let mut out = Vec::new();
    let mut ip = 0usize;
    while ip + 2 <= code.len() {
        let at = ip;
        let word = u16::from_be_bytes([code[ip], code[ip + 1]]);
        ip += 2;

        let (op, operand) = if word & 0x8000 != 0 {
            (Op::JumpOrConst, Some((word & 0x7FFF) as i32))
        } else if word & 0x4000 != 0 {
            let raw = (word & 0xFF) as u8;
            (Op::from_code(((word >> 8) & 0x1F) as u8), Some(raw as i8 as i32))
        } else if word & 0x2000 != 0 {
            if ip + 2 > code.len() {
                break;
            }
            let extra = u16::from_be_bytes([code[ip], code[ip + 1]]);
            ip += 2;
            (
                Op::from_code(((word >> 8) & 0x1F) as u8),
                Some(extra as i32),
            )
        } else {
            (Op::from_code(((word >> 8) & 0x1F) as u8), None)
        };

        out.push(Instruction { at, op, operand });
    }
    out
}

/// A call to the engine's function table, with the constants it was
/// given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    pub at: usize,
    /// Index into the engine's function table.
    pub function: i32,
    /// Constants pushed for it, first argument first.
    ///
    /// Empty when the arguments were computed rather than written out,
    /// which is common and is not an error.
    pub args: Vec<i32>,
}

/// Find every call and the constant arguments it was given.
///
/// Arguments are pushed in reverse, and the pushes are interleaved with
/// stack bookkeeping - in the retail scripts, every push is followed by
/// an `Eval`. Stopping the scan at the first non-push instruction, the
/// obvious implementation, finds no arguments at all anywhere.
pub fn call_sites(instructions: &[Instruction]) -> Vec<CallSite> {
    (0..instructions.len())
        .filter_map(|index| call_at(instructions, index))
        .collect()
}

/// The call at `index`, if there is one there.
///
/// Separate from [`call_sites`] because a decompiler that recovers the
/// shape of the code walks instructions rather than a flat list of
/// calls, and still needs each call's arguments when it reaches one.
pub fn call_at(instructions: &[Instruction], index: usize) -> Option<CallSite> {
    let instruction = instructions.get(index)?;
    let (Op::Call, Some(function)) = (instruction.op, instruction.operand) else {
        return None;
    };
    // The byte-operand encoding is signed, which is right for
    // constants and wrong here: a call's operand is an index into the
    // function table, and the table has slots past 127. Read as
    // signed, `message` (139) becomes -117 and a fifth of the game's
    // narration goes untranslated.
    let function = if function < 0 { function & 0xFF } else { function };
    let mut args = Vec::new();
    let mut back = index;
    while back > 0 {
        back -= 1;
        let previous = &instructions[back];
        if previous.op.is_push() {
            if let Some(value) = previous.operand {
                args.push(value);
                continue;
            }
            break;
        }
        // Bookkeeping between the pushes; keep walking back.
        if matches!(previous.op, Op::Eval) {
            continue;
        }
        break;
    }
    Some(CallSite {
        at: instruction.at,
        function,
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode one instruction with no operand.
    fn plain(op: u8) -> [u8; 2] {
        [op & 0x1F, 0]
    }

    /// Encode one with a signed byte operand.
    fn byte_arg(op: u8, value: i8) -> [u8; 2] {
        [0x40 | (op & 0x1F), value as u8]
    }

    /// Encode one with a following-word operand.
    fn word_arg(op: u8, value: u16) -> [u8; 4] {
        let head = [0x20 | (op & 0x1F), 0];
        let tail = value.to_be_bytes();
        [head[0], head[1], tail[0], tail[1]]
    }

    #[test]
    fn the_opcode_is_five_bits_not_eight() {
        // 0x44 is opcode 4 with a byte operand, not opcode 0x44.
        let code = byte_arg(4, 7);
        let ins = disassemble(&code);
        assert_eq!(ins[0].op, Op::PushAlt);
        assert_eq!(ins[0].operand, Some(7));
    }

    #[test]
    fn a_byte_operand_is_signed() {
        let ins = disassemble(&byte_arg(3, -2));
        assert_eq!(ins[0].operand, Some(-2));
    }

    #[test]
    fn the_high_bit_makes_the_whole_word_the_operand() {
        let code = [0x80 | 0x12, 0x34];
        let ins = disassemble(&code);
        assert_eq!(ins[0].op, Op::JumpOrConst);
        assert_eq!(ins[0].operand, Some(0x1234));
    }

    #[test]
    fn a_word_operand_comes_from_the_next_word() {
        let ins = disassemble(&word_arg(14, 45));
        assert_eq!(ins[0].op, Op::Call);
        assert_eq!(ins[0].operand, Some(45));
        assert_eq!(ins.len(), 1, "the operand word is not an instruction");
    }

    #[test]
    fn an_instruction_without_an_operand_has_none() {
        let ins = disassemble(&plain(16));
        assert_eq!(ins[0].op, Op::Negate);
        assert_eq!(ins[0].operand, None);
    }

    #[test]
    fn an_unnamed_opcode_still_decodes() {
        // Losing the rest of a room's script over one odd word would be
        // worse than carrying it through unnamed.
        let ins = disassemble(&plain(30));
        assert_eq!(ins[0].op, Op::Other(30));
    }

    #[test]
    fn arguments_survive_the_bookkeeping_between_pushes() {
        // The retail scripts put an Eval after every push. Stopping at
        // the first non-push finds no arguments anywhere.
        let mut code = Vec::new();
        for value in [1i8, 68, 11, 19] {
            code.extend_from_slice(&byte_arg(4, value));
            code.extend_from_slice(&byte_arg(17, 8));
        }
        code.extend_from_slice(&word_arg(14, 45));

        let sites = call_sites(&disassemble(&code));
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].function, 45);
        assert_eq!(
            sites[0].args,
            vec![19, 11, 68, 1],
            "pushed in reverse, so the last push is the first argument"
        );
    }

    #[test]
    fn a_call_whose_arguments_are_computed_reports_none() {
        // Common and not an error: the value came from a variable.
        let mut code = Vec::new();
        code.extend_from_slice(&plain(5)); // PushReg
        code.extend_from_slice(&word_arg(14, 45));
        let sites = call_sites(&disassemble(&code));
        assert_eq!(sites.len(), 1);
        assert!(sites[0].args.is_empty());
    }

    #[test]
    fn every_call_in_a_stream_is_found() {
        let mut code = Vec::new();
        code.extend_from_slice(&byte_arg(4, 3));
        code.extend_from_slice(&word_arg(14, 1));
        code.extend_from_slice(&byte_arg(4, 9));
        code.extend_from_slice(&word_arg(14, 52));
        let sites = call_sites(&disassemble(&code));
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].function, 1);
        assert_eq!(sites[1].function, 52);
        assert_eq!(sites[1].args, vec![9]);
    }

    #[test]
    fn a_truncated_word_operand_ends_the_stream_cleanly() {
        let mut code = word_arg(14, 45).to_vec();
        code.truncate(3);
        assert!(disassemble(&code).is_empty());
    }

    #[test]
    fn an_empty_chunk_decodes_to_nothing() {
        assert!(disassemble(&[]).is_empty());
        assert!(call_sites(&[]).is_empty());
    }

    #[test]
    fn instructions_print_readably() {
        let ins = disassemble(&word_arg(14, 45));
        assert_eq!(ins[0].to_string(), "    0: Call 45");
        let ins = disassemble(&plain(16));
        assert_eq!(ins[0].to_string(), "    0: Negate");
    }
}
