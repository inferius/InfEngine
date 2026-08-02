//! Plays an InfEngine adventure project.
//!
//! The shipped game: it opens a project directory, shows the title
//! screen if there is one, and plays. No panels, no tools.
//!
//! ```text
//! adventure-game [project-dir]
//! ```
//!
//! With no argument it looks in the directory it was started from,
//! which is what a game installed next to its own data needs.

#![forbid(unsafe_code)]

mod app;
mod draw;
mod ui;
mod view;

use std::path::PathBuf;

use clap::Parser;
use infengine_adventure::Project;
use infengine_window::{WindowConfig, run};

#[derive(Parser, Debug)]
#[command(name = "adventure-game", about = "Play an InfEngine adventure")]
struct Cli {
    /// Project directory. Defaults to where the game was started.
    #[arg(default_value = ".")]
    project: PathBuf,

    /// Play without a window for this many seconds and report what
    /// happened. What the game does is decided by the host, which needs
    /// no window, so this shows whether a problem is in the game or in
    /// the drawing of it.
    #[arg(long)]
    check: Option<f32>,

    /// Walk the game without a window: start it, then keep taking exits
    /// and clicking on things for this many moves, and report where it
    /// got to.
    ///
    /// The question a project cannot answer by loading cleanly is
    /// whether it can be played, and that is a question about doors
    /// leading somewhere and clicks running something.
    #[arg(long)]
    walk: Option<usize>,

    /// Visit every room, run its entry script, click everything in it,
    /// and report what broke. The whole game, not a stroll through it.
    ///
    /// A walk finds what a player would; this finds what any player
    /// ever could, because it does not depend on the doors between -
    /// a room is audited even if nothing leads to it yet.
    #[arg(long)]
    audit: bool,
}

/// Close whatever screens a script opened, so the world keeps moving.
fn dismiss_screens(host: &mut infengine_adventure::Host) {
    use infengine_adventure::screen::ScreenAction;
    if host.screen.is_some() {
        host.fire(&ScreenAction::Resume);
    }
}

/// Everything wrong with a project, found by playing all of it.
fn audit(project: Project) {
    use infengine_adventure::host::Note;
    use infengine_adventure::Host;
    use std::collections::BTreeMap;

    let ids = project.room_ids();
    let mut host = Host::new(project);
    host.log_limit = usize::MAX;

    let (mut clicked, mut exits_taken) = (0usize, 0usize);
    for id in &ids {
        host.enter(id, None);
        settle(&mut host);
        // A script may open a screen - the intro ends on the title menu -
        // and a screen stops the world, which for an audit means one
        // room's cutscene freezing the remaining two hundred. Found the
        // hard way: the first run reported every door in the game shut.
        dismiss_screens(&mut host);

        let spots: Vec<glam::Vec2> = host
            .room
            .hotspots
            .iter()
            .filter(|spot| spot.enabled)
            .map(|spot| spot.shape.center())
            .collect();
        for point in spots {
            host.click(point);
            settle(&mut host);
            dismiss_screens(&mut host);
            clicked += 1;
        }

        // Exits change the room, so the room is re-entered after each -
        // an audit of a room has to happen in that room.
        //
        // The click lands on a point of the door the click actually
        // reaches. A door's centre is often under a scripted hotspot -
        // the cave walls sit over the edges of the cave exits - and a
        // player clicks around such things without noticing. Only a
        // door with no reachable point at all is a finding.
        let doors: Vec<(String, Vec<glam::Vec2>)> = host
            .room
            .exits
            .iter()
            .filter(|exit| exit.enabled && !exit.target_room.is_empty())
            .map(|exit| {
                let c = exit.shape.center();
                let (lo, hi) = match exit.shape {
                    infengine_adventure::interact::Shape::Rect { min, max } => (min, max),
                    _ => (c, c),
                };
                let inset = |a: f32, b: f32, t: f32| a + (b - a) * t;
                let samples = vec![
                    c,
                    glam::Vec2::new(inset(lo.x, hi.x, 0.2), inset(lo.y, hi.y, 0.2)),
                    glam::Vec2::new(inset(lo.x, hi.x, 0.8), inset(lo.y, hi.y, 0.2)),
                    glam::Vec2::new(inset(lo.x, hi.x, 0.2), inset(lo.y, hi.y, 0.8)),
                    glam::Vec2::new(inset(lo.x, hi.x, 0.8), inset(lo.y, hi.y, 0.8)),
                    glam::Vec2::new(inset(lo.x, hi.x, 0.5), inset(lo.y, hi.y, 0.1)),
                    glam::Vec2::new(inset(lo.x, hi.x, 0.5), inset(lo.y, hi.y, 0.9)),
                ];
                (exit.id.clone(), samples)
            })
            .collect();
        for (door, samples) in doors {
            let point = samples.iter().copied().find(|p| {
                matches!(
                    host.resolve(*p),
                    infengine_adventure::game::Click::Exit { id, .. } if id == door
                )
            });
            let Some(point) = point else {
                tracing::warn!(room = %id, %door, "a door fully covered by scenery");
                continue;
            };
            let was = host.room.id.clone();
            host.click(point);
            settle(&mut host);
            dismiss_screens(&mut host);
            if host.room.id != was {
                exits_taken += 1;
                host.enter(id, None);
                settle(&mut host);
            } else {
                tracing::warn!(room = %was, "a door that did not open");
            }
        }
    }

    // The log is the evidence; the summary is what it adds up to.
    let mut missing: BTreeMap<String, usize> = BTreeMap::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    for note in &host.log {
        match note {
            Note::Missing(what) => {
                // "sound kyra.sfx7" groups as "sound", keeping the ids
                // for the detail list.
                *missing.entry(what.clone()).or_default() += 1;
            }
            Note::Skipped(call) => *skipped.entry(call.clone()).or_default() += 1,
            _ => {}
        }
    }

    tracing::info!(
        rooms = ids.len(),
        clicks = clicked,
        exits = exits_taken,
        notes = host.log.len(),
        "audited"
    );
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for what in missing.keys() {
        let kind = what.split_whitespace().next().unwrap_or("other");
        *kinds.entry(kind).or_default() += 1;
    }
    for (kind, count) in &kinds {
        tracing::warn!(kind, distinct = count, "missing");
    }
    for (what, count) in missing.iter().take(30) {
        tracing::info!(%what, count, "missing detail");
    }
    let total_skipped: usize = skipped.values().sum();
    tracing::info!(
        distinct = skipped.len(),
        total = total_skipped,
        "steps the import could not translate"
    );
    let mut by_count: Vec<(&String, &usize)> = skipped.iter().collect();
    by_count.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    for (call, count) in by_count.iter().take(10) {
        tracing::info!(%call, count, "skipped");
    }
}

