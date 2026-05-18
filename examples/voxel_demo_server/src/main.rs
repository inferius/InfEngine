//! Headless dedicated server for the voxel demo.
//!
//! Hosts a `voxel_demo_mp`-compatible world authoritatively. No
//! renderer, window, audio, input - the whole binary is a tick loop
//! that drives the world simulation, accepts client connections
//! over UDP, and persists the world via the typed save layer
//! exposed by `voxel_demo_mp::world_save`.
//!
//! Run modes:
//!
//! ```sh
//! voxel_demo_server --port 7777 --world my-world --seed 42 \
//!                   --worldgen amplified
//! ```
//!
//! Phase B.7b ships the minimum viable shape: bootstrap a world,
//! bind a TCP / UDP listener, run the tick loop, autosave every
//! `--autosave-secs`, flush + save cleanly on SIGINT / SIGTERM.
//! Mob AI, scheduled timers, drop physics, chat / admin commands
//! land in Phase B.7c (stdin REPL) and the follow-up "headless
//! parity" pass that lifts those subsystems out of
//! `voxel_demo_mp::main::Voxel`.

use std::process::ExitCode;

use clap::Parser;
use tracing::{error, info};

mod args;
mod audit;
mod runtime;

use args::Args;
use runtime::HeadlessServer;
use runtime::shutdown;

fn main() -> ExitCode {
    init_tracing();
    let args = Args::parse();
    info!(
        port = args.port,
        world = %args.world,
        worldgen = ?args.worldgen,
        seed = ?args.seed,
        bind = %args.bind,
        autosave_secs = args.autosave_secs,
        "voxel_demo_server starting"
    );

    let mut srv = match HeadlessServer::bootstrap(&args) {
        Ok(s) => s,
        Err(err) => {
            error!(error = %err, "bootstrap failed");
            return ExitCode::from(2);
        }
    };

    let shutdown = match shutdown::install_signal_handler() {
        Ok(t) => t,
        Err(err) => {
            error!(error = %err, "signal handler install failed");
            return ExitCode::from(3);
        }
    };

    // 20 Hz server tick - matches the design doc baseline. Stays
    // independent of the client's render rate; clients receive
    // per-tick broadcasts.
    let tick_rate = std::time::Duration::from_millis(50);
    let mut next_tick = std::time::Instant::now() + tick_rate;

    while !shutdown.is_requested() {
        let now = std::time::Instant::now();
        if now >= next_tick {
            srv.tick();
            next_tick += tick_rate;
            // Catch-up budget: if we've fallen behind > 5 ticks
            // (likely a stop-the-world IO hiccup or a paused
            // process), reset the cadence rather than burst-running.
            if now > next_tick + 5 * tick_rate {
                next_tick = now + tick_rate;
            }
        }
        if srv.autosave_due()
            && let Err(err) = srv.save()
        {
            tracing::warn!(error = %err, "autosave failed");
        }
        // Idle a millisecond so we don't pin the core when the
        // tick budget isn't due yet.
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    info!("graceful shutdown requested - flushing world");
    if let Err(err) = srv.save() {
        error!(error = %err, "shutdown save failed");
        return ExitCode::from(4);
    }
    info!("voxel_demo_server stopped");
    ExitCode::SUCCESS
}

/// Configure `tracing-subscriber` using the same env-filter
/// convention as `voxel_demo_mp`. `RUST_LOG` controls per-target
/// verbosity; default is `info`.
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        // Send logs to stderr - leaves stdout free for any future
        // stdin REPL output (Phase B.7c).
        .with_writer(std::io::stderr)
        .init();
}
