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
