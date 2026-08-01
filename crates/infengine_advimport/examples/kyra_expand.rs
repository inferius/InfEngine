//! Apply the room graph to rooms that are already imported.
//!
//! The same step `adv-import kyra --kyra-dat` runs at the end of an
//! import, on its own. Useful when the art is already converted and only
//! the connections need redoing - a full import takes minutes and
//! rewrites every PNG for a change that touches nothing but the room
//! files.
//!
//! ```text
//! cargo run -p infengine_advimport --example kyra_expand -- <rooms dir> <kyra.dat>
//! ```

#![allow(clippy::print_stdout, clippy::print_stderr)]

use infengine_advimport::kyra::kyradat::KyraDat;
use infengine_advimport::kyra::rooms;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(dir), Some(dat)) = (args.next(), args.next()) else {
        println!("usage: kyra_expand <rooms dir> <kyra.dat>");
        return;
    };

    let bytes = match std::fs::read(&dat) {
        Ok(bytes) => bytes,
        Err(err) => {
            println!("could not read {dat}: {err}");
            return;
        }
    };
    let graph = match KyraDat::parse(&bytes).and_then(|archive| archive.kyrandia1()) {
        Ok(graph) => graph,
        Err(err) => {
            println!("{dat}: {err}");
            return;
        }
    };

    let (agree, total) = graph.symmetry();
    println!(
        "graph: {} rooms from {} backgrounds, {agree}/{total} links lead back",
        graph.rooms.len(),
        graph.names.len()
    );

    match rooms::expand(std::path::Path::new(&dir), &graph) {
        Ok(done) => {
            println!(
                "wrote {} rooms with {} exits, replaced {} backgrounds",
                done.written, done.exits, done.replaced
            );
            if !done.missing.is_empty() {
                println!(
                    "{} backgrounds the graph names are not imported: {}",
                    done.missing.len(),
                    done.missing.join(", ")
                );
            }
        }
        Err(err) => println!("{dir}: {err}"),
    }
}
