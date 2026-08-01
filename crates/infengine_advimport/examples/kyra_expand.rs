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

    let rooms_dir = std::path::Path::new(&dir);
    match rooms::expand(rooms_dir, &graph) {
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

    // The scripts sit beside the rooms, and their scene numbers index
    // the same table. Resolving them here is what joins the game's
    // three regions into one world.
    if let Some(scripts) = rooms_dir.parent().map(|p| p.join("scripts")) {
        let resolved = resolve_scripts(&scripts, &graph);
        println!("{resolved} scripts had scene numbers turned into rooms");
    }
}

/// Rewrite `sceneN` into room ids across a scripts directory.
fn resolve_scripts(dir: &std::path::Path, graph: &rooms::RoomGraph) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut touched = 0;
    for path in entries.flatten().map(|e| e.path()) {
        if !path.to_string_lossy().ends_with(".script.ron") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut scripts) = ron::from_str::<Vec<infengine_adventure::Script>>(&text) else {
            continue;
        };
        let before = text.clone();
        for script in &mut scripts {
            rooms::resolve_scenes(script, graph);
        }
        let Ok(out) = ron::ser::to_string_pretty(
            &scripts,
            ron::ser::PrettyConfig::new().indentor("  ").struct_names(true),
        ) else {
            continue;
        };
        if out != before && std::fs::write(&path, out).is_ok() {
            touched += 1;
        }
    }
    touched
}
