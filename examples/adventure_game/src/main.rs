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
