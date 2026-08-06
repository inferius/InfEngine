//! A102: measure what stands before a call whose arguments we lose.
//!
//! The decompiler collects a call's arguments by walking back over
//! plain pushes; anything computed ends the walk and the call comes
//! through with no arguments - 329 `displayWsa` and 52
//! `walkPlayerToPoint` calls at last count. Before writing an
//! evaluator to recover them, this measures what those windows
//! actually contain: which opcodes, which `Eval` operators, how deep.
//! The evaluator then has to handle what the data holds and nothing
//! it does not.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example emc_arg_probe -- <game dir>
//! ```

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use infengine_advimport::encoding::Encoding;
use infengine_advimport::kyra::{emc, pak, vm};

/// Functions whose lost arguments matter most right now.
const WATCHED: &[(i32, &str)] = &[(64, "displayWsa"), (11, "walkPlayerToPoint")];

fn main() {
    let Some(game) = std::env::args().nth(1).map(PathBuf::from) else {
        println!("usage: emc_arg_probe <game dir with .PAK files>");
        return;
    };

    let mut op_histogram: BTreeMap<String, usize> = BTreeMap::new();
    let mut windows = 0usize;
    let mut samples: Vec<String> = Vec::new();

    let mut paks: Vec<PathBuf> = std::fs::read_dir(&game)
        .expect("readable game dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pak"))
        })
        .collect();
    paks.sort();

    // The game ships as .PAK archives, but an extracted install is a
    // folder of loose files - this one is - so both are sources.
    let mut sources: Vec<(String, Vec<u8>)> = Vec::new();
    for path in &paks {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let Ok(archive) = pak::Pak::parse(&bytes) else { continue };
        for (index, entry) in archive.with_extension("EMC") {
            if let Some(data) = archive.data_at(index) {
                sources.push((entry.name.clone(), data.to_vec()));
            }
        }
    }
    let mut loose: Vec<PathBuf> = std::fs::read_dir(&game)
        .expect("readable game dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("emc"))
        })
        .collect();
    loose.sort();
    for path in loose {
        if let Ok(bytes) = std::fs::read(&path) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            sources.push((name, bytes));
        }
    }

    {
        for (entry_name, data) in &sources {
            let data = &data[..];
            let entry_name = entry_name.as_str();
            *op_histogram.entry("~emc files".into()).or_default() += 1;
            let Ok(script) = emc::decode(data, Encoding::Latin1) else { continue };
            *op_histogram.entry("~emc decoded".into()).or_default() += 1;
            let instructions = vm::disassemble(&script.code);
            *op_histogram.entry("~instructions".into()).or_default() += instructions.len();

            for at in 0..instructions.len() {
                let Some(site) = vm::call_at(&instructions, at) else {
                    continue;
                };
                *op_histogram
                    .entry(format!("~call fn {}", site.function))
                    .or_default() += 1;
                let Some((_, name)) = WATCHED.iter().find(|(f, _)| *f == site.function)
                else {
                    continue;
                };
                // Count every call and what it carries: the old note
                // said the arguments were computed, and the zero it
                // measured says the note was wrong - so record arg
                // counts and values instead.
                *op_histogram
                    .entry(format!("{name} argc={}", site.args.len()))
                    .or_default() += 1;
                if samples.len() < 24 {
                    samples.push(format!("{entry_name} {name} args={:?}", site.args));
                }
                if site.args.len() == 7 && site.function == 64 {
                    for (index, value) in site.args.iter().enumerate() {
                        let entry = op_histogram
                            .entry(format!("~wsa7 arg{index} min/max"))
                            .or_insert(0);
                        let _ = entry;
                        let lo = op_histogram
                            .entry(format!("~wsa7 arg{index} out-of-screen"))
                            .or_insert(0);
                        if index == 2 && !(0..=320).contains(value) {
                            *lo += 1;
                        }
                        if index == 3 && !(0..=200).contains(value) {
                            *lo += 1;
                        }
                        if (index == 5 || index == 6) && !(0..=11).contains(value) {
                            *op_histogram
                                .entry(format!("~wsa7 arg{index} not-a-slot ({value})"))
                                .or_default() += 1;
                        }
                    }
                }
                if !site.args.is_empty() {
                    continue;
                }
                windows += 1;
                // The window: back from the call to the previous call or
                // jump target boundary, capped - what a straight-line
                // evaluator would get to see.
                let start = at.saturating_sub(24);
                let window = &instructions[start..at];
                for ins in window {
                    let key = match (ins.op, ins.operand) {
                        (vm::Op::Eval, Some(operator)) => format!("Eval({operator})"),
                        (op, _) => format!("{op:?}"),
                    };
                    *op_histogram.entry(key).or_default() += 1;
                }
                if samples.len() < 12 {
                    let listing: Vec<String> = window
                        .iter()
                        .rev()
                        .take(10)
                        .rev()
                        .map(|ins| match ins.operand {
                            Some(value) => format!("{:?}({value})", ins.op),
                            None => format!("{:?}", ins.op),
                        })
                        .collect();
                    samples.push(format!("{entry_name} {name}: … {}", listing.join(" ")));
                }
            }
        }
    }

    println!("windows with computed args: {windows}");
    println!("\nopcode histogram inside those windows:");
    for (op, count) in &op_histogram {
        println!("  {op:<20} {count}");
    }
    println!("\nfirst samples (last 10 instructions before the call):");
    for line in samples {
        println!("  {line}");
    }
}
