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
mod assets;
mod draw;
mod edit;
mod play;
mod playback;
mod project;
mod sheet;
mod ui;
mod ui_game;
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

    /// List what an export would take and what it cannot find, then
    /// exit. Opens no window, so a build can run it.
    #[arg(long)]
    plan: bool,

    /// Copy the project's shippable files into this directory and exit.
    #[arg(long)]
    export: Option<PathBuf>,
}

/// Report an export plan without opening a window.
///
/// Exits non-zero when something the data names is not on disk: that is
/// a broken game, and a build ought to hear about it.
fn plan_only(project: &std::path::Path, export: Option<&std::path::Path>) -> i32 {
    use infengine_adventure::pack;

    let root = project::Project::open(project).root;
    let plan = pack::plan(&root);
    tracing::info!(
        files = plan.files.len(),
        megabytes = plan.bytes as f64 / 1_048_576.0,
        "export plan"
    );
    for missing in &plan.missing {
        tracing::warn!(path = %missing, "named but not on disk");
    }
    if !plan.unmapped.is_empty() {
        tracing::warn!(
            count = plan.unmapped.len(),
            "sound ids no index maps to a file"
        );
    }

    if let Some(dest) = export {
        match pack::export(&root, dest, &plan) {
            Ok(copied) => tracing::info!(copied, to = %dest.display(), "exported"),
            Err(err) => {
                tracing::error!(%err, "export failed");
                return 2;
            }
        }
    }
    i32::from(!plan.missing.is_empty())
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
    if cli.plan || cli.export.is_some() {
        std::process::exit(plan_only(&cli.project, cli.export.as_deref()));
    }

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
