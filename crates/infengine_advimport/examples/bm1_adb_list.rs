//! A77 probe: what Posel smrti's object database holds.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example bm1_adb_list -- <data.adb>
//! ```

#![allow(clippy::print_stdout)]

use infengine_advimport::bm1::adb::Adb;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        println!("usage: bm1_adb_list <data.adb>");
        return;
    };
    let bytes = std::fs::read(&path).expect("readable database");
    let adb = match Adb::parse(&bytes) {
        Ok(adb) => adb,
        Err(err) => {
            println!("{path}: {err}");
            return;
        }
    };

    let mut by_suffix: std::collections::BTreeMap<String, (usize, usize, usize)> =
        Default::default();
    for entry in adb.entries() {
        let slot = by_suffix.entry(entry.suffix().to_string()).or_default();
        slot.0 += 1;
        slot.1 += entry.size;
        if Adb::is_code(adb.data_of(entry)) {
            slot.2 += 1;
        }
    }
    println!("{path}: {} entries", adb.entries().len());
    for (suffix, (count, size, code)) in &by_suffix {
        println!("  .{suffix}: {count} entries, {} KB, {code} with bytecode", size / 1024);
    }

    // A taste of the text entries, to see the encoding is right.
    let mut shown = 0;
    for entry in adb.entries() {
        if entry.suffix() != "ts" || shown >= 5 {
            continue;
        }
        let data = adb.data_of(entry);
        let printable: Vec<u8> = data
            .iter()
            .copied()
            .filter(|&b| b == b' ' || !b.is_ascii_control())
            .take(60)
            .collect();
        let (text, _) = infengine_advimport::encoding::decode(
            &printable,
            infengine_advimport::encoding::Encoding::Cp1250,
        );
        println!("  {} = {:?}", entry.key, text);
        shown += 1;
    }
}
