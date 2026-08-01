//! A44.1: the click regions, which were in the bytecode all along.
//!
//! Every room's `.EMC` opens with a subroutine nobody calls from
//! outside, and its whole body is one expression:
//!
//! ```text
//!   reg1 >= arg1 && reg1 <= arg1 + arg3 &&
//!   reg2 >= arg2 && reg2 <= arg2 + arg4
//! ```
//!
//! which is a point in a rectangle: `reg1`/`reg2` are the cursor and
//! `arg1..arg4` are `x, y, w, h`. The room's entry point then calls it
//! once per clickable thing, and the whole hotspot table of the game is
//! that chain:
//!
//! ```text
//!   Push h; Push w; Push y; Push x   arguments, last one first
//!   PushRet 1                        return position
//!   Jmp <hit test>
//!   AddSP 4
//!   PushRet 0                        the answer
//!   IfNotJmp <next region>
//!     ... what happens when you click it ...
//!   Jmp <end>
//!   <next region>: ...
//! ```
//!
//! Three things had to be right together for this to read, which is why
//! it was called absent three times before it was found: the opcode is
//! five bits and not eight, a jump operand carries `0x8000` that has to
//! be masked off, and arguments are pushed last-first so the rectangle
//! is the four pushes read backwards. Get any one wrong and the
//! rectangles come out as noise rather than as screen coordinates.
//!
//! The hit test is found by what it does rather than by where it sits.
//! It is at word zero in every retail room, and relying on that would
//! work until the first room that was compiled differently.

use std::ops::Range;

use super::vm::{Instruction, Op};

/// Register the cursor's x is read from, and its y.
const REG_CURSOR_X: i32 = 1;
const REG_CURSOR_Y: i32 = 2;
/// Arguments the hit test takes: x, y, w, h.
const RECT_ARGS: usize = 4;
/// How far into a candidate subroutine to look for its signature.
///
/// The real one is nineteen instructions long. Twice that is room for a
/// variant without being room for a different routine that happens to
/// touch the same things eventually.
const SIGNATURE_WINDOW: usize = 40;

/// One clickable rectangle and the code behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Left edge, in room pixels.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Instruction index of the call that tests it, for reporting.
    pub at: usize,
    /// Instructions that run when the click is inside: the half of the
    /// branch the test guards.
    pub body: Range<usize>,
}

impl Region {
    /// Is this a rectangle on a screen of this size?
    ///
    /// A call to the hit test with computed arguments leaves constants
    /// from further up the stack in their place, and those read as
    /// rectangles the size of a scene number. Checking against the
    /// screen is what separates the two.
    pub fn fits(&self, width: i32, height: i32) -> bool {
        self.width > 0
            && self.height > 0
            && self.x >= 0
            && self.y >= 0
            && self.x + self.width <= width
            && self.y + self.height <= height
    }
}

/// Where the cursor-in-rectangle subroutine starts, as an instruction
/// index.
///
/// Identified by its body: it reads both cursor registers and all four
/// of its arguments, and does nothing else. No other routine in the
/// retail scripts touches that particular set.
pub fn find_hit_test(instructions: &[Instruction]) -> Option<usize> {
    let mut regs = [false; 2];
    let mut args = [false; RECT_ARGS];
    for instruction in instructions.iter().take(SIGNATURE_WINDOW) {
        match (instruction.op, instruction.operand) {
            (Op::PushReg, Some(REG_CURSOR_X)) => regs[0] = true,
            (Op::PushReg, Some(REG_CURSOR_Y)) => regs[1] = true,
            (Op::PushFrame, Some(n)) if (1..=RECT_ARGS as i32).contains(&n) => {
                args[n as usize - 1] = true;
            }
            _ => {}
        }
    }
    // The subroutine is not an entry point and always compiles first,
    // so index zero is where it is - but only if it looks like it.
    (regs.iter().all(|&r| r) && args.iter().all(|&a| a)).then_some(0)
}