/// Play the game by taking doors, and report what was reachable.
fn walk(project: Project, moves: usize) {
    use infengine_adventure::screen::ScreenAction;
    use infengine_adventure::Host;
    use std::collections::BTreeSet;

    let mut host = Host::new(project);
    host.log_limit = usize::MAX;

    // Straight to the first room of the story. Starting properly would
    // play the intro, and an intro waits on animations that only a
    // renderer reports as finished - so it never ends and the walk
    // never begins.
    host.new_game();
    settle(&mut host);
    if host.screen.is_some() {
        host.fire(&ScreenAction::NewGame);
        settle(&mut host);
    }

    let mut visited = BTreeSet::new();
    // How many times each room has been left, so a room reached again
    // is left by a different door. Without it the walk bounces between
    // the same two rooms as soon as everything next to it has been seen,
    // and reports eleven rooms for a world of two hundred.
    let mut left: std::collections::BTreeMap<String, usize> = Default::default();
    let mut clicked = 0usize;
    let mut dead_ends = 0usize;
    visited.insert(host.room.id.clone());

    for _ in 0..moves {
        // Whatever the room started on arrival has to finish first. A
        // click while a script is running is dropped, and a room with
        // nothing to click had no other reason to wait - which is why a
        // walk stalled in exactly the rooms that were quietest.
        settle(&mut host);

        // Click everything here before leaving, since a hotspot that
        // runs nothing is the failure this is looking for.
        let spots: Vec<glam::Vec2> = host
            .room
            .hotspots
            .iter()
            .filter(|spot| spot.enabled)
            .map(|spot| spot.shape.center())
            .collect();
        for point in spots {
            host.click(point);
            settle(&mut host);
            dismiss_screens(&mut host);
            clicked += 1;
        }

        // Then take a door that leads somewhere new, or any door at all
        // once everything nearby has been seen.
        let doors: Vec<(String, glam::Vec2)> = host
            .room
            .exits
            .iter()
            .filter(|exit| exit.enabled && !exit.target_room.is_empty())
            .map(|exit| (exit.target_room.clone(), exit.shape.center()))
            .collect();
        if doors.is_empty() {
            dead_ends += 1;
            break;
        }
        let was = host.room.id.clone();
        let turn = left.entry(was.clone()).or_default();
        let point = doors
            .iter()
            .find(|(target, _)| !visited.contains(target))
            .map(|(_, point)| *point)
            .unwrap_or_else(|| doors[*turn % doors.len()].1);
        *turn += 1;
        let outcome = host.click(point);
        settle(&mut host);
        if host.room.id == was {
            dead_ends += 1;
            if dead_ends < 6 {
                tracing::warn!(room = %was, click = ?outcome, "door did not open");
            }
        }
        visited.insert(host.room.id.clone());
    }

    tracing::info!(
        rooms = visited.len(),
        clicks = clicked,
        stuck = dead_ends,
        ended_in = %host.room.id,
        "walked"
    );
    let skipped = host
        .log
        .iter()
        .filter(|note| note.to_string().contains("skipped"))
        .count();
    tracing::info!(notes = host.log.len(), skipped, "log");
    for note in host.log.iter().take(20) {
        tracing::info!("{note}");
    }
}

