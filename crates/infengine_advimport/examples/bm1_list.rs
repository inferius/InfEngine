//! A76 probe: what a Posel smrti archive holds.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example bm1_list -- <file.grp>
//! ```

#![allow(clippy::print_stdout)]

use infengine_advimport::bm1::grp::Grp;

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        println!("usage: bm1_list <file.grp>");
        return;
    };
    let bytes = std::fs::read(&path).expect("readable archive");
    let grp = match Grp::parse(&bytes) {
        Ok(grp) => grp,
        Err(err) => {
            println!("{path}: {err}");
            return;
        }
    };
    println!("{path}: {} entries", grp.entries().len());
    let mut by_ext: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    for entry in grp.entries() {
        let ext = entry
            .name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        let slot = by_ext.entry(ext).or_default();
        slot.0 += 1;
        slot.1 += entry.size;
    }
    for (ext, (count, size)) in &by_ext {
        println!("  .{ext}: {count} files, {} KB", size / 1024);
    }
    for entry in grp.entries().iter().take(8) {
        println!("  e.g. {} ({} B)", entry.name, entry.size);
    }
}
