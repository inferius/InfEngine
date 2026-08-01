//! What Kyrandia's sound effects are made of.
//!
//! `playSoundEffect(n)` names a track, and a track names a program in
//! the `.ADL` file. This reports which of those tracks the scripts
//! actually ask for and which sequencer commands their programs use -
//! the number that says whether rendering them is a week or an evening.
//!
//! ```text
//! cargo run -p infengine_advimport --example adl_sfx -- <dir of .ADL>
//! ```

#![allow(clippy::print_stdout)]

use std::collections::{BTreeMap, BTreeSet};

use infengine_advimport::kyra::adl::{self, AdlFile, Step};

/// Tracks the imported scripts ask for, gathered from a walk of the
/// game and pasted here rather than re-derived: this is a probe.
const WANTED: &[usize] = &[
    1, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48,
    49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 70, 71, 72, 75,
    78, 79, 80, 88, 96, 100, 117,
];

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        println!("usage: adl_sfx <dir of .ADL files>");
        return;
    };
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("adl"))
        })
        .collect();
    files.sort();

    let wanted: BTreeSet<usize> = WANTED.iter().copied().collect();
    // Every opcode a wanted track's program uses, and how often.
    let mut opcodes: BTreeMap<u8, usize> = BTreeMap::new();
    let mut named = 0usize;
    let mut notes = 0usize;
    let mut covered: BTreeSet<usize> = BTreeSet::new();
    let mut lengths: Vec<usize> = Vec::new();

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(file) = AdlFile::parse(&bytes) else {
            continue;
        };
        for (track, program) in file.tracks() {
            if !wanted.contains(&track) {
                continue;
            }
            let Some(code) = file.program(program as usize) else {
                continue;
            };
            covered.insert(track);
            let steps = adl::disassemble(code);
            lengths.push(steps.len());
            for step in &steps {
                match step {
                    Step::Note { .. } => notes += 1,
                    Step::Command { opcode, .. } => {
                        *opcodes.entry(*opcode).or_default() += 1;
                        if step.name().is_some() {
                            named += 1;
                        }
                    }
                    Step::Truncated => {}
                }
            }
        }
    }

    println!("{} .ADL files", files.len());
    println!(
        "{} of the {} tracks the scripts ask for have a program",
        covered.len(),
        wanted.len()
    );
    let missing: Vec<usize> = wanted.difference(&covered).copied().collect();
    if !missing.is_empty() {
        println!("   no program: {missing:?}");
    }
    lengths.sort_unstable();
    if let (Some(first), Some(last)) = (lengths.first(), lengths.last()) {
        let middle = lengths[lengths.len() / 2];
        println!("program length: {first} to {last} steps, middle {middle}");
    }

    let commands: usize = opcodes.values().sum();
    println!("\n{notes} notes, {commands} commands, {named} of them named");
    println!("{} distinct commands in these programs:", opcodes.len());
    let mut by_use: Vec<(u8, usize)> = opcodes.iter().map(|(k, v)| (*k, *v)).collect();
    by_use.sort_by_key(|(opcode, count)| (std::cmp::Reverse(*count), *opcode));
    let mut running = 0usize;
    for (opcode, count) in by_use.iter().copied() {
        running += count;
        let name = Step::Command { opcode, argument: 0 }.name().unwrap_or("?");
        println!(
            "   {opcode:#04x} {name:<12} {count:>4}   {:>3}% of commands so far",
            100 * running / commands.max(1)
        );
    }
}
