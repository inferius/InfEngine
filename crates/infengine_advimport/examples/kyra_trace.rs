//! A126: watch a room actually run, step by step.
//!
//! Loads a project, starts a new game (or enters a named room), then
//! ticks the host and prints every note it emits - lines spoken, sounds
//! and music asked for, animations, room changes, and every Skipped
//! call that stalls a conversation - until the run goes quiet or a cap
//! is hit. This is how a "the tree says one line and stops" report
//! becomes a list of exactly which steps ran and which one was the wall.
//!
//! ```text
//! cargo run -p infengine_advimport --release --example kyra_trace -- <project> [room] [entry]
//! ```

#![allow(clippy::print_stdout)]

use infengine_adventure::Project;
use infengine_adventure::host::{Host, Note};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(project_dir) = args.next() else {
        println!("usage: kyra_trace <project> [room] [entry]");
        return;
    };
    let room = args.next();
    let entry = args.next();
    let click_x: f32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(160.0);
    let click_y: f32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(100.0);

    let project = Project::open(std::path::Path::new(&project_dir));
    if project.manifest.is_none() {
        println!("could not load the project at {project_dir}");
        return;
    }
    let mut host = Host::new(project);

    match &room {
        Some(room) => {
            println!("== enter {room} (entry {:?})", entry);
            host.enter(room, entry.as_deref());
        }
        None => {
            println!("== new game");
            host.new_game();
        }
    }
    drain(&mut host, "on entry");

    // Tick a few seconds of game time; a conversation blocks on speech
    // and animation, so stepping past those is what shows the next line.
    for second in 0..20 {
        host.tick(0.5);
        drain(&mut host, &format!("t+{:.1}s", second as f32 * 0.5 + 0.5));
    }

    // Click in the scene - the way a player carries the conversation on
    // past the greeting - and step through what that opens.
    println!("== click at ({click_x}, {click_y})");
    host.click(glam::Vec2::new(click_x, click_y));
    host.tick(0.1);
    host.click(glam::Vec2::new(click_x, click_y));
    drain(&mut host, "on click");
    for second in 0..80 {
        host.tick(0.5);
        drain(&mut host, &format!("t+{:.1}s", second as f32 * 0.5 + 0.5));
    }
    println!("== done");
}

fn drain(host: &mut Host, when: &str) {
    let notes: Vec<String> = host.log.drain(..).map(|note| describe(&note)).collect();
    for note in notes {
        println!("  [{when}] {note}");
    }
}

fn describe(note: &Note) -> String {
    match note {
        Note::Entered(room) => format!("ENTER room {room}"),
        Note::Said(text) => format!("SAY \"{text}\""),
        Note::Sound(id) => format!("sound {id}"),
        Note::Music(Some(id)) => format!("music {id}"),
        Note::Music(None) => "music stop".into(),
        Note::Image(id) => format!("image {id}"),
        Note::Missing(what) => format!("!! MISSING {what}"),
        Note::Skipped(call) => format!("!! SKIPPED {call}"),
    }
}
