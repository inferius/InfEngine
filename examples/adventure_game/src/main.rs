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
