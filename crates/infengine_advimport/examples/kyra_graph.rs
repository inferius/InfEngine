//! What the Kyrandia room graph looks like.
//!
//! A probe rather than a test: it needs a `kyra.dat`, and what it
//! reports is a fact about that file, not about this code.
//!
//! ```text
//! cargo run -p infengine_advimport --example kyra_graph -- <kyra.dat> [room]
//! ```
//!
//! With no room it summarises the graph and lists the backgrounds that
//! carry more than one room - the mazes. With one, it prints that room's
//! exits and what leads back to it.

#![allow(clippy::print_stdout)]

use infengine_advimport::kyra::kyradat::{Direction, KyraDat};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        println!("usage: kyra_graph <kyra.dat> [room number]");
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            println!("could not read {path}: {err}");
            return;
        }
    };
    let dat = match KyraDat::parse(&bytes) {
        Ok(dat) => dat,
        Err(err) => {
            println!("{path}: {err}");
            return;
        }
    };
    let graph = match dat.kyrandia1() {
        Ok(graph) => graph,
        Err(err) => {
            println!("{path}: {err}");
            return;
        }
    };

    let name = |room: usize| graph.name_of(room).unwrap_or("?").to_string();

    if let Some(want) = args.next().and_then(|s| s.parse::<usize>().ok()) {
        let Some(link) = graph.rooms.get(want) else {
            println!("no room {want}; the table holds {}", graph.rooms.len());
            return;
        };
        println!("room {want} - {}", name(want));
        for (dir, to) in link.present() {
            println!("  {:<5} -> {to:>3} {}", dir.name(), name(to as usize));
        }
        for (from, other) in graph.linked() {
            for (dir, to) in other.present() {
                if to as usize == want {
                    println!("  <- {from:>3} {} going {}", name(from), dir.name());
                }
            }
        }
        return;
    }

    let linked = graph.linked().count();
    let (agree, total) = graph.symmetry();
    println!("{} rooms, {} of them reachable", graph.rooms.len(), linked);
    println!("{} backgrounds", graph.names.len());
    println!(
        "{agree}/{total} links lead back the way they came ({:.0}%)",
        100.0 * agree as f32 / total.max(1) as f32
    );

    let mut uses: Vec<(u8, usize)> = graph.uses().into_iter().collect();
    uses.sort_by_key(|(index, count)| (std::cmp::Reverse(*count), *index));
    println!("\nbackgrounds carrying more than one room:");
    for (index, count) in uses.iter().filter(|(_, c)| *c > 1) {
        let name = graph.names.get(*index as usize).map(String::as_str);
        println!("  {:<10} {count:>3} rooms", name.unwrap_or("?"));
    }
    let once = uses.iter().filter(|(_, c)| *c == 1).count();
    println!("  ({once} more carry exactly one)");

    let mut ways = [0usize; 4];
    for (_, link) in graph.linked() {
        for (dir, _) in link.present() {
            ways[dir as usize] += 1;
        }
    }
    print!("\nexits:");
    for dir in Direction::ALL {
        print!("  {} {}", dir.name(), ways[dir as usize]);
    }
    println!();
}
