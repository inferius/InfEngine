//! Screen editor for point-and-click adventure rooms.
//!
//! Opens a project - a folder holding `rooms/` - and lets you shape the
//! walkable floor, place hotspots, and preview the actor's route over
//! the real background art.
//!
//! ```text
//! adventure-editor <project-dir>
//! ```
//!
//! `main` is bootstrap only; state lives in [`app::Editor`] and
//! behaviour in the modules beside it.

#![forbid(unsafe_code)]

mod app;
mod draw;
mod edit;
mod play;
mod playback;
mod project;
mod ui;
mod ui_play;
mod view;

use std::path::PathBuf;

use clap::Parser;
use infengine_window::{WindowConfig, run};

#[derive(Parser, Debug)]
#[command(name = "adventure-editor", about = "Edit point-and-click adventure rooms")]
struct Cli {
    /// Project directory holding `rooms/`, or the `rooms` directory
    /// itself.
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
    let config = WindowConfig {
        title: "InfEngine - Adventure editor".to_owned(),
        width: 1600,
        height: 900,
        resizable: true,
    };

    if let Err(err) = run(config, app::Editor::new(cli.project)) {
        tracing::error!(%err, "editor exited with an error");
    }
}