/// Word index a jump operand points at, with the marker bit removed.
///
/// Jump targets carry `0x8000`. Leaving it on turns every target into
/// an offset past the end of the code, and the branch bodies come out
/// empty rather than wrong - which looks like a room with no logic.
fn jump_target(operand: i32) -> usize {
    (operand as u32 & 0x7FFF) as usize
}

/// Map word index to instruction index.
fn word_index(instructions: &[Instruction]) -> std::collections::HashMap<usize, usize> {
    instructions
        .iter()
        .enumerate()
        .map(|(index, instruction)| (instruction.at / 2, index))
        .collect()
}

/// The first instruction at or after `word`.
///
/// Not a lookup: a jump can target the word after the last instruction,
/// which is how a branch that runs to the end of the script is written.
/// Requiring an exact hit makes that body empty, and an empty body reads
/// as a hotspot that does nothing rather than as one whose code was not
/// found.
fn instruction_at_or_after(instructions: &[Instruction], word: usize) -> usize {
    instructions
        .partition_point(|instruction| instruction.at / 2 < word)
}

/// Every clickable rectangle a script defines.
pub fn regions(instructions: &[Instruction]) -> Vec<Region> {
    let Some(hit_test) = find_hit_test(instructions) else {
        return Vec::new();
    };
    let words = word_index(instructions);
    let mut out = Vec::new();

    for index in 0..instructions.len() {
        // Push the return position, then jump into the hit test.
        if instructions[index].op != Op::PushReturn || instructions[index].operand != Some(1) {
            continue;
        }
        let Some(jump) = instructions.get(index + 1) else {
            continue;
        };
        let (Op::JumpOrConst, Some(target)) = (jump.op, jump.operand) else {
            continue;
        };
        if words.get(&jump_target(target)).copied() != Some(hit_test) {
            continue;
        }
        // Four arguments cleaned up afterwards, and four constants
        // pushed before. A call that computed one of them is skipped
        // rather than half-read.
        let cleanup = instructions.get(index + 2);
        if !matches!(
            cleanup.map(|i| (i.op, i.operand)),
            Some((Op::AddStack, Some(n))) if n == RECT_ARGS as i32
        ) {
            continue;
        }
        let Some(rect) = constants_before(instructions, index, RECT_ARGS) else {
            continue;
        };

        // The answer is pushed and then branched on - but not always
        // immediately. A third of the room's hotspots want the click to
        // be inside *and* something else to be true, and combine the two
        // with an `Eval` first. Insisting the branch come next drops
        // those rectangles, and a rectangle the game clearly defines is
        // worth keeping even when its guard is more than a click.
        let (body_start, body_end) = match find_branch(instructions, index + 3) {
            Some((branch, skip)) => {
                let start = branch + 1;
                (
                    start,
                    instruction_at_or_after(instructions, jump_target(skip)).max(start),
                )
            }
            // No branch found: the rectangle is real, the code behind it
            // is not something this can point at.
            None => (index + 3, index + 3),
        };

        out.push(Region {
            // Pushed last-first, so the nearest push is the first
            // argument. Reading them the other way makes every
            // rectangle a height where its x should be.
            x: rect[0],
            y: rect[1],
            width: rect[2],
            height: rect[3],
            at: index,
            body: body_start..body_end,
        });
    }
    out
}

/// How far past the hit test to look for the branch that uses it.
///
/// The tightest form branches on the next instruction. The combined
/// form pushes another condition and evaluates it first, which in the
/// retail scripts never takes more than a handful of instructions. A
/// wider window would start attributing one hotspot's body to the
/// rectangle before it.
const BRANCH_WINDOW: usize = 8;

