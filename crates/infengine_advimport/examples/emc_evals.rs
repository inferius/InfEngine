//! A64 probe: what Eval operands mean, measured rather than assumed.
//!
//! Run over a directory of `.EMC` files:
//!
//! ```text
//! cargo run -p infengine_advimport --release --example emc_evals -- <dir>
//! ```
//!
//! Prints, for every conditional jump, the shape of the code between it
//! and the nearest preceding call - which is exactly what the
//! decompiler's `condition()` has to phrase. The histograms say which
//! Eval operands the retail scripts actually use in that position, and
//! with which pushed constants, so the comparison table can be written
//! from evidence.

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

use infengine_advimport::kyra::emc;
use infengine_advimport::kyra::vm::{self, Op};

/// Which side of a `queryGameFlag(f) K0 Eval2` guard sets the same flag.
///
/// The do-once idiom decides what Eval2 means: with equality the guard
/// is "flag == 0" and the set lands on the then side; with inequality
/// it would land on the else side.
static DO_ONCE_THEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static DO_ONCE_ELSE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        println!("usage: emc_evals <dir of .EMC files>");
        return;
    };

    // Overall Eval operand histogram, and the same split by position:
    // between a call and its conditional jump, versus elsewhere.
    let mut all: BTreeMap<i32, usize> = BTreeMap::new();
    let mut in_condition: BTreeMap<String, usize> = BTreeMap::new();
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();

    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .expect("readable dir")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("emc"))
        })
        .collect();
    files.sort();

    for path in &files {
        let bytes = std::fs::read(path).expect("readable file");
        let Ok(script) = emc::decode(&bytes, infengine_advimport::encoding::Encoding::Latin1)
        else {
            continue;
        };
        let instructions = vm::disassemble(&script.code);

        for (index, instruction) in instructions.iter().enumerate() {
            if instruction.op == Op::Eval
                && let Some(operand) = instruction.operand
            {
                *all.entry(operand).or_default() += 1;
            }
            if instruction.op != Op::JumpIfFalse {
                continue;
            }
            // Walk back to the nearest call, recording the ops between.
            let mut trail: Vec<String> = Vec::new();
            let mut back = index;
            let mut found = false;
            while back > 0 && index - back < 8 {
                back -= 1;
                let previous = instructions[back];
                match previous.op {
                    Op::Call => {
                        found = true;
                        break;
                    }
                    Op::Eval => {
                        let operand = previous.operand.unwrap_or(-1);
                        trail.push(format!("Eval{operand}"));
                        *in_condition.entry(format!("Eval{operand}")).or_default() += 1;
                    }
                    Op::Push | Op::PushAlt | Op::JumpOrConst => {
                        trail.push(format!("K{}", previous.operand.unwrap_or(0)));
                    }
                    other => trail.push(format!("{other:?}")),
                }
            }
            if found {
                trail.reverse();
                *shapes.entry(trail.join(" ")).or_default() += 1;

                // The do-once probe: guard is queryFlag(f), K0, Eval2.
                let shape = trail.join(" ");
                if shape.ends_with("K0 Eval2")
                    && let Some(site) = vm::call_at(&instructions, back)
                    && site.function == 4
                    && let Some(&flag) = site.args.first()
                {
                    let target = (instruction.operand.unwrap_or(0) as u32 & 0x7FFF) as usize;
                    let then_side: Vec<_> = instructions[index + 1..]
                        .iter()
                        .take_while(|i| i.at / 2 < target)
                        .cloned()
                        .collect();
                    let end_then = index + 1 + then_side.len();
                    let else_side: Vec<_> = instructions
                        [end_then..(end_then + then_side.len().max(8)).min(instructions.len())]
                        .to_vec();
                    let sets = |body: &[vm::Instruction]| {
                        (0..body.len()).any(|i| {
                            vm::call_at(body, i).is_some_and(|s| {
                                s.function == 5 && s.args.first() == Some(&flag)
                            })
                        })
                    };
                    if sets(&then_side) {
                        DO_ONCE_THEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    } else if sets(&else_side) {
                        DO_ONCE_ELSE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        }
    }

    println!(
        "do-once idiom: guard queryFlag(f) K0 Eval2 -> setFlag(f) in then {} / else {}",
        DO_ONCE_THEN.load(std::sync::atomic::Ordering::Relaxed),
        DO_ONCE_ELSE.load(std::sync::atomic::Ordering::Relaxed),
    );
    println!("== Eval operands everywhere ==");
    for (operand, count) in &all {
        println!("  {operand:>4}: {count}");
    }
    println!("== Eval operands between a call and its jump ==");
    for (operand, count) in &in_condition {
        println!("  {operand}: {count}");
    }
    println!("== shapes call..jump (top 30) ==");
    let mut by_count: Vec<(&String, &usize)> = shapes.iter().collect();
    by_count.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    for (shape, count) in by_count.iter().take(30) {
        println!("  {count:>4}  [call] {shape} [jump]");
    }
}
