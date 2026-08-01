//! What a Kyrandia room script is actually made of.
//!
//! The importer turns call sites into actions and drops everything
//! else. This says how much "everything else" is - and whether the
//! logic that decides what the player clicked is in there.

#![allow(clippy::print_stdout)]

use std::collections::BTreeMap;

use infengine_advimport::encoding::Encoding;
use infengine_advimport::kyra::{emc, pak, vm};

fn main() {
    let dir = std::env::args().nth(1).expect("usage: emc_mix <DAT dir>");
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut jumps = 0usize;
    let mut scripts = 0usize;

    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("pak"))
        })
        .collect();
    paths.sort();

    for path in paths {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(archive) = pak::Pak::parse(&bytes) else { continue };
        for (index, entry) in archive.with_extension("EMC") {
            let Some(data) = archive.data_at(index) else { continue };
            let Ok(script) = emc::decode(data, Encoding::Latin1) else {
                continue;
            };
            let _ = entry;
            scripts += 1;
            for instruction in vm::disassemble(&script.code) {
                *kinds.entry(format!("{:?}", instruction.op)).or_default() += 1;
                if matches!(instruction.op, vm::Op::JumpIfFalse) {
                    jumps += 1;
                }
            }
        }
    }

    let total: usize = kinds.values().sum();
    println!("{scripts} scripts, {total} instructions");
    let mut rows: Vec<_> = kinds.into_iter().collect();
    rows.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (kind, n) in rows {
        println!("  {kind:20} {n:7}  {:>3}%", n * 100 / total.max(1));
    }
    println!("\nconditional branches: {jumps}");
}