/// The branch that acts on the hit test, and where it skips to.
///
/// Returns the instruction index of the `JumpIfFalse` and its target.
/// Stops at anything that would mean the answer has been used for
/// something other than a branch.
fn find_branch(instructions: &[Instruction], from: usize) -> Option<(usize, i32)> {
    let end = (from + BRANCH_WINDOW).min(instructions.len());
    for (index, instruction) in instructions.iter().enumerate().take(end).skip(from) {
        match (instruction.op, instruction.operand) {
            (Op::JumpIfFalse, Some(target)) => return Some((index, target)),
            // Reading the answer, combining it, calling something to
            // decide: all still part of working out the condition.
            (Op::PushReturn | Op::Push | Op::PushAlt | Op::PushReg, _)
            | (Op::Eval | Op::Negate | Op::Call | Op::AddStack | Op::PushFrame, _) => {}
            // A store or a jump means the answer went somewhere else.
            _ => return None,
        }
    }
    None
}

/// The `count` constants pushed immediately before `index`, first
/// argument first.
///
/// Returns `None` if anything other than a constant push intervenes:
/// an argument that was computed cannot be read off the code, and
/// guessing one would put a rectangle somewhere nobody drew it.
fn constants_before(
    instructions: &[Instruction],
    index: usize,
    count: usize,
) -> Option<Vec<i32>> {
    let mut out = Vec::with_capacity(count);
    let mut back = index;
    while out.len() < count {
        back = back.checked_sub(1)?;
        let previous = instructions[back];
        if !previous.op.is_push() {
            return None;
        }
        out.push(previous.operand?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kyra::vm::disassemble;

    /// One instruction with a signed byte operand.
    fn byte_arg(op: u8, value: i8) -> [u8; 2] {
        [0x40 | (op & 0x1F), value as u8]
    }

    /// One with a following-word operand.
    fn word_arg(op: u8, value: u16) -> Vec<u8> {
        let mut v = vec![0x20 | (op & 0x1F), 0];
        v.extend_from_slice(&value.to_be_bytes());
        v
    }

    const PUSH: u8 = 4;
    const PUSH_REG: u8 = 5;
    const PUSH_FRAME: u8 = 7;
    const PUSH_RETURN: u8 = 2;
    const JUMP: u8 = 0;
    const ADD_STACK: u8 = 12;
    const EVAL: u8 = 17;
    const IF_NOT_JUMP: u8 = 15;
    const SYS_CALL: u8 = 14;

    /// The hit test as the retail scripts compile it, shortened to the
    /// instructions the signature looks at.
    fn hit_test() -> Vec<u8> {
        let mut v = Vec::new();
        for (reg, arg) in [(1i8, 1i8), (2, 2)] {
            v.extend_from_slice(&byte_arg(PUSH_REG, reg));
            v.extend_from_slice(&byte_arg(PUSH_FRAME, arg));
            v.extend_from_slice(&byte_arg(EVAL, 7));
            v.extend_from_slice(&byte_arg(PUSH_REG, reg));
            v.extend_from_slice(&byte_arg(PUSH_FRAME, arg));
            v.extend_from_slice(&byte_arg(PUSH_FRAME, arg + 2));
            v.extend_from_slice(&byte_arg(EVAL, 8));
            v.extend_from_slice(&byte_arg(EVAL, 5));
            v.extend_from_slice(&byte_arg(EVAL, 0));
        }
        v
    }

    /// A call to the hit test with a rectangle, and a body of `steps`
    /// instructions.
    ///
    /// `here` is the word index this code will start at, since jump
    /// targets are absolute.
    fn region_call(rect: [i8; 4], here: usize, steps: usize) -> Vec<u8> {
        let mut v = Vec::new();
        // Pushed last argument first, so x is nearest the call.
        for value in rect.iter().rev() {
            v.extend_from_slice(&byte_arg(PUSH, *value));
        }
        v.extend_from_slice(&byte_arg(PUSH_RETURN, 1));
        v.extend_from_slice(&word_arg(JUMP, 0x8000));
        v.extend_from_slice(&byte_arg(ADD_STACK, 4));
        v.extend_from_slice(&byte_arg(PUSH_RETURN, 0));
        // 4 pushes + PushRet + Jmp(2 words) + AddSP + PushRet + IfNot(2)
        let after = here + 4 + 1 + 2 + 1 + 1 + 2;
        v.extend_from_slice(&word_arg(IF_NOT_JUMP, 0x8000 | (after + steps) as u16));
        for _ in 0..steps {
            v.extend_from_slice(&byte_arg(SYS_CALL, 1));
        }
        v
    }

    #[test]
    fn the_hit_test_is_recognised_by_what_it_reads() {
        let ins = disassemble(&hit_test());
        assert_eq!(find_hit_test(&ins), Some(0));
    }

    #[test]
    fn a_routine_that_does_not_read_the_cursor_is_not_the_hit_test() {
        // Relying on it being first would accept whatever is first.
        let mut code = Vec::new();
        for arg in 1..=4i8 {
            code.extend_from_slice(&byte_arg(PUSH_FRAME, arg));
        }
        assert_eq!(find_hit_test(&disassemble(&code)), None);
    }

    #[test]
    fn a_rectangle_is_the_four_pushes_read_backwards() {
        let mut code = hit_test();
        let here = code.len() / 2;
        code.extend_from_slice(&region_call([40, 30, 20, 10], here, 2));

        let found = regions(&disassemble(&code));
        assert_eq!(found.len(), 1);
        let region = &found[0];
        assert_eq!(
            (region.x, region.y, region.width, region.height),
            (40, 30, 20, 10)
        );
    }

    #[test]
    fn the_body_is_the_branch_the_test_guards() {
        let mut code = hit_test();
        let here = code.len() / 2;
        code.extend_from_slice(&region_call([40, 30, 20, 10], here, 3));

        let ins = disassemble(&code);
        let found = regions(&ins);
        assert_eq!(found[0].body.len(), 3, "three steps behind the click");
        for index in found[0].body.clone() {
            assert_eq!(ins[index].op, Op::Call, "the body is what runs");
        }
    }

    #[test]
    fn several_regions_in_a_chain_are_all_found() {
        let mut code = hit_test();
        for rect in [[10i8, 10, 20, 20], [50, 40, 30, 10], [90, 12, 8, 60]] {
            let here = code.len() / 2;
            code.extend_from_slice(&region_call(rect, here, 1));
        }
        let found = regions(&disassemble(&code));
        assert_eq!(found.len(), 3);
        assert_eq!(found[1].x, 50);
        assert_eq!(found[2].height, 60);
    }

    #[test]
    fn a_call_with_a_computed_argument_is_skipped_rather_than_guessed() {
        // The constants left on the stack from further up would read as
        // a rectangle somewhere nobody drew one.
        let mut code = hit_test();
        let here = code.len() / 2;
        let mut call = region_call([40, 30, 20, 10], here, 1);
        // Replace the first push - the height - with a register read.
        call[0] = 0x40 | PUSH_REG;
        code.extend_from_slice(&call);
        assert!(regions(&disassemble(&code)).is_empty());
    }

    #[test]
    fn a_script_with_no_hit_test_yields_no_regions() {
        let mut code = Vec::new();
        code.extend_from_slice(&byte_arg(PUSH, 1));
        code.extend_from_slice(&word_arg(SYS_CALL, 45));
        assert!(regions(&disassemble(&code)).is_empty());
    }

    #[test]
    fn a_rectangle_off_the_screen_is_reported_as_not_fitting() {
        let region = Region {
            x: 300,
            y: 10,
            width: 200,
            height: 10,
            at: 0,
            body: 0..0,
        };
        assert!(!region.fits(320, 144));
        assert!(region.fits(640, 144));

        let empty = Region {
            width: 0,
            ..region.clone()
        };
        assert!(!empty.fits(640, 144), "a rectangle with no width is not one");
    }

    #[test]
    fn the_marker_bit_on_a_jump_target_is_masked_off() {
        // Left on, every target lands past the end of the code and the
        // bodies come out empty - which reads as a room with no logic
        // rather than as a misread one.
        assert_eq!(jump_target(0x8018), 24);
        assert_eq!(jump_target(24), 24);
    }
}