/// Let whatever the last click started run to a stop.
///
/// A walk that lasts a minute of game time is one the actor is taking
/// across a room, and waiting for it is the difference between reading
/// where the game went and reading where it was going.
///
/// Animations have to be answered for. A script that plays one and
/// waits is waiting on the renderer to say it finished, and with no
/// window nobody ever does - so the script stops there and the room
/// never changes. That is not the game being stuck, it is this standing
/// in for a window badly, and it read as 84 broken doors before the
/// difference was noticed.
fn settle(host: &mut infengine_adventure::Host) {
    let step = 1.0 / 60.0;
    for _ in 0..600 {
        let request = host.tick(step);
        if let Some(id) = request.animation {
            host.game.runner.finish_animation(&id);
        }
        // And read the line, which is what a player does by waiting or
        // by pressing a key. A script stopped on a line nobody dismissed
        // blocks every click after it, which is what made a walk through
        // the game freeze in the first room with anything to say.
        host.skip_subtitle();
    }
}

/// Run the host on its own and print its log.
fn check(project: Project, seconds: f32) {
    use infengine_adventure::Host;

    // What loaded, before anything runs: a project that dropped a file
    // is the commonest reason a game does nothing.
    for err in &project.script_errors {
        tracing::error!("{err}");
    }
    tracing::info!(
        rooms = project.rooms.len(),
        scripts = project.scripts.len(),
        screens = project.screens.len(),
        animations = project.animations.len(),
        sounds = project.sounds.len(),
        "project"
    );
    let mut host = Host::new(project);
    host.log_limit = usize::MAX;
    let request = host.start();
    tracing::info!(room = %host.room.id, screen = ?host.screen, "started");
    if let Some(id) = request.animation {
        tracing::info!(id, "animation");
    }

    let step = 1.0 / 60.0;
    let mut ticks = 0;
    while (ticks as f32) * step < seconds {
        let request = host.tick(step);
        if let Some(id) = request.animation {
            tracing::info!(id, at = format!("{:.1}s", ticks as f32 * step), "animation");
        }
        if let Some(file) = request.voice {
            tracing::info!(file, "voice");
        }
        if let Some(file) = request.music.flatten() {
            tracing::info!(file, "music");
        }
        ticks += 1;
    }
    tracing::info!(room = %host.room.id, screen = ?host.screen, "ended");
    for note in &host.log {
        tracing::info!("{note}");
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .without_time()
        .init();

    let cli = Cli::parse();
    let project = Project::open(&cli.project);
    if project.is_empty() {
        tracing::error!(path = %cli.project.display(), "no rooms here");
        return;
    }
    // The window is named after the game, not after the engine: this is
    // the thing a player runs.
    let title = project
        .manifest
        .as_ref()
        .map(|manifest| manifest.title.clone())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "InfEngine adventure".to_string());

    if let Some(seconds) = cli.check {
        check(project, seconds);
        return;
    }
    if let Some(moves) = cli.walk {
        walk(project, moves);
        return;
    }
    if cli.audit {
        audit(project);
        return;
    }

    let config = WindowConfig {
        title,
        width: 1280,
        height: 800,
        resizable: true,
    };
    if let Err(err) = run(config, app::Game::new(project)) {
        tracing::error!(%err, "the game exited with an error");
    }
}
