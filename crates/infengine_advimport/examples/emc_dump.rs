//! Print a room script the way the VM sees it.
//!
//! The importer keeps the calls and drops the rest. This shows the rest.

#![allow(clippy::print_stdout)]

use infengine_advimport::encoding::Encoding;
use infengine_advimport::kyra::{emc, pak, vm};

fn main() {
    let mut args = std::env::args().skip(1);
    let archive = args.next().expect("usage: emc_dump <pak> <stem> [count]");
    let stem = args.next().expect("stem");
    let count: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(60);

    let bytes = std::fs::read(&archive).expect("read");
    let pak = pak::Pak::parse(&bytes).expect("pak");
    let data = pak.get(&format!("{stem}.EMC")).expect("no such script");
    let script = emc::decode(data, Encoding::Latin1).expect("decode");

    println!("entry points: {:?}", script.entry_points);
    println!("texts: {}", script.texts.len());
    for (index, instruction) in vm::disassemble(&script.code).iter().take(count).enumerate() {
        println!(
            "  {index:4} @{:04x}  {:?} {:?}",
            instruction.at, instruction.op, instruction.operand
        );
    }
}
